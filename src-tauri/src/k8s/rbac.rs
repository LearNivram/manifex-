//! RBAC awareness: access checks for the current identity. The UI uses these to
//! disable actions the user is not permitted to perform – the API server remains
//! the final authority.

use std::time::{Duration, Instant};

use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
    SelfSubjectRulesReview, SelfSubjectRulesReviewSpec,
};
use kube::api::PostParams;
use kube::Api;
use serde::{Deserialize, Serialize};

use super::connection::ClusterConn;
use crate::error::AppResult;
use crate::security::validate;

const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessCheck {
    pub verb: String,
    #[serde(default)]
    pub group: String,
    pub resource: String,
    pub subresource: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
}

impl AccessCheck {
    fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.verb,
            self.group,
            self.resource,
            self.subresource.as_deref().unwrap_or(""),
            self.namespace.as_deref().unwrap_or(""),
            self.name.as_deref().unwrap_or("")
        )
    }
}

pub async fn can_i(conn: &ClusterConn, checks: Vec<AccessCheck>) -> AppResult<Vec<bool>> {
    let api: Api<SelfSubjectAccessReview> = Api::all(conn.client.clone());
    let futures = checks.into_iter().map(|check| {
        let api = api.clone();
        async move {
            let key = check.cache_key();
            if let Some(entry) = conn.access_cache.get(&key) {
                if entry.1.elapsed() < CACHE_TTL {
                    return Ok(entry.0);
                }
            }
            validate::api_segment("verb", &check.verb, false)?;
            validate::api_segment("group", &check.group, true)?;
            validate::api_segment("resource", &check.resource, false)?;
            validate::optional_namespace(&check.namespace)?;
            let review = SelfSubjectAccessReview {
                spec: SelfSubjectAccessReviewSpec {
                    resource_attributes: Some(ResourceAttributes {
                        verb: Some(check.verb.clone()),
                        group: Some(check.group.clone()),
                        resource: Some(check.resource.clone()),
                        subresource: check.subresource.clone(),
                        namespace: check.namespace.clone().filter(|n| !n.is_empty()),
                        name: check.name.clone(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            };
            let allowed = match tokio::time::timeout(
                Duration::from_secs(10),
                api.create(&PostParams::default(), &review),
            )
            .await
            {
                Ok(Ok(r)) => r.status.map(|s| s.allowed).unwrap_or(false),
                // If the check itself fails we optimistically allow; the API server
                // will still reject unauthorised requests.
                _ => true,
            };
            conn.access_cache.insert(key, (allowed, Instant::now()));
            Ok::<bool, crate::error::AppError>(allowed)
        }
    });
    futures::future::try_join_all(futures).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSummary {
    pub verbs: Vec<String>,
    pub api_groups: Vec<String>,
    pub resources: Vec<String>,
    pub resource_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesReview {
    pub namespace: String,
    pub identity: Option<String>,
    pub incomplete: bool,
    pub evaluation_error: Option<String>,
    pub resource_rules: Vec<RuleSummary>,
    pub non_resource_rules: Vec<RuleSummary>,
}

pub async fn rules_review(conn: &ClusterConn, namespace: &str) -> AppResult<RulesReview> {
    validate::namespace(namespace)?;
    let api: Api<SelfSubjectRulesReview> = Api::all(conn.client.clone());
    let review = SelfSubjectRulesReview {
        spec: SelfSubjectRulesReviewSpec {
            namespace: Some(namespace.into()),
        },
        ..Default::default()
    };
    let res = tokio::time::timeout(
        Duration::from_secs(15),
        api.create(&PostParams::default(), &review),
    )
    .await
    .map_err(|_| crate::error::AppError::Connection("rules review timed out".into()))??;
    let status = res.status.unwrap_or_default();
    Ok(RulesReview {
        namespace: namespace.into(),
        identity: conn.identity(),
        incomplete: status.incomplete,
        evaluation_error: status.evaluation_error,
        resource_rules: status
            .resource_rules
            .into_iter()
            .map(|r| RuleSummary {
                verbs: r.verbs,
                api_groups: r.api_groups.unwrap_or_default(),
                resources: r.resources.unwrap_or_default(),
                resource_names: r.resource_names.unwrap_or_default(),
            })
            .collect(),
        non_resource_rules: status
            .non_resource_rules
            .into_iter()
            .map(|r| RuleSummary {
                verbs: r.verbs,
                api_groups: vec![],
                resources: r.non_resource_urls.unwrap_or_default(),
                resource_names: vec![],
            })
            .collect(),
    })
}
