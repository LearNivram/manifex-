//! Resource metrics (metrics.k8s.io) and capacity overview.

use std::collections::HashMap;
use std::time::Duration;

use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{ApiResource, DynamicObject, ListParams};
use kube::{Api, Client};
use serde::Serialize;
use serde_json::Value;

use super::quantity;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetric {
    pub name: String,
    pub cpu_millis: f64,
    pub memory_bytes: f64,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerMetric {
    pub name: String,
    pub cpu_millis: f64,
    pub memory_bytes: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodMetric {
    pub name: String,
    pub namespace: String,
    pub cpu_millis: f64,
    pub memory_bytes: f64,
    pub containers: Vec<ContainerMetric>,
    pub timestamp: Option<String>,
}

fn metrics_api(client: Client, plural: &str, kind: &str, ns: Option<&str>) -> Api<DynamicObject> {
    let ar = ApiResource {
        group: "metrics.k8s.io".into(),
        version: "v1beta1".into(),
        api_version: "metrics.k8s.io/v1beta1".into(),
        kind: kind.into(),
        plural: plural.into(),
    };
    match ns.filter(|n| !n.is_empty()) {
        Some(ns) => Api::namespaced_with(client, ns, &ar),
        None => Api::all_with(client, &ar),
    }
}

fn usage(v: &Value) -> (f64, f64) {
    let cpu = v.pointer("/usage/cpu").and_then(Value::as_str).and_then(quantity::cpu_millis).unwrap_or(0.0);
    let mem = v.pointer("/usage/memory").and_then(Value::as_str).and_then(quantity::bytes).unwrap_or(0.0);
    (cpu, mem)
}

fn map_metrics_err(e: kube::Error) -> AppError {
    match &e {
        kube::Error::Api(s) if s.code == 404 || s.code == 503 => {
            AppError::NotFound("metrics API (metrics-server) is not available in this cluster".into())
        }
        _ => e.into(),
    }
}

pub async fn node_metrics(client: Client) -> AppResult<Vec<NodeMetric>> {
    let api = metrics_api(client, "nodes", "NodeMetrics", None);
    let list = tokio::time::timeout(Duration::from_secs(20), api.list(&ListParams::default()))
        .await
        .map_err(|_| AppError::Connection("metrics request timed out".into()))?
        .map_err(map_metrics_err)?;
    Ok(list
        .items
        .into_iter()
        .map(|o| {
            let (cpu, mem) = usage(&o.data);
            NodeMetric {
                name: o.metadata.name.unwrap_or_default(),
                cpu_millis: cpu,
                memory_bytes: mem,
                timestamp: o.data.get("timestamp").and_then(Value::as_str).map(String::from),
            }
        })
        .collect())
}

pub async fn pod_metrics(client: Client, namespace: Option<&str>) -> AppResult<Vec<PodMetric>> {
    let api = metrics_api(client, "pods", "PodMetrics", namespace);
    let list = tokio::time::timeout(Duration::from_secs(30), api.list(&ListParams::default()))
        .await
        .map_err(|_| AppError::Connection("metrics request timed out".into()))?
        .map_err(map_metrics_err)?;
    Ok(list
        .items
        .into_iter()
        .map(|o| {
            let containers: Vec<ContainerMetric> = o
                .data
                .get("containers")
                .and_then(Value::as_array)
                .map(|cs| {
                    cs.iter()
                        .map(|c| {
                            let (cpu, mem) = usage(c);
                            ContainerMetric {
                                name: c.get("name").and_then(Value::as_str).unwrap_or("").into(),
                                cpu_millis: cpu,
                                memory_bytes: mem,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            PodMetric {
                name: o.metadata.name.unwrap_or_default(),
                namespace: o.metadata.namespace.unwrap_or_default(),
                cpu_millis: containers.iter().map(|c| c.cpu_millis).sum(),
                memory_bytes: containers.iter().map(|c| c.memory_bytes).sum(),
                containers,
                timestamp: o.data.get("timestamp").and_then(Value::as_str).map(String::from),
            }
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTotals {
    pub capacity: f64,
    pub allocatable: f64,
    pub requests: f64,
    pub limits: f64,
    pub usage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NodeCapacity {
    pub name: String,
    pub ready: bool,
    pub cpu: ResourceTotals,
    pub memory: ResourceTotals,
    pub pods: i64,
    pub pod_capacity: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClusterCapacity {
    pub nodes: Vec<NodeCapacity>,
    pub cpu: ResourceTotals,
    pub memory: ResourceTotals,
    pub pods: i64,
    pub pod_capacity: i64,
    pub metrics_available: bool,
}

fn qty_cpu(m: Option<&std::collections::BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>>, key: &str) -> f64 {
    m.and_then(|m| m.get(key)).and_then(|q| quantity::cpu_millis(&q.0)).unwrap_or(0.0)
}

fn qty_bytes(m: Option<&std::collections::BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>>, key: &str) -> f64 {
    m.and_then(|m| m.get(key)).and_then(|q| quantity::bytes(&q.0)).unwrap_or(0.0)
}

/// Capacity, requests, limits and (if metrics-server is installed) live usage per node.
pub async fn cluster_capacity(client: Client) -> AppResult<ClusterCapacity> {
    let nodes: Api<Node> = Api::all(client.clone());
    let pods: Api<Pod> = Api::all(client.clone());
    let lp = ListParams::default().fields("status.phase!=Succeeded,status.phase!=Failed");
    let all = ListParams::default();
    let (node_list, pod_list, metrics) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(30), nodes.list(&all)),
        tokio::time::timeout(Duration::from_secs(60), pods.list(&lp)),
        node_metrics(client.clone()),
    );
    let node_list = node_list.map_err(|_| AppError::Connection("node list timed out".into()))??;
    let pod_list = pod_list.map_err(|_| AppError::Connection("pod list timed out".into()))??;
    let metrics_available = metrics.is_ok();
    let metrics: HashMap<String, NodeMetric> =
        metrics.as_ref().map(|m| m.iter().map(|n| (n.name.clone(), n.clone())).collect()).unwrap_or_default();

    let mut per_node: HashMap<String, NodeCapacity> = HashMap::new();
    for n in &node_list.items {
        let name = n.metadata.name.clone().unwrap_or_default();
        let status = n.status.as_ref();
        let ready = status
            .and_then(|s| s.conditions.as_ref())
            .map(|cs| cs.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
            .unwrap_or(false);
        let cap = status.and_then(|s| s.capacity.as_ref());
        let alloc = status.and_then(|s| s.allocatable.as_ref());
        let m = metrics.get(&name);
        per_node.insert(
            name.clone(),
            NodeCapacity {
                name,
                ready,
                cpu: ResourceTotals {
                    capacity: qty_cpu(cap, "cpu"),
                    allocatable: qty_cpu(alloc, "cpu"),
                    usage: m.map(|m| m.cpu_millis),
                    ..Default::default()
                },
                memory: ResourceTotals {
                    capacity: qty_bytes(cap, "memory"),
                    allocatable: qty_bytes(alloc, "memory"),
                    usage: m.map(|m| m.memory_bytes),
                    ..Default::default()
                },
                pods: 0,
                pod_capacity: cap.and_then(|c| c.get("pods")).and_then(|q| q.0.parse().ok()).unwrap_or(0),
            },
        );
    }
    for p in &pod_list.items {
        let Some(node) = p.spec.as_ref().and_then(|s| s.node_name.clone()) else { continue };
        let Some(entry) = per_node.get_mut(&node) else { continue };
        entry.pods += 1;
        if let Some(spec) = &p.spec {
            for c in &spec.containers {
                let r = c.resources.as_ref();
                entry.cpu.requests += qty_cpu(r.and_then(|r| r.requests.as_ref()), "cpu");
                entry.cpu.limits += qty_cpu(r.and_then(|r| r.limits.as_ref()), "cpu");
                entry.memory.requests += qty_bytes(r.and_then(|r| r.requests.as_ref()), "memory");
                entry.memory.limits += qty_bytes(r.and_then(|r| r.limits.as_ref()), "memory");
            }
        }
    }
    let mut out = ClusterCapacity { metrics_available, ..Default::default() };
    let mut nodes: Vec<NodeCapacity> = per_node.into_values().collect();
    nodes.sort_by(|a, b| a.name.cmp(&b.name));
    let mut cpu_usage = 0.0;
    let mut mem_usage = 0.0;
    for n in &nodes {
        out.cpu.capacity += n.cpu.capacity;
        out.cpu.allocatable += n.cpu.allocatable;
        out.cpu.requests += n.cpu.requests;
        out.cpu.limits += n.cpu.limits;
        out.memory.capacity += n.memory.capacity;
        out.memory.allocatable += n.memory.allocatable;
        out.memory.requests += n.memory.requests;
        out.memory.limits += n.memory.limits;
        cpu_usage += n.cpu.usage.unwrap_or(0.0);
        mem_usage += n.memory.usage.unwrap_or(0.0);
        out.pods += n.pods;
        out.pod_capacity += n.pod_capacity;
    }
    if out.metrics_available {
        out.cpu.usage = Some(cpu_usage);
        out.memory.usage = Some(mem_usage);
    }
    out.nodes = nodes;
    Ok(out)
}
