//! Cluster profile and connection commands.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, AppResult};
use crate::k8s::connection::{ClusterStatus, ConnectionTestResult};
use crate::k8s::kubeconfig::{self, CertificateCredentials, DiscoveredKubeconfig};
use crate::k8s::resources::{self, ApiResourceInfo};
use crate::persistence::audit::NewAudit;
use crate::persistence::clusters::{ClusterPatch, ClusterProfile, ClusterSource, ENVIRONMENTS};
use crate::security::validate;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterView {
    pub profile: ClusterProfile,
    pub status: Option<ClusterStatus>,
}

#[tauri::command]
pub async fn list_clusters(state: State<'_, AppState>) -> AppResult<Vec<ClusterView>> {
    Ok(state
        .db
        .list_clusters()?
        .into_iter()
        .map(|p| {
            let status = state.clusters.status(&p.id);
            ClusterView { profile: p, status }
        })
        .collect())
}

fn mark_added(state: &AppState, mut d: DiscoveredKubeconfig) -> DiscoveredKubeconfig {
    for c in d.contexts.iter_mut() {
        c.already_added = state.db.cluster_exists_for_kubeconfig(&d.path, &c.name).unwrap_or(false);
    }
    d
}

#[tauri::command]
pub async fn discover_kubeconfigs(state: State<'_, AppState>) -> AppResult<Vec<DiscoveredKubeconfig>> {
    let found = tokio::task::spawn_blocking(|| {
        kubeconfig::candidate_paths().into_iter().map(|p| kubeconfig::inspect_path(&p)).collect::<Vec<_>>()
    })
    .await?;
    Ok(found
        .into_iter()
        .filter(|d| !d.contexts.is_empty() || d.error.is_some())
        .map(|d| mark_added(&state, d))
        .collect())
}

