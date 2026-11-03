//! Resource commands: watches, reads, apply, lifecycle operations, RBAC, metrics,
//! insights.

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::k8s::insights::{self, Application, ClusterComparison, SearchHit};
use crate::k8s::metrics::{self, ClusterCapacity, NodeMetric, PodMetric};
use crate::k8s::ops::{
    self, ApplyMode, ApplyResult, DeleteOptions, DrainOptions, DrainReport, ObjectView,
    RolloutRevision,
};
use crate::k8s::rbac::{self, AccessCheck, RulesReview};
use crate::k8s::resources::{builtin, ResourceRef};
use crate::k8s::schema;
use crate::k8s::summary::{summarize, Row};
use crate::k8s::watch::{WatchKey, WatchMessage};
use crate::security::validate;
use crate::state::{AppState, AuditTarget};

fn ns_opt(ns: &Option<String>) -> Option<&str> {
    ns.as_deref().filter(|n| !n.is_empty())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn watch_resources(
    state: State<'_, AppState>,
    cluster_id: String,
    resource: ResourceRef,
    namespace: Option<String>,
    label_selector: Option<String>,
    field_selector: Option<String>,
    on_event: Channel<WatchMessage>,
) -> AppResult<String> {
    resource.validate()?;
    validate::optional_namespace(&namespace)?;
    for sel in [&label_selector, &field_selector].into_iter().flatten() {
        validate::selector(sel)?;
    }
    let conn = state.conn(&cluster_id)?;
    let key = WatchKey {
        cluster_id,
        namespace: if resource.namespaced {
            namespace.filter(|n| !n.is_empty())
        } else {
            None
        },
        resource,
        label_selector: label_selector.filter(|s| !s.is_empty()),
        field_selector: field_selector.filter(|s| !s.is_empty()),
    };
    state
        .watches
        .subscribe(conn.client.clone(), key, move |msg: WatchMessage| {
            on_event.send(msg).is_ok()
        })
}

#[tauri::command]
pub async fn unwatch_resources(
    state: State<'_, AppState>,
    subscription_id: String,
) -> AppResult<()> {
    state.watches.unsubscribe(&subscription_id);
    Ok(())
}

#[tauri::command]
pub async fn list_resources(
    state: State<'_, AppState>,
    cluster_id: String,
    resource: ResourceRef,
    namespace: Option<String>,
    label_selector: Option<String>,
    field_selector: Option<String>,
    limit: Option<u32>,
) -> AppResult<Vec<Row>> {
    let conn = state.conn(&cluster_id)?;
    let items = ops::list_objects(
        conn.client.clone(),
        &resource,
        ns_opt(&namespace),
        label_selector.as_deref(),
        field_selector.as_deref(),
        limit,
    )
    .await?;
    Ok(items.iter().map(|v| summarize(&resource.kind, v)).collect())
}

#[tauri::command]
pub async fn get_resource(
    state: State<'_, AppState>,
    cluster_id: String,
    resource: ResourceRef,
    namespace: Option<String>,
    name: String,
) -> AppResult<ObjectView> {
    let conn = state.conn(&cluster_id)?;
    ops::get_object(conn.client.clone(), &resource, ns_opt(&namespace), &name).await
}

#[tauri::command]
pub async fn apply_yaml(
    state: State<'_, AppState>,
    cluster_id: String,
    yaml: String,
    namespace: Option<String>,
    mode: ApplyMode,
    dry_run: bool,
    force_conflicts: Option<bool>,
) -> AppResult<Vec<ApplyResult>> {
    validate::optional_namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    let run = ops::apply_manifests(
        &conn,
        &yaml,
        ns_opt(&namespace),
        mode,
        dry_run,
        force_conflicts.unwrap_or(false),
    );
    if dry_run {
        return run.await;
    }
    let results = match run.await {
        Ok(r) => r,
        Err(e) => {
            // the request failed as a whole (e.g. unparsable YAML, discovery error)
            let _ = state.db.add_audit(crate::persistence::audit::NewAudit {
                cluster_id: Some(cluster_id.clone()),
                cluster_name: Some(conn.name()),
                identity: conn.identity(),
                action: if mode == ApplyMode::Apply {
                    "apply".into()
                } else {
                    "replace".into()
                },
                namespace: namespace.clone(),
                outcome: "failure".into(),
                detail: Some(e.to_string()),
                ..Default::default()
            });
            return Err(e);
        }
    };
    // one audit row per object for traceability
    for r in &results {
        let _ = state.db.add_audit(crate::persistence::audit::NewAudit {
            cluster_id: Some(cluster_id.clone()),
            cluster_name: Some(conn.name()),
            identity: conn.identity(),
            action: format!("apply.{}", r.action),
            resource: Some(r.kind.clone()),
            namespace: r.namespace.clone(),
            name: Some(r.name.clone()),
            outcome: if r.error.is_some() {
                "failure".into()
            } else {
                "success".into()
            },
            detail: r.error.clone(),
        });
    }
    Ok(results)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn delete_resource(
    state: State<'_, AppState>,
    cluster_id: String,
    resource: ResourceRef,
    namespace: Option<String>,
    name: String,
    options: Option<DeleteOptions>,
    confirm: Option<String>,
) -> AppResult<()> {
    state.check_production_confirmation(&cluster_id, &name, confirm.as_deref())?;
    let conn = state.conn(&cluster_id)?;
    let opts = options.unwrap_or_default();
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "delete",
                resource: Some(resource.kind.clone()),
                namespace: namespace.clone(),
                name: Some(name.clone()),
                detail: opts.propagation.clone().map(|p| format!("propagation={p}")),
            },
            ops::delete_object(
                conn.client.clone(),
                &resource,
                ns_opt(&namespace),
                &name,
                &opts,
            ),
        )
        .await
}

