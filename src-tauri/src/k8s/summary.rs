//! Converts full Kubernetes objects into compact table rows.
//!
//! Only rows travel over the IPC bridge during watches, which keeps payloads small
//! even for clusters with tens of thousands of objects. The full object is fetched
//! on demand when the user opens the detail view. Status logic mirrors kubectl.

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::quantity;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    Ok,
    Warning,
    Error,
    Pending,
    Neutral,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub uid: String,
    pub name: String,
    pub namespace: Option<String>,
    pub created: Option<String>,
    pub resource_version: Option<String>,
    pub labels: Map<String, Value>,
    pub health: Health,
    pub status: String,
    pub cells: Map<String, Value>,
}

fn s<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    get(v, path).and_then(Value::as_str)
}

fn i(v: &Value, path: &[&str]) -> i64 {
    get(v, path).and_then(Value::as_i64).unwrap_or(0)
}

fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    Some(cur)
}

fn arr<'a>(v: &'a Value, path: &[&str]) -> &'a [Value] {
    get(v, path).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn condition<'a>(v: &'a Value, ty: &str) -> Option<&'a Value> {
    arr(v, &["status", "conditions"]).iter().find(|c| c.get("type").and_then(Value::as_str) == Some(ty))
}

fn condition_true(v: &Value, ty: &str) -> Option<bool> {
    condition(v, ty).and_then(|c| c.get("status")).and_then(Value::as_str).map(|s| s == "True")
}

fn owner(v: &Value) -> (Option<String>, Option<String>) {
    let owners = arr(v, &["metadata", "ownerReferences"]);
    let o = owners
        .iter()
        .find(|o| o.get("controller").and_then(Value::as_bool) == Some(true))
        .or_else(|| owners.first());
    match o {
        Some(o) => (
            o.get("kind").and_then(Value::as_str).map(String::from),
            o.get("name").and_then(Value::as_str).map(String::from),
        ),
        None => (None, None),
    }
}

fn images(spec_pod: Option<&Value>) -> Vec<String> {
    spec_pod
        .and_then(|p| p.get("containers"))
        .and_then(Value::as_array)
        .map(|cs| cs.iter().filter_map(|c| c.get("image").and_then(Value::as_str).map(String::from)).collect())
        .unwrap_or_default()
}

