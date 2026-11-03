//! Resource operations: read, apply, replace, delete, scale, restart, rollouts,
//! node maintenance and CronJob actions. All functions talk to the real API server.

use std::time::Duration;

use k8s_openapi::api::core::v1::Pod;
use kube::api::{DeleteParams, DynamicObject, EvictParams, ListParams, Patch, PatchParams, PostParams, PropagationPolicy};
use kube::{Api, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::connection::ClusterConn;
use super::resources::{builtin, resolve_kind, ResourceRef};
use crate::error::{AppError, AppResult};
use crate::security::validate;

pub const FIELD_MANAGER: &str = "manifex";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

async fn timed<T>(fut: impl std::future::Future<Output = Result<T, kube::Error>>) -> AppResult<T> {
    match tokio::time::timeout(REQUEST_TIMEOUT, fut).await {
        Ok(r) => r.map_err(AppError::from),
        Err(_) => Err(AppError::Connection("request timed out after 30s".into())),
    }
}

/// Removes noisy server-managed fields for display / editing.
pub fn clean_for_display(mut v: Value) -> Value {
    if let Some(meta) = v.get_mut("metadata").and_then(Value::as_object_mut) {
        meta.remove("managedFields");
    }
    v
}

pub fn to_yaml(v: &Value) -> AppResult<String> {
    Ok(serde_yaml::to_string(v)?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectView {
    pub object: Value,
    pub yaml: String,
}

pub async fn get_object(client: Client, res: &ResourceRef, ns: Option<&str>, name: &str) -> AppResult<ObjectView> {
    res.validate()?;
    validate::object_name(name)?;
    let api = res.api_for_object(client, ns)?;
    let obj = timed(api.get(name)).await?;
    let value = clean_for_display(serde_json::to_value(&obj)?);
    let yaml = to_yaml(&value)?;
    Ok(ObjectView { object: value, yaml })
}

pub async fn list_objects(
    client: Client,
    res: &ResourceRef,
    ns: Option<&str>,
    label_selector: Option<&str>,
    field_selector: Option<&str>,
    limit: Option<u32>,
) -> AppResult<Vec<Value>> {
    res.validate()?;
    let api = res.api(client, ns);
    let mut lp = ListParams::default();
    if let Some(l) = label_selector.filter(|s| !s.is_empty()) {
        validate::selector(l)?;
        lp = lp.labels(l);
    }
    if let Some(f) = field_selector.filter(|s| !s.is_empty()) {
        validate::selector(f)?;
        lp = lp.fields(f);
    }
    if let Some(l) = limit {
        lp = lp.limit(l);
    }
    let list = timed(api.list(&lp)).await?;
    list.items.into_iter().map(|o| Ok(clean_for_display(serde_json::to_value(o)?))).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
    /// created | configured | replaced | error
    pub action: String,
    pub dry_run: bool,
    pub yaml: Option<String>,
    pub previous_yaml: Option<String>,
    pub error: Option<String>,
}

/// Parses a (multi-document) YAML string into JSON values, expanding `List` kinds.
pub fn parse_manifests(yaml: &str) -> AppResult<Vec<Value>> {
    use serde::Deserialize as _;
    if yaml.len() > 8 * 1024 * 1024 {
        return Err(AppError::validation("manifest is larger than 8 MiB"));
    }
    let mut out = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(yaml) {
        let value = serde_yaml::Value::deserialize(doc)?;
        if value.is_null() {
            continue;
        }
        let json: Value = serde_json::to_value(value)?;
        if json.get("kind").and_then(Value::as_str).map(|k| k.ends_with("List")).unwrap_or(false) {
            if let Some(items) = json.get("items").and_then(Value::as_array) {
                out.extend(items.iter().cloned());
                continue;
            }
        }
        out.push(json);
    }
    if out.is_empty() {
        return Err(AppError::validation("no Kubernetes objects found in the document"));
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApplyMode {
    /// Server-side apply (create or update, field ownership aware).
    Apply,
    /// Full replacement with optimistic concurrency (resourceVersion), like `kubectl edit`.
    Replace,
}

pub async fn apply_manifests(
    conn: &ClusterConn,
    yaml: &str,
    default_namespace: Option<&str>,
    mode: ApplyMode,
    dry_run: bool,
    force_conflicts: bool,
) -> AppResult<Vec<ApplyResult>> {
    let docs = parse_manifests(yaml)?;
    let mut discovery = conn.discovery(false).await?;
    let mut results = Vec::with_capacity(docs.len());
    for mut doc in docs {
        let api_version = doc.get("apiVersion").and_then(Value::as_str).unwrap_or("").to_string();
        let kind = doc.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
        let name = doc.pointer("/metadata/name").and_then(Value::as_str).unwrap_or("").to_string();
        let mut result = ApplyResult {
            api_version: api_version.clone(),
            kind: kind.clone(),
            name: name.clone(),
            namespace: None,
            action: "error".into(),
            dry_run,
            yaml: None,
            previous_yaml: None,
            error: None,
        };
        let outcome: AppResult<()> = async {
            if api_version.is_empty() || kind.is_empty() {
                return Err(AppError::validation("apiVersion and kind are required"));
            }
            if name.is_empty() {
                return Err(AppError::validation("metadata.name is required"));
            }
            validate::object_name(&name)?;
            let res = match resolve_kind(&discovery, &api_version, &kind) {
                Ok(r) => r,
                Err(_) => {
                    // A CRD may have been created moments ago – refresh discovery once.
                    discovery = conn.discovery(true).await?;
                    resolve_kind(&discovery, &api_version, &kind)?
                }
            };
            let ns = if res.namespaced {
                let ns = doc
                    .pointer("/metadata/namespace")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| default_namespace.map(String::from))
                    .unwrap_or_else(|| conn.default_namespace.clone());
                validate::namespace(&ns)?;
                doc["metadata"]["namespace"] = json!(ns);
                Some(ns)
            } else {
                if let Some(m) = doc.get_mut("metadata").and_then(Value::as_object_mut) {
                    m.remove("namespace");
                }
                None
            };
            result.namespace = ns.clone();
            let api = res.api_for_object(conn.client.clone(), ns.as_deref())?;
            let existing = timed(api.get_opt(&name)).await?;
            if let Some(e) = &existing {
                result.previous_yaml = Some(to_yaml(&clean_for_display(serde_json::to_value(e)?))?);
            }
            let applied: DynamicObject = match mode {
                ApplyMode::Apply => {
                    if let Some(m) = doc.get_mut("metadata").and_then(Value::as_object_mut) {
                        m.remove("managedFields");
                        m.remove("resourceVersion");
                        m.remove("uid");
                        m.remove("creationTimestamp");
                    }
                    let mut pp = PatchParams::apply(FIELD_MANAGER).validation_strict();
                    if force_conflicts {
                        pp = pp.force();
                    }
                    if dry_run {
                        pp = pp.dry_run();
                    }
                    timed(api.patch(&name, &pp, &Patch::Apply(&doc))).await?
                }
                ApplyMode::Replace => {
                    if let Some(m) = doc.get_mut("metadata").and_then(Value::as_object_mut) {
                        m.remove("managedFields");
                    }
                    let obj: DynamicObject = serde_json::from_value(doc.clone())?;
                    let pp = PostParams { dry_run, field_manager: Some(FIELD_MANAGER.into()) };
                    if existing.is_some() {
                        timed(api.replace(&name, &pp, &obj)).await?
                    } else {
                        timed(api.create(&pp, &obj)).await?
                    }
                }
            };
            result.action = match (existing.is_some(), mode) {
                (false, _) => "created".into(),
                (true, ApplyMode::Apply) => "configured".into(),
                (true, ApplyMode::Replace) => "replaced".into(),
            };
            result.yaml = Some(to_yaml(&clean_for_display(serde_json::to_value(&applied)?))?);
            Ok(())
        }
        .await;
        if let Err(e) = outcome {
            result.action = "error".into();
            result.error = Some(e.to_string());
        }
        results.push(result);
    }
    Ok(results)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeleteOptions {
    /// Foreground | Background | Orphan
    pub propagation: Option<String>,
    pub grace_period_seconds: Option<u32>,
    pub dry_run: bool,
}

pub async fn delete_object(client: Client, res: &ResourceRef, ns: Option<&str>, name: &str, opts: &DeleteOptions) -> AppResult<()> {
    res.validate()?;
    validate::object_name(name)?;
    let api = res.api_for_object(client, ns)?;
    let propagation_policy = match opts.propagation.as_deref() {
        None | Some("") => None,
        Some("Foreground") => Some(PropagationPolicy::Foreground),
        Some("Background") => Some(PropagationPolicy::Background),
        Some("Orphan") => Some(PropagationPolicy::Orphan),
        Some(other) => return Err(AppError::validation(format!("unknown propagation policy {other}"))),
    };
    let dp = DeleteParams {
        dry_run: opts.dry_run,
        grace_period_seconds: opts.grace_period_seconds,
        propagation_policy,
        ..Default::default()
    };
    timed(api.delete(name, &dp)).await?;
    Ok(())
}

pub async fn scale(client: Client, res: &ResourceRef, ns: &str, name: &str, replicas: i32) -> AppResult<()> {
    if !matches!(res.kind.as_str(), "Deployment" | "StatefulSet" | "ReplicaSet") {
        return Err(AppError::validation(format!("{} cannot be scaled", res.kind)));
    }
    if !(0..=10_000).contains(&replicas) {
        return Err(AppError::validation("replicas must be between 0 and 10000"));
    }
    validate::object_name(name)?;
    let api = res.api_for_object(client, Some(ns))?;
    let patch = json!({ "spec": { "replicas": replicas } });
    timed(api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))).await?;
    Ok(())
}

pub async fn restart(client: Client, res: &ResourceRef, ns: &str, name: &str) -> AppResult<()> {
    if !matches!(res.kind.as_str(), "Deployment" | "StatefulSet" | "DaemonSet") {
        return Err(AppError::validation(format!("{} cannot be restarted", res.kind)));
    }
    validate::object_name(name)?;
    let api = res.api_for_object(client, Some(ns))?;
    let patch = json!({
        "spec": { "template": { "metadata": { "annotations": {
            "kubectl.kubernetes.io/restartedAt": chrono::Utc::now().to_rfc3339()
        }}}}
    });
    timed(api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))).await?;
    Ok(())
}

pub async fn set_paused(client: Client, ns: &str, name: &str, paused: bool) -> AppResult<()> {
    validate::object_name(name)?;
    let api = builtin::deployments().api_for_object(client, Some(ns))?;
    let patch = json!({ "spec": { "paused": paused } });
    timed(api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloutRevision {
    pub revision: i64,
    pub replica_set: String,
    pub images: Vec<String>,
    pub created: Option<String>,
    pub replicas: i64,
    pub change_cause: Option<String>,
    pub current: bool,
}

pub async fn rollout_history(client: Client, ns: &str, name: &str) -> AppResult<Vec<RolloutRevision>> {
    validate::object_name(name)?;
    let dep_api = builtin::deployments().api_for_object(client.clone(), Some(ns))?;
    let dep = timed(dep_api.get(name)).await?;
    let dep_uid = dep.metadata.uid.clone().unwrap_or_default();
    let current_rev = dep
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get("deployment.kubernetes.io/revision"))
        .and_then(|r| r.parse::<i64>().ok());
    let rs_api = builtin::replicasets().api(client, Some(ns));
    let selector = super::summary::summarize("Deployment", &serde_json::to_value(&dep)?)
        .cells
        .get("selector")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut lp = ListParams::default();
    if !selector.is_empty() {
        lp = lp.labels(&selector);
    }
    let rs_list = timed(rs_api.list(&lp)).await?;
    let mut out: Vec<RolloutRevision> = rs_list
        .items
        .into_iter()
        .filter(|rs| rs.metadata.owner_references.as_ref().map(|o| o.iter().any(|o| o.uid == dep_uid)).unwrap_or(false))
        .filter_map(|rs| {
            let v = serde_json::to_value(&rs).ok()?;
            let ann = v.pointer("/metadata/annotations");
            let revision = ann
                .and_then(|a| a.get("deployment.kubernetes.io/revision"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok())?;
            Some(RolloutRevision {
                revision,
                replica_set: rs.metadata.name.clone().unwrap_or_default(),
                images: v
                    .pointer("/spec/template/spec/containers")
                    .and_then(Value::as_array)
                    .map(|cs| cs.iter().filter_map(|c| c.get("image").and_then(Value::as_str).map(String::from)).collect())
                    .unwrap_or_default(),
                created: v.pointer("/metadata/creationTimestamp").and_then(Value::as_str).map(String::from),
                replicas: v.pointer("/status/replicas").and_then(Value::as_i64).unwrap_or(0),
                change_cause: ann.and_then(|a| a.get("kubernetes.io/change-cause")).and_then(Value::as_str).map(String::from),
                current: Some(revision) == current_rev,
            })
        })
        .collect();
    out.sort_by(|a, b| b.revision.cmp(&a.revision));
    Ok(out)
}

/// Equivalent of `kubectl rollout undo --to-revision`.
pub async fn rollback(client: Client, ns: &str, name: &str, revision: i64) -> AppResult<()> {
    let history = rollout_history(client.clone(), ns, name).await?;
    let target = history
        .iter()
        .find(|r| r.revision == revision)
        .ok_or_else(|| AppError::NotFound(format!("revision {revision}")))?;
    if target.current {
        return Err(AppError::validation("this revision is already active"));
    }
    let rs_api = builtin::replicasets().api_for_object(client.clone(), Some(ns))?;
    let rs = timed(rs_api.get(&target.replica_set)).await?;
    let mut template = rs.data.pointer("/spec/template").cloned().ok_or_else(|| AppError::internal("replica set has no template"))?;
    if let Some(labels) = template.pointer_mut("/metadata/labels").and_then(Value::as_object_mut) {
        labels.remove("pod-template-hash");
    }
    let dep_api = builtin::deployments().api_for_object(client, Some(ns))?;
    let mut dep = timed(dep_api.get(name)).await?;
    dep.data["spec"]["template"] = template;
    timed(dep_api.replace(name, &PostParams { dry_run: false, field_manager: Some(FIELD_MANAGER.into()) }, &dep)).await?;
    Ok(())
}

pub async fn cordon(client: Client, node: &str, unschedulable: bool) -> AppResult<()> {
    validate::object_name(node)?;
    let api = builtin::nodes().api(client, None);
    let patch = json!({ "spec": { "unschedulable": unschedulable } });
    timed(api.patch(node, &PatchParams::default(), &Patch::Merge(&patch))).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DrainOptions {
    pub ignore_daemonsets: bool,
    pub delete_emptydir_data: bool,
    /// Also evict pods that are not managed by a controller.
    pub force: bool,
    pub grace_period_seconds: Option<u32>,
}

impl Default for DrainOptions {
    fn default() -> Self {
        Self { ignore_daemonsets: true, delete_emptydir_data: false, force: false, grace_period_seconds: None }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DrainReport {
    pub evicted: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub failed: Vec<(String, String)>,
}

pub async fn drain(client: Client, node: &str, opts: &DrainOptions) -> AppResult<DrainReport> {
    cordon(client.clone(), node, true).await?;
    let pods: Api<Pod> = Api::all(client.clone());
    let list = timed(pods.list(&ListParams::default().fields(&format!("spec.nodeName={node}")))).await?;
    let mut report = DrainReport::default();
    for pod in list.items {
        let ns = pod.metadata.namespace.clone().unwrap_or_default();
        let name = pod.metadata.name.clone().unwrap_or_default();
        let id = format!("{ns}/{name}");
        let phase = pod.status.as_ref().and_then(|s| s.phase.clone()).unwrap_or_default();
        if phase == "Succeeded" || phase == "Failed" {
            // finished pods can be deleted without eviction semantics
        }
        let annotations = pod.metadata.annotations.clone().unwrap_or_default();
        if annotations.contains_key("kubernetes.io/config.mirror") {
            report.skipped.push((id, "mirror pod".into()));
            continue;
        }
        let controller = pod
            .metadata
            .owner_references
            .as_ref()
            .and_then(|o| o.iter().find(|o| o.controller == Some(true)).cloned());
        if let Some(c) = &controller {
            if c.kind == "DaemonSet" {
                if opts.ignore_daemonsets {
                    report.skipped.push((id, "managed by DaemonSet".into()));
                    continue;
                }
                report.failed.push((id, "managed by DaemonSet (enable 'ignore DaemonSets')".into()));
                continue;
            }
        } else if !opts.force && phase != "Succeeded" && phase != "Failed" {
            report.failed.push((id, "not managed by a controller (use force)".into()));
            continue;
        }
        let uses_emptydir = pod
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_ref())
            .map(|v| v.iter().any(|v| v.empty_dir.is_some()))
            .unwrap_or(false);
        if uses_emptydir && !opts.delete_emptydir_data {
            report.failed.push((id, "uses emptyDir volume (enable 'delete emptyDir data')".into()));
            continue;
        }
        let api: Api<Pod> = Api::namespaced(client.clone(), &ns);
        let ep = EvictParams {
            delete_options: Some(DeleteParams { grace_period_seconds: opts.grace_period_seconds, ..Default::default() }),
            ..Default::default()
        };
        match timed(api.evict(&name, &ep)).await {
            Ok(_) => report.evicted.push(id),
            Err(e) => report.failed.push((id, e.to_string())),
        }
    }
    Ok(report)
}

pub async fn set_cronjob_suspended(client: Client, ns: &str, name: &str, suspend: bool) -> AppResult<()> {
    validate::object_name(name)?;
    let api = builtin::cronjobs().api_for_object(client, Some(ns))?;
    let patch = json!({ "spec": { "suspend": suspend } });
    timed(api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))).await?;
    Ok(())
}

/// Equivalent of `kubectl create job --from=cronjob/<name>`.
pub async fn trigger_cronjob(client: Client, ns: &str, name: &str) -> AppResult<String> {
    validate::object_name(name)?;
    let cj_api = builtin::cronjobs().api_for_object(client.clone(), Some(ns))?;
    let cj = timed(cj_api.get(name)).await?;
    let template = cj
        .data
        .pointer("/spec/jobTemplate")
        .cloned()
        .ok_or_else(|| AppError::internal("cron job has no jobTemplate"))?;
    let suffix = chrono::Utc::now().format("%m%d%H%M%S").to_string();
    let mut base: String = name.chars().take(52 - suffix.len().min(52)).collect();
    base = base.trim_end_matches('-').to_string();
    let job_name = format!("{base}-manual-{suffix}");
    let job_name: String = job_name.chars().take(63).collect();
    let mut annotations = template.pointer("/metadata/annotations").cloned().unwrap_or(json!({}));
    annotations["cronjob.kubernetes.io/instantiate"] = json!("manual");
    let job = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": job_name,
            "namespace": ns,
            "labels": template.pointer("/metadata/labels").cloned().unwrap_or(json!({})),
            "annotations": annotations,
            "ownerReferences": [{
                "apiVersion": "batch/v1",
                "kind": "CronJob",
                "name": name,
                "uid": cj.metadata.uid.clone().unwrap_or_default(),
                "controller": true,
                "blockOwnerDeletion": true
            }]
        },
        "spec": template.get("spec").cloned().unwrap_or(json!({}))
    });
    let job_obj: DynamicObject = serde_json::from_value(job)?;
    let job_api = builtin::jobs().api_for_object(client, Some(ns))?;
    let created = timed(job_api.create(&PostParams::default(), &job_obj)).await?;
    Ok(created.metadata.name.unwrap_or(job_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_document_yaml() {
        let docs = parse_manifests(
            "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: a\n---\n# comment only\n---\napiVersion: v1\nkind: List\nitems:\n- apiVersion: v1\n  kind: ConfigMap\n  metadata:\n    name: b\n",
        )
        .unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[1]["kind"], "ConfigMap");
    }

    #[test]
    fn rejects_empty_and_invalid() {
        assert!(parse_manifests("---\n").is_err());
        let err = parse_manifests("kind: [unclosed").unwrap_err();
        assert!(err.to_string().contains("line"));
    }

    #[test]
    fn strips_managed_fields() {
        let v = clean_for_display(json!({"metadata": {"name": "x", "managedFields": [{}]}}));
        assert!(v["metadata"].get("managedFields").is_none());
    }
}
