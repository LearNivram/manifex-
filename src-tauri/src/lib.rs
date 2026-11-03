//! Manifex – native Kubernetes management desktop application (Rust core).
//!
//! Layers:
//! * `desktop`     – native shell integration (tray, menu, windows, notifications)
//! * `commands`    – the IPC surface exposed to the webview
//! * `k8s`         – Kubernetes integration (connections, watches, operations)
//! * `persistence` – local SQLite storage
//! * `security`    – credential vault and input validation

pub mod commands;
pub mod desktop;
pub mod error;
pub mod k8s;
pub mod persistence;
pub mod security;
pub mod state;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::error::{AppError, AppResult};
use crate::k8s::alerts::AlertEngine;
use crate::k8s::connection::ClusterManager;
use crate::k8s::exec::ExecManager;
use crate::k8s::logs::LogManager;
use crate::k8s::watch::WatchManager;
use crate::persistence::Db;
use crate::security::vault::SecretVault;
use crate::state::AppState;

/// Deep links received before the webview was ready.
#[derive(Default)]
pub struct PendingDeepLinks(pub Mutex<Vec<String>>);

fn accept_deep_link(url: &str) -> bool {
    url.starts_with("manifex://") && url.len() < 2048 && !url.chars().any(char::is_control)
}

fn dispatch_deep_links(app: &tauri::AppHandle, urls: Vec<String>) {
    let urls: Vec<String> = urls.into_iter().filter(|u| accept_deep_link(u)).collect();
    if urls.is_empty() {
        return;
    }
    desktop::tray::show_main(app);
    if let Some(p) = app.try_state::<PendingDeepLinks>() {
        p.0.lock().extend(urls.clone());
    }
    let _ = app.emit_to("main", "deeplink://open", urls);
}

#[tauri::command]
fn take_deep_links(pending: tauri::State<'_, PendingDeepLinks>) -> Vec<String> {
    std::mem::take(&mut *pending.0.lock())
}

