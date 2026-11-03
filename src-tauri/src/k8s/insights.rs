//! Cross-cluster search, cluster comparison and application grouping.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use kube::api::ListParams;
use kube::Client;
use serde::Serialize;
use serde_json::Value;

use super::connection::ClusterManager;
use super::resources::{builtin, ResourceRef};
use super::summary::{summarize, Health};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub cluster_id: String,
    pub cluster_name: String,
    pub resource: ResourceRef,
    pub namespace: Option<String>,
    pub name: String,
}

fn search_kinds() -> Vec<ResourceRef> {
    vec![
        builtin::pods(),
        builtin::deployments(),
        builtin::statefulsets(),
        builtin::daemonsets(),
        builtin::services(),
        builtin::ingresses(),
        builtin::jobs(),
        builtin::cronjobs(),
        builtin::configmaps(),
        builtin::namespaces(),
        builtin::nodes(),
        ResourceRef::new("", "v1", "Secret", "secrets", true),
        ResourceRef::new(
            "",
            "v1",
            "PersistentVolumeClaim",
            "persistentvolumeclaims",
            true,
        ),
    ]
}

/// Searches object names across all connected clusters using metadata-only list
/// requests (much cheaper than full objects).
pub async fn global_search(
    mgr: &Arc<ClusterManager>,
    query: &str,
    limit: usize,
) -> AppResult<Vec<SearchHit>> {
    let q = query.trim().to_lowercase();
    if q.len() < 2 {
        return Ok(vec![]);
    }
    if q.len() > 128 {
        return Err(AppError::validation("search query too long"));
    }
    let mut tasks = Vec::new();
    for id in mgr.connected_ids() {
        let Ok(conn) = mgr.get(&id) else { continue };
        for res in search_kinds() {
            let client = conn.client.clone();
            let cluster_id = id.clone();
            let cluster_name = conn.name();
            let q = q.clone();
            tasks.push(async move {
                let api = res.api(client, None);
                let list = tokio::time::timeout(
                    Duration::from_secs(10),
                    api.list_metadata(&ListParams::default()),
                )
                .await;
                let Ok(Ok(list)) = list else {
                    return Vec::new();
                };
                list.items
                    .into_iter()
                    .filter_map(|o| {
                        let name = o.metadata.name.clone()?;
                        if !name.to_lowercase().contains(&q) {
                            return None;
                        }
                        Some(SearchHit {
                            cluster_id: cluster_id.clone(),
                            cluster_name: cluster_name.clone(),
                            resource: res.clone(),
                            namespace: o.metadata.namespace.clone(),
                            name,
                        })
                    })
                    .collect::<Vec<_>>()
            });
        }
    }
    let mut hits: Vec<SearchHit> = futures::future::join_all(tasks)
        .await
        .into_iter()
        .flatten()
        .collect();
    // exact / prefix matches first
    hits.sort_by_key(|h| {
        let n = h.name.to_lowercase();
        (
            if n == q {
                0
            } else if n.starts_with(&q) {
                1
            } else {
                2
            },
            n.len(),
        )
    });
    hits.truncate(limit.clamp(1, 500));
    Ok(hits)
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClusterFacts {
    pub cluster_id: String,
    pub name: String,
    pub version: Option<String>,
    pub nodes: usize,
    pub ready_nodes: usize,
    pub cpu_millis: f64,
    pub memory_bytes: f64,
    pub namespaces: Vec<String>,
    pub pods: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadDiff {
    pub namespace: String,
    pub name: String,
    pub kind: String,
    pub left: Option<WorkloadFacts>,
    pub right: Option<WorkloadFacts>,
    pub differs: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadFacts {
    pub replicas: i64,
    pub ready: String,
    pub images: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterComparison {
    pub left: ClusterFacts,
    pub right: ClusterFacts,
    pub only_left_namespaces: Vec<String>,
    pub only_right_namespaces: Vec<String>,
    pub workloads: Vec<WorkloadDiff>,
}

async fn list_values(client: Client, res: &ResourceRef, ns: Option<&str>) -> Vec<Value> {
    let api = res.api(client, ns);
    match tokio::time::timeout(Duration::from_secs(30), api.list(&ListParams::default())).await {
        Ok(Ok(list)) => list
            .items
            .into_iter()
            .filter_map(|o| serde_json::to_value(o).ok())
            .collect(),
        _ => vec![],
    }
}

async fn facts(
    mgr: &Arc<ClusterManager>,
    id: &str,
    namespace: Option<&str>,
) -> AppResult<(
    ClusterFacts,
    BTreeMap<(String, String, String), WorkloadFacts>,
)> {
    let conn = mgr.get(id)?;
    let client = conn.client.clone();
    let (r_nodes, r_ns, r_dep, r_sts, r_ds) = (
        builtin::nodes(),
        builtin::namespaces(),
        builtin::deployments(),
        builtin::statefulsets(),
        builtin::daemonsets(),
    );
    let (nodes, nss, pods, deps, sts, dss) = tokio::join!(
        list_values(client.clone(), &r_nodes, None),
        list_values(client.clone(), &r_ns, None),
        async {
            let api = builtin::pods().api(client.clone(), namespace);
            tokio::time::timeout(
                Duration::from_secs(30),
                api.list_metadata(&ListParams::default()),
            )
            .await
            .ok()
            .and_then(|r| r.ok())
            .map(|l| l.items.len())
            .unwrap_or(0)
        },
        list_values(client.clone(), &r_dep, namespace),
        list_values(client.clone(), &r_sts, namespace),
        list_values(client.clone(), &r_ds, namespace),
    );
    let node_rows: Vec<_> = nodes.iter().map(|n| summarize("Node", n)).collect();
    let mut f = ClusterFacts {
        cluster_id: id.into(),
        name: conn.name(),
        version: conn.status().version,
        nodes: node_rows.len(),
        ready_nodes: node_rows
            .iter()
            .filter(|r| r.status.starts_with("Ready"))
            .count(),
        cpu_millis: node_rows
            .iter()
            .filter_map(|r| r.cells.get("cpuCapacityMillis").and_then(Value::as_f64))
            .sum(),
        memory_bytes: node_rows
            .iter()
            .filter_map(|r| r.cells.get("memoryCapacityBytes").and_then(Value::as_f64))
            .sum(),
        namespaces: nss
            .iter()
            .filter_map(|n| {
                n.pointer("/metadata/name")
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .collect(),
        pods,
    };
    f.namespaces.sort();
    let mut workloads = BTreeMap::new();
    for (kind, items) in [
        ("Deployment", deps),
        ("StatefulSet", sts),
        ("DaemonSet", dss),
    ] {
        for item in items {
            let row = summarize(kind, &item);
            let images = row
                .cells
                .get("images")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            workloads.insert(
                (
                    row.namespace.clone().unwrap_or_default(),
                    row.name.clone(),
                    kind.to_string(),
                ),
                WorkloadFacts {
                    replicas: row
                        .cells
                        .get("replicas")
                        .or(row.cells.get("desired"))
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                    ready: row
                        .cells
                        .get("ready")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    images,
                },
            );
        }
    }
    Ok((f, workloads))
}

pub async fn compare_clusters(
    mgr: &Arc<ClusterManager>,
    left: &str,
    right: &str,
    namespace: Option<&str>,
) -> AppResult<ClusterComparison> {
    let (l, r) = tokio::join!(facts(mgr, left, namespace), facts(mgr, right, namespace));
    let (lf, lw) = l?;
    let (rf, rw) = r?;
    let lns: BTreeSet<_> = lf.namespaces.iter().cloned().collect();
    let rns: BTreeSet<_> = rf.namespaces.iter().cloned().collect();
    let keys: BTreeSet<_> = lw.keys().chain(rw.keys()).cloned().collect();
    let workloads = keys
        .into_iter()
        .map(|k| {
            let a = lw.get(&k).cloned();
            let b = rw.get(&k).cloned();
            let differs = match (&a, &b) {
                (Some(a), Some(b)) => a.images != b.images || a.replicas != b.replicas,
                _ => true,
            };
            WorkloadDiff {
                namespace: k.0,
                name: k.1,
                kind: k.2,
                left: a,
                right: b,
                differs,
            }
        })
        .collect();
    Ok(ClusterComparison {
        only_left_namespaces: lns.difference(&rns).cloned().collect(),
        only_right_namespaces: rns.difference(&lns).cloned().collect(),
        left: lf,
        right: rf,
        workloads,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppComponent {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub status: String,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub name: String,
    pub namespace: String,
    /// label used for grouping (e.g. app.kubernetes.io/instance)
    pub source: String,
    pub version: Option<String>,
    pub managed_by: Option<String>,
    pub part_of: Option<String>,
    pub health: Health,
    pub components: Vec<AppComponent>,
}

fn label<'a>(labels: &'a serde_json::Map<String, Value>, k: &str) -> Option<&'a str> {
    labels.get(k).and_then(Value::as_str)
}

/// Groups workloads and their exposing resources into logical applications using
/// the recommended `app.kubernetes.io/*` labels (falling back to `app`).
pub async fn list_applications(
    client: Client,
    namespace: Option<&str>,
) -> AppResult<Vec<Application>> {
    let kinds = [
        ("Deployment", builtin::deployments()),
        ("StatefulSet", builtin::statefulsets()),
        ("DaemonSet", builtin::daemonsets()),
        ("CronJob", builtin::cronjobs()),
        ("Service", builtin::services()),
        ("Ingress", builtin::ingresses()),
    ];
    let lists = futures::future::join_all(
        kinds
            .iter()
            .map(|(_, r)| list_values(client.clone(), r, namespace)),
    )
    .await;
    let mut apps: HashMap<(String, String), Application> = HashMap::new();
    for ((kind, _), items) in kinds.iter().zip(lists) {
        for item in items {
            let row = summarize(kind, &item);
            let ns = row.namespace.clone().unwrap_or_default();
            let (key, source) = if let Some(v) = label(&row.labels, "app.kubernetes.io/instance") {
                (v.to_string(), "app.kubernetes.io/instance")
            } else if let Some(v) = label(&row.labels, "app.kubernetes.io/name") {
                (v.to_string(), "app.kubernetes.io/name")
            } else if let Some(v) = label(&row.labels, "app") {
                (v.to_string(), "app")
            } else if *kind != "Service" && *kind != "Ingress" {
                (row.name.clone(), "name")
            } else {
                continue;
            };
            let entry = apps
                .entry((ns.clone(), key.clone()))
                .or_insert_with(|| Application {
                    name: key.clone(),
                    namespace: ns.clone(),
                    source: source.into(),
                    version: None,
                    managed_by: None,
                    part_of: None,
                    health: Health::Neutral,
                    components: vec![],
                });
            if entry.version.is_none() {
                entry.version = label(&row.labels, "app.kubernetes.io/version").map(String::from);
            }
            if entry.managed_by.is_none() {
                entry.managed_by =
                    label(&row.labels, "app.kubernetes.io/managed-by").map(String::from);
            }
            if entry.part_of.is_none() {
                entry.part_of = label(&row.labels, "app.kubernetes.io/part-of").map(String::from);
            }
            entry.components.push(AppComponent {
                kind: kind.to_string(),
                name: row.name.clone(),
                namespace: ns,
                status: row.status.clone(),
                health: row.health,
            });
        }
    }
    let mut out: Vec<Application> = apps
        .into_values()
        .filter(|a| {
            a.components
                .iter()
                .any(|c| !matches!(c.kind.as_str(), "Service" | "Ingress"))
        })
        .map(|mut a| {
            let hs: Vec<Health> = a.components.iter().map(|c| c.health).collect();
            a.health = if hs.contains(&Health::Error) {
                Health::Error
            } else if hs.contains(&Health::Warning) {
                Health::Warning
            } else if hs.contains(&Health::Pending) {
                Health::Pending
            } else if hs.contains(&Health::Ok) {
                Health::Ok
            } else {
                Health::Neutral
            };
            a.components
                .sort_by(|x, y| x.kind.cmp(&y.kind).then(x.name.cmp(&y.name)));
            a
        })
        .collect();
    out.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));
    Ok(out)
}
