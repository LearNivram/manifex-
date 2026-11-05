//! One-click installation, status, update and removal of `manifex-agent`.
//!
//! Using an existing connection (kubeconfig or SSH), Manifex
//!   1. creates the namespace, service account, RBAC binding and service,
//!   2. waits for the external address (LoadBalancer) or picks a node address
//!      (NodePort),
//!   3. generates a private CA + server certificate for exactly those addresses
//!      and a random access token (nothing leaves the user's machine except into
//!      the cluster's own Secrets),
//!   4. deploys the agent, waits for the rollout and
//!   5. returns endpoint, CA and token so a ready-to-use "agent" cluster profile
//!      can be created.

use std::collections::BTreeSet;
use std::time::Duration;

use base64::Engine;
use kube::api::{DeleteParams, DynamicObject, Patch, PatchParams};
use kube::Client;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ops::FIELD_MANAGER;
use super::resources::{builtin, ResourceRef};
use crate::error::{AppError, AppResult};

pub const NAMESPACE: &str = "manifex-system";
pub const NAME: &str = "manifex-agent";
pub const IMAGE_REPO: &str = "ghcr.io/learnivram/manifex-agent";
/// The agent is versioned together with the desktop app.
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const PORT: u16 = 8443;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallOptions {
    /// ClusterRole granted to the agent: view | edit | admin | cluster-admin
    pub role: String,
    /// LoadBalancer | NodePort
    pub exposure: String,
    /// Optional public DNS name / IP under which the agent is reachable
    /// (e.g. a node's public IP or a DNS record).
    pub external_address: Option<String>,
    /// Optional image override (air-gapped registries). Default: official image.
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallResult {
    pub endpoint: String,
    pub alternative_endpoints: Vec<String>,
    #[serde(skip_serializing)]
    pub ca_pem: String,
    #[serde(skip_serializing)]
    pub token: String,
    pub version: String,
    pub image: String,
    pub role: String,
    pub exposure: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub installed: bool,
    pub image: Option<String>,
    pub version: Option<String>,
    pub ready: Option<String>,
    pub healthy: bool,
    pub exposure: Option<String>,
    pub role: Option<String>,
    pub latest_version: String,
    pub update_available: bool,
    pub message: Option<String>,
}

fn res_namespace() -> ResourceRef {
    builtin::namespaces()
}
fn res_sa() -> ResourceRef {
    ResourceRef::new("", "v1", "ServiceAccount", "serviceaccounts", true)
}
fn res_crb() -> ResourceRef {
    ResourceRef::new(
        "rbac.authorization.k8s.io",
        "v1",
        "ClusterRoleBinding",
        "clusterrolebindings",
        false,
    )
}
fn res_secret() -> ResourceRef {
    ResourceRef::new("", "v1", "Secret", "secrets", true)
}

fn labels(version: &str) -> Value {
    json!({
        "app.kubernetes.io/name": NAME,
        "app.kubernetes.io/part-of": "manifex",
        "app.kubernetes.io/managed-by": "manifex",
        "app.kubernetes.io/version": version,
    })
}

async fn apply(
    client: &Client,
    res: &ResourceRef,
    ns: Option<&str>,
    obj: Value,
) -> AppResult<DynamicObject> {
    let name = obj
        .pointer("/metadata/name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let api = res.api_for_object(client.clone(), ns)?;
    let pp = PatchParams::apply(FIELD_MANAGER).force();
    tokio::time::timeout(
        Duration::from_secs(30),
        api.patch(&name, &pp, &Patch::Apply(&obj)),
    )
    .await
    .map_err(|_| AppError::Connection("request timed out".into()))?
    .map_err(AppError::from)
}

fn validate_host(h: &str) -> AppResult<()> {
    if h.is_empty()
        || h.len() > 253
        || !h
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return Err(AppError::validation(
            "external address must be a DNS name or IP address",
        ));
    }
    Ok(())
}

fn validate_image(i: &str) -> AppResult<()> {
    if i.is_empty()
        || i.len() > 512
        || i.starts_with('-')
        || !i.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'/' | b':' | b'@' | b'_' | b'-')
        })
    {
        return Err(AppError::validation("image reference is invalid"));
    }
    Ok(())
}

