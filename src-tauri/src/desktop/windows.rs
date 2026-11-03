//! Additional native windows (e.g. one window per cluster for side-by-side work).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppError, AppResult};

pub fn open_window(
    app: &AppHandle,
    cluster_id: Option<&str>,
    view: Option<&str>,
) -> AppResult<String> {
    let mut query = Vec::new();
    if let Some(c) = cluster_id {
        if !c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(AppError::validation("invalid cluster id"));
        }
        query.push(format!("cluster={c}"));
    }
    if let Some(v) = view {
        if !v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(AppError::validation("invalid view"));
        }
        query.push(format!("view={v}"));
    }
    let label = format!(
        "cluster-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let url = if query.is_empty() {
        "index.html".to_string()
    } else {
        format!("index.html?{}", query.join("&"))
    };
    let title = match cluster_id.and_then(|id| {
        app.try_state::<crate::state::AppState>()
            .and_then(|s| s.db.get_cluster(id).ok())
    }) {
        Some(p) => format!("{} – Manifex", p.name),
        None => "Manifex".to_string(),
    };
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(1280.0, 820.0)
        .min_inner_size(1024.0, 640.0)
        .disable_drag_drop_handler();
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build()?;
    Ok(label)
}
