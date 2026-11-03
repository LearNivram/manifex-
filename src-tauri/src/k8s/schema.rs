//! JSON schemas for the YAML editor, derived from the connected cluster's own
//! OpenAPI v3 document. This gives exact schemas for the cluster's Kubernetes
//! version *and* every installed CRD.

use std::time::Duration;

use serde_json::{json, Value};

use super::connection::ClusterConn;
use crate::error::{AppError, AppResult};
use crate::security::validate;

async fn get_json(conn: &ClusterConn, path: &str) -> AppResult<Value> {
    let req = http::Request::get(path)
        .header("Accept", "application/json")
        .body(Vec::new())
        .map_err(|e| AppError::internal(e.to_string()))?;
    let text = tokio::time::timeout(Duration::from_secs(30), conn.client.request_text(req))
        .await
        .map_err(|_| AppError::Connection("OpenAPI request timed out".into()))??;
    Ok(serde_json::from_str(&text)?)
}

/// Returns a self-contained JSON schema for `apiVersion` / `kind`: the root `$ref`s
/// the matching component and carries all components so references resolve.
pub async fn schema_for(conn: &ClusterConn, api_version: &str, kind: &str) -> AppResult<Value> {
    let (group, version) = match api_version.split_once('/') {
        Some((g, v)) => (g, v),
        None => ("", api_version),
    };
    validate::api_segment("group", group, true)?;
    validate::api_segment("version", version, false)?;
    let path = if group.is_empty() { format!("api/{version}") } else { format!("apis/{group}/{version}") };
    let index = get_json(conn, "/openapi/v3").await?;
    let server_url = index
        .pointer(&format!("/paths/{}/serverRelativeURL", path.replace('/', "~1")))
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| AppError::NotFound(format!("OpenAPI document for {api_version}")))?;
    if !server_url.starts_with("/openapi/v3/") {
        return Err(AppError::Security("unexpected OpenAPI URL".into()));
    }
    let doc = get_json(conn, &server_url).await?;
    let schemas = doc
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::NotFound("schemas".into()))?;
    let component = schemas
        .iter()
        .find(|(_, s)| {
            s.get("x-kubernetes-group-version-kind")
                .and_then(Value::as_array)
                .map(|gvks| {
                    gvks.iter().any(|g| {
                        g.get("group").and_then(Value::as_str) == Some(group)
                            && g.get("version").and_then(Value::as_str) == Some(version)
                            && g.get("kind").and_then(Value::as_str) == Some(kind)
                    })
                })
                .unwrap_or(false)
        })
        .map(|(name, _)| name.clone())
        .ok_or_else(|| AppError::NotFound(format!("schema for {kind}")))?;
    Ok(json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$ref": format!("#/components/schemas/{}", component.replace('/', "~1")),
        "components": { "schemas": schemas },
    }))
}
