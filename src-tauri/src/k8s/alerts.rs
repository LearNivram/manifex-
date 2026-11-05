//! Alerting engine.
//!
//! For each connected cluster with alerting enabled two lightweight watches run:
//! Warning events (server-side filtered with `type=Warning`) and Nodes. Events are
//! classified into pod failures, workload/deployment failures, node problems and
//! generic warnings, matched against user-defined rules, de-duplicated and then
//! forwarded to the desktop shell (in-app notification centre + native
//! notifications).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use k8s_openapi::api::core::v1::{Event, Node};
use kube::runtime::{watcher, WatchStreamExt};
use kube::{Api, Client};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::task::AbortHandle;

use crate::persistence::rules::AlertRule;
use crate::persistence::Db;

const MAX_RECENT: usize = 500;
const DEDUPE_WINDOW: Duration = Duration::from_secs(600);
const MAX_EVENT_AGE_SECS: i64 = 180;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub ts: String,
    pub cluster_id: String,
    pub cluster_name: String,
    /// info | warning | critical
    pub severity: String,
    /// pod | workload | node | event | rule | connection
    pub category: String,
    pub title: String,
    pub message: String,
    pub namespace: Option<String>,
    pub object_kind: Option<String>,
    pub object_name: Option<String>,
    pub rule_id: Option<String>,
}

pub trait AlertSink: Send + Sync {
    fn alert(&self, alert: &Alert);
}

pub struct AlertEngine {
    db: Arc<Db>,
    sink: Arc<dyn AlertSink>,
    recent: Mutex<VecDeque<Alert>>,
    tasks: Mutex<HashMap<String, Vec<AbortHandle>>>,
    dedupe: Mutex<HashMap<String, Instant>>,
}

const POD_CRITICAL: &[&str] = &[
    "BackOff",
    "Failed",
    "OOMKilling",
    "Evicted",
    "ErrImagePull",
    "ImagePullBackOff",
    "CrashLoopBackOff",
];
const POD_WARNING: &[&str] = &[
    "Unhealthy",
    "FailedMount",
    "FailedAttachVolume",
    "FailedScheduling",
    "FailedCreatePodSandBox",
    "InspectFailed",
    "FailedKillPod",
];
const WORKLOAD_KINDS: &[&str] = &[
    "Deployment",
    "ReplicaSet",
    "StatefulSet",
    "DaemonSet",
    "Job",
    "CronJob",
    "HorizontalPodAutoscaler",
];

/// Classifies a Warning event. Returns (category, severity, title).
pub fn classify(kind: &str, reason: &str, name: &str) -> (&'static str, &'static str, String) {
    if kind == "Pod" {
        if POD_CRITICAL.contains(&reason) {
            let title = match reason {
                "BackOff" | "CrashLoopBackOff" => format!("Pod {name} is crash looping"),
                "OOMKilling" => format!("Pod {name} was OOM killed"),
                "ErrImagePull" | "ImagePullBackOff" => format!("Pod {name} cannot pull its image"),
                "Evicted" => format!("Pod {name} was evicted"),
                _ => format!("Pod {name} failed"),
            };
            return ("pod", "critical", title);
        }
        if POD_WARNING.contains(&reason) {
            return ("pod", "warning", format!("Pod {name}: {reason}"));
        }
    }
    if WORKLOAD_KINDS.contains(&kind) {
        let sev = if reason.starts_with("Failed")
            || reason == "ProgressDeadlineExceeded"
            || reason == "BackoffLimitExceeded"
        {
            "critical"
        } else {
            "warning"
        };
        return ("workload", sev, format!("{kind} {name}: {reason}"));
    }
    if kind == "Node" {
        return ("node", "warning", format!("Node {name}: {reason}"));
    }
    ("event", "warning", format!("{kind} {name}: {reason}"))
}

