//! Generic resource addressing (group / version / kind / plural / scope) and API
//! discovery helpers.

use kube::api::{ApiResource, DynamicObject};
use kube::discovery::Scope;
use kube::{Api, Client};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::security::validate;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    #[serde(default)]
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub namespaced: bool,
}

impl ResourceRef {
    pub fn new(group: &str, version: &str, kind: &str, plural: &str, namespaced: bool) -> Self {
        Self { group: group.into(), version: version.into(), kind: kind.into(), plural: plural.into(), namespaced }
    }

    pub fn validate(&self) -> AppResult<()> {
        validate::api_segment("group", &self.group, true)?;
        validate::api_segment("version", &self.version, false)?;
        validate::api_segment("plural", &self.plural, false)?;
        if self.kind.is_empty() || self.kind.len() > 128 || !self.kind.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(AppError::validation("kind is invalid"));
        }
        Ok(())
    }

    pub fn api_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }

    pub fn api_resource(&self) -> ApiResource {
        ApiResource {
            group: self.group.clone(),
            version: self.version.clone(),
            api_version: self.api_version(),
            kind: self.kind.clone(),
            plural: self.plural.clone(),
        }
    }

    /// Display label, e.g. `deployments.apps`.
    pub fn label(&self) -> String {
        if self.group.is_empty() {
            self.plural.clone()
        } else {
            format!("{}.{}", self.plural, self.group)
        }
    }

    pub fn api(&self, client: Client, namespace: Option<&str>) -> Api<DynamicObject> {
        let ar = self.api_resource();
        match (self.namespaced, namespace.filter(|n| !n.is_empty())) {
            (true, Some(ns)) => Api::namespaced_with(client, ns, &ar),
            _ => Api::all_with(client, &ar),
        }
    }

    /// Api handle for a single named object; namespaced resources need a namespace.
    pub fn api_for_object(&self, client: Client, namespace: Option<&str>) -> AppResult<Api<DynamicObject>> {
        if self.namespaced {
            let ns = namespace.filter(|n| !n.is_empty()).ok_or_else(|| AppError::validation("namespace is required"))?;
            validate::namespace(ns)?;
            Ok(Api::namespaced_with(client, ns, &self.api_resource()))
        } else {
            Ok(Api::all_with(client, &self.api_resource()))
        }
    }
}

pub mod builtin {
    use super::ResourceRef;

    pub fn pods() -> ResourceRef {
        ResourceRef::new("", "v1", "Pod", "pods", true)
    }
    pub fn nodes() -> ResourceRef {
        ResourceRef::new("", "v1", "Node", "nodes", false)
    }
    pub fn namespaces() -> ResourceRef {
        ResourceRef::new("", "v1", "Namespace", "namespaces", false)
    }
    pub fn events() -> ResourceRef {
        ResourceRef::new("", "v1", "Event", "events", true)
    }
    pub fn services() -> ResourceRef {
        ResourceRef::new("", "v1", "Service", "services", true)
    }
    pub fn deployments() -> ResourceRef {
        ResourceRef::new("apps", "v1", "Deployment", "deployments", true)
    }
    pub fn statefulsets() -> ResourceRef {
        ResourceRef::new("apps", "v1", "StatefulSet", "statefulsets", true)
    }
    pub fn daemonsets() -> ResourceRef {
        ResourceRef::new("apps", "v1", "DaemonSet", "daemonsets", true)
    }
    pub fn replicasets() -> ResourceRef {
        ResourceRef::new("apps", "v1", "ReplicaSet", "replicasets", true)
    }
    pub fn jobs() -> ResourceRef {
        ResourceRef::new("batch", "v1", "Job", "jobs", true)
    }
    pub fn cronjobs() -> ResourceRef {
        ResourceRef::new("batch", "v1", "CronJob", "cronjobs", true)
    }
    pub fn ingresses() -> ResourceRef {
        ResourceRef::new("networking.k8s.io", "v1", "Ingress", "ingresses", true)
    }
    pub fn configmaps() -> ResourceRef {
        ResourceRef::new("", "v1", "ConfigMap", "configmaps", true)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResourceInfo {
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub namespaced: bool,
    pub verbs: Vec<String>,
    pub subresources: Vec<String>,
    /// true when the resource is served by a CustomResourceDefinition / aggregated API.
    pub custom: bool,
}

const CORE_GROUPS: &[&str] = &[
    "",
    "apps",
    "batch",
    "autoscaling",
    "policy",
    "networking.k8s.io",
    "storage.k8s.io",
    "rbac.authorization.k8s.io",
    "apiextensions.k8s.io",
    "apiregistration.k8s.io",
    "admissionregistration.k8s.io",
    "authentication.k8s.io",
    "authorization.k8s.io",
    "certificates.k8s.io",
    "coordination.k8s.io",
    "discovery.k8s.io",
    "events.k8s.io",
    "flowcontrol.apiserver.k8s.io",
    "node.k8s.io",
    "scheduling.k8s.io",
    "resource.k8s.io",
    "storagemigration.k8s.io",
    "internal.apiserver.k8s.io",
];

pub fn list_api_resources(discovery: &kube::discovery::Discovery) -> Vec<ApiResourceInfo> {
    let mut out = Vec::new();
    for group in discovery.groups() {
        for (ar, caps) in group.recommended_resources() {
            out.push(ApiResourceInfo {
                group: ar.group.clone(),
                version: ar.version.clone(),
                kind: ar.kind.clone(),
                plural: ar.plural.clone(),
                namespaced: caps.scope == Scope::Namespaced,
                verbs: caps.operations.clone(),
                subresources: caps.subresources.iter().map(|(sr, _)| sr.plural.clone()).collect(),
                custom: !CORE_GROUPS.contains(&ar.group.as_str()),
            });
        }
    }
    out.sort_by(|a, b| a.group.cmp(&b.group).then(a.kind.cmp(&b.kind)));
    out
}

/// Resolves `apiVersion` + `kind` (from a manifest) to a resource reference.
pub fn resolve_kind(
    discovery: &kube::discovery::Discovery,
    api_version: &str,
    kind: &str,
) -> AppResult<ResourceRef> {
    let (group, version) = match api_version.split_once('/') {
        Some((g, v)) => (g.to_string(), v.to_string()),
        None => (String::new(), api_version.to_string()),
    };
    let gvk = kube::api::GroupVersionKind::gvk(&group, &version, kind);
    let (ar, caps) = discovery
        .resolve_gvk(&gvk)
        .ok_or_else(|| AppError::validation(format!("the server does not know {kind} in {api_version}")))?;
    Ok(ResourceRef {
        group: ar.group,
        version: ar.version,
        kind: ar.kind,
        plural: ar.plural,
        namespaced: caps.scope == Scope::Namespaced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_and_label() {
        let d = builtin::deployments();
        assert_eq!(d.api_version(), "apps/v1");
        assert_eq!(d.label(), "deployments.apps");
        assert_eq!(builtin::pods().api_version(), "v1");
        assert!(d.validate().is_ok());
        let bad = ResourceRef::new("apps", "v1", "Dep/loy", "deployments", true);
        assert!(bad.validate().is_err());
    }
}
