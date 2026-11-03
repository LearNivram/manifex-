//! Application state shared by all IPC commands.

use std::future::Future;
use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::k8s::alerts::AlertEngine;
use crate::k8s::connection::{ClusterConn, ClusterManager};
use crate::k8s::exec::ExecManager;
use crate::k8s::logs::LogManager;
use crate::k8s::watch::WatchManager;
use crate::persistence::audit::NewAudit;
use crate::persistence::Db;
use crate::security::vault::SecretVault;

pub struct AppState {
    pub db: Arc<Db>,
    pub vault: Arc<SecretVault>,
    pub clusters: Arc<ClusterManager>,
    pub watches: Arc<WatchManager>,
    pub logs: Arc<LogManager>,
    pub exec: Arc<ExecManager>,
    pub alerts: Arc<AlertEngine>,
}

pub struct AuditTarget<'a> {
    pub action: &'a str,
    pub resource: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub detail: Option<String>,
}

impl AppState {
    pub fn conn(&self, cluster_id: &str) -> AppResult<Arc<ClusterConn>> {
        self.clusters.get(cluster_id)
    }

    /// Runs a mutating operation and records it in the local audit log together
    /// with its outcome.
    pub async fn audited<T, F>(
        &self,
        cluster_id: &str,
        target: AuditTarget<'_>,
        fut: F,
    ) -> AppResult<T>
    where
        F: Future<Output = AppResult<T>>,
    {
        let (cluster_name, identity) = match self.clusters.get(cluster_id) {
            Ok(c) => (Some(c.name()), c.identity()),
            Err(_) => (self.db.get_cluster(cluster_id).ok().map(|p| p.name), None),
        };
        let result = fut.await;
        let (outcome, detail) = match &result {
            Ok(_) => ("success".to_string(), target.detail.clone()),
            Err(e) => (
                "failure".to_string(),
                Some(match &target.detail {
                    Some(d) => format!("{d} – {e}"),
                    None => e.to_string(),
                }),
            ),
        };
        if let Err(e) = self.db.add_audit(NewAudit {
            cluster_id: Some(cluster_id.to_string()),
            cluster_name,
            identity,
            action: target.action.to_string(),
            resource: target.resource,
            namespace: target.namespace,
            name: target.name,
            outcome,
            detail,
        }) {
            tracing::warn!(error = %e, "failed to write audit entry");
        }
        result
    }

    /// Defence in depth for destructive operations on production clusters: the UI
    /// asks the user to type the object name, and the backend verifies it.
    pub fn check_production_confirmation(
        &self,
        cluster_id: &str,
        expected: &str,
        confirm: Option<&str>,
    ) -> AppResult<()> {
        let profile = self.db.get_cluster(cluster_id)?;
        let settings = self.db.load_settings()?;
        if profile.environment == "production"
            && settings.strict_production_confirmations
            && confirm != Some(expected)
        {
            return Err(AppError::Security(format!(
                "'{}' is a production cluster – type '{expected}' to confirm this operation",
                profile.name
            )));
        }
        Ok(())
    }
}
