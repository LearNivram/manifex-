//! One-click agent installation / update / removal.

use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::k8s::agent_install::{self, AgentInstallOptions, AgentInstallResult, AgentStatus};
use crate::persistence::audit::NewAudit;
use crate::persistence::clusters::{ClusterProfile, ClusterSource};
use crate::state::{AppState, AuditTarget};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallResponse {
    pub result: AgentInstallResult,
    /// The agent connection profile that was created.
    pub profile: ClusterProfile,
}

#[tauri::command]
pub async fn agent_status(
    state: State<'_, AppState>,
    cluster_id: String,
) -> AppResult<AgentStatus> {
    let conn = state.conn(&cluster_id)?;
    agent_install::status(conn.client.clone()).await
}

#[tauri::command]
pub async fn agent_install(
    app: AppHandle,
    state: State<'_, AppState>,
    cluster_id: String,
    options: AgentInstallOptions,
) -> AppResult<AgentInstallResponse> {
    let conn = state.conn(&cluster_id)?;
    let source_profile = state.db.get_cluster(&cluster_id)?;
    let result = state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "agent.install",
                resource: Some("Deployment".into()),
                namespace: Some(agent_install::NAMESPACE.into()),
                name: Some(agent_install::NAME.into()),
                detail: Some(format!(
                    "role={} exposure={}",
                    options.role, options.exposure
                )),
            },
            agent_install::install(conn.client.clone(), &options),
        )
        .await?;

    // Ready-to-use agent profile; token in the vault, CA in the profile.
    let secret_id = format!("cred-{}", uuid::Uuid::new_v4());
    state
        .db
        .put_secret(&state.vault, &secret_id, result.token.as_bytes())?;
    let profile = ClusterProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: format!("{} (agent)", source_profile.name),
        source: ClusterSource::Agent {
            endpoint: result.endpoint.clone(),
            ca_pem: Some(result.ca_pem.clone()),
            proxy_url: None,
            secret_id: secret_id.clone(),
        },
        group: source_profile.group.clone(),
        favorite: false,
        color: source_profile.color.clone(),
        environment: source_profile.environment.clone(),
        default_namespace: source_profile.default_namespace.clone(),
        alerts_enabled: true,
        sort_order: source_profile.sort_order,
        created_at: chrono::Utc::now().to_rfc3339(),
        last_connected_at: None,
    };
    if let Err(e) = state.db.insert_cluster(&profile) {
        let _ = state.db.delete_secret(&secret_id);
        return Err(e);
    }
    state.db.add_audit(NewAudit {
        cluster_id: Some(profile.id.clone()),
        cluster_name: Some(profile.name.clone()),
        action: "cluster.add".into(),
        outcome: "success".into(),
        detail: Some(format!("agent {}", result.endpoint)),
        ..Default::default()
    })?;
    crate::desktop::tray::refresh(&app);
    Ok(AgentInstallResponse { result, profile })
}

#[tauri::command]
pub async fn agent_update(
    state: State<'_, AppState>,
    cluster_id: String,
    version: Option<String>,
) -> AppResult<AgentStatus> {
    let conn = state.conn(&cluster_id)?;
    let target = version
        .clone()
        .unwrap_or_else(|| agent_install::AGENT_VERSION.to_string());
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "agent.update",
                resource: Some("Deployment".into()),
                namespace: Some(agent_install::NAMESPACE.into()),
                name: Some(agent_install::NAME.into()),
                detail: Some(format!("to {target}")),
            },
            agent_install::update(conn.client.clone(), version.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn agent_uninstall(
    state: State<'_, AppState>,
    cluster_id: String,
    confirm: Option<String>,
) -> AppResult<()> {
    state.check_production_confirmation(&cluster_id, agent_install::NAME, confirm.as_deref())?;
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "agent.uninstall",
                resource: Some("Deployment".into()),
                namespace: Some(agent_install::NAMESPACE.into()),
                name: Some(agent_install::NAME.into()),
                detail: None,
            },
            agent_install::uninstall(conn.client.clone()),
        )
        .await
}