/// Compares dotted versions ("0.10.1" > "0.9.3"); non-numeric parts compare as 0.
pub fn version_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-'])
            .take(3)
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(candidate) > parse(current)
}

fn image_tag(image: &str) -> Option<String> {
    let last = image.rsplit('/').next()?;
    last.split_once(':')
        .map(|(_, t)| t.split('@').next().unwrap_or(t).to_string())
}

struct Certs {
    ca_pem: String,
    cert_pem: String,
    key_pem: String,
}

fn generate_certs(sans: &BTreeSet<String>) -> AppResult<Certs> {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer,
        KeyPair, KeyUsagePurpose,
    };
    let err = |e: rcgen::Error| AppError::internal(format!("certificate generation failed: {e}"));
    let ca_key = KeyPair::generate().map_err(err)?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).map_err(err)?;
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Manifex Agent CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let now = chrono::Utc::now();
    use chrono::Datelike;
    ca_params.not_before = rcgen::date_time_ymd(now.year() - 1, 1, 1);
    ca_params.not_after = rcgen::date_time_ymd(now.year() + 10, 1, 1);
    let ca_cert = ca_params.self_signed(&ca_key).map_err(err)?;
    let issuer = Issuer::new(ca_params, ca_key);

    let key = KeyPair::generate().map_err(err)?;
    let mut params =
        CertificateParams::new(sans.iter().cloned().collect::<Vec<_>>()).map_err(err)?;
    params.distinguished_name.push(DnType::CommonName, NAME);
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = rcgen::date_time_ymd(now.year() - 1, 1, 1);
    params.not_after = rcgen::date_time_ymd(now.year() + 5, 1, 1);
    let cert = params.signed_by(&key, &issuer).map_err(err)?;
    Ok(Certs {
        ca_pem: ca_cert.pem(),
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
    })
}