#[tauri::command]
pub async fn scale_resource(
    state: State<'_, AppState>,
    cluster_id: String,
    resource: ResourceRef,
    namespace: String,
    name: String,
    replicas: i32,
) -> AppResult<()> {
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "scale",
                resource: Some(resource.kind.clone()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: Some(format!("replicas={replicas}")),
            },
            ops::scale(conn.client.clone(), &resource, &namespace, &name, replicas),
        )
        .await
}

#[tauri::command]
pub async fn restart_workload(
    state: State<'_, AppState>,
    cluster_id: String,
    resource: ResourceRef,
    namespace: String,
    name: String,
) -> AppResult<()> {
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "rollout.restart",
                resource: Some(resource.kind.clone()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: None,
            },
            ops::restart(conn.client.clone(), &resource, &namespace, &name),
        )
        .await
}

#[tauri::command]
pub async fn set_deployment_paused(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
    paused: bool,
) -> AppResult<()> {
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: if paused {
                    "rollout.pause"
                } else {
                    "rollout.resume"
                },
                resource: Some("Deployment".into()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: None,
            },
            ops::set_paused(conn.client.clone(), &namespace, &name, paused),
        )
        .await
}

#[tauri::command]
pub async fn rollout_history(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
) -> AppResult<Vec<RolloutRevision>> {
    validate::namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    ops::rollout_history(conn.client.clone(), &namespace, &name).await
}

#[tauri::command]
pub async fn rollback_deployment(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
    revision: i64,
) -> AppResult<()> {
    validate::namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "rollout.undo",
                resource: Some("Deployment".into()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: Some(format!("to revision {revision}")),
            },
            ops::rollback(conn.client.clone(), &namespace, &name, revision),
        )
        .await
}

#[tauri::command]
pub async fn cordon_node(
    state: State<'_, AppState>,
    cluster_id: String,
    name: String,
    unschedulable: bool,
) -> AppResult<()> {
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: if unschedulable {
                    "node.cordon"
                } else {
                    "node.uncordon"
                },
                resource: Some("Node".into()),
                namespace: None,
                name: Some(name.clone()),
                detail: None,
            },
            ops::cordon(conn.client.clone(), &name, unschedulable),
        )
        .await
}

#[tauri::command]
pub async fn drain_node(
    state: State<'_, AppState>,
    cluster_id: String,
    name: String,
    options: Option<DrainOptions>,
    confirm: Option<String>,
) -> AppResult<DrainReport> {
    state.check_production_confirmation(&cluster_id, &name, confirm.as_deref())?;
    let conn = state.conn(&cluster_id)?;
    let opts = options.unwrap_or_default();
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "node.drain",
                resource: Some("Node".into()),
                namespace: None,
                name: Some(name.clone()),
                detail: None,
            },
            ops::drain(conn.client.clone(), &name, &opts),
        )
        .await
}

