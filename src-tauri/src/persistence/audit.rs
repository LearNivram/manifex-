//! Local audit trail of every mutating operation executed through Manifex.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
    pub identity: Option<String>,
    pub action: String,
    pub resource: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    /// "success" | "failure" | "dry-run"
    pub outcome: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewAudit {
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
    pub identity: Option<String>,
    pub action: String,
    pub resource: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub outcome: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuditQuery {
    pub cluster_id: Option<String>,
    pub search: Option<String>,
    pub outcome: Option<String>,
    pub limit: Option<i64>,
    pub before_id: Option<i64>,
}

impl Db {
    pub fn add_audit(&self, e: NewAudit) -> AppResult<()> {
        let ts = chrono::Utc::now().to_rfc3339();
        // Truncate details so error payloads cannot bloat the database.
        let detail = e.detail.map(|d| d.chars().take(4000).collect::<String>());
        self.with(|c| {
            c.execute(
                "INSERT INTO audit_log (ts, cluster_id, cluster_name, identity, action, resource, namespace, name, outcome, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![ts, e.cluster_id, e.cluster_name, e.identity, e.action, e.resource, e.namespace, e.name, e.outcome, detail],
            )
        })?;
        Ok(())
    }

    pub fn query_audit(&self, q: &AuditQuery) -> AppResult<Vec<AuditEntry>> {
        let limit = q.limit.unwrap_or(500).clamp(1, 10_000);
        let search = q
            .search
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s.replace('%', "\\%")));
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, ts, cluster_id, cluster_name, identity, action, resource, namespace, name, outcome, detail
                 FROM audit_log
                 WHERE (?1 IS NULL OR cluster_id = ?1)
                   AND (?2 IS NULL OR outcome = ?2)
                   AND (?3 IS NULL OR action LIKE ?3 ESCAPE '\\' OR name LIKE ?3 ESCAPE '\\' OR resource LIKE ?3 ESCAPE '\\' OR namespace LIKE ?3 ESCAPE '\\')
                   AND (?4 IS NULL OR id < ?4)
                 ORDER BY id DESC LIMIT ?5",
            )?;
            let rows = stmt.query_map(params![q.cluster_id, q.outcome, search, q.before_id, limit], |r| {
                Ok(AuditEntry {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    cluster_id: r.get(2)?,
                    cluster_name: r.get(3)?,
                    identity: r.get(4)?,
                    action: r.get(5)?,
                    resource: r.get(6)?,
                    namespace: r.get(7)?,
                    name: r.get(8)?,
                    outcome: r.get(9)?,
                    detail: r.get(10)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn clear_audit(&self) -> AppResult<()> {
        self.with(|c| c.execute("DELETE FROM audit_log", []))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_filter() {
        let db = Db::open_in_memory().unwrap();
        for (i, outcome) in ["success", "failure", "success"].iter().enumerate() {
            db.add_audit(NewAudit {
                cluster_id: Some("c1".into()),
                action: "delete".into(),
                name: Some(format!("pod-{i}")),
                outcome: outcome.to_string(),
                ..Default::default()
            })
            .unwrap();
        }
        assert_eq!(db.query_audit(&AuditQuery::default()).unwrap().len(), 3);
        let failures = db
            .query_audit(&AuditQuery {
                outcome: Some("failure".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(failures.len(), 1);
        let search = db
            .query_audit(&AuditQuery {
                search: Some("pod-2".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(search.len(), 1);
    }
}