/// External addresses of the service, plus node addresses for NodePort.
async fn wait_for_addresses(
    client: &Client,
    exposure: &str,
) -> AppResult<(Vec<String>, u16, Vec<String>)> {
    let svc_api = builtin::services().api_for_object(client.clone(), Some(NAMESPACE))?;
    let nodes =
        super::ops::list_objects(client.clone(), &builtin::nodes(), None, None, None, None).await?;
    let mut node_ips: Vec<String> = Vec::new();
    for prefer in ["ExternalIP", "InternalIP"] {
        for n in &nodes {
            for a in n
                .pointer("/status/addresses")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if a.get("type").and_then(Value::as_str) == Some(prefer) {
                    if let Some(ip) = a.get("address").and_then(Value::as_str) {
                        if !node_ips.contains(&ip.to_string()) {
                            node_ips.push(ip.to_string());
                        }
                    }
                }
            }
        }
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let svc = svc_api.get(NAME).await?;
        let v = serde_json::to_value(&svc)?;
        if exposure == "NodePort" {
            let port = v
                .pointer("/spec/ports/0/nodePort")
                .and_then(Value::as_u64)
                .ok_or_else(|| AppError::internal("service has no node port"))?;
            return Ok((node_ips.clone(), port as u16, node_ips));
        }
        let ingress: Vec<String> = v
            .pointer("/status/loadBalancer/ingress")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|i| {
                i.get("ip")
                    .or(i.get("hostname"))
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .collect();
        if !ingress.is_empty() {
            return Ok((ingress, PORT, node_ips));
        }
        if std::time::Instant::now() > deadline {
            return Err(AppError::Connection(
                "the cluster did not assign a LoadBalancer address within 90s. Choose NodePort or enter an external address."
                    .into(),
            ));
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn wait_rollout(client: &Client, timeout: Duration) -> AppResult<()> {
    let dep_api = builtin::deployments().api_for_object(client.clone(), Some(NAMESPACE))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let dep = match dep_api.get(NAME).await {
            Ok(d) => d,
            // When Manifex is connected *through* the agent being updated, the old
            // pod goes away mid-rollout and a request may fail at the transport
            // level. Keep polling – the Service routes to the new pod shortly.
            Err(e) if !matches!(e, kube::Error::Api(_)) && std::time::Instant::now() < deadline => {
                tracing::debug!(error = %e, "transient error while waiting for agent rollout");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let v = serde_json::to_value(&dep)?;
        let gen = v
            .pointer("/metadata/generation")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let observed = v
            .pointer("/status/observedGeneration")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let updated = v
            .pointer("/status/updatedReplicas")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let available = v
            .pointer("/status/availableReplicas")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let total = v
            .pointer("/status/replicas")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if observed >= gen && updated >= 1 && available >= 1 && total == updated {
            return Ok(());
        }
        // Surface image pull / crash problems early.
        let pods = super::ops::list_objects(
            client.clone(),
            &builtin::pods(),
            Some(NAMESPACE),
            Some(&format!("app.kubernetes.io/name={NAME}")),
            None,
            None,
        )
        .await
        .unwrap_or_default();
        for p in &pods {
            let row = super::summary::summarize("Pod", p);
            if matches!(
                row.status.as_str(),
                "ErrImagePull" | "ImagePullBackOff" | "InvalidImageName"
            ) {
                return Err(AppError::Connection(format!(
                    "the cluster cannot pull the agent image ({}). Make sure the node can reach the registry \
                     and that the image ghcr.io/learnivram/manifex-agent is public.",
                    row.status
                )));
            }
            let unschedulable = p
                .pointer("/status/conditions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|c| {
                    c.get("type").and_then(Value::as_str) == Some("PodScheduled")
                        && c.get("reason").and_then(Value::as_str) == Some("Unschedulable")
                })
                .and_then(|c| c.get("message").and_then(Value::as_str));
            if let Some(msg) = unschedulable {
                if std::time::Instant::now() + Duration::from_secs(150) > deadline {
                    return Err(AppError::Connection(format!(
                        "the agent pod cannot be scheduled: {msg}"
                    )));
                }
            }
            if row.status == "CrashLoopBackOff" {
                return Err(AppError::Connection(
                    "the agent pod is crash looping – check its logs in Manifex".into(),
                ));
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(AppError::Connection(
                "the agent did not become ready in time".into(),
            ));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn deployment(image: &str, version: &str, config_hash: &str, role: &str) -> Value {
    json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": NAME,
            "namespace": NAMESPACE,
            "labels": labels(version),
            // Readable without RBAC permissions (the agent itself may not read ClusterRoleBindings).
            "annotations": { "manifex.dev/role": role }
        },
        "spec": {
            "replicas": 1,
            // Start the new agent before stopping the old one so connections through
            // the agent survive an update.
            "strategy": { "type": "RollingUpdate", "rollingUpdate": { "maxUnavailable": 0, "maxSurge": 1 } },
            "selector": { "matchLabels": { "app.kubernetes.io/name": NAME } },
            "template": {
                "metadata": {
                    "labels": labels(version),
                    "annotations": { "manifex.dev/config-hash": config_hash }
                },
                "spec": {
                    "serviceAccountName": NAME,
                    "securityContext": { "runAsNonRoot": true, "runAsUser": 65532, "runAsGroup": 65532, "seccompProfile": { "type": "RuntimeDefault" } },
                    "containers": [{
                        "name": "agent",
                        "image": image,
                        "imagePullPolicy": "IfNotPresent",
                        "ports": [{ "containerPort": PORT, "name": "https" }],
                        "env": [{ "name": "MANIFEX_AGENT_LISTEN", "value": format!("0.0.0.0:{PORT}") }],
                        "readinessProbe": { "httpGet": { "path": "/healthz", "port": "https", "scheme": "HTTPS" }, "periodSeconds": 5 },
                        "resources": { "requests": { "cpu": "10m", "memory": "16Mi" }, "limits": { "memory": "64Mi" } },
                        "securityContext": {
                            "allowPrivilegeEscalation": false,
                            "readOnlyRootFilesystem": true,
                            "capabilities": { "drop": ["ALL"] }
                        },
                        "volumeMounts": [
                            { "name": "tls", "mountPath": "/etc/manifex-agent/tls", "readOnly": true },
                            { "name": "auth", "mountPath": "/etc/manifex-agent/auth", "readOnly": true }
                        ]
                    }],
                    "volumes": [
                        { "name": "tls", "secret": { "secretName": format!("{NAME}-tls") } },
                        { "name": "auth", "secret": { "secretName": format!("{NAME}-auth") } }
                    ]
                }
            }
        }
    })
}

pub async fn install(client: Client, opts: &AgentInstallOptions) -> AppResult<AgentInstallResult> {
    if !["view", "edit", "admin", "cluster-admin"].contains(&opts.role.as_str()) {
        return Err(AppError::validation(
            "role must be view, edit, admin or cluster-admin",
        ));
    }
    if !["LoadBalancer", "NodePort"].contains(&opts.exposure.as_str()) {
        return Err(AppError::validation(
            "exposure must be LoadBalancer or NodePort",
        ));
    }
    let external = opts
        .external_address
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    if let Some(h) = &external {
        validate_host(h)?;
    }
    let image = match opts
        .image
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(i) => {
            validate_image(i)?;
            i.to_string()
        }
        None => format!("{IMAGE_REPO}:{AGENT_VERSION}"),
    };
    let version = image_tag(&image).unwrap_or_else(|| AGENT_VERSION.to_string());

    apply(&client, &res_namespace(), None, json!({
        "apiVersion": "v1", "kind": "Namespace",
        "metadata": { "name": NAMESPACE, "labels": { "app.kubernetes.io/managed-by": "manifex" } }
    }))
    .await?;
    apply(
        &client,
        &res_sa(),
        Some(NAMESPACE),
        json!({
            "apiVersion": "v1", "kind": "ServiceAccount",
            "metadata": { "name": NAME, "namespace": NAMESPACE, "labels": labels(&version) }
        }),
    )
    .await?;
    apply(&client, &res_crb(), None, json!({
        "apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRoleBinding",
        "metadata": { "name": NAME, "labels": labels(&version), "annotations": { "manifex.dev/role": opts.role } },
        "roleRef": { "apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": opts.role },
        "subjects": [{ "kind": "ServiceAccount", "name": NAME, "namespace": NAMESPACE }]
    }))
    .await?;
    apply(
        &client,
        &builtin::services(),
        Some(NAMESPACE),
        json!({
            "apiVersion": "v1", "kind": "Service",
            "metadata": { "name": NAME, "namespace": NAMESPACE, "labels": labels(&version) },
            "spec": {
                "type": opts.exposure,
                "selector": { "app.kubernetes.io/name": NAME },
                "ports": [{ "name": "https", "port": PORT, "targetPort": "https" }]
            }
        }),
    )
    .await?;

    let (addresses, port, node_ips) = wait_for_addresses(&client, &opts.exposure).await?;
    let mut sans: BTreeSet<String> = [
        NAME.to_string(),
        format!("{NAME}.{NAMESPACE}"),
        format!("{NAME}.{NAMESPACE}.svc"),
        format!("{NAME}.{NAMESPACE}.svc.cluster.local"),
        "localhost".into(),
        "127.0.0.1".into(),
    ]
    .into_iter()
    .collect();
    sans.extend(addresses.iter().cloned());
    sans.extend(node_ips.iter().cloned());
    if let Some(h) = &external {
        sans.insert(h.clone());
    }
    let certs = generate_certs(&sans)?;
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();

    apply(&client, &res_secret(), Some(NAMESPACE), json!({
        "apiVersion": "v1", "kind": "Secret", "type": "kubernetes.io/tls",
        "metadata": { "name": format!("{NAME}-tls"), "namespace": NAMESPACE, "labels": labels(&version) },
        "data": {
            "tls.crt": B64.encode(&certs.cert_pem),
            "tls.key": B64.encode(&certs.key_pem),
            "ca.crt": B64.encode(&certs.ca_pem)
        }
    }))
    .await?;
    apply(&client, &res_secret(), Some(NAMESPACE), json!({
        "apiVersion": "v1", "kind": "Secret", "type": "Opaque",
        "metadata": { "name": format!("{NAME}-auth"), "namespace": NAMESPACE, "labels": labels(&version) },
        "data": { "token": B64.encode(&token) }
    }))
    .await?;
    // Restart pods whenever certificates / token change.
    let config_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        certs.cert_pem.hash(&mut h);
        token.hash(&mut h);
        format!("{:016x}", h.finish())
    };
    apply(
        &client,
        &builtin::deployments(),
        Some(NAMESPACE),
        deployment(&image, &version, &config_hash, &opts.role),
    )
    .await?;
    wait_rollout(&client, Duration::from_secs(180)).await?;

    let primary_host = external
        .clone()
        .or_else(|| addresses.first().cloned())
        .ok_or_else(|| AppError::internal("no address"))?;
    let fmt_host = |h: &str| {
        if h.contains(':') {
            format!("[{h}]")
        } else {
            h.to_string()
        }
    };
    let endpoint = format!("https://{}:{port}", fmt_host(&primary_host));
    let alternative_endpoints = addresses
        .iter()
        .chain(node_ips.iter())
        .filter(|a| **a != primary_host)
        .map(|a| format!("https://{}:{port}", fmt_host(a)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(AgentInstallResult {
        endpoint,
        alternative_endpoints,
        ca_pem: certs.ca_pem,
        token,
        version,
        image,
        role: opts.role.clone(),
        exposure: opts.exposure.clone(),
    })
}

pub async fn status(client: Client) -> AppResult<AgentStatus> {
    let dep_api = builtin::deployments().api_for_object(client.clone(), Some(NAMESPACE))?;
    let dep = match dep_api.get_opt(NAME).await {
        Ok(d) => d,
        Err(e) => {
            return Ok(AgentStatus {
                installed: false,
                image: None,
                version: None,
                ready: None,
                healthy: false,
                exposure: None,
                role: None,
                latest_version: AGENT_VERSION.into(),
                update_available: false,
                message: Some(AppError::from(e).to_string()),
            })
        }
    };
    let Some(dep) = dep else {
        return Ok(AgentStatus {
            installed: false,
            image: None,
            version: None,
            ready: None,
            healthy: false,
            exposure: None,
            role: None,
            latest_version: AGENT_VERSION.into(),
            update_available: false,
            message: None,
        });
    };
    let v = serde_json::to_value(&dep)?;
    let image = v
        .pointer("/spec/template/spec/containers/0/image")
        .and_then(Value::as_str)
        .map(String::from);
    let version = image.as_deref().and_then(image_tag);
    let row = super::summary::summarize("Deployment", &v);
    let exposure = builtin::services()
        .api_for_object(client.clone(), Some(NAMESPACE))?
        .get_opt(NAME)
        .await
        .ok()
        .flatten()
        .and_then(|s| {
            s.data
                .pointer("/spec/type")
                .and_then(Value::as_str)
                .map(String::from)
        });
    let annotated_role = v
        .pointer("/metadata/annotations/manifex.dev~1role")
        .and_then(Value::as_str)
        .map(String::from);
    let role = res_crb()
        .api_for_object(client, None)?
        .get_opt(NAME)
        .await
        .ok()
        .flatten()
        .and_then(|c| {
            c.data
                .pointer("/roleRef/name")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .or(annotated_role);
    let update_available = version
        .as_deref()
        .map(|cur| version_newer(AGENT_VERSION, cur))
        .unwrap_or(false);
    Ok(AgentStatus {
        installed: true,
        healthy: row.health == super::summary::Health::Ok,
        ready: row
            .cells
            .get("ready")
            .and_then(Value::as_str)
            .map(String::from),
        image,
        version,
        exposure,
        role,
        latest_version: AGENT_VERSION.into(),
        update_available,
        message: None,
    })
}

/// Rolls the agent to `version` (default: the version bundled with this app).
pub async fn update(client: Client, version: Option<&str>) -> AppResult<AgentStatus> {
    let target = version
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(AGENT_VERSION);
    if !target
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        || target.len() > 64
    {
        return Err(AppError::validation("invalid version"));
    }
    let dep_api = builtin::deployments().api_for_object(client.clone(), Some(NAMESPACE))?;
    let dep = dep_api.get(NAME).await?;
    let current = dep
        .data
        .pointer("/spec/template/spec/containers/0/image")
        .and_then(Value::as_str)
        .unwrap_or(IMAGE_REPO)
        .to_string();
    let repo = current
        .rsplit_once(':')
        .filter(|(r, _)| !r.ends_with('/') && !r.contains('@'))
        .map(|(r, _)| r.to_string())
        .unwrap_or(current);
    let image = format!("{repo}:{target}");
    let patch = json!({
        "metadata": { "labels": { "app.kubernetes.io/version": target } },
        "spec": { "template": {
            "metadata": { "labels": { "app.kubernetes.io/version": target } },
            "spec": { "containers": [{ "name": "agent", "image": image }] }
        }}
    });
    dep_api
        .patch(NAME, &PatchParams::default(), &Patch::Strategic(&patch))
        .await?;
    wait_rollout(&client, Duration::from_secs(180)).await?;
    status(client).await
}

pub async fn uninstall(client: Client) -> AppResult<()> {
    let dp = DeleteParams::background();
    fn ignore_missing<T>(r: Result<T, kube::Error>) -> AppResult<()> {
        match r {
            Ok(_) => Ok(()),
            Err(kube::Error::Api(s)) if s.code == 404 => Ok(()),
            Err(e) => Err(AppError::from(e)),
        }
    }
    ignore_missing(
        res_crb()
            .api_for_object(client.clone(), None)?
            .delete(NAME, &dp)
            .await,
    )?;
    let ns_api = res_namespace().api_for_object(client, None)?;
    if let Some(ns) = ns_api.get_opt(NAMESPACE).await? {
        let managed = ns
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("app.kubernetes.io/managed-by"))
            .map(|v| v == "manifex")
            .unwrap_or(false);
        if managed {
            ignore_missing(ns_api.delete(NAMESPACE, &dp).await)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(version_newer("0.10.0", "0.9.9"));
        assert!(version_newer("v1.2.3", "1.2.2"));
        assert!(!version_newer("0.3.0", "0.3.0"));
        assert!(!version_newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn extracts_image_tags() {
        assert_eq!(
            image_tag("ghcr.io/learnivram/manifex-agent:0.3.0").as_deref(),
            Some("0.3.0")
        );
        assert_eq!(
            image_tag("registry:5000/agent:1.0@sha256:abc").as_deref(),
            Some("1.0")
        );
        assert_eq!(image_tag("registry:5000/agent"), None);
    }

    #[test]
    fn generates_ca_signed_server_certificate() {
        let sans: BTreeSet<String> = [
            "manifex-agent.manifex-system.svc".to_string(),
            "203.0.113.7".to_string(),
        ]
        .into_iter()
        .collect();
        let c = generate_certs(&sans).unwrap();
        assert!(c.ca_pem.contains("BEGIN CERTIFICATE"));
        assert!(c.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(c.key_pem.contains("PRIVATE KEY"));
        assert_ne!(c.ca_pem, c.cert_pem);
    }

    #[test]
    fn validates_inputs() {
        assert!(validate_host("agent.example.com").is_ok());
        assert!(validate_host("bad host").is_err());
        assert!(validate_image("ghcr.io/x/y:1.0").is_ok());
        assert!(validate_image("--privileged").is_err());
    }
}
