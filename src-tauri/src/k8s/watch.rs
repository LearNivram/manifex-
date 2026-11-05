//! Shared, cached Kubernetes watches.
//!
//! * One upstream `watch` per (cluster, resource, namespace, selectors) no matter
//!   how many views subscribe to it.
//! * Subscribers first receive a snapshot, then batched deltas (every 150 ms) via a
//!   Tauri channel – this avoids flooding the webview with thousands of messages.
//! * When the last subscriber leaves, the upstream watch is kept warm for 60s so
//!   navigating back and forth is instant, and is then torn down.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use futures::StreamExt;
use kube::api::DynamicObject;
use kube::runtime::{watcher, WatchStreamExt};
use kube::Client;
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::task::AbortHandle;

use super::resources::ResourceRef;
use super::summary::{summarize, Row};
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WatchKey {
    pub cluster_id: String,
    pub resource: ResourceRef,
    pub namespace: Option<String>,
    pub label_selector: Option<String>,
    pub field_selector: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WatchMessage {
    Snapshot {
        rows: Vec<Row>,
        ready: bool,
    },
    Batch {
        upserts: Vec<Row>,
        deletes: Vec<String>,
    },
    Error {
        message: String,
        status: Option<u16>,
    },
    Closed,
}

#[derive(Clone)]
enum Delta {
    Upsert(Row),
    Delete(String),
    Reset,
    Error(String, Option<u16>),
}

struct State {
    rows: HashMap<String, Row>,
    ready: bool,
    last_error: Option<(String, Option<u16>)>,
}

struct SharedWatch {
    state: Mutex<State>,
    tx: broadcast::Sender<Delta>,
    task: Mutex<Option<AbortHandle>>,
    subscribers: AtomicUsize,
}

impl Drop for SharedWatch {
    fn drop(&mut self) {
        if let Some(h) = self.task.lock().take() {
            h.abort();
        }
    }
}

struct Subscription {
    key: WatchKey,
    task: AbortHandle,
}

pub struct WatchManager {
    shared: Mutex<HashMap<WatchKey, Arc<SharedWatch>>>,
    subs: Mutex<HashMap<String, Subscription>>,
    self_ref: Mutex<Weak<WatchManager>>,
}

/// Sends messages to a subscriber; returns false when the receiver is gone.
pub trait WatchSink: Send + Sync + 'static {
    fn send(&self, msg: WatchMessage) -> bool;
}

impl<F> WatchSink for F
where
    F: Fn(WatchMessage) -> bool + Send + Sync + 'static,
{
    fn send(&self, msg: WatchMessage) -> bool {
        self(msg)
    }
}

struct SubscriberGuard {
    shared: Arc<SharedWatch>,
    key: WatchKey,
    mgr: Weak<WatchManager>,
}

impl Drop for SubscriberGuard {
    fn drop(&mut self) {
        if self.shared.subscribers.fetch_sub(1, Ordering::SeqCst) == 1 {
            // last subscriber left – schedule teardown after a grace period
            let mgr = self.mgr.clone();
            let key = self.key.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    if let Some(m) = mgr.upgrade() {
                        m.reap(&key);
                    }
                });
            }
        }
    }
}

/// Delay before a watch that failed with 401/403 is attempted again.
pub(crate) const PERMISSION_RETRY: Duration = Duration::from_secs(60);

pub(crate) fn status_of(err: &watcher::Error) -> Option<u16> {
    match err {
        watcher::Error::InitialListFailed(kube::Error::Api(s))
        | watcher::Error::WatchStartFailed(kube::Error::Api(s))
        | watcher::Error::WatchFailed(kube::Error::Api(s)) => Some(s.code),
        watcher::Error::WatchError(s) => Some(s.code),
        _ => None,
    }
}

impl WatchManager {
    pub fn new() -> Arc<Self> {
        let m = Arc::new(Self {
            shared: Mutex::new(HashMap::new()),
            subs: Mutex::new(HashMap::new()),
            self_ref: Mutex::new(Weak::new()),
        });
        *m.self_ref.lock() = Arc::downgrade(&m);
        m
    }