fn selector_string(sel: Option<&Value>) -> String {
    let Some(sel) = sel else { return String::new() };
    let mut parts: Vec<String> = Vec::new();
    let labels = sel.get("matchLabels").or(Some(sel)).and_then(Value::as_object);
    if let Some(m) = labels {
        for (k, v) in m {
            if let Some(v) = v.as_str() {
                parts.push(format!("{k}={v}"));
            }
        }
    }
    if let Some(exprs) = sel.get("matchExpressions").and_then(Value::as_array) {
        for e in exprs {
            let key = e.get("key").and_then(Value::as_str).unwrap_or("");
            let op = e.get("operator").and_then(Value::as_str).unwrap_or("");
            let values: Vec<&str> = e
                .get("values")
                .and_then(Value::as_array)
                .map(|v| v.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            match op {
                "In" => parts.push(format!("{key} in ({})", values.join(","))),
                "NotIn" => parts.push(format!("{key} notin ({})", values.join(","))),
                "Exists" => parts.push(key.to_string()),
                "DoesNotExist" => parts.push(format!("!{key}")),
                _ => {}
            }
        }
    }
    parts.join(",")
}

/// kubectl-compatible pod status computation.
fn pod_status(v: &Value) -> (String, Health, i64, String) {
    let phase = s(v, &["status", "phase"]).unwrap_or("Unknown").to_string();
    let mut reason = s(v, &["status", "reason"]).map(String::from).unwrap_or_else(|| phase.clone());
    let statuses = arr(v, &["status", "containerStatuses"]);
    let init_statuses = arr(v, &["status", "initContainerStatuses"]);
    let total = arr(v, &["spec", "containers"]).len();
    let mut ready = 0;
    let mut restarts = 0i64;

    let mut initializing = false;
    for (idx, cs) in init_statuses.iter().enumerate() {
        restarts += cs.get("restartCount").and_then(Value::as_i64).unwrap_or(0);
        if let Some(t) = get(cs, &["state", "terminated"]) {
            if t.get("exitCode").and_then(Value::as_i64) == Some(0) {
                continue;
            }
            reason = match t.get("reason").and_then(Value::as_str) {
                Some(r) => format!("Init:{r}"),
                None => format!("Init:ExitCode:{}", t.get("exitCode").and_then(Value::as_i64).unwrap_or(0)),
            };
            initializing = true;
        } else if let Some(w) = get(cs, &["state", "waiting"]) {
            let r = w.get("reason").and_then(Value::as_str).unwrap_or("");
            if !r.is_empty() && r != "PodInitializing" {
                reason = format!("Init:{r}");
            } else {
                reason = format!("Init:{}/{}", idx, init_statuses.len());
            }
            initializing = true;
        } else if cs.get("started").and_then(Value::as_bool) == Some(true) {
            // sidecar (restartable init container) that is running
            continue;
        } else {
            reason = format!("Init:{}/{}", idx, init_statuses.len());
            initializing = true;
        }
        break;
    }

    if !initializing {
        let mut has_running = false;
        for cs in statuses.iter().rev() {
            restarts += cs.get("restartCount").and_then(Value::as_i64).unwrap_or(0);
            if let Some(w) = get(cs, &["state", "waiting"]) {
                if let Some(r) = w.get("reason").and_then(Value::as_str) {
                    reason = r.to_string();
                }
            } else if let Some(t) = get(cs, &["state", "terminated"]) {
                reason = match t.get("reason").and_then(Value::as_str) {
                    Some(r) => r.to_string(),
                    None => match t.get("signal").and_then(Value::as_i64) {
                        Some(sig) if sig != 0 => format!("Signal:{sig}"),
                        _ => format!("ExitCode:{}", t.get("exitCode").and_then(Value::as_i64).unwrap_or(0)),
                    },
                };
            } else if cs.get("ready").and_then(Value::as_bool) == Some(true) && get(cs, &["state", "running"]).is_some() {
                has_running = true;
                ready += 1;
            }
        }
        if reason == "Completed" && has_running {
            reason = if condition_true(v, "Ready") == Some(true) { "Running".into() } else { "NotReady".into() };
        }
    } else {
        restarts += statuses.iter().map(|cs| cs.get("restartCount").and_then(Value::as_i64).unwrap_or(0)).sum::<i64>();
    }

    if get(v, &["metadata", "deletionTimestamp"]).is_some() {
        reason = if s(v, &["status", "reason"]) == Some("NodeLost") { "Unknown".into() } else { "Terminating".into() };
    }

    let health = match reason.as_str() {
        "Running" if ready == total => Health::Ok,
        "Running" => Health::Warning,
        "Succeeded" | "Completed" => Health::Neutral,
        "Pending" | "ContainerCreating" | "PodInitializing" | "Terminating" => Health::Pending,
        r if r.starts_with("Init:") && r.contains('/') => Health::Pending,
        "Failed" | "Error" | "CrashLoopBackOff" | "ImagePullBackOff" | "ErrImagePull" | "OOMKilled"
        | "CreateContainerConfigError" | "InvalidImageName" | "Evicted" | "Unknown" | "NodeLost" => Health::Error,
        r if r.starts_with("Init:") || r.starts_with("ExitCode") || r.starts_with("Signal") => Health::Error,
        _ => Health::Warning,
    };
    (reason, health, restarts, format!("{ready}/{total}"))
}

fn ports_string(v: &Value) -> String {
    arr(v, &["spec", "ports"])
        .iter()
        .map(|p| {
            let port = p.get("port").and_then(Value::as_i64).unwrap_or(0);
            let proto = p.get("protocol").and_then(Value::as_str).unwrap_or("TCP");
            match p.get("nodePort").and_then(Value::as_i64) {
                Some(np) => format!("{port}:{np}/{proto}"),
                None => format!("{port}/{proto}"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn ready_ratio(ready: i64, desired: i64) -> Health {
    if desired == 0 {
        Health::Neutral
    } else if ready >= desired {
        Health::Ok
    } else if ready == 0 {
        Health::Error
    } else {
        Health::Warning
    }
}

pub fn summarize(kind: &str, v: &Value) -> Row {
    let meta = v.get("metadata").cloned().unwrap_or(Value::Null);
    let name = meta.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let namespace = meta.get("namespace").and_then(Value::as_str).map(String::from);
    let uid = meta
        .get("uid")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| format!("{}/{}", namespace.clone().unwrap_or_default(), name));
    let labels = meta.get("labels").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut cells = Map::new();
    let mut health = Health::Neutral;
    let mut status = String::new();
    let (owner_kind, owner_name) = owner(v);
    if let Some(k) = &owner_kind {
        cells.insert("ownerKind".into(), json!(k));
        cells.insert("ownerName".into(), json!(owner_name));
    }

    match kind {
        "Pod" => {
            let (reason, h, restarts, ready) = pod_status(v);
            status = reason;
            health = h;
            cells.insert("ready".into(), json!(ready));
            cells.insert("restarts".into(), json!(restarts));
            cells.insert("node".into(), json!(s(v, &["spec", "nodeName"])));
            cells.insert("ip".into(), json!(s(v, &["status", "podIP"])));
            cells.insert("qos".into(), json!(s(v, &["status", "qosClass"])));
            let containers: Vec<&str> =
                arr(v, &["spec", "containers"]).iter().filter_map(|c| c.get("name").and_then(Value::as_str)).collect();
            let init: Vec<&str> = arr(v, &["spec", "initContainers"])
                .iter()
                .filter_map(|c| c.get("name").and_then(Value::as_str))
                .collect();
            cells.insert("containers".into(), json!(containers));
            cells.insert("initContainers".into(), json!(init));
            cells.insert("images".into(), json!(images(get(v, &["spec"]))));
            // aggregated requests for capacity planning views
            let mut cpu = 0.0;
            let mut mem = 0.0;
            for c in arr(v, &["spec", "containers"]) {
                cpu += s(c, &["resources", "requests", "cpu"]).and_then(quantity::cpu_millis).unwrap_or(0.0);
                mem += s(c, &["resources", "requests", "memory"]).and_then(quantity::bytes).unwrap_or(0.0);
            }
            cells.insert("cpuRequestMillis".into(), json!(cpu));
            cells.insert("memoryRequestBytes".into(), json!(mem));
            let last_restart = arr(v, &["status", "containerStatuses"])
                .iter()
                .filter_map(|cs| s(cs, &["lastState", "terminated", "reason"]))
                .next();
            cells.insert("lastTermination".into(), json!(last_restart));
        }
        "Deployment" => {
            let desired = get(v, &["spec", "replicas"]).and_then(Value::as_i64).unwrap_or(1);
            let ready = i(v, &["status", "readyReplicas"]);
            let updated = i(v, &["status", "updatedReplicas"]);
            let available = i(v, &["status", "availableReplicas"]);
            cells.insert("ready".into(), json!(format!("{ready}/{desired}")));
            cells.insert("replicas".into(), json!(desired));
            cells.insert("upToDate".into(), json!(updated));
            cells.insert("available".into(), json!(available));
            cells.insert("images".into(), json!(images(get(v, &["spec", "template", "spec"]))));
            cells.insert("selector".into(), json!(selector_string(get(v, &["spec", "selector"]))));
            cells.insert("strategy".into(), json!(s(v, &["spec", "strategy", "type"])));
            cells.insert("paused".into(), json!(get(v, &["spec", "paused"]).and_then(Value::as_bool).unwrap_or(false)));
            health = ready_ratio(ready, desired);
            status = if desired == 0 { "Scaled to zero".into() } else if ready >= desired { "Available".into() } else { "Progressing".into() };
            if let Some(c) = condition(v, "Progressing") {
                if c.get("reason").and_then(Value::as_str) == Some("ProgressDeadlineExceeded") {
                    health = Health::Error;
                    status = "ProgressDeadlineExceeded".into();
                }
            }
            if condition(v, "ReplicaFailure").and_then(|c| c.get("status")).and_then(Value::as_str) == Some("True") {
                health = Health::Error;
                status = "ReplicaFailure".into();
            }
        }
        "StatefulSet" => {
            let desired = get(v, &["spec", "replicas"]).and_then(Value::as_i64).unwrap_or(1);
            let ready = i(v, &["status", "readyReplicas"]);
            cells.insert("ready".into(), json!(format!("{ready}/{desired}")));
            cells.insert("replicas".into(), json!(desired));
            cells.insert("serviceName".into(), json!(s(v, &["spec", "serviceName"])));
            cells.insert("images".into(), json!(images(get(v, &["spec", "template", "spec"]))));
            cells.insert("selector".into(), json!(selector_string(get(v, &["spec", "selector"]))));
            health = ready_ratio(ready, desired);
            status = if ready >= desired { "Ready".into() } else { "Progressing".into() };
        }
        "DaemonSet" => {
            let desired = i(v, &["status", "desiredNumberScheduled"]);
            let ready = i(v, &["status", "numberReady"]);
            cells.insert("desired".into(), json!(desired));
            cells.insert("current".into(), json!(i(v, &["status", "currentNumberScheduled"])));
            cells.insert("ready".into(), json!(format!("{ready}/{desired}")));
            cells.insert("upToDate".into(), json!(i(v, &["status", "updatedNumberScheduled"])));
            cells.insert("available".into(), json!(i(v, &["status", "numberAvailable"])));
            cells.insert("images".into(), json!(images(get(v, &["spec", "template", "spec"]))));
            cells.insert("selector".into(), json!(selector_string(get(v, &["spec", "selector"]))));
            let ns = get(v, &["spec", "template", "spec", "nodeSelector"]).map(|n| selector_string(Some(n)));
            cells.insert("nodeSelector".into(), json!(ns));
            health = ready_ratio(ready, desired);
            status = if ready >= desired { "Ready".into() } else { "Progressing".into() };
        }
        "ReplicaSet" => {
            let desired = get(v, &["spec", "replicas"]).and_then(Value::as_i64).unwrap_or(1);
            let ready = i(v, &["status", "readyReplicas"]);
            cells.insert("desired".into(), json!(desired));
            cells.insert("current".into(), json!(i(v, &["status", "replicas"])));
            cells.insert("ready".into(), json!(format!("{ready}/{desired}")));
            cells.insert("revision".into(), json!(s(v, &["metadata", "annotations", "deployment.kubernetes.io/revision"])));
            cells.insert("images".into(), json!(images(get(v, &["spec", "template", "spec"]))));
            health = ready_ratio(ready, desired);
            status = if desired == 0 { "Inactive".into() } else if ready >= desired { "Ready".into() } else { "Progressing".into() };
        }
        "Job" => {
            let completions = get(v, &["spec", "completions"]).and_then(Value::as_i64).unwrap_or(1);
            let succeeded = i(v, &["status", "succeeded"]);
            let failed = i(v, &["status", "failed"]);
            let active = i(v, &["status", "active"]);
            cells.insert("completions".into(), json!(format!("{succeeded}/{completions}")));
            cells.insert("active".into(), json!(active));
            cells.insert("failed".into(), json!(failed));
            cells.insert("startTime".into(), json!(s(v, &["status", "startTime"])));
            cells.insert("completionTime".into(), json!(s(v, &["status", "completionTime"])));
            if condition_true(v, "Complete") == Some(true) {
                status = "Complete".into();
                health = Health::Ok;
            } else if condition_true(v, "Failed") == Some(true) {
                status = condition(v, "Failed")
                    .and_then(|c| c.get("reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("Failed")
                    .to_string();
                health = Health::Error;
            } else if get(v, &["spec", "suspend"]).and_then(Value::as_bool) == Some(true) {
                status = "Suspended".into();
                health = Health::Neutral;
            } else {
                status = "Running".into();
                health = Health::Pending;
            }
        }
        "CronJob" => {
            let suspended = get(v, &["spec", "suspend"]).and_then(Value::as_bool).unwrap_or(false);
            cells.insert("schedule".into(), json!(s(v, &["spec", "schedule"])));
            cells.insert("timeZone".into(), json!(s(v, &["spec", "timeZone"])));
            cells.insert("suspend".into(), json!(suspended));
            cells.insert("active".into(), json!(arr(v, &["status", "active"]).len()));
            cells.insert("lastSchedule".into(), json!(s(v, &["status", "lastScheduleTime"])));
            cells.insert("lastSuccessful".into(), json!(s(v, &["status", "lastSuccessfulTime"])));
            status = if suspended { "Suspended".into() } else { "Scheduled".into() };
            health = if suspended { Health::Neutral } else { Health::Ok };
        }
        "Service" => {
            let ty = s(v, &["spec", "type"]).unwrap_or("ClusterIP");
            cells.insert("type".into(), json!(ty));
            cells.insert("clusterIP".into(), json!(s(v, &["spec", "clusterIP"])));
            let mut external: Vec<String> = arr(v, &["status", "loadBalancer", "ingress"])
                .iter()
                .filter_map(|i| i.get("ip").or(i.get("hostname")).and_then(Value::as_str).map(String::from))
                .collect();
            external.extend(arr(v, &["spec", "externalIPs"]).iter().filter_map(|x| x.as_str().map(String::from)));
            if let Some(en) = s(v, &["spec", "externalName"]) {
                external.push(en.to_string());
            }
            cells.insert("externalIP".into(), json!(external.join(", ")));
            cells.insert("ports".into(), json!(ports_string(v)));
            cells.insert("selector".into(), json!(selector_string(get(v, &["spec", "selector"]))));
            if ty == "LoadBalancer" && external.is_empty() {
                status = "Pending".into();
                health = Health::Pending;
            } else {
                status = "Active".into();
                health = Health::Ok;
            }
        }
        "Ingress" => {
            let hosts: Vec<&str> = arr(v, &["spec", "rules"]).iter().filter_map(|r| r.get("host").and_then(Value::as_str)).collect();
            let address: Vec<&str> = arr(v, &["status", "loadBalancer", "ingress"])
                .iter()
                .filter_map(|i| i.get("ip").or(i.get("hostname")).and_then(Value::as_str))
                .collect();
            cells.insert("class".into(), json!(s(v, &["spec", "ingressClassName"])));
            cells.insert("hosts".into(), json!(if hosts.is_empty() { "*".to_string() } else { hosts.join(", ") }));
            cells.insert("address".into(), json!(address.join(", ")));
            cells.insert("tls".into(), json!(!arr(v, &["spec", "tls"]).is_empty()));
            status = if address.is_empty() { "Pending".into() } else { "Active".into() };
            health = if address.is_empty() { Health::Pending } else { Health::Ok };
        }
        "IngressClass" => {
            cells.insert("controller".into(), json!(s(v, &["spec", "controller"])));
            let default = s(v, &["metadata", "annotations", "ingressclass.kubernetes.io/is-default-class"]) == Some("true");
            cells.insert("default".into(), json!(default));
        }
        "NetworkPolicy" => {
            let sel = selector_string(get(v, &["spec", "podSelector"]));
            cells.insert("podSelector".into(), json!(if sel.is_empty() { "<all pods>".to_string() } else { sel }));
            let types: Vec<&str> = arr(v, &["spec", "policyTypes"]).iter().filter_map(Value::as_str).collect();
            cells.insert("policyTypes".into(), json!(types.join(", ")));
            cells.insert("ingressRules".into(), json!(arr(v, &["spec", "ingress"]).len()));
            cells.insert("egressRules".into(), json!(arr(v, &["spec", "egress"]).len()));
        }
        "Gateway" => {
            cells.insert("class".into(), json!(s(v, &["spec", "gatewayClassName"])));
            let addrs: Vec<&str> = arr(v, &["status", "addresses"]).iter().filter_map(|a| a.get("value").and_then(Value::as_str)).collect();
            cells.insert("addresses".into(), json!(addrs.join(", ")));
            let listeners: Vec<String> = arr(v, &["spec", "listeners"])
                .iter()
                .map(|l| {
                    format!(
                        "{}:{}",
                        l.get("protocol").and_then(Value::as_str).unwrap_or("?"),
                        l.get("port").and_then(Value::as_i64).unwrap_or(0)
                    )
                })
                .collect();
            cells.insert("listeners".into(), json!(listeners.join(", ")));
            match condition_true(v, "Programmed") {
                Some(true) => {
                    status = "Programmed".into();
                    health = Health::Ok;
                }
                Some(false) => {
                    status = "Not programmed".into();
                    health = Health::Error;
                }
                None => {
                    status = "Pending".into();
                    health = Health::Pending;
                }
            }
        }
        "GatewayClass" => {
            cells.insert("controller".into(), json!(s(v, &["spec", "controllerName"])));
            match condition_true(v, "Accepted") {
                Some(true) => {
                    status = "Accepted".into();
                    health = Health::Ok;
                }
                Some(false) => {
                    status = "Rejected".into();
                    health = Health::Error;
                }
                None => status = "Unknown".into(),
            }
        }
        "HTTPRoute" | "GRPCRoute" | "TLSRoute" | "TCPRoute" | "UDPRoute" => {
            let hosts: Vec<&str> = arr(v, &["spec", "hostnames"]).iter().filter_map(Value::as_str).collect();
            cells.insert("hostnames".into(), json!(hosts.join(", ")));
            let parents: Vec<&str> = arr(v, &["spec", "parentRefs"]).iter().filter_map(|p| p.get("name").and_then(Value::as_str)).collect();
            cells.insert("parents".into(), json!(parents.join(", ")));
            cells.insert("rules".into(), json!(arr(v, &["spec", "rules"]).len()));
            let accepted = arr(v, &["status", "parents"]).iter().all(|p| {
                p.get("conditions")
                    .and_then(Value::as_array)
                    .map(|cs| {
                        cs.iter().any(|c| {
                            c.get("type").and_then(Value::as_str) == Some("Accepted")
                                && c.get("status").and_then(Value::as_str) == Some("True")
                        })
                    })
                    .unwrap_or(false)
            });
            let has_status = !arr(v, &["status", "parents"]).is_empty();
            status = if !has_status { "Pending".into() } else if accepted { "Accepted".into() } else { "Not accepted".into() };
            health = if !has_status { Health::Pending } else if accepted { Health::Ok } else { Health::Error };
        }
        "PersistentVolume" => {
            let phase = s(v, &["status", "phase"]).unwrap_or("Unknown");
            cells.insert("capacity".into(), json!(s(v, &["spec", "capacity", "storage"])));
            cells.insert("accessModes".into(), json!(arr(v, &["spec", "accessModes"]).iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
            cells.insert("reclaimPolicy".into(), json!(s(v, &["spec", "persistentVolumeReclaimPolicy"])));
            cells.insert("storageClass".into(), json!(s(v, &["spec", "storageClassName"])));
            let claim = match (s(v, &["spec", "claimRef", "namespace"]), s(v, &["spec", "claimRef", "name"])) {
                (Some(ns), Some(n)) => Some(format!("{ns}/{n}")),
                _ => None,
            };
            cells.insert("claim".into(), json!(claim));
            status = phase.to_string();
            health = match phase {
                "Bound" => Health::Ok,
                "Available" => Health::Neutral,
                "Released" => Health::Warning,
                "Failed" => Health::Error,
                _ => Health::Pending,
            };
        }
        "PersistentVolumeClaim" => {
            let phase = s(v, &["status", "phase"]).unwrap_or("Unknown");
            cells.insert("volume".into(), json!(s(v, &["spec", "volumeName"])));
            cells.insert("capacity".into(), json!(s(v, &["status", "capacity", "storage"]).or(s(v, &["spec", "resources", "requests", "storage"]))));
            cells.insert("accessModes".into(), json!(arr(v, &["spec", "accessModes"]).iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
            cells.insert("storageClass".into(), json!(s(v, &["spec", "storageClassName"])));
            status = phase.to_string();
            health = match phase {
                "Bound" => Health::Ok,
                "Pending" => Health::Pending,
                "Lost" => Health::Error,
                _ => Health::Warning,
            };
        }
        "StorageClass" => {
            cells.insert("provisioner".into(), json!(v.get("provisioner")));
            cells.insert("reclaimPolicy".into(), json!(v.get("reclaimPolicy").and_then(Value::as_str).unwrap_or("Delete")));
            cells.insert("volumeBindingMode".into(), json!(v.get("volumeBindingMode").and_then(Value::as_str).unwrap_or("Immediate")));
            cells.insert("allowExpansion".into(), json!(v.get("allowVolumeExpansion").and_then(Value::as_bool).unwrap_or(false)));
            let default = s(v, &["metadata", "annotations", "storageclass.kubernetes.io/is-default-class"]) == Some("true");
            cells.insert("default".into(), json!(default));
        }
        "Node" => {
            let ready = condition_true(v, "Ready");
            let unschedulable = get(v, &["spec", "unschedulable"]).and_then(Value::as_bool).unwrap_or(false);
            let mut st = match ready {
                Some(true) => "Ready".to_string(),
                Some(false) => "NotReady".to_string(),
                None => "Unknown".to_string(),
            };
            if unschedulable {
                st.push_str(",SchedulingDisabled");
            }
            let pressures: Vec<&str> = ["MemoryPressure", "DiskPressure", "PIDPressure", "NetworkUnavailable"]
                .into_iter()
                .filter(|c| condition_true(v, c) == Some(true))
                .collect();
            health = match (ready, pressures.is_empty(), unschedulable) {
                (Some(true), true, false) => Health::Ok,
                (Some(true), _, _) => Health::Warning,
                _ => Health::Error,
            };
            status = st;
            let roles: Vec<String> = labels
                .keys()
                .filter_map(|k| k.strip_prefix("node-role.kubernetes.io/").map(String::from))
                .filter(|r| !r.is_empty())
                .collect();
            cells.insert("roles".into(), json!(if roles.is_empty() { "<none>".to_string() } else { roles.join(",") }));
            cells.insert("version".into(), json!(s(v, &["status", "nodeInfo", "kubeletVersion"])));
            cells.insert("os".into(), json!(s(v, &["status", "nodeInfo", "osImage"])));
            cells.insert("kernel".into(), json!(s(v, &["status", "nodeInfo", "kernelVersion"])));
            cells.insert("runtime".into(), json!(s(v, &["status", "nodeInfo", "containerRuntimeVersion"])));
            cells.insert("arch".into(), json!(s(v, &["status", "nodeInfo", "architecture"])));
            let internal_ip = arr(v, &["status", "addresses"])
                .iter()
                .find(|a| a.get("type").and_then(Value::as_str) == Some("InternalIP"))
                .and_then(|a| a.get("address"))
                .and_then(Value::as_str);
            cells.insert("internalIP".into(), json!(internal_ip));
            cells.insert("cpuCapacityMillis".into(), json!(s(v, &["status", "capacity", "cpu"]).and_then(quantity::cpu_millis)));
            cells.insert("memoryCapacityBytes".into(), json!(s(v, &["status", "capacity", "memory"]).and_then(quantity::bytes)));
            cells.insert("cpuAllocatableMillis".into(), json!(s(v, &["status", "allocatable", "cpu"]).and_then(quantity::cpu_millis)));
            cells.insert("memoryAllocatableBytes".into(), json!(s(v, &["status", "allocatable", "memory"]).and_then(quantity::bytes)));
            cells.insert("podsCapacity".into(), json!(s(v, &["status", "capacity", "pods"])));
            cells.insert("taints".into(), json!(arr(v, &["spec", "taints"]).len()));
            cells.insert("unschedulable".into(), json!(unschedulable));
            cells.insert("pressures".into(), json!(pressures));
        }
        "Namespace" => {
            let phase = s(v, &["status", "phase"]).unwrap_or("Active");
            status = phase.to_string();
            health = if phase == "Active" { Health::Ok } else { Health::Pending };
        }
        "Event" => {
            let ty = v.get("type").and_then(Value::as_str).unwrap_or("Normal");
            let count = v.get("count").and_then(Value::as_i64).or_else(|| get(v, &["series", "count"]).and_then(Value::as_i64)).unwrap_or(1);
            let last = v
                .get("lastTimestamp")
                .and_then(Value::as_str)
                .or_else(|| get(v, &["series", "lastObservedTime"]).and_then(Value::as_str))
                .or_else(|| v.get("eventTime").and_then(Value::as_str))
                .or_else(|| s(v, &["metadata", "creationTimestamp"]));
            cells.insert("type".into(), json!(ty));
            cells.insert("reason".into(), json!(v.get("reason")));
            cells.insert("message".into(), json!(v.get("message")));
            cells.insert("count".into(), json!(count));
            cells.insert("lastSeen".into(), json!(last));
            cells.insert("objectKind".into(), json!(s(v, &["involvedObject", "kind"])));
            cells.insert("objectName".into(), json!(s(v, &["involvedObject", "name"])));
            cells.insert("objectNamespace".into(), json!(s(v, &["involvedObject", "namespace"])));
            cells.insert("source".into(), json!(s(v, &["source", "component"]).or(v.get("reportingComponent").and_then(Value::as_str))));
            status = ty.to_string();
            health = if ty == "Warning" { Health::Warning } else { Health::Neutral };
        }
        "ConfigMap" => {
            let n = v.get("data").and_then(Value::as_object).map(|m| m.len()).unwrap_or(0)
                + v.get("binaryData").and_then(Value::as_object).map(|m| m.len()).unwrap_or(0);
            cells.insert("keys".into(), json!(n));
        }
        "Secret" => {
            // never include secret values in rows
            cells.insert("type".into(), json!(v.get("type")));
            cells.insert("keys".into(), json!(v.get("data").and_then(Value::as_object).map(|m| m.len()).unwrap_or(0)));
        }
        "ServiceAccount" => {
            cells.insert("secrets".into(), json!(v.get("secrets").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0)));
        }
        "Role" | "ClusterRole" => {
            cells.insert("rules".into(), json!(v.get("rules").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0)));
            cells.insert("aggregated".into(), json!(v.get("aggregationRule").is_some()));
        }
        "RoleBinding" | "ClusterRoleBinding" => {
            cells.insert("role".into(), json!(format!("{}/{}", s(v, &["roleRef", "kind"]).unwrap_or(""), s(v, &["roleRef", "name"]).unwrap_or(""))));
            let subjects: Vec<String> = arr(v, &["subjects"])
                .iter()
                .map(|s| format!("{}:{}", s.get("kind").and_then(Value::as_str).unwrap_or(""), s.get("name").and_then(Value::as_str).unwrap_or("")))
                .collect();
            cells.insert("subjects".into(), json!(subjects.join(", ")));
        }
        "HorizontalPodAutoscaler" => {
            cells.insert("reference".into(), json!(format!("{}/{}", s(v, &["spec", "scaleTargetRef", "kind"]).unwrap_or(""), s(v, &["spec", "scaleTargetRef", "name"]).unwrap_or(""))));
            cells.insert("minReplicas".into(), json!(get(v, &["spec", "minReplicas"]).and_then(Value::as_i64).unwrap_or(1)));
            cells.insert("maxReplicas".into(), json!(i(v, &["spec", "maxReplicas"])));
            cells.insert("replicas".into(), json!(i(v, &["status", "currentReplicas"])));
            if condition_true(v, "ScalingActive") == Some(false) {
                health = Health::Warning;
                status = "Scaling inactive".into();
            } else {
                health = Health::Ok;
                status = "Active".into();
            }
        }
        "PodDisruptionBudget" => {
            cells.insert("minAvailable".into(), json!(get(v, &["spec", "minAvailable"])));
            cells.insert("maxUnavailable".into(), json!(get(v, &["spec", "maxUnavailable"])));
            let allowed = i(v, &["status", "disruptionsAllowed"]);
            cells.insert("allowedDisruptions".into(), json!(allowed));
            health = if allowed > 0 { Health::Ok } else { Health::Warning };
        }
        "EndpointSlice" => {
            cells.insert("addressType".into(), json!(v.get("addressType")));
            cells.insert("endpoints".into(), json!(v.get("endpoints").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0)));
        }
        "CustomResourceDefinition" => {
            cells.insert("group".into(), json!(s(v, &["spec", "group"])));
            cells.insert("scope".into(), json!(s(v, &["spec", "scope"])));
            cells.insert("kindName".into(), json!(s(v, &["spec", "names", "kind"])));
            let versions: Vec<&str> = arr(v, &["spec", "versions"]).iter().filter_map(|x| x.get("name").and_then(Value::as_str)).collect();
            cells.insert("versions".into(), json!(versions.join(", ")));
        }
        _ => {
            // Generic: surface Ready condition if present (common for CRDs).
            if let Some(ready) = condition_true(v, "Ready") {
                status = if ready { "Ready".into() } else { "NotReady".into() };
                health = if ready { Health::Ok } else { Health::Warning };
            } else if let Some(phase) = s(v, &["status", "phase"]) {
                status = phase.to_string();
            }
        }
    }

    Row {
        uid,
        name,
        namespace,
        created: meta.get("creationTimestamp").and_then(Value::as_str).map(String::from),
        resource_version: meta.get("resourceVersion").and_then(Value::as_str).map(String::from),
        labels,
        health,
        status,
        cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_pod() {
        let pod = json!({
            "metadata": {"name": "web-1", "namespace": "default", "uid": "u1", "creationTimestamp": "2024-01-01T00:00:00Z"},
            "spec": {"nodeName": "n1", "containers": [{"name": "app", "image": "nginx", "resources": {"requests": {"cpu": "100m", "memory": "64Mi"}}}]},
            "status": {"phase": "Running", "podIP": "10.0.0.1",
                "containerStatuses": [{"name": "app", "ready": true, "restartCount": 2, "state": {"running": {}}}]}
        });
        let r = summarize("Pod", &pod);
        assert_eq!(r.status, "Running");
        assert_eq!(r.health, Health::Ok);
        assert_eq!(r.cells["ready"], "1/1");
        assert_eq!(r.cells["restarts"], 2);
        assert_eq!(r.cells["cpuRequestMillis"], 100.0);
    }

    #[test]
    fn crashlooping_pod() {
        let pod = json!({
            "metadata": {"name": "bad", "namespace": "default", "uid": "u2"},
            "spec": {"containers": [{"name": "app"}]},
            "status": {"phase": "Running",
                "containerStatuses": [{"name": "app", "ready": false, "restartCount": 7, "state": {"waiting": {"reason": "CrashLoopBackOff"}}}]}
        });
        let r = summarize("Pod", &pod);
        assert_eq!(r.status, "CrashLoopBackOff");
        assert_eq!(r.health, Health::Error);
    }

    #[test]
    fn terminating_pod() {
        let pod = json!({
            "metadata": {"name": "x", "uid": "u3", "deletionTimestamp": "2024-01-01T00:00:00Z"},
            "spec": {"containers": [{"name": "app"}]},
            "status": {"phase": "Running"}
        });
        assert_eq!(summarize("Pod", &pod).status, "Terminating");
    }

    #[test]
    fn init_container_pod() {
        let pod = json!({
            "metadata": {"name": "x", "uid": "u4"},
            "spec": {"containers": [{"name": "app"}], "initContainers": [{"name": "init"}]},
            "status": {"phase": "Pending", "initContainerStatuses": [{"name": "init", "restartCount": 0, "state": {"running": {}}}]}
        });
        let r = summarize("Pod", &pod);
        assert_eq!(r.status, "Init:0/1");
        assert_eq!(r.health, Health::Pending);
    }

    #[test]
    fn deployment_progress_deadline() {
        let d = json!({
            "metadata": {"name": "api", "uid": "d1"},
            "spec": {"replicas": 3, "selector": {"matchLabels": {"app": "api"}}, "template": {"spec": {"containers": [{"image": "api:1"}]}}},
            "status": {"readyReplicas": 1, "conditions": [{"type": "Progressing", "status": "False", "reason": "ProgressDeadlineExceeded"}]}
        });
        let r = summarize("Deployment", &d);
        assert_eq!(r.health, Health::Error);
        assert_eq!(r.cells["ready"], "1/3");
        assert_eq!(r.cells["selector"], "app=api");
    }

    #[test]
    fn node_status() {
        let n = json!({
            "metadata": {"name": "n1", "uid": "n1", "labels": {"node-role.kubernetes.io/control-plane": ""}},
            "spec": {"unschedulable": true},
            "status": {"conditions": [{"type": "Ready", "status": "True"}], "capacity": {"cpu": "4", "memory": "8Gi"}}
        });
        let r = summarize("Node", &n);
        assert_eq!(r.status, "Ready,SchedulingDisabled");
        assert_eq!(r.health, Health::Warning);
        assert_eq!(r.cells["roles"], "control-plane");
        assert_eq!(r.cells["cpuCapacityMillis"], 4000.0);
    }

    #[test]
    fn secret_row_has_no_values() {
        let sec = json!({"metadata": {"name": "s", "uid": "s"}, "type": "Opaque", "data": {"password": "c2VjcmV0"}});
        let r = summarize("Secret", &sec);
        let txt = serde_json::to_string(&r).unwrap();
        assert!(!txt.contains("c2VjcmV0"));
        assert_eq!(r.cells["keys"], 1);
    }

    #[test]
    fn service_ports() {
        let svc = json!({"metadata": {"name": "s", "uid": "s"}, "spec": {"type": "NodePort", "clusterIP": "10.0.0.5",
            "ports": [{"port": 80, "nodePort": 30080, "protocol": "TCP"}]}});
        let r = summarize("Service", &svc);
        assert_eq!(r.cells["ports"], "80:30080/TCP");
    }
}