pub fn rule_matches(
    rule: &AlertRule,
    cluster_id: &str,
    namespace: Option<&str>,
    kind: &str,
    reason: &str,
    message: &str,
) -> bool {
    if !rule.enabled {
        return false;
    }
    if rule.cluster_id.as_deref().is_some_and(|c| c != cluster_id) {
        return false;
    }
    if rule
        .namespace
        .as_deref()
        .filter(|n| !n.is_empty())
        .is_some_and(|n| Some(n) != namespace)
    {
        return false;
    }
    if rule
        .kind
        .as_deref()
        .filter(|k| !k.is_empty())
        .is_some_and(|k| !k.eq_ignore_ascii_case(kind))
    {
        return false;
    }
    let re_match = |pattern: &Option<String>, text: &str| -> bool {
        match pattern.as_deref().filter(|p| !p.is_empty()) {
            None => true,
            Some(p) => regex::RegexBuilder::new(p)
                .size_limit(1 << 20)
                .build()
                .map(|r| r.is_match(text))
                .unwrap_or(false),
        }
    };
    re_match(&rule.reason_pattern, reason) && re_match(&rule.message_pattern, message)
}

fn event_age_secs(ev: &Event) -> Option<i64> {
    let ts = ev
        .last_timestamp
        .as_ref()
        .map(|t| t.0)
        .or_else(|| ev.event_time.as_ref().map(|t| t.0))
        .or_else(|| ev.metadata.creation_timestamp.as_ref().map(|t| t.0))?;
    let now = k8s_openapi::jiff::Timestamp::now();
    Some(now.as_second() - ts.as_second())
}

impl AlertEngine {
    pub fn new(db: Arc<Db>, sink: Arc<dyn AlertSink>) -> Arc<Self> {
        Arc::new(Self {
            db,
            sink,
            recent: Mutex::new(VecDeque::new()),
            tasks: Mutex::new(HashMap::new()),
            dedupe: Mutex::new(HashMap::new()),
        })
    }

    pub fn recent(&self) -> Vec<Alert> {
        self.recent.lock().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.recent.lock().clear();
    }

    /// Records and forwards an alert (also used for connection alerts).
    pub fn emit(&self, alert: Alert, dedupe_key: Option<String>) {
        if let Some(key) = dedupe_key {
            let mut d = self.dedupe.lock();
            let now = Instant::now();
            d.retain(|_, t| now.duration_since(*t) < DEDUPE_WINDOW);
            if d.contains_key(&key) {
                return;
            }
            d.insert(key, now);
        }
        {
            let mut r = self.recent.lock();
            r.push_front(alert.clone());
            r.truncate(MAX_RECENT);
        }
        self.sink.alert(&alert);
    }

    pub fn stop(&self, cluster_id: &str) {
        if let Some(handles) = self.tasks.lock().remove(cluster_id) {
            for h in handles {
                h.abort();
            }
        }
    }

    pub fn is_running(&self, cluster_id: &str) -> bool {
        self.tasks.lock().contains_key(cluster_id)
    }

    pub fn start(self: &Arc<Self>, cluster_id: &str, cluster_name: &str, client: Client) {
        self.stop(cluster_id);
        let events = tokio::spawn(Self::watch_events(
            Arc::downgrade(self),
            cluster_id.into(),
            cluster_name.into(),
            client.clone(),
        ));
        let nodes = tokio::spawn(Self::watch_nodes(
            Arc::downgrade(self),
            cluster_id.into(),
            cluster_name.into(),
            client,
        ));
        self.tasks.lock().insert(
            cluster_id.into(),
            vec![events.abort_handle(), nodes.abort_handle()],
        );
    }

    fn new_alert(
        cluster_id: &str,
        cluster_name: &str,
        severity: &str,
        category: &str,
        title: String,
        message: String,
    ) -> Alert {
        Alert {
            id: uuid::Uuid::new_v4().to_string(),
            ts: chrono::Utc::now().to_rfc3339(),
            cluster_id: cluster_id.into(),
            cluster_name: cluster_name.into(),
            severity: severity.into(),
            category: category.into(),
            title,
            message,
            namespace: None,
            object_kind: None,
            object_name: None,
            rule_id: None,
        }
    }

