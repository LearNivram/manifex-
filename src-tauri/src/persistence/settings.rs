//! User preferences (non-sensitive) stored as a single JSON document.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// "system" | "dark" | "light"
    pub theme: String,
    /// Interface density: "comfortable" | "compact"
    pub density: String,
    /// Require typing the resource name before deleting on production clusters.
    pub strict_production_confirmations: bool,
    /// Ask for confirmation for every destructive operation.
    pub confirm_destructive: bool,
    /// Minimise to the system tray instead of quitting when the main window closes.
    pub close_to_tray: bool,
    pub native_notifications: bool,
    pub notify_connection_loss: bool,
    pub notify_pod_failures: bool,
    pub notify_deployment_failures: bool,
    pub notify_node_problems: bool,
    pub notify_warning_events: bool,
    /// Explicit Helm binary path. Empty = discover on PATH.
    pub helm_path: String,
    pub log_tail_lines: i64,
    pub log_timestamps: bool,
    pub terminal_font_size: i64,
    pub editor_font_size: i64,
    /// Global OS shortcut to show / focus Manifex. Empty disables it.
    pub global_shortcut: String,
    pub auto_check_updates: bool,
    pub auto_reconnect: bool,
    /// Seconds between connection health probes.
    pub health_interval_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            density: "comfortable".into(),
            strict_production_confirmations: true,
            confirm_destructive: true,
            close_to_tray: false,
            native_notifications: true,
            notify_connection_loss: true,
            notify_pod_failures: true,
            notify_deployment_failures: true,
            notify_node_problems: true,
            notify_warning_events: false,
            helm_path: String::new(),
            log_tail_lines: 500,
            log_timestamps: false,
            terminal_font_size: 13,
            editor_font_size: 13,
            global_shortcut: "CommandOrControl+Shift+M".into(),
            auto_check_updates: true,
            auto_reconnect: true,
            health_interval_secs: 20,
        }
    }
}

impl AppSettings {
    pub fn sanitized(mut self) -> Self {
        if !["system", "dark", "light"].contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        if !["comfortable", "compact"].contains(&self.density.as_str()) {
            self.density = "comfortable".into();
        }
        self.log_tail_lines = self.log_tail_lines.clamp(10, 50_000);
        self.terminal_font_size = self.terminal_font_size.clamp(9, 24);
        self.editor_font_size = self.editor_font_size.clamp(9, 24);
        self.health_interval_secs = self.health_interval_secs.clamp(5, 300);
        self.helm_path = self.helm_path.trim().to_string();
        self
    }
}

const KEY: &str = "app";

impl Db {
    pub fn load_settings(&self) -> AppResult<AppSettings> {
        let raw: Option<String> = self.with(|c| {
            c.query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
                r.get(0)
            })
            .optional()
        })?;
        Ok(raw
            .and_then(|r| serde_json::from_str::<AppSettings>(&r).ok())
            .unwrap_or_default()
            .sanitized())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> AppResult<AppSettings> {
        let s = settings.clone().sanitized();
        let json = serde_json::to_string(&s)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![KEY, json],
            )
        })?;
        Ok(s)
    }

    pub fn get_kv(&self, key: &str) -> AppResult<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
        })
    }

    pub fn set_kv(&self, key: &str, value: &str) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.load_settings().unwrap(), AppSettings::default());
        let s = AppSettings {
            theme: "dark".into(),
            log_tail_lines: 1_000_000,
            ..Default::default()
        };
        let saved = db.save_settings(&s).unwrap();
        assert_eq!(saved.log_tail_lines, 50_000);
        assert_eq!(db.load_settings().unwrap().theme, "dark");
    }
}