#[tauri::command]
pub async fn suspend_cronjob(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
    suspend: bool,
) -> AppResult<()> {
    validate::namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: if suspend {
                    "cronjob.suspend"
                } else {
                    "cronjob.resume"
                },
                resource: Some("CronJob".into()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: None,
            },
            ops::set_cronjob_suspended(conn.client.clone(), &namespace, &name, suspend),
        )
        .await
}

#[tauri::command]
pub async fn trigger_cronjob(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    name: String,
) -> AppResult<String> {
    validate::namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    state
        .audited(
            &cluster_id,
            AuditTarget {
                action: "cronjob.trigger",
                resource: Some("CronJob".into()),
                namespace: Some(namespace.clone()),
                name: Some(name.clone()),
                detail: None,
            },
            ops::trigger_cronjob(conn.client.clone(), &namespace, &name),
        )
        .await
}

#[tauri::command]
pub async fn can_i(
    state: State<'_, AppState>,
    cluster_id: String,
    checks: Vec<AccessCheck>,
) -> AppResult<Vec<bool>> {
    if checks.len() > 100 {
        return Err(AppError::validation("too many access checks"));
    }
    let conn = state.conn(&cluster_id)?;
    rbac::can_i(&conn, checks).await
}

#[tauri::command]
pub async fn rbac_rules(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
) -> AppResult<RulesReview> {
    let conn = state.conn(&cluster_id)?;
    rbac::rules_review(&conn, &namespace).await
}

#[tauri::command]
pub async fn resource_schema(
    state: State<'_, AppState>,
    cluster_id: String,
    api_version: String,
    kind: String,
) -> AppResult<Value> {
    if kind.is_empty() || kind.len() > 128 || !kind.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(AppError::validation("invalid kind"));
    }
    let conn = state.conn(&cluster_id)?;
    schema::schema_for(&conn, &api_version, &kind).await
}

#[tauri::command]
pub async fn node_metrics(
    state: State<'_, AppState>,
    cluster_id: String,
) -> AppResult<Vec<NodeMetric>> {
    let conn = state.conn(&cluster_id)?;
    metrics::node_metrics(conn.client.clone()).await
}

#[tauri::command]
pub async fn pod_metrics(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: Option<String>,
) -> AppResult<Vec<PodMetric>> {
    validate::optional_namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    metrics::pod_metrics(conn.client.clone(), ns_opt(&namespace)).await
}

#[tauri::command]
pub async fn cluster_capacity(
    state: State<'_, AppState>,
    cluster_id: String,
) -> AppResult<ClusterCapacity> {
    let conn = state.conn(&cluster_id)?;
    metrics::cluster_capacity(conn.client.clone()).await
}

#[tauri::command]
pub async fn list_applications(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: Option<String>,
) -> AppResult<Vec<Application>> {
    validate::optional_namespace(&namespace)?;
    let conn = state.conn(&cluster_id)?;
    insights::list_applications(conn.client.clone(), ns_opt(&namespace)).await
}

#[tauri::command]
pub async fn global_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> AppResult<Vec<SearchHit>> {
    insights::global_search(&state.clusters, &query, limit.unwrap_or(100)).await
}

#[tauri::command]
pub async fn compare_clusters(
    state: State<'_, AppState>,
    left: String,
    right: String,
    namespace: Option<String>,
) -> AppResult<ClusterComparison> {
    validate::optional_namespace(&namespace)?;
    insights::compare_clusters(&state.clusters, &left, &right, ns_opt(&namespace)).await
}

/// Pods selected by a workload's label selector (used for aggregated logs/exec).
#[tauri::command]
pub async fn workload_pods(
    state: State<'_, AppState>,
    cluster_id: String,
    namespace: String,
    selector: String,
) -> AppResult<Vec<Row>> {
    validate::namespace(&namespace)?;
    validate::selector(&selector)?;
    if selector.is_empty() {
        return Ok(vec![]);
    }
    let conn = state.conn(&cluster_id)?;
    let items = ops::list_objects(
        conn.client.clone(),
        &builtin::pods(),
        Some(&namespace),
        Some(&selector),
        None,
        Some(500),
    )
    .await?;
    Ok(items.iter().map(|v| summarize("Pod", v)).collect())
}