    async fn watch_events(
        engine: std::sync::Weak<Self>,
        cluster_id: String,
        cluster_name: String,
        client: Client,
    ) {
        let api: Api<Event> = Api::all(client);
        let cfg = watcher::Config::default().fields("type=Warning");
        let mut stream = watcher(api, cfg).default_backoff().boxed();
        let mut rules: (Vec<AlertRule>, Instant) =
            (Vec::new(), Instant::now() - Duration::from_secs(3600));
        while let Some(ev) = stream.next().await {
            let Some(engine) = engine.upgrade() else {
                return;
            };
            if let Err(e) = &ev {
                if is_permission_error(e) {
                    drop(engine);
                    tokio::time::sleep(super::watch::PERMISSION_RETRY).await;
                    continue;
                }
            }
            // Only live updates – the initial list contains historic events.
            let Ok(watcher::Event::Apply(ev)) = ev else {
                continue;
            };
            if event_age_secs(&ev).is_some_and(|age| age > MAX_EVENT_AGE_SECS) {
                continue;
            }
            if rules.1.elapsed() > Duration::from_secs(15) {
                rules = (engine.db.list_rules().unwrap_or_default(), Instant::now());
            }
            let obj = &ev.involved_object;
            let kind = obj.kind.clone().unwrap_or_default();
            let name = obj.name.clone().unwrap_or_default();
            let ns = obj.namespace.clone();
            let reason = ev.reason.clone().unwrap_or_default();
            let message = ev.message.clone().unwrap_or_default();
            let uid = obj.uid.clone().unwrap_or_else(|| format!("{kind}/{name}"));

            for rule in rules
                .0
                .iter()
                .filter(|r| rule_matches(r, &cluster_id, ns.as_deref(), &kind, &reason, &message))
            {
                let mut alert = Self::new_alert(
                    &cluster_id,
                    &cluster_name,
                    &rule.severity,
                    "rule",
                    format!("{}: {kind} {name}", rule.name),
                    message.clone(),
                );
                alert.namespace = ns.clone();
                alert.object_kind = Some(kind.clone());
                alert.object_name = Some(name.clone());
                alert.rule_id = Some(rule.id.clone());
                engine.emit(
                    alert,
                    Some(format!("rule|{}|{cluster_id}|{uid}|{reason}", rule.id)),
                );
            }

            let (category, severity, title) = classify(&kind, &reason, &name);
            let mut alert = Self::new_alert(
                &cluster_id,
                &cluster_name,
                severity,
                category,
                title,
                message,
            );
            alert.namespace = ns;
            alert.object_kind = Some(kind);
            alert.object_name = Some(name);
            engine.emit(alert, Some(format!("{cluster_id}|{uid}|{reason}")));
        }
    }

    async fn watch_nodes(
        engine: std::sync::Weak<Self>,
        cluster_id: String,
        cluster_name: String,
        client: Client,
    ) {
        let api: Api<Node> = Api::all(client);
        let mut stream = watcher(api, watcher::Config::default())
            .default_backoff()
            .boxed();
        // node name -> (ready, pressures)
        let mut state: HashMap<String, (bool, Vec<String>)> = HashMap::new();
        let mut initialised = false;
        while let Some(ev) = stream.next().await {
            let Some(engine) = engine.upgrade() else {
                return;
            };
            let node = match ev {
                Err(e) if is_permission_error(&e) => {
                    // No permission to list nodes (e.g. an agent with the "edit" role).
                    drop(engine);
                    tokio::time::sleep(super::watch::PERMISSION_RETRY).await;
                    continue;
                }
                Ok(watcher::Event::InitApply(n)) => {
                    if let Some((name, s)) = node_state(&n) {
                        state.insert(name, s);
                    }
                    continue;
                }
                Ok(watcher::Event::InitDone) => {
                    initialised = true;
                    continue;
                }
                Ok(watcher::Event::Apply(n)) => n,
                Ok(watcher::Event::Delete(n)) => {
                    if let Some(name) = n.metadata.name {
                        state.remove(&name);
                    }
                    continue;
                }
                _ => continue,
            };
            let Some((name, (ready, pressures))) = node_state(&node) else {
                continue;
            };
            let prev = state.insert(name.clone(), (ready, pressures.clone()));
            if !initialised {
                continue;
            }
            let (was_ready, prev_pressures) = prev.unwrap_or((true, vec![]));
            if was_ready && !ready {
                let mut a = Self::new_alert(
                    &cluster_id,
                    &cluster_name,
                    "critical",
                    "node",
                    format!("Node {name} is NotReady"),
                    "The node stopped reporting Ready.".into(),
                );
                a.object_kind = Some("Node".into());
                a.object_name = Some(name.clone());
                engine.emit(a, Some(format!("{cluster_id}|node|{name}|notready")));
            } else if !was_ready && ready {
                let mut a = Self::new_alert(
                    &cluster_id,
                    &cluster_name,
                    "info",
                    "node",
                    format!("Node {name} is Ready again"),
                    String::new(),
                );
                a.object_kind = Some("Node".into());
                a.object_name = Some(name.clone());
                engine.emit(a, None);
            }
            for p in pressures.iter().filter(|p| !prev_pressures.contains(p)) {
                let mut a = Self::new_alert(
                    &cluster_id,
                    &cluster_name,
                    "warning",
                    "node",
                    format!("Node {name}: {p}"),
                    format!("The node reports {p}."),
                );
                a.object_kind = Some("Node".into());
                a.object_name = Some(name.clone());
                engine.emit(a, Some(format!("{cluster_id}|node|{name}|{p}")));
            }
        }
    }
}

