//! Interactive container shells via the Kubernetes exec API (WebSocket).

use std::collections::HashMap;
use std::sync::Arc;

use futures::SinkExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{AttachParams, TerminalSize};
use kube::{Api, Client};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::error::{AppError, AppResult};
use crate::security::validate;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExecEvent {
    Exit {
        code: Option<i32>,
        message: Option<String>,
    },
    Error {
        message: String,
    },
}

enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
}

struct Session {
    tx: mpsc::Sender<Input>,
    task: AbortHandle,
    cluster_id: String,
}

#[derive(Default)]
pub struct ExecManager {
    sessions: Mutex<HashMap<String, Session>>,
}

pub struct ExecTarget<'a> {
    pub cluster_id: &'a str,
    pub namespace: &'a str,
    pub pod: &'a str,
    pub container: Option<&'a str>,
    pub shell: &'a str,
    pub cols: u16,
    pub rows: u16,
}

impl ExecManager {
    pub async fn start(
        self: &Arc<Self>,
        client: Client,
        target: ExecTarget<'_>,
        on_data: impl Fn(Vec<u8>) -> bool + Send + Sync + 'static,
        on_event: impl Fn(ExecEvent) + Send + Sync + 'static,
    ) -> AppResult<String> {
        validate::namespace(target.namespace)?;
        validate::object_name(target.pod)?;
        let command = validate::shell(target.shell)?;
        let mut ap = AttachParams::interactive_tty();
        if let Some(c) = target.container.filter(|c| !c.is_empty()) {
            validate::object_name(c)?;
            ap = ap.container(c);
        }
        let api: Api<Pod> = Api::namespaced(client, target.namespace);
        let mut attached = api.exec(target.pod, command, &ap).await?;
        let mut stdin = attached
            .stdin()
            .ok_or_else(|| AppError::internal("exec stdin unavailable"))?;
        let mut stdout = attached
            .stdout()
            .ok_or_else(|| AppError::internal("exec stdout unavailable"))?;
        let mut resize = attached.terminal_size();
        if let Some(r) = resize.as_mut() {
            let _ = r
                .send(TerminalSize {
                    width: target.cols.max(10),
                    height: target.rows.max(2),
                })
                .await;
        }
        let status = attached.take_status();

        let (tx, mut rx) = mpsc::channel::<Input>(256);
        let id = uuid::Uuid::new_v4().to_string();
        let mgr = Arc::downgrade(self);
        let session_id = id.clone();
        let task = tokio::spawn(async move {
            // The AttachedProcess owns the WebSocket task; it must live as long as
            // the session (dropping it closes the connection).
            let attached = attached;
            let mut buf = vec![0u8; 16 * 1024];
            let mut stdin_open = true;
            loop {
                tokio::select! {
                    read = stdout.read(&mut buf) => match read {
                        Ok(0) => break,
                        Ok(n) => {
                            if !on_data(buf[..n].to_vec()) { break; }
                        }
                        Err(e) => {
                            on_event(ExecEvent::Error { message: e.to_string() });
                            break;
                        }
                    },
                    input = rx.recv(), if stdin_open => match input {
                        Some(Input::Data(bytes)) => {
                            if stdin.write_all(&bytes).await.is_err() { break; }
                        }
                        Some(Input::Resize(w, h)) => {
                            if let Some(r) = resize.as_mut() {
                                let _ = r.send(TerminalSize { width: w, height: h }).await;
                            }
                        }
                        None => { stdin_open = false; }
                    }
                }
            }
            drop(stdin);
            drop(stdout);
            let mut code = None;
            let mut message = None;
            if let Some(status) = status {
                if let Ok(Some(st)) =
                    tokio::time::timeout(std::time::Duration::from_secs(2), status).await
                {
                    if st.status.as_deref() == Some("Success") {
                        code = Some(0);
                    } else {
                        message = st.message.clone();
                        code = st
                            .details
                            .as_ref()
                            .and_then(|d| d.causes.as_ref())
                            .and_then(|c| {
                                c.iter().find(|c| c.reason.as_deref() == Some("ExitCode"))
                            })
                            .and_then(|c| c.message.as_ref())
                            .and_then(|m| m.parse().ok());
                    }
                }
            }
            drop(attached);
            on_event(ExecEvent::Exit { code, message });
            if let Some(m) = mgr.upgrade() {
                m.sessions.lock().remove(&session_id);
            }
        });
        self.sessions.lock().insert(
            id.clone(),
            Session {
                tx,
                task: task.abort_handle(),
                cluster_id: target.cluster_id.to_string(),
            },
        );
        Ok(id)
    }

    fn sender(&self, id: &str) -> AppResult<mpsc::Sender<Input>> {
        self.sessions
            .lock()
            .get(id)
            .map(|s| s.tx.clone())
            .ok_or_else(|| AppError::NotFound(format!("terminal session {id}")))
    }

    pub async fn write(&self, id: &str, data: Vec<u8>) -> AppResult<()> {
        if data.len() > 1024 * 1024 {
            return Err(AppError::validation("input chunk too large"));
        }
        self.sender(id)?
            .send(Input::Data(data))
            .await
            .map_err(|_| AppError::Cancelled)
    }

    pub async fn resize(&self, id: &str, cols: u16, rows: u16) -> AppResult<()> {
        self.sender(id)?
            .send(Input::Resize(cols.clamp(10, 1000), rows.clamp(2, 500)))
            .await
            .map_err(|_| AppError::Cancelled)
    }

    pub fn close(&self, id: &str) {
        if let Some(s) = self.sessions.lock().remove(id) {
            s.task.abort();
        }
    }

    pub fn close_cluster(&self, cluster_id: &str) {
        self.sessions.lock().retain(|_, s| {
            if s.cluster_id == cluster_id {
                s.task.abort();
                false
            } else {
                true
            }
        });
    }

    pub fn count(&self) -> usize {
        self.sessions.lock().len()
    }
}
