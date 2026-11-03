//! Cluster profiles: everything Manifex needs to know to connect to a cluster,
//! except secrets. Secrets are referenced by id and live encrypted in the vault.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::Db;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClusterSource {
    /// References a context inside a kubeconfig file on disk. The file itself is
    /// never copied; it stays under the user's control.
    Kubeconfig { path: String, context: String },
    /// A single-context kubeconfig imported into the encrypted vault.
    StoredKubeconfig { context: String, secret_id: String },
    /// Direct API server connection with a bearer token (stored in the vault).
    Token {
        server: String,
        ca_pem: Option<String>,
        tls_server_name: Option<String>,
        proxy_url: Option<String>,
        secret_id: String,
    },
    /// Direct API server connection with a client certificate (cert + key in vault).
    Certificate {
        server: String,
        ca_pem: Option<String>,
        tls_server_name: Option<String>,
        proxy_url: Option<String>,
        secret_id: String,
    },
    /// Connection through a Manifex agent / authenticating gateway that exposes the
    /// Kubernetes API of a remote (e.g. private) cluster.
    Agent {
        endpoint: String,
        ca_pem: Option<String>,
        proxy_url: Option<String>,
        secret_id: String,
    },
}

impl ClusterSource {
    pub fn secret_id(&self) -> Option<&str> {
        match self {
            ClusterSource::Kubeconfig { .. } => None,
            ClusterSource::StoredKubeconfig { secret_id, .. }
            | ClusterSource::Token { secret_id, .. }
            | ClusterSource::Certificate { secret_id, .. }
            | ClusterSource::Agent { secret_id, .. } => Some(secret_id),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ClusterSource::Kubeconfig { .. } => "kubeconfig",
            ClusterSource::StoredKubeconfig { .. } => "storedKubeconfig",
            ClusterSource::Token { .. } => "token",
            ClusterSource::Certificate { .. } => "certificate",
            ClusterSource::Agent { .. } => "agent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClusterProfile {
    pub id: String,
    pub name: String,
    pub source: ClusterSource,
    pub group: Option<String>,
    pub favorite: bool,
    pub color: Option<String>,
    /// development | staging | production | other. Production clusters get
    /// stricter confirmations for destructive operations.
    pub environment: String,
    pub default_namespace: Option<String>,
    pub alerts_enabled: bool,
    pub sort_order: i64,
    pub created_at: String,
    pub last_connected_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterPatch {
    pub name: Option<String>,
    pub group: Option<Option<String>>,
    pub favorite: Option<bool>,
    pub color: Option<Option<String>>,
    pub environment: Option<String>,
    pub default_namespace: Option<Option<String>>,
    pub alerts_enabled: Option<bool>,
    pub sort_order: Option<i64>,
}

pub const ENVIRONMENTS: &[&str] = &["development", "staging", "production", "other"];

fn from_row(row: &Row<'_>) -> rusqlite::Result<ClusterProfile> {
    let source_json: String = row.get("source")?;
    let source: ClusterSource = serde_json::from_str(&source_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(ClusterProfile {
        id: row.get("id")?,
        name: row.get("name")?,
        source,
        group: row.get("group_name")?,
        favorite: row.get::<_, i64>("favorite")? != 0,
        color: row.get("color")?,
        environment: row.get("environment")?,
        default_namespace: row.get("default_namespace")?,
        alerts_enabled: row.get::<_, i64>("alerts_enabled")? != 0,
        sort_order: row.get("sort_order")?,
        created_at: row.get("created_at")?,
        last_connected_at: row.get("last_connected_at")?,
    })
}

impl Db {
    pub fn list_clusters(&self) -> AppResult<Vec<ClusterProfile>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM clusters ORDER BY favorite DESC, sort_order ASC, name COLLATE NOCASE ASC",
            )?;
            let rows = stmt.query_map([], from_row)?;
            rows.collect()
        })
    }

    pub fn get_cluster(&self, id: &str) -> AppResult<ClusterProfile> {
        self.with(|c| {
            c.query_row("SELECT * FROM clusters WHERE id = ?1", [id], from_row)
                .optional()
        })?
        .ok_or_else(|| AppError::NotFound(format!("cluster {id}")))
    }