    fn reap(&self, key: &WatchKey) {
        let mut shared = self.shared.lock();
        if let Some(sw) = shared.get(key) {
            if sw.subscribers.load(Ordering::SeqCst) == 0 {
                shared.remove(key);
                tracing::debug!(resource = %key.resource.label(), "watch torn down");
            }
        }
    }

    fn get_or_start(&self, client: Client, key: &WatchKey) -> Arc<SharedWatch> {
        let mut shared = self.shared.lock();
        if let Some(sw) = shared.get(key) {
            return sw.clone();
        }
        let (tx, _) = broadcast::channel(4096);
        let sw = Arc::new(SharedWatch {
            state: Mutex::new(State {
                rows: HashMap::new(),
                ready: false,
                last_error: None,
            }),
            tx,
            task: Mutex::new(None),
            subscribers: AtomicUsize::new(0),
        });
        let api = key.resource.api(client, key.namespace.as_deref());
        let mut cfg = watcher::Config::default().page_size(500);
        if let Some(l) = &key.label_selector {
            cfg = cfg.labels(l);
        }
        if let Some(f) = &key.field_selector {
            cfg = cfg.fields(f);
        }
        let kind = key.resource.kind.clone();
        let weak = Arc::downgrade(&sw);
        let task = tokio::spawn(async move {
            // Permission errors (401/403) don't heal by retrying quickly – the default
            // back-off would hammer the API server (and an agent) several times a
            // second. Pause instead and re-check RBAC periodically.
            loop {
                let mut stream = watcher(api.clone(), cfg.clone()).default_backoff().boxed();
                let mut init_buf: HashMap<String, Row> = HashMap::new();
                let mut denied = false;
                while let Some(ev) = stream.next().await {
                    let Some(sw) = weak.upgrade() else { return };
                    match ev {
                        Ok(watcher::Event::Init) => init_buf.clear(),
                        Ok(watcher::Event::InitApply(obj)) => {
                            let row = to_row(&kind, &obj);
                            init_buf.insert(row.uid.clone(), row);
                        }
                        Ok(watcher::Event::InitDone) => {
                            let mut st = sw.state.lock();
                            st.rows = std::mem::take(&mut init_buf);
                            st.ready = true;
                            st.last_error = None;
                            let _ = sw.tx.send(Delta::Reset);
                        }
                        Ok(watcher::Event::Apply(obj)) => {
                            let row = to_row(&kind, &obj);
                            let mut st = sw.state.lock();
                            st.rows.insert(row.uid.clone(), row.clone());
                            let _ = sw.tx.send(Delta::Upsert(row));
                        }
                        Ok(watcher::Event::Delete(obj)) => {
                            let row = to_row(&kind, &obj);
                            let mut st = sw.state.lock();
                            st.rows.remove(&row.uid);
                            let _ = sw.tx.send(Delta::Delete(row.uid));
                        }
                        Err(e) => {
                            let status = status_of(&e);
                            let msg = match &e {
                                watcher::Error::InitialListFailed(kube::Error::Api(s))
                                | watcher::Error::WatchStartFailed(kube::Error::Api(s))
                                | watcher::Error::WatchFailed(kube::Error::Api(s)) => {
                                    s.message.clone()
                                }
                                other => other.to_string(),
                            };
                            tracing::debug!(error = %msg, "watch error");
                            let mut st = sw.state.lock();
                            st.last_error = Some((msg.clone(), status));
                            let _ = sw.tx.send(Delta::Error(msg, status));
                            if matches!(status, Some(401 | 403)) {
                                denied = true;
                                break;
                            }
                        }
                    }
                }
                drop(stream);
                if !denied {
                    return;
                }
                tokio::time::sleep(PERMISSION_RETRY).await;
                if weak.strong_count() == 0 {
                    return;
                }
            }
        });
        *sw.task.lock() = Some(task.abort_handle());
        shared.insert(key.clone(), sw.clone());
        sw
    }

