//! System tray: quick access to clusters and the main window.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::k8s::connection::ConnState;
use crate::state::AppState;

const TRAY_ID: &str = "manifex-tray";

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let show = MenuItem::with_id(app, "tray:show", "Open Manifex", true, None::<&str>)?;
    let palette = MenuItem::with_id(app, "tray:palette", "Command Palette…", true, None::<&str>)?;
    let clusters = Submenu::with_id(app, "tray:clusters", "Clusters", true)?;
    if let Some(state) = app.try_state::<AppState>() {
        let profiles = state.db.list_clusters().unwrap_or_default();
        if profiles.is_empty() {
            clusters.append(&MenuItem::with_id(
                app,
                "tray:none",
                "No clusters configured",
                false,
                None::<&str>,
            )?)?;
        }
        for p in profiles.iter().take(30) {
            let marker = match state.clusters.status(&p.id).map(|s| s.state) {
                Some(ConnState::Connected) => "●",
                Some(ConnState::Reconnecting) | Some(ConnState::Connecting) => "◐",
                Some(ConnState::Error) => "✕",
                _ => "○",
            };
            let label = format!("{marker}  {}", p.name);
            clusters.append(&MenuItem::with_id(
                app,
                format!("tray:cluster:{}", p.id),
                label,
                true,
                None::<&str>,
            )?)?;
        }
    }
    let quit = MenuItem::with_id(app, "tray:quit", "Quit Manifex", true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &show,
            &palette,
            &PredefinedMenuItem::separator(app)?,
            &clusters,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Manifex")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "tray:show" => show_main(app),
                "tray:palette" => {
                    show_main(app);
                    let _ = app.emit_to("main", "menu://action", "command-palette");
                }
                "tray:quit" => app.exit(0),
                other => {
                    if let Some(cluster_id) = other.strip_prefix("tray:cluster:") {
                        show_main(app);
                        let _ = app.emit_to("main", "tray://open-cluster", cluster_id.to_string());
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuilds the tray menu (cluster list / status markers).
pub fn refresh(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = app2.tray_by_id(TRAY_ID) {
            if let Ok(menu) = build_menu(&app2) {
                let _ = tray.set_menu(Some(menu));
            }
            if let Some(state) = app2.try_state::<AppState>() {
                let connected = state.clusters.connected_ids().len();
                let _ =
                    tray.set_tooltip(Some(format!("Manifex – {connected} cluster(s) connected")));
            }
        }
    });
}
