//! Bridges events from the Kubernetes layer to the webview and to native OS
//! notifications.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::k8s::alerts::{Alert, AlertSink};
use crate::k8s::connection::ClusterStatus;
use crate::k8s::EventSink;
use crate::persistence::Db;

pub const EVT_CLUSTER_STATUS: &str = "cluster://status";
pub const EVT_ALERT: &str = "alerts://new";
pub const EVT_NOTIFY: &str = "notify://message";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotifyPayload<'a> {
    severity: &'a str,
    title: &'a str,
    body: &'a str,
    cluster_id: Option<&'a str>,
}

pub struct TauriSink {
    app: AppHandle,
    db: Arc<Db>,
    /// Sliding window used to rate-limit native notifications (max 6 / 30 s).
    native_window: Mutex<VecDeque<Instant>>,
}

impl TauriSink {
    pub fn new(app: AppHandle, db: Arc<Db>) -> Arc<Self> {
        Arc::new(Self {
            app,
            db,
            native_window: Mutex::new(VecDeque::new()),
        })
    }

    fn allow_native(&self) -> bool {
        let mut w = self.native_window.lock();
        let now = Instant::now();
        while w
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(30))
        {
            w.pop_front();
        }
        if w.len() >= 6 {
            return false;
        }
        w.push_back(now);
        true
    }

    fn native(&self, title: &str, body: &str) {
        if !self.allow_native() {
            return;
        }
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            tracing::debug!(error = %e, "native notification failed");
        }
    }
}

impl EventSink for TauriSink {
    fn cluster_status(&self, status: &ClusterStatus) {
        let _ = self.app.emit(EVT_CLUSTER_STATUS, status);
        super::tray::refresh(&self.app);
    }

    fn notify(&self, severity: &str, title: &str, body: &str, cluster_id: Option<&str>) {
        let _ = self.app.emit(
            EVT_NOTIFY,
            NotifyPayload {
                severity,
                title,
                body,
                cluster_id,
            },
        );
        let settings = self.db.load_settings().unwrap_or_default();
        if settings.native_notifications && settings.notify_connection_loss {
            self.native(title, body);
        }
    }
}

impl AlertSink for TauriSink {
    fn alert(&self, alert: &Alert) {
        let _ = self.app.emit(EVT_ALERT, alert);
        let s = self.db.load_settings().unwrap_or_default();
        if !s.native_notifications || alert.severity == "info" {
            return;
        }
        let wanted = match alert.category.as_str() {
            "pod" => s.notify_pod_failures,
            "workload" => s.notify_deployment_failures,
            "node" => s.notify_node_problems,
            "event" => s.notify_warning_events,
            "connection" => s.notify_connection_loss,
            _ => true, // user-defined rules always notify
        };
        if !wanted {
            return;
        }
        // Don't interrupt the user with OS toasts while they are looking at the app,
        // unless it is critical.
        let focused = self
            .app
            .webview_windows()
            .values()
            .any(|w| w.is_focused().unwrap_or(false));
        if focused && alert.severity != "critical" {
            return;
        }
        let body = if alert.message.is_empty() {
            alert.cluster_name.clone()
        } else {
            format!("[{}] {}", alert.cluster_name, alert.message)
        };
        self.native(&alert.title, &body);
    }
}
