//! Helm commands.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::error::AppResult;
use crate::k8s::helm::{self, HelmCli, HelmKubeconfig, HelmOutput, HelmRelease, HelmReleaseDetail};
use crate::state::{AppState, AuditTarget};

fn cli(state: &AppState) -> AppResult<HelmCli> {
    let settings = state.db.load_settings()?;
    HelmCli::locate(&settings.helm_path)
}

fn kubeconfig_for(state: &AppState, cluster_id: &str) -> AppResult<HelmKubeconfig> {
    // Helm operations require an active connection so the user sees which
    // cluster they act on, and the same credentials are used.
    let conn = state.conn(cluster_id)?;
    if let Some(ssh) = &conn.ssh {
        return HelmKubeconfig::from_yaml(&ssh.kubeconfig_yaml, &ssh.context);
    }
    let profile = state.db.get_cluster(cluster_id)?;
    let secret = state.clusters.load_secret(&profile.source)?;
    HelmKubeconfig::for_source(&profile.source, secret)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmStatus {
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn helm_status(state: State<'_, AppState>) -> AppResult<HelmStatus> {
    match cli(&state) {
        Ok(c) => match c.version().await {
            Ok(v) => Ok(HelmStatus {
                available: true,
                version: Some(v),
                path: Some(c.path().to_string_lossy().into()),
                message: None,
            }),
            Err(e) => Ok(HelmStatus {
                available: false,
                version: None,
                path: Some(c.path().to_string_lossy().into()),
                message: Some(e.to_string()),
            }),
        },
        Err(e) => Ok(HelmStatus {
            available: false,
            version: None,
            path: None,
            message: Some(e.to_string()),
        }),
    }
}

#[tauri::command]
pub async fn helm_releases(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: Option<String>,
) -> AppResult<Vec<HelmRelease>> {
    let conn = state.conn(&cluster_id)?;
    helm::list_releases(
        conn.client.clone(),
        namespace.as_deref().filter(|n| !n.is_empty()),
    )
    .await
}

#[tauri::command]
pub async fn helm_release(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
) -> AppResult<HelmReleaseDetail> {
    let conn = state.conn(&cluster_id)?;
    helm::release_detail(conn.client.clone(), &namespace, &name).await
}

#[tauri::command]
pub async fn helm_rollback(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
    revision: i64,
) -> AppResult<HelmOutput> {
    let helm = cli(&state)?;
    let kc = kubeconfig_for(&state, &cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "helm.rollback",
                resource: Some("HelmRelease".into()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: Some(format!("to revision {revision}")),
            },
            helm.rollback(&kc, &namespace, &name, revision),
        )
        .await
}

#[tauri::command]
pub async fn helm_uninstall(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
    keep_history: bool,
    confirm: Option<String>,
) -> AppResult<HelmOutput> {
    state.check_production_confirmation(&cluster_id, &name, confirm.as_deref())?;
    let helm = cli(&state)?;
    let kc = kubeconfig_for(&state, &cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "helm.uninstall",
                resource: Some("HelmRelease".into()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: None,
            },
            helm.uninstall(&kc, &namespace, &name, keep_history),
        )
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmUpgradeRequest {
    pub namespace: String,
    pub name: String,
    pub chart: String,
    pub version: Option<String>,
    pub values: Option<String>,
    #[serde(default)]
    pub create_namespace: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[tauri::command]
pub async fn helm_upgrade(
    state: State<'_, AppState>,
    cluster_id: String,
    request: HelmUpgradeRequest,
) -> AppResult<HelmOutput> {
    let helm = cli(&state)?;
    let kc = kubeconfig_for(&state, &cluster_id)?;
    let run = helm.upgrade_install(
        &kc,
        &request.namespace,
        &request.name,
        &request.chart,
        request.version.as_deref(),
        request.values.as_deref(),
        request.create_namespace,
        request.dry_run,
    );
    if request.dry_run {
        return run.await;
    }
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "helm.upgrade",
                resource: Some("HelmRelease".into()),
                namespace: Some(request.namespace.clone()),
                name: Some(request.name.clone()),
                detail: Some(format!(
                    "{} {}",
                    request.chart,
                    request.version.clone().unwrap_or_default()
                )),
            },
            run,
        )
        .await
}

#[tauri::command]
pub async fn helm_repos(state: State<'_, AppState>) -> AppResult<Value> {
    cli(&state)?.repo_list().await
}

#[tauri::command]
pub async fn helm_repo_add(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> AppResult<HelmOutput> {
    cli(&state)?.repo_add(&name, &url).await
}

#[tauri::command]
pub async fn helm_repo_remove(state: State<'_, AppState>, name: String) -> AppResult<HelmOutput> {
    cli(&state)?.repo_remove(&name).await
}

#[tauri::command]
pub async fn helm_repo_update(state: State<'_, AppState>) -> AppResult<HelmOutput> {
    cli(&state)?.repo_update().await
}

#[tauri::command]
pub async fn helm_search(state: State<'_, AppState>, query: String) -> AppResult<Value> {
    cli(&state)?.search(&query).await
}

#[tauri::command]
pub async fn helm_show_values(
    state: State<'_, AppState>,
    chart: String,
    version: Option<String>,
) -> AppResult<String> {
    cli(&state)?.show_values(&chart, version.as_deref()).await
}
