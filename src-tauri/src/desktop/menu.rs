//! Native application menu (macOS). On macOS the menu bar is part of the platform
//! conventions and is required for standard editing shortcuts inside the webview.
//! On Windows and Linux Manifex uses in-app navigation and keyboard shortcuts.

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = AboutMetadata {
        name: Some("Manifex".into()),
        ..Default::default()
    };
    let app_menu = Submenu::with_items(
        app,
        "Manifex",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Manifex"), Some(about))?,
            &MenuItem::with_id(
                app,
                "menu:check-updates",
                "Check for Updates…",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "menu:settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(
                app,
                "menu:new-window",
                "New Window",
                true,
                Some("CmdOrCtrl+Shift+N"),
            )?,
            &MenuItem::with_id(
                app,
                "menu:add-cluster",
                "Add Cluster…",
                true,
                Some("CmdOrCtrl+Shift+A"),
            )?,
            &MenuItem::with_id(
                app,
                "menu:new-resource",
                "New Resource…",
                true,
                Some("CmdOrCtrl+N"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(
                app,
                "menu:command-palette",
                "Command Palette…",
                true,
                Some("CmdOrCtrl+K"),
            )?,
            &MenuItem::with_id(
                app,
                "menu:search",
                "Search Resources…",
                true,
                Some("CmdOrCtrl+Shift+F"),
            )?,
            &MenuItem::with_id(
                app,
                "menu:toggle-sidebar",
                "Toggle Sidebar",
                true,
                Some("CmdOrCtrl+B"),
            )?,
            &MenuItem::with_id(
                app,
                "menu:toggle-details",
                "Toggle Details Panel",
                true,
                Some("CmdOrCtrl+J"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "menu:terminal", "Terminal", true, Some("CmdOrCtrl+`"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window])
}

/// Forwards custom menu items to the focused webview as `menu://action`.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if let Some(action) = id.strip_prefix("menu:") {
        let _ = app.emit("menu://action", action.to_string());
    }
}
