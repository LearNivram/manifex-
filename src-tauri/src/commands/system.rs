//! Settings, alerts, audit log, files and windows.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, AppResult};
use crate::k8s::alerts::Alert;
use crate::persistence::audit::{AuditEntry, AuditQuery};
use crate::persistence::rules::AlertRule;
use crate::persistence::settings::AppSettings;
use crate::state::AppState;

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state.db.load_settings()
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: AppSettings) -> AppResult<AppSettings> {
    let previous = state.db.load_settings()?;
    let saved = state.db.save_settings(&settings)?;
    if previous.global_shortcut != saved.global_shortcut {
        crate::register_global_shortcut(&app, &saved.global_shortcut)?;
    }
    Ok(saved)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: String,
    pub vault_available: bool,
    pub vault_message: Option<String>,
    pub active_watches: usize,
    pub active_subscriptions: usize,
    pub terminal_sessions: usize,
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppInfo> {
    let vault = state.vault.clone();
    let vault_status = tokio::task::spawn_blocking(move || vault.status()).await?;
    let (watches, subs) = state.watches.active_count();
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        data_dir: app.path().app_data_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        vault_available: vault_status.is_ok(),
        vault_message: vault_status.err(),
        active_watches: watches,
        active_subscriptions: subs,
        terminal_sessions: state.exec.count(),
    })
}

#[tauri::command]
pub async fn list_alerts(state: State<'_, AppState>) -> AppResult<Vec<Alert>> {
    Ok(state.alerts.recent())
}

#[tauri::command]
pub async fn clear_alerts(state: State<'_, AppState>) -> AppResult<()> {
    state.alerts.clear();
    Ok(())
}

#[tauri::command]
pub async fn list_alert_rules(state: State<'_, AppState>) -> AppResult<Vec<AlertRule>> {
    state.db.list_rules()
}

#[tauri::command]
pub async fn save_alert_rule(state: State<'_, AppState>, mut rule: AlertRule) -> AppResult<AlertRule> {
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
        rule.created_at = chrono::Utc::now().to_rfc3339();
    }
    rule.namespace = rule.namespace.filter(|n| !n.is_empty());
    rule.kind = rule.kind.filter(|n| !n.is_empty());
    rule.reason_pattern = rule.reason_pattern.filter(|n| !n.is_empty());
    rule.message_pattern = rule.message_pattern.filter(|n| !n.is_empty());
    rule.cluster_id = rule.cluster_id.filter(|n| !n.is_empty());
    state.db.upsert_rule(&rule)?;
    Ok(rule)
}

#[tauri::command]
pub async fn delete_alert_rule(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.delete_rule(&id)
}

#[tauri::command]
pub async fn query_audit(state: State<'_, AppState>, query: AuditQuery) -> AppResult<Vec<AuditEntry>> {
    state.db.query_audit(&query)
}

#[tauri::command]
pub async fn clear_audit(state: State<'_, AppState>) -> AppResult<()> {
    state.db.clear_audit()?;
    state.db.add_audit(crate::persistence::audit::NewAudit {
        action: "audit.clear".into(),
        outcome: "success".into(),
        ..Default::default()
    })
}

fn csv_escape(s: &str) -> String {
    // Neutralise spreadsheet formula injection and quote fields.
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", s.replace('"', "\"\""))
}

pub fn audit_to_csv(entries: &[AuditEntry]) -> String {
    let mut out = String::from("timestamp,cluster,identity,action,resource,namespace,name,outcome,detail\n");
    for e in entries {
        let fields = [
            e.ts.as_str(),
            e.cluster_name.as_deref().unwrap_or(""),
            e.identity.as_deref().unwrap_or(""),
            e.action.as_str(),
            e.resource.as_deref().unwrap_or(""),
            e.namespace.as_deref().unwrap_or(""),
            e.name.as_deref().unwrap_or(""),
            e.outcome.as_str(),
            e.detail.as_deref().unwrap_or(""),
        ];
        out.push_str(&fields.iter().map(|f| csv_escape(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// Shows a native save dialog and writes the content. The frontend never receives
/// direct file system access; the user picks every destination explicitly.
#[tauri::command]
pub async fn save_text_file(app: AppHandle, default_name: String, content: String) -> AppResult<Option<String>> {
    if content.len() > 256 * 1024 * 1024 {
        return Err(AppError::validation("content too large"));
    }
    let safe_name: String = default_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(120)
        .collect();
    let dialog = app.dialog().file().set_file_name(if safe_name.is_empty() { "export.txt".to_string() } else { safe_name });
    let picked = tokio::task::spawn_blocking(move || dialog.blocking_save_file()).await?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(|_| AppError::validation("unsupported destination"))?;
    tokio::fs::write(&path, content).await?;
    Ok(Some(path.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn export_audit(app: AppHandle, state: State<'_, AppState>, query: AuditQuery, format: String) -> AppResult<Option<String>> {
    let mut q = query;
    q.limit = Some(10_000);
    let entries = state.db.query_audit(&q)?;
    let (content, ext) = match format.as_str() {
        "json" => (serde_json::to_string_pretty(&entries)?, "json"),
        _ => (audit_to_csv(&entries), "csv"),
    };
    save_text_file(app, format!("manifex-audit-{}.{ext}", chrono::Utc::now().format("%Y%m%d-%H%M%S")), content).await
}

#[tauri::command]
pub async fn open_window(app: AppHandle, cluster_id: Option<String>, view: Option<String>) -> AppResult<String> {
    crate::desktop::windows::open_window(&app, cluster_id.as_deref(), view.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_is_escaped() {
        let e = AuditEntry {
            id: 1,
            ts: "t".into(),
            cluster_id: None,
            cluster_name: Some("prod \"eu\"".into()),
            identity: None,
            action: "delete".into(),
            resource: None,
            namespace: None,
            name: Some("=cmd()".into()),
            outcome: "success".into(),
            detail: None,
        };
        let csv = audit_to_csv(&[e]);
        assert!(csv.contains("\"prod \"\"eu\"\"\""));
        assert!(csv.contains("\"'=cmd()\""));
    }
}
