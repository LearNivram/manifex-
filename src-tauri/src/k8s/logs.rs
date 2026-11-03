//! Container log streaming.

use std::collections::HashMap;

use futures::{AsyncBufReadExt, StreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::task::AbortHandle;

use crate::error::AppResult;
use crate::security::validate;

const MAX_LINE: usize = 64 * 1024;
const MAX_BATCH: usize = 1000;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogOptions {
    pub container: Option<String>,
    pub follow: bool,
    pub tail_lines: Option<i64>,
    pub since_seconds: Option<i64>,
    pub timestamps: bool,
    pub previous: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LogMessage {
    Lines { lines: Vec<String> },
    End,
    Error { message: String },
}

pub struct LogManager {
    streams: Mutex<HashMap<String, AbortHandle>>,
}

impl Default for LogManager {
    fn default() -> Self {
        Self { streams: Mutex::new(HashMap::new()) }
    }
}

impl LogManager {
    pub async fn start(
        &self,
        client: Client,
        namespace: &str,
        pod: &str,
        opts: LogOptions,
        sink: impl Fn(LogMessage) -> bool + Send + Sync + 'static,
    ) -> AppResult<String> {
        validate::namespace(namespace)?;
        validate::object_name(pod)?;
        if let Some(c) = &opts.container {
            validate::object_name(c)?;
        }
        let api: Api<Pod> = Api::namespaced(client, namespace);
        let lp = LogParams {
            container: opts.container.clone(),
            follow: opts.follow,
            tail_lines: opts.tail_lines.map(|t| t.clamp(1, 100_000)),
            since_seconds: opts.since_seconds.filter(|s| *s > 0),
            timestamps: opts.timestamps,
            previous: opts.previous,
            ..Default::default()
        };
        // Open the stream eagerly so errors (e.g. container not found) surface to the caller.
        let reader = api.log_stream(pod, &lp).await?;
        let id = uuid::Uuid::new_v4().to_string();
        let task = tokio::spawn(async move {
            let mut chunks = reader.lines().ready_chunks(MAX_BATCH);
            while let Some(chunk) = chunks.next().await {
                let mut lines = Vec::with_capacity(chunk.len());
                let mut error = None;
                for line in chunk {
                    match line {
                        Ok(mut l) => {
                            if l.len() > MAX_LINE {
                                let mut cut = MAX_LINE;
                                while !l.is_char_boundary(cut) {
                                    cut -= 1;
                                }
                                l.truncate(cut);
                                l.push_str(" …[truncated]");
                            }
                            lines.push(l);
                        }
                        Err(e) => error = Some(e.to_string()),
                    }
                }
                if !lines.is_empty() && !sink(LogMessage::Lines { lines }) {
                    return;
                }
                if let Some(message) = error {
                    sink(LogMessage::Error { message });
                    return;
                }
            }
            sink(LogMessage::End);
        });
        self.streams.lock().insert(id.clone(), task.abort_handle());
        Ok(id)
    }

    pub fn stop(&self, id: &str) {
        if let Some(h) = self.streams.lock().remove(id) {
            h.abort();
        }
    }

    pub fn stop_all(&self) {
        for (_, h) in self.streams.lock().drain() {
            h.abort();
        }
    }
}