    /// Starts a subscription. Returns the subscription id.
    pub fn subscribe(
        &self,
        client: Client,
        key: WatchKey,
        sink: impl WatchSink,
    ) -> AppResult<String> {
        let sw = self.get_or_start(client, &key);
        sw.subscribers.fetch_add(1, Ordering::SeqCst);
        let guard = SubscriberGuard {
            shared: sw.clone(),
            key: key.clone(),
            mgr: self.self_ref.lock().clone(),
        };
        let sub_id = uuid::Uuid::new_v4().to_string();

        let task = tokio::spawn(async move {
            let _guard = guard;
            let sw = _guard.shared.clone();
            let snapshot = |sw: &SharedWatch| -> (WatchMessage, Option<(String, Option<u16>)>) {
                let st = sw.state.lock();
                (
                    WatchMessage::Snapshot {
                        rows: st.rows.values().cloned().collect(),
                        ready: st.ready,
                    },
                    st.last_error.clone(),
                )
            };
            // Subscribe before taking the snapshot so no delta is lost in between.
            let mut rx = sw.tx.subscribe();
            let (snap, err) = snapshot(&sw);
            if !sink.send(snap) {
                return;
            }
            if let Some((message, status)) = err {
                if !sink.send(WatchMessage::Error { message, status }) {
                    return;
                }
            }
            let mut upserts: HashMap<String, Row> = HashMap::new();
            let mut deletes: HashSet<String> = HashSet::new();
            let mut tick = tokio::time::interval(Duration::from_millis(150));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    msg = rx.recv() => match msg {
                        Ok(Delta::Upsert(row)) => {
                            deletes.remove(&row.uid);
                            upserts.insert(row.uid.clone(), row);
                        }
                        Ok(Delta::Delete(uid)) => {
                            upserts.remove(&uid);
                            deletes.insert(uid);
                        }
                        Ok(Delta::Reset) | Err(broadcast::error::RecvError::Lagged(_)) => {
                            upserts.clear();
                            deletes.clear();
                            let (snap, _) = snapshot(&sw);
                            if !sink.send(snap) { return; }
                        }
                        Ok(Delta::Error(message, status)) => {
                            if !sink.send(WatchMessage::Error { message, status }) { return; }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            let _ = sink.send(WatchMessage::Closed);
                            return;
                        }
                    },
                    _ = tick.tick() => {
                        if !upserts.is_empty() || !deletes.is_empty() {
                            let msg = WatchMessage::Batch {
                                upserts: upserts.drain().map(|(_, r)| r).collect(),
                                deletes: deletes.drain().collect(),
                            };
                            if !sink.send(msg) { return; }
                        }
                    }
                }
            }
        });
        self.subs.lock().insert(
            sub_id.clone(),
            Subscription {
                key,
                task: task.abort_handle(),
            },
        );
        Ok(sub_id)
    }

    pub fn unsubscribe(&self, sub_id: &str) {
        if let Some(sub) = self.subs.lock().remove(sub_id) {
            sub.task.abort();
        }
    }

    /// Stops every watch belonging to a cluster (on disconnect).
    pub fn stop_cluster(&self, cluster_id: &str) {
        let mut subs = self.subs.lock();
        subs.retain(|_, s| {
            if s.key.cluster_id == cluster_id {
                s.task.abort();
                false
            } else {
                true
            }
        });
        drop(subs);
        self.shared.lock().retain(|k, _| k.cluster_id != cluster_id);
    }

    /// Returns the cached rows for a key, if a watch is currently active.
    pub fn cached_rows(&self, key: &WatchKey) -> Option<Vec<Row>> {
        let shared = self.shared.lock();
        let sw = shared.get(key)?;
        let st = sw.state.lock();
        st.ready.then(|| st.rows.values().cloned().collect())
    }

    pub fn active_count(&self) -> (usize, usize) {
        (self.shared.lock().len(), self.subs.lock().len())
    }
}

fn to_row(kind: &str, obj: &DynamicObject) -> Row {
    let value = serde_json::to_value(obj).unwrap_or(serde_json::Value::Null);
    summarize(kind, &value)
}