    pub fn insert_cluster(&self, p: &ClusterProfile) -> AppResult<()> {
        let source = serde_json::to_string(&p.source)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO clusters (id, name, source, group_name, favorite, color, environment,
                    default_namespace, alerts_enabled, sort_order, created_at, last_connected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    p.id,
                    p.name,
                    source,
                    p.group,
                    p.favorite as i64,
                    p.color,
                    p.environment,
                    p.default_namespace,
                    p.alerts_enabled as i64,
                    p.sort_order,
                    p.created_at,
                    p.last_connected_at
                ],
            )
        })?;
        Ok(())
    }

    pub fn update_cluster(&self, id: &str, patch: ClusterPatch) -> AppResult<ClusterProfile> {
        let mut p = self.get_cluster(id)?;
        if let Some(v) = patch.name {
            p.name = v;
        }
        if let Some(v) = patch.group {
            p.group = v.filter(|g| !g.trim().is_empty());
        }
        if let Some(v) = patch.favorite {
            p.favorite = v;
        }
        if let Some(v) = patch.color {
            p.color = v;
        }
        if let Some(v) = patch.environment {
            if !ENVIRONMENTS.contains(&v.as_str()) {
                return Err(AppError::validation("unknown environment"));
            }
            p.environment = v;
        }
        if let Some(v) = patch.default_namespace {
            p.default_namespace = v.filter(|n| !n.is_empty());
        }
        if let Some(v) = patch.alerts_enabled {
            p.alerts_enabled = v;
        }
        if let Some(v) = patch.sort_order {
            p.sort_order = v;
        }
        self.with(|c| {
            c.execute(
                "UPDATE clusters SET name=?2, group_name=?3, favorite=?4, color=?5, environment=?6,
                    default_namespace=?7, alerts_enabled=?8, sort_order=?9 WHERE id=?1",
                params![
                    p.id,
                    p.name,
                    p.group,
                    p.favorite as i64,
                    p.color,
                    p.environment,
                    p.default_namespace,
                    p.alerts_enabled as i64,
                    p.sort_order
                ],
            )
        })?;
        Ok(p)
    }

    pub fn touch_cluster_connected(&self, id: &str) -> AppResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with(|c| {
            c.execute(
                "UPDATE clusters SET last_connected_at=?2 WHERE id=?1",
                params![id, now],
            )
        })?;
        Ok(())
    }

    pub fn delete_cluster(&self, id: &str) -> AppResult<Option<ClusterProfile>> {
        let profile = self.get_cluster(id).ok();
        self.with(|c| c.execute("DELETE FROM clusters WHERE id=?1", [id]))?;
        Ok(profile)
    }

    pub fn cluster_exists_for_kubeconfig(&self, path: &str, context: &str) -> AppResult<bool> {
        Ok(self.list_clusters()?.iter().any(|p| {
            matches!(&p.source, ClusterSource::Kubeconfig { path: pp, context: cc } if pp == path && cc == context)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> ClusterProfile {
        ClusterProfile {
            id: id.into(),
            name: format!("cluster {id}"),
            source: ClusterSource::Kubeconfig {
                path: "/home/u/.kube/config".into(),
                context: "kind".into(),
            },
            group: None,
            favorite: false,
            color: None,
            environment: "development".into(),
            default_namespace: None,
            alerts_enabled: true,
            sort_order: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_connected_at: None,
        }
    }

    #[test]
    fn crud() {
        let db = Db::open_in_memory().unwrap();
        db.insert_cluster(&sample("a")).unwrap();
        db.insert_cluster(&sample("b")).unwrap();
        assert_eq!(db.list_clusters().unwrap().len(), 2);
        let updated = db
            .update_cluster(
                "a",
                ClusterPatch {
                    favorite: Some(true),
                    group: Some(Some("prod".into())),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(updated.favorite);
        assert_eq!(db.list_clusters().unwrap()[0].id, "a");
        assert!(db
            .cluster_exists_for_kubeconfig("/home/u/.kube/config", "kind")
            .unwrap());
        db.delete_cluster("a").unwrap();
        assert!(db.get_cluster("a").is_err());
    }

    #[test]
    fn source_serialization_is_camel_case() {
        let s = ClusterSource::Token {
            server: "https://x".into(),
            ca_pem: None,
            tls_server_name: None,
            proxy_url: None,
            secret_id: "s".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["type"], "token");
        assert_eq!(v["secretId"], "s");
    }

    #[test]
    fn rejects_unknown_environment() {
        let db = Db::open_in_memory().unwrap();
        db.insert_cluster(&sample("a")).unwrap();
        assert!(db
            .update_cluster(
                "a",
                ClusterPatch {
                    environment: Some("prd".into()),
                    ..Default::default()
                }
            )
            .is_err());
    }
}
