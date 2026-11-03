//! User-defined alert rules evaluated against Kubernetes Warning events.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Db;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub cluster_id: Option<String>,
    pub namespace: Option<String>,
    /// Involved object kind (e.g. "Pod").
    pub kind: Option<String>,
    /// Regular expression matched against the event reason.
    pub reason_pattern: Option<String>,
    /// Regular expression matched against the event message.
    pub message_pattern: Option<String>,
    /// "info" | "warning" | "critical"
    pub severity: String,
    pub created_at: String,
}

impl AlertRule {
    pub fn validate(&self) -> AppResult<()> {
        crate::security::validate::display_name("rule name", &self.name)?;
        if !["info", "warning", "critical"].contains(&self.severity.as_str()) {
            return Err(AppError::validation(
                "severity must be info, warning or critical",
            ));
        }
        for p in [&self.reason_pattern, &self.message_pattern]
            .into_iter()
            .flatten()
        {
            if p.len() > 512 {
                return Err(AppError::validation("pattern too long"));
            }
            regex::RegexBuilder::new(p)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| AppError::validation(format!("invalid pattern: {e}")))?;
        }
        Ok(())
    }
}

impl Db {
    pub fn list_rules(&self) -> AppResult<Vec<AlertRule>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, enabled, cluster_id, namespace, kind, reason_pattern, message_pattern, severity, created_at
                 FROM alert_rules ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(AlertRule {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    enabled: r.get::<_, i64>(2)? != 0,
                    cluster_id: r.get(3)?,
                    namespace: r.get(4)?,
                    kind: r.get(5)?,
                    reason_pattern: r.get(6)?,
                    message_pattern: r.get(7)?,
                    severity: r.get(8)?,
                    created_at: r.get(9)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn upsert_rule(&self, rule: &AlertRule) -> AppResult<()> {
        rule.validate()?;
        self.with(|c| {
            c.execute(
                "INSERT INTO alert_rules (id, name, enabled, cluster_id, namespace, kind, reason_pattern, message_pattern, severity, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name, enabled=excluded.enabled, cluster_id=excluded.cluster_id,
                   namespace=excluded.namespace, kind=excluded.kind, reason_pattern=excluded.reason_pattern,
                   message_pattern=excluded.message_pattern, severity=excluded.severity",
                params![
                    rule.id,
                    rule.name.trim(),
                    rule.enabled as i64,
                    rule.cluster_id,
                    rule.namespace,
                    rule.kind,
                    rule.reason_pattern,
                    rule.message_pattern,
                    rule.severity,
                    rule.created_at
                ],
            )
        })?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> AppResult<()> {
        self.with(|c| c.execute("DELETE FROM alert_rules WHERE id=?1", [id]))?;
        Ok(())
    }
}