pub fn register_global_shortcut(app: &tauri::AppHandle, accelerator: &str) -> AppResult<()> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if accelerator.trim().is_empty() {
        return Ok(());
    }
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| AppError::validation(format!("invalid shortcut '{accelerator}': {e}")))?;
    gs.register(shortcut).map_err(|e| AppError::validation(format!("could not register shortcut: {e}")))?;
    Ok(())
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            let _ = w.hide();
        } else {
            desktop::tray::show_main(app);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("MANIFEX_LOG").unwrap_or_else(|_| "manifex_lib=info,warn".into()),
        )
        .try_init();

    let builder = tauri::Builder::default()
        // must be registered first so a second launch forwards to this instance
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let urls: Vec<String> = argv.into_iter().filter(|a| a.starts_with("manifex://")).collect();
            if urls.is_empty() {
                desktop::tray::show_main(app);
            } else {
                dispatch_deep_links(app, urls);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .with_denylist(&[])
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_main_window(app);
                    }
                })
                .build(),
        )
        .manage(PendingDeepLinks::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            let db = Arc::new(Db::open(&data_dir.join("manifex.db")).map_err(|e| anyhow::anyhow!(e.to_string()))?);
            let vault = Arc::new(SecretVault::os_backed());
            let sink = desktop::sink::TauriSink::new(handle.clone(), db.clone());
            let clusters = ClusterManager::new(db.clone(), vault.clone(), sink.clone());
            let alerts = AlertEngine::new(db.clone(), sink);
            app.manage(AppState {
                db: db.clone(),
                vault,
                clusters,
                watches: WatchManager::new(),
                logs: Arc::new(LogManager::default()),
                exec: Arc::new(ExecManager::default()),
                alerts,
            });

            #[cfg(target_os = "macos")]
            {
                let menu = desktop::menu::build(&handle)?;
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| desktop::menu::handle_event(app, event.id().as_ref()));
            }

            if let Err(e) = desktop::tray::init(&handle) {
                tracing::warn!(error = %e, "system tray unavailable");
            }

            let settings = db.load_settings().unwrap_or_default();
            if let Err(e) = register_global_shortcut(&handle, &settings.global_shortcut) {
                tracing::warn!(error = %e, "global shortcut not registered");
            }

            #[cfg(any(windows, target_os = "linux"))]
            {
                // Register the manifex:// scheme for the current (possibly portable) install.
                if let Err(e) = app.deep_link().register_all() {
                    tracing::debug!(error = %e, "deep link registration skipped");
                }
            }
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                dispatch_deep_links(&handle, urls.into_iter().map(|u| u.to_string()).collect());
            }
            let dl_handle = handle.clone();
            app.deep_link().on_open_url(move |event| {
                dispatch_deep_links(&dl_handle, event.urls().into_iter().map(|u| u.to_string()).collect());
            });

            // The window is created hidden and shown by the frontend once the first
            // frame is rendered (avoids a white flash). Fallback after 4 seconds.
            let show_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                if let Some(w) = show_handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    if let Some(state) = window.try_state::<AppState>() {
                        if state.db.load_settings().map(|s| s.close_to_tray).unwrap_or(false) {
                            api.prevent_close();
                            let _ = window.hide();
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            take_deep_links,
            commands::clusters::list_clusters,
            commands::clusters::discover_kubeconfigs,
            commands::clusters::pick_kubeconfig_file,
            commands::clusters::add_kubeconfig_clusters,
            commands::clusters::test_direct_cluster,
            commands::clusters::add_direct_cluster,
            commands::clusters::update_cluster,
            commands::clusters::remove_cluster,
            commands::clusters::test_cluster,
            commands::clusters::connect_cluster,
            commands::clusters::disconnect_cluster,
            commands::clusters::cluster_statuses,
            commands::clusters::list_api_resources,
            commands::resources::watch_resources,
            commands::resources::unwatch_resources,
            commands::resources::list_resources,
            commands::resources::get_resource,
            commands::resources::apply_yaml,
            commands::resources::delete_resource,
            commands::resources::scale_resource,
            commands::resources::restart_workload,
            commands::resources::set_deployment_paused,
            commands::resources::rollout_history,
            commands::resources::rollback_deployment,
            commands::resources::cordon_node,
            commands::resources::drain_node,
            commands::resources::suspend_cronjob,
            commands::resources::trigger_cronjob,
            commands::resources::can_i,
            commands::resources::rbac_rules,
            commands::resources::resource_schema,
            commands::resources::node_metrics,
            commands::resources::pod_metrics,
            commands::resources::cluster_capacity,
            commands::resources::list_applications,
            commands::resources::global_search,
            commands::resources::compare_clusters,
            commands::resources::workload_pods,
            commands::streams::logs_start,
            commands::streams::logs_stop,
            commands::streams::exec_start,
            commands::streams::exec_write,
            commands::streams::exec_resize,
            commands::streams::exec_close,
            commands::helm::helm_status,
            commands::helm::helm_releases,
            commands::helm::helm_release,
            commands::helm::helm_rollback,
            commands::helm::helm_uninstall,
            commands::helm::helm_upgrade,
            commands::helm::helm_repos,
            commands::helm::helm_repo_add,
            commands::helm::helm_repo_remove,
            commands::helm::helm_repo_update,
            commands::helm::helm_search,
            commands::helm::helm_show_values,
            commands::system::get_settings,
            commands::system::save_settings,
            commands::system::app_info,
            commands::system::list_alerts,
            commands::system::clear_alerts,
            commands::system::list_alert_rules,
            commands::system::save_alert_rule,
            commands::system::delete_alert_rule,
            commands::system::query_audit,
            commands::system::clear_audit,
            commands::system::export_audit,
            commands::system::save_text_file,
            commands::system::open_window,
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running Manifex");
}
