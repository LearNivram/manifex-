//! Kubernetes integration layer.

pub mod agent_install;
pub mod alerts;
pub mod connection;
pub mod exec;
pub mod helm;
pub mod insights;
pub mod kubeconfig;
pub mod logs;
pub mod metrics;
pub mod ops;
pub mod quantity;
pub mod rbac;
pub mod resources;
pub mod schema;
pub mod ssh;
pub mod summary;
pub mod watch;

use connection::ClusterStatus;

/// Outbound events from the integration layer to the desktop shell. Implemented
/// by the Tauri layer (webview events + native notifications) and by test sinks.
pub trait EventSink: Send + Sync {
    fn cluster_status(&self, status: &ClusterStatus);
    fn notify(&self, severity: &str, title: &str, body: &str, cluster_id: Option<&str>);
}

/// Sink that drops everything (used in tests).
pub struct NullSink;

impl EventSink for NullSink {
    fn cluster_status(&self, _status: &ClusterStatus) {}
    fn notify(&self, _severity: &str, _title: &str, _body: &str, _cluster_id: Option<&str>) {}
}