/// Opens a native file dialog in the Rust core; the frontend never gets file
/// system access.
#[tauri::command]
pub async fn pick_kubeconfig_file(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<DiscoveredKubeconfig>> {
    let mut builder = app.dialog().file().set_title("Import kubeconfig");
    if let Some(home) = dirs::home_dir() {
        let kube = home.join(".kube");
        builder = builder.set_directory(if kube.is_dir() { kube } else { home });
    }
    let picked = tokio::task::spawn_blocking(move || builder.blocking_pick_file()).await?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(|_| AppError::validation("unsupported file location"))?;
    let d = kubeconfig::inspect_path(&path);
    Ok(Some(mark_added(&state, d)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeconfigImport {
    pub path: String,
    pub context: String,
    pub name: Option<String>,
    /// Copy the context into the encrypted vault instead of referencing the file.
    #[serde(default)]
    pub store_in_vault: bool,
    pub group: Option<String>,
    pub environment: Option<String>,
}

fn new_profile(name: String, source: ClusterSource, group: Option<String>, environment: Option<String>) -> AppResult<ClusterProfile> {
    validate::display_name("cluster name", &name)?;
    let environment = environment.unwrap_or_else(|| "development".into());
    if !ENVIRONMENTS.contains(&environment.as_str()) {
        return Err(AppError::validation("unknown environment"));
    }
    if let Some(g) = &group {
        if !g.is_empty() {
            validate::display_name("group", g)?;
        }
    }
    let lower = name.to_lowercase();
    // Sensible default: names that look like production are flagged as production.
    let environment = if environment == "development" && (lower.contains("prod") || lower.contains("prd")) {
        "production".to_string()
    } else {
        environment
    };
    Ok(ClusterProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        source,
        group: group.filter(|g| !g.trim().is_empty()),
        favorite: false,
        color: None,
        environment,
        default_namespace: None,
        alerts_enabled: true,
        sort_order: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        last_connected_at: None,
    })
}

#[tauri::command]
pub async fn add_kubeconfig_clusters(app: AppHandle, state: State<'_, AppState>, entries: Vec<KubeconfigImport>) -> AppResult<Vec<ClusterProfile>> {
    if entries.is_empty() || entries.len() > 200 {
        return Err(AppError::validation("select between 1 and 200 contexts"));
    }
    let mut created = Vec::new();
    for e in entries {
        let path = std::path::PathBuf::from(&e.path);
        let kc = kubeconfig::read_kubeconfig(&path)?;
        let ctx = kubeconfig::list_contexts(&kc)
            .into_iter()
            .find(|c| c.name == e.context)
            .ok_or_else(|| AppError::NotFound(format!("context '{}'", e.context)))?;
        let name = e.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| e.context.clone());
        let source = if e.store_in_vault {
            let single = kubeconfig::extract_single_context(&kc, &e.context)?;
            let yaml = kubeconfig::kubeconfig_to_yaml_with_secrets(&single)?;
            let secret_id = format!("kubeconfig-{}", uuid::Uuid::new_v4());
            state.db.put_secret(&state.vault, &secret_id, yaml.as_bytes())?;
            ClusterSource::StoredKubeconfig { context: e.context.clone(), secret_id }
        } else {
            if state.db.cluster_exists_for_kubeconfig(&e.path, &e.context)? {
                continue;
            }
            let canonical = path.canonicalize().unwrap_or(path).to_string_lossy().to_string();
            ClusterSource::Kubeconfig { path: canonical, context: e.context.clone() }
        };
        let mut profile = new_profile(name, source, e.group.clone(), e.environment.clone())?;
        profile.default_namespace = ctx.namespace.clone();
        state.db.insert_cluster(&profile)?;
        state.db.add_audit(NewAudit {
            cluster_id: Some(profile.id.clone()),
            cluster_name: Some(profile.name.clone()),
            action: "cluster.add".into(),
            outcome: "success".into(),
            detail: Some(format!("{} ({})", profile.source.kind(), e.context)),
            ..Default::default()
        })?;
        created.push(profile);
    }
    crate::desktop::tray::refresh(&app);
    Ok(created)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectClusterRequest {
    pub name: String,
    /// token | certificate | agent
    pub kind: String,
    pub server: String,
    pub ca_pem: Option<String>,
    pub tls_server_name: Option<String>,
    pub proxy_url: Option<String>,
    pub token: Option<String>,
    pub cert_pem: Option<String>,
    pub key_pem: Option<String>,
    pub group: Option<String>,
    pub environment: Option<String>,
}

impl DirectClusterRequest {
    /// Validates the request and returns (source, secret bytes).
    fn into_parts(self, secret_id: String) -> AppResult<(ClusterSource, Vec<u8>, String, Option<String>, Option<String>)> {
        validate::https_url("server", &self.server)?;
        if let Some(p) = self.proxy_url.as_deref().filter(|p| !p.is_empty()) {
            validate::proxy_url(p)?;
        }
        let ca_pem = self.ca_pem.filter(|c| !c.trim().is_empty());
        if let Some(ca) = &ca_pem {
            kubeconfig::pem_to_b64(ca)?;
        }
        let tls = self.tls_server_name.filter(|s| !s.trim().is_empty());
        let proxy = self.proxy_url.filter(|s| !s.trim().is_empty());
        let (source, secret) = match self.kind.as_str() {
            "token" | "agent" => {
                let token = self.token.unwrap_or_default().trim().to_string();
                if token.is_empty() || token.len() > 16 * 1024 || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err(AppError::validation("a valid bearer token is required"));
                }
                let source = if self.kind == "token" {
                    ClusterSource::Token { server: self.server.trim().into(), ca_pem, tls_server_name: tls, proxy_url: proxy, secret_id }
                } else {
                    ClusterSource::Agent { endpoint: self.server.trim().into(), ca_pem, proxy_url: proxy, secret_id }
                };
                (source, token.into_bytes())
            }
            "certificate" => {
                let cert = self.cert_pem.unwrap_or_default();
                let key = self.key_pem.unwrap_or_default();
                kubeconfig::pem_to_b64(&cert)?;
                if !key.contains("PRIVATE KEY-----") {
                    return Err(AppError::validation("a PEM encoded private key is required"));
                }
                let creds = serde_json::to_vec(&CertificateCredentials { cert_pem: cert, key_pem: key })?;
                (
                    ClusterSource::Certificate { server: self.server.trim().into(), ca_pem, tls_server_name: tls, proxy_url: proxy, secret_id },
                    creds,
                )
            }
            other => return Err(AppError::validation(format!("unknown connection type {other}"))),
        };
        Ok((source, secret, self.name, self.group, self.environment))
    }
}

#[tauri::command]
pub async fn test_direct_cluster(state: State<'_, AppState>, request: DirectClusterRequest) -> AppResult<ConnectionTestResult> {
    let (source, secret, name, group, env) = request.into_parts("unsaved".into())?;
    let profile = new_profile(if name.trim().is_empty() { "test".into() } else { name }, source, group, env)?;
    state.clusters.test_with_secret(&profile, Some(secret)).await
}

#[tauri::command]
pub async fn add_direct_cluster(app: AppHandle, state: State<'_, AppState>, request: DirectClusterRequest) -> AppResult<ClusterProfile> {
    let secret_id = format!("cred-{}", uuid::Uuid::new_v4());
    let (source, secret, name, group, env) = request.into_parts(secret_id.clone())?;
    let profile = new_profile(name, source, group, env)?;
    state.db.put_secret(&state.vault, &secret_id, &secret)?;
    if let Err(e) = state.db.insert_cluster(&profile) {
        let _ = state.db.delete_secret(&secret_id);
        return Err(e);
    }
    state.db.add_audit(NewAudit {
        cluster_id: Some(profile.id.clone()),
        cluster_name: Some(profile.name.clone()),
        action: "cluster.add".into(),
        outcome: "success".into(),
        detail: Some(profile.source.kind().into()),
        ..Default::default()
    })?;
    crate::desktop::tray::refresh(&app);
    Ok(profile)
}

#[tauri::command]
pub async fn update_cluster(app: AppHandle, state: State<'_, AppState>, id: String, patch: ClusterPatch) -> AppResult<ClusterProfile> {
    if let Some(n) = &patch.name {
        validate::display_name("cluster name", n)?;
    }
    if let Some(Some(ns)) = &patch.default_namespace {
        if !ns.is_empty() {
            validate::namespace(ns)?;
        }
    }
    if let Some(Some(c)) = &patch.color {
        if !(c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(AppError::validation("color must be #RRGGBB"));
        }
    }
    let alerts_changed = patch.alerts_enabled;
    let profile = state.db.update_cluster(&id, patch)?;
    state.clusters.refresh_profile(&profile);
    if let Some(enabled) = alerts_changed {
        if enabled {
            if let Ok(conn) = state.clusters.get(&id) {
                state.alerts.start(&id, &profile.name, conn.client.clone());
            }
        } else {
            state.alerts.stop(&id);
        }
    }
    crate::desktop::tray::refresh(&app);
    Ok(profile)
}

#[tauri::command]
pub async fn remove_cluster(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    disconnect_internal(&state, &id);
    if let Some(profile) = state.db.delete_cluster(&id)? {
        if let Some(sid) = profile.source.secret_id() {
            state.db.delete_secret(sid)?;
        }
        state.clusters.forget(&id);
        state.db.add_audit(NewAudit {
            cluster_id: Some(id.clone()),
            cluster_name: Some(profile.name.clone()),
            action: "cluster.remove".into(),
            outcome: "success".into(),
            ..Default::default()
        })?;
    }
    crate::desktop::tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub async fn test_cluster(state: State<'_, AppState>, id: String) -> AppResult<ConnectionTestResult> {
    let profile = state.db.get_cluster(&id)?;
    state.clusters.test(&profile).await
}

#[tauri::command]
pub async fn connect_cluster(state: State<'_, AppState>, id: String) -> AppResult<ClusterStatus> {
    let status = state.clusters.connect(&id).await?;
    let profile = state.db.get_cluster(&id)?;
    if profile.alerts_enabled && !state.alerts.is_running(&id) {
        if let Ok(conn) = state.clusters.get(&id) {
            state.alerts.start(&id, &profile.name, conn.client.clone());
        }
    }
    Ok(status)
}

pub fn disconnect_internal(state: &AppState, id: &str) {
    state.alerts.stop(id);
    state.watches.stop_cluster(id);
    state.exec.close_cluster(id);
    state.clusters.disconnect(id);
}

#[tauri::command]
pub async fn disconnect_cluster(state: State<'_, AppState>, id: String) -> AppResult<()> {
    disconnect_internal(&state, &id);
    Ok(())
}

#[tauri::command]
pub async fn cluster_statuses(state: State<'_, AppState>) -> AppResult<Vec<ClusterStatus>> {
    Ok(state.clusters.statuses())
}

#[tauri::command]
pub async fn list_api_resources(state: State<'_, AppState>, cluster_id: String, refresh: Option<bool>) -> AppResult<Vec<ApiResourceInfo>> {
    let conn = state.conn(&cluster_id)?;
    let discovery = conn.discovery(refresh.unwrap_or(false)).await?;
    Ok(resources::list_api_resources(&discovery))
}