fn node_state(n: &Node) -> Option<(String, (bool, Vec<String>))> {
    let name = n.metadata.name.clone()?;
    let conds = n.status.as_ref().and_then(|s| s.conditions.as_ref());
    let ready = conds
        .map(|c| c.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
        .unwrap_or(false);
    let pressures = conds
        .map(|c| {
            c.iter()
                .filter(|c| c.type_ != "Ready" && c.status == "True")
                .map(|c| c.type_.clone())
                .collect()
        })
        .unwrap_or_default();
    Some((name, (ready, pressures)))
}

/// 401/403 don't heal by retrying quickly; callers pause instead of hammering the API server.
fn is_permission_error(e: &watcher::Error) -> bool {
    matches!(super::watch::status_of(e), Some(401 | 403))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_events() {
        assert_eq!(classify("Pod", "BackOff", "web").1, "critical");
        assert_eq!(classify("Pod", "Unhealthy", "web").1, "warning");
        assert_eq!(
            classify("Deployment", "ProgressDeadlineExceeded", "api").0,
            "workload"
        );
        assert_eq!(
            classify("Deployment", "ProgressDeadlineExceeded", "api").1,
            "critical"
        );
        assert_eq!(
            classify("Service", "SyncLoadBalancerFailed", "lb").0,
            "event"
        );
    }

    #[test]
    fn rule_matching() {
        let rule = AlertRule {
            id: "r".into(),
            name: "oom".into(),
            enabled: true,
            cluster_id: None,
            namespace: Some("prod".into()),
            kind: Some("pod".into()),
            reason_pattern: Some("^OOM".into()),
            message_pattern: None,
            severity: "critical".into(),
            created_at: String::new(),
        };
        assert!(rule_matches(
            &rule,
            "c",
            Some("prod"),
            "Pod",
            "OOMKilling",
            "x"
        ));
        assert!(!rule_matches(
            &rule,
            "c",
            Some("dev"),
            "Pod",
            "OOMKilling",
            "x"
        ));
        assert!(!rule_matches(
            &rule,
            "c",
            Some("prod"),
            "Pod",
            "BackOff",
            "x"
        ));
        let disabled = AlertRule {
            enabled: false,
            ..rule
        };
        assert!(!rule_matches(
            &disabled,
            "c",
            Some("prod"),
            "Pod",
            "OOMKilling",
            "x"
        ));
    }

    struct Collect(Mutex<Vec<Alert>>);
    impl AlertSink for Collect {
        fn alert(&self, alert: &Alert) {
            self.0.lock().push(alert.clone());
        }
    }

    #[test]
    fn deduplicates() {
        let sink = Arc::new(Collect(Mutex::new(vec![])));
        let engine = AlertEngine::new(Arc::new(Db::open_in_memory().unwrap()), sink.clone());
        let a = AlertEngine::new_alert("c", "c", "critical", "pod", "t".into(), "m".into());
        engine.emit(a.clone(), Some("k".into()));
        engine.emit(a.clone(), Some("k".into()));
        engine.emit(a, None);
        assert_eq!(sink.0.lock().len(), 2);
        assert_eq!(engine.recent().len(), 2);
    }
}
