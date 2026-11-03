//! Helm integration.
//!
//! * Reading releases is native: Helm 3 stores releases as Secrets
//!   (`owner=helm`, type `helm.sh/release.v1`) whose payload is
//!   base64(gzip(JSON)). Listing, history, values, manifests and notes therefore
//!   work without any external binary.
//! * Mutating operations (install, upgrade, rollback, uninstall) and repository
//!   management delegate to the official `helm` binary, which guarantees identical
//!   semantics (hooks, chart rendering, OCI registries). The binary is executed
//!   directly with an argument vector – never through a shell – and every argument
//!   is validated beforehand.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use k8s_openapi::api::core::v1::Secret;
use kube::api::ListParams;
use kube::{Api, Client};
use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::persistence::clusters::ClusterSource;
use crate::security::validate;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmRelease {
    pub name: String,
    pub namespace: String,
    pub revision: i64,
    pub status: String,
    pub chart: String,
    pub chart_version: String,
    pub app_version: Option<String>,
    pub updated: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmRevision {
    pub revision: i64,
    pub status: String,
    pub chart: String,
    pub chart_version: String,
    pub app_version: Option<String>,
    pub updated: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmReleaseDetail {
    pub release: HelmRelease,
    pub history: Vec<HelmRevision>,
    /// User supplied values (as YAML).
    pub values: String,
    /// Chart default values (as YAML).
    pub chart_values: String,
    pub manifest: String,
    pub notes: Option<String>,
    pub resources: Vec<ManifestResource>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestResource {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
}

/// Decodes the payload of a Helm release secret (`data.release`).
pub fn decode_release(raw: &[u8]) -> AppResult<Value> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| AppError::validation("helm release payload is not base64"))?;
    let json_bytes = if decoded.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&decoded[..])
            .take(64 * 1024 * 1024)
            .read_to_end(&mut out)
            .map_err(|e| AppError::validation(format!("helm release payload is corrupt: {e}")))?;
        out
    } else {
        decoded
    };
    Ok(serde_json::from_slice(&json_bytes)?)
}

fn s(v: &Value, p: &str) -> Option<String> {
    v.pointer(p).and_then(Value::as_str).map(String::from)
}

fn release_from_json(v: &Value) -> HelmRelease {
    HelmRelease {
        name: s(v, "/name").unwrap_or_default(),
        namespace: s(v, "/namespace").unwrap_or_default(),
        revision: v.pointer("/version").and_then(Value::as_i64).unwrap_or(0),
        status: s(v, "/info/status").unwrap_or_else(|| "unknown".into()),
        chart: s(v, "/chart/metadata/name").unwrap_or_default(),
        chart_version: s(v, "/chart/metadata/version").unwrap_or_default(),
        app_version: s(v, "/chart/metadata/appVersion"),
        updated: s(v, "/info/last_deployed"),
        description: s(v, "/info/description"),
        icon: s(v, "/chart/metadata/icon").filter(|i| i.starts_with("https://")),
    }
}

async fn release_secrets(
    client: Client,
    namespace: Option<&str>,
    name: Option<&str>,
) -> AppResult<Vec<Secret>> {
    let api: Api<Secret> = match namespace.filter(|n| !n.is_empty()) {
        Some(ns) => Api::namespaced(client, ns),
        None => Api::all(client),
    };
    let mut selector = "owner=helm".to_string();
    if let Some(n) = name {
        selector.push_str(&format!(",name={n}"));
    }
    let list = tokio::time::timeout(
        Duration::from_secs(30),
        api.list(&ListParams::default().labels(&selector)),
    )
    .await
    .map_err(|_| AppError::Connection("listing helm releases timed out".into()))??;
    Ok(list
        .items
        .into_iter()
        .filter(|s| s.type_.as_deref() == Some("helm.sh/release.v1"))
        .collect())
}

fn label(secret: &Secret, key: &str) -> Option<String> {
    secret
        .metadata
        .labels
        .as_ref()
        .and_then(|l| l.get(key))
        .cloned()
}

pub async fn list_releases(client: Client, namespace: Option<&str>) -> AppResult<Vec<HelmRelease>> {
    validate::optional_namespace(&namespace.map(String::from))?;
    let secrets = release_secrets(client, namespace, None).await?;
    // Keep only the latest revision per (namespace, name); decode just those.
    let mut latest: BTreeMap<(String, String), (i64, &Secret)> = BTreeMap::new();
    for sec in &secrets {
        let (Some(name), Some(ver)) = (
            label(sec, "name"),
            label(sec, "version").and_then(|v| v.parse::<i64>().ok()),
        ) else {
            continue;
        };
        let ns = sec.metadata.namespace.clone().unwrap_or_default();
        let entry = latest.entry((ns, name)).or_insert((ver, sec));
        if ver > entry.0 {
            *entry = (ver, sec);
        }
    }
    let mut out = Vec::new();
    for ((ns, name), (ver, sec)) in latest {
        let decoded = sec
            .data
            .as_ref()
            .and_then(|d| d.get("release"))
            .map(|b| decode_release(&b.0));
        match decoded {
            Some(Ok(v)) => out.push(release_from_json(&v)),
            _ => out.push(HelmRelease {
                name,
                namespace: ns,
                revision: ver,
                status: label(sec, "status").unwrap_or_else(|| "unknown".into()),
                chart: String::new(),
                chart_version: String::new(),
                app_version: None,
                updated: None,
                description: Some("release payload could not be decoded".into()),
                icon: None,
            }),
        }
    }
    Ok(out)
}

pub fn manifest_resources(manifest: &str) -> Vec<ManifestResource> {
    let mut out = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(manifest) {
        use serde::Deserialize as _;
        let Ok(v) = serde_yaml::Value::deserialize(doc) else {
            continue;
        };
        let Ok(j) = serde_json::to_value(v) else {
            continue;
        };
        let (Some(api_version), Some(kind), Some(name)) = (
            s(&j, "/apiVersion"),
            s(&j, "/kind"),
            s(&j, "/metadata/name"),
        ) else {
            continue;
        };
        out.push(ManifestResource {
            api_version,
            kind,
            name,
            namespace: s(&j, "/metadata/namespace"),
        });
    }
    out
}

pub async fn release_detail(
    client: Client,
    namespace: &str,
    name: &str,
) -> AppResult<HelmReleaseDetail> {
    validate::namespace(namespace)?;
    validate::helm_release(name)?;
    let secrets = release_secrets(client, Some(namespace), Some(name)).await?;
    let mut revisions: Vec<Value> = secrets
        .iter()
        .filter_map(|s| {
            s.data
                .as_ref()?
                .get("release")
                .and_then(|b| decode_release(&b.0).ok())
        })
        .collect();
    revisions.sort_by_key(|v| -v.pointer("/version").and_then(Value::as_i64).unwrap_or(0));
    let latest = revisions
        .first()
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("helm release {namespace}/{name}")))?;
    let to_yaml = |v: Option<&Value>| -> String {
        match v {
            Some(v) if !v.is_null() && v.as_object().map(|o| !o.is_empty()).unwrap_or(true) => {
                serde_yaml::to_string(v).unwrap_or_default()
            }
            _ => String::new(),
        }
    };
    let manifest = s(&latest, "/manifest").unwrap_or_default();
    Ok(HelmReleaseDetail {
        release: release_from_json(&latest),
        history: revisions
            .iter()
            .map(|v| HelmRevision {
                revision: v.pointer("/version").and_then(Value::as_i64).unwrap_or(0),
                status: s(v, "/info/status").unwrap_or_default(),
                chart: s(v, "/chart/metadata/name").unwrap_or_default(),
                chart_version: s(v, "/chart/metadata/version").unwrap_or_default(),
                app_version: s(v, "/chart/metadata/appVersion"),
                updated: s(v, "/info/last_deployed"),
                description: s(v, "/info/description"),
            })
            .collect(),
        values: to_yaml(latest.get("config")),
        chart_values: to_yaml(latest.pointer("/chart/values")),
        resources: manifest_resources(&manifest),
        manifest,
        notes: s(&latest, "/info/notes").filter(|n| !n.is_empty()),
    })
}

// ---------------------------------------------------------------- Helm CLI

pub struct HelmCli {
    binary: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Kubeconfig handed to the Helm CLI. For file based profiles we pass the user's
/// own kubeconfig path; for vault based profiles a private temporary file (0600) is
/// written and deleted as soon as the command finishes.
pub enum HelmKubeconfig {
    Path {
        path: String,
        context: String,
    },
    Temp {
        file: tempfile::NamedTempFile,
        context: String,
    },
}

impl HelmKubeconfig {
    pub fn for_source(source: &ClusterSource, secret: Option<Vec<u8>>) -> AppResult<Self> {
        match source {
            ClusterSource::Kubeconfig { path, context } => Ok(Self::Path {
                path: path.clone(),
                context: context.clone(),
            }),
            other => {
                let (kc, context) = super::kubeconfig::resolve_kubeconfig(other, secret)?;
                let yaml = super::kubeconfig::kubeconfig_to_yaml_with_secrets(&kc)?;
                let mut file = tempfile::Builder::new()
                    .prefix("manifex-kc-")
                    .suffix(".yaml")
                    .tempfile()?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600))?;
                }
                use std::io::Write;
                file.write_all(yaml.as_bytes())?;
                file.flush()?;
                Ok(Self::Temp { file, context })
            }
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            HelmKubeconfig::Path { path, context } => {
                vec![
                    "--kubeconfig".into(),
                    path.clone(),
                    "--kube-context".into(),
                    context.clone(),
                ]
            }
            HelmKubeconfig::Temp { file, context } => vec![
                "--kubeconfig".into(),
                file.path().to_string_lossy().to_string(),
                "--kube-context".into(),
                context.clone(),
            ],
        }
    }
}

impl HelmCli {
    pub fn locate(configured: &str) -> AppResult<Self> {
        let binary = if !configured.is_empty() {
            let p = PathBuf::from(configured);
            let fname = p.file_stem().and_then(|f| f.to_str()).unwrap_or("");
            if !fname.starts_with("helm") {
                return Err(AppError::Security(
                    "the configured Helm binary must be named 'helm'".into(),
                ));
            }
            if !p.is_file() {
                return Err(AppError::NotFound(format!("Helm binary at {configured}")));
            }
            p
        } else {
            which::which("helm").map_err(|_| {
                AppError::NotFound(
                    "Helm CLI not found on PATH. Install Helm 3 or set its path in Settings → Helm"
                        .into(),
                )
            })?
        };
        Ok(Self { binary })
    }

    pub fn path(&self) -> &Path {
        &self.binary
    }

    async fn run(&self, args: Vec<String>, timeout: Duration) -> AppResult<HelmOutput> {
        let mut cmd = tokio::process::Command::new(&self.binary);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("HELM_NO_PLUGINS", "1");
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW – avoid flashing console windows
            cmd.creation_flags(0x0800_0000);
        }
        let child = cmd
            .spawn()
            .map_err(|e| AppError::Io(format!("failed to start helm: {e}")))?;
        let out = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| AppError::Internal("helm command timed out".into()))??;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() {
            return Err(AppError::Internal(format!(
                "helm failed: {}",
                stderr.trim()
            )));
        }
        Ok(HelmOutput { stdout, stderr })
    }

    pub async fn version(&self) -> AppResult<String> {
        Ok(self
            .run(
                vec!["version".into(), "--short".into()],
                Duration::from_secs(10),
            )
            .await?
            .stdout
            .trim()
            .to_string())
    }

    pub async fn rollback(
        &self,
        kc: &HelmKubeconfig,
        ns: &str,
        name: &str,
        revision: i64,
    ) -> AppResult<HelmOutput> {
        validate::namespace(ns)?;
        validate::helm_release(name)?;
        if revision < 1 {
            return Err(AppError::validation("revision must be positive"));
        }
        let mut args = vec![
            "rollback".into(),
            name.into(),
            revision.to_string(),
            "--namespace".into(),
            ns.into(),
            "--wait".into(),
            "--timeout".into(),
            "5m".into(),
        ];
        args.extend(kc.args());
        self.run(args, Duration::from_secs(360)).await
    }

    pub async fn uninstall(
        &self,
        kc: &HelmKubeconfig,
        ns: &str,
        name: &str,
        keep_history: bool,
    ) -> AppResult<HelmOutput> {
        validate::namespace(ns)?;
        validate::helm_release(name)?;
        let mut args = vec![
            "uninstall".into(),
            name.into(),
            "--namespace".into(),
            ns.into(),
        ];
        if keep_history {
            args.push("--keep-history".into());
        }
        args.extend(kc.args());
        self.run(args, Duration::from_secs(360)).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upgrade_install(
        &self,
        kc: &HelmKubeconfig,
        ns: &str,
        name: &str,
        chart: &str,
        version: Option<&str>,
        values_yaml: Option<&str>,
        create_namespace: bool,
        dry_run: bool,
    ) -> AppResult<HelmOutput> {
        validate::namespace(ns)?;
        validate::helm_release(name)?;
        validate::helm_chart_ref(chart)?;
        let mut args = vec![
            "upgrade".into(),
            "--install".into(),
            name.into(),
            chart.into(),
            "--namespace".into(),
            ns.into(),
            "--wait".into(),
            "--timeout".into(),
            "5m".into(),
        ];
        if let Some(v) = version.filter(|v| !v.is_empty()) {
            validate::helm_version(v)?;
            args.push("--version".into());
            args.push(v.into());
        }
        if create_namespace {
            args.push("--create-namespace".into());
        }
        if dry_run {
            args.push("--dry-run=server".into());
        }
        // values are passed through a private temp file
        let mut values_file = None;
        if let Some(values) = values_yaml.filter(|v| !v.trim().is_empty()) {
            let _: serde_yaml::Value = serde_yaml::from_str(values)?;
            let mut f = tempfile::Builder::new()
                .prefix("manifex-values-")
                .suffix(".yaml")
                .tempfile()?;
            use std::io::Write;
            f.write_all(values.as_bytes())?;
            f.flush()?;
            args.push("--values".into());
            args.push(f.path().to_string_lossy().to_string());
            values_file = Some(f);
        }
        args.extend(kc.args());
        let res = self.run(args, Duration::from_secs(360)).await;
        drop(values_file);
        res
    }

    pub async fn repo_list(&self) -> AppResult<Value> {
        match self
            .run(
                vec!["repo".into(), "list".into(), "-o".into(), "json".into()],
                Duration::from_secs(15),
            )
            .await
        {
            Ok(out) => Ok(serde_json::from_str(&out.stdout).unwrap_or(Value::Array(vec![]))),
            // `helm repo list` exits non-zero when no repositories exist
            Err(AppError::Internal(msg)) if msg.contains("no repositories") => {
                Ok(Value::Array(vec![]))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn repo_add(&self, name: &str, url: &str) -> AppResult<HelmOutput> {
        validate::helm_repo_name(name)?;
        let uri = validate::https_url("repository URL", url)?;
        self.run(
            vec![
                "repo".into(),
                "add".into(),
                name.into(),
                uri.to_string(),
                "--force-update".into(),
            ],
            Duration::from_secs(60),
        )
        .await
    }

    pub async fn repo_remove(&self, name: &str) -> AppResult<HelmOutput> {
        validate::helm_repo_name(name)?;
        self.run(
            vec!["repo".into(), "remove".into(), name.into()],
            Duration::from_secs(15),
        )
        .await
    }

    pub async fn repo_update(&self) -> AppResult<HelmOutput> {
        self.run(
            vec!["repo".into(), "update".into()],
            Duration::from_secs(180),
        )
        .await
    }

    pub async fn search(&self, query: &str) -> AppResult<Value> {
        if query.len() > 200 || query.starts_with('-') || query.chars().any(char::is_control) {
            return Err(AppError::validation("invalid search query"));
        }
        let mut args = vec!["search".into(), "repo".into()];
        if !query.trim().is_empty() {
            args.push(query.trim().into());
        }
        args.extend([
            "-o".into(),
            "json".into(),
            "--max-col-width".into(),
            "0".into(),
        ]);
        let out = self.run(args, Duration::from_secs(30)).await?;
        Ok(serde_json::from_str(&out.stdout).unwrap_or(Value::Array(vec![])))
    }

    pub async fn show_values(&self, chart: &str, version: Option<&str>) -> AppResult<String> {
        validate::helm_chart_ref(chart)?;
        let mut args = vec!["show".into(), "values".into(), chart.into()];
        if let Some(v) = version.filter(|v| !v.is_empty()) {
            validate::helm_version(v)?;
            args.push("--version".into());
            args.push(v.into());
        }
        Ok(self.run(args, Duration::from_secs(60)).await?.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn encode(v: &Value) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(serde_json::to_string(v).unwrap().as_bytes())
            .unwrap();
        base64::engine::general_purpose::STANDARD
            .encode(gz.finish().unwrap())
            .into_bytes()
    }

    #[test]
    fn decodes_helm_release_payload() {
        let rel = serde_json::json!({
            "name": "web", "namespace": "prod", "version": 3,
            "info": {"status": "deployed", "last_deployed": "2024-05-01T10:00:00Z", "notes": "hello"},
            "chart": {"metadata": {"name": "nginx", "version": "15.0.0", "appVersion": "1.25"}, "values": {"replicaCount": 1}},
            "config": {"replicaCount": 3},
            "manifest": "---\napiVersion: v1\nkind: Service\nmetadata:\n  name: web\n---\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n"
        });
        let decoded = decode_release(&encode(&rel)).unwrap();
        let r = release_from_json(&decoded);
        assert_eq!(r.name, "web");
        assert_eq!(r.revision, 3);
        assert_eq!(r.chart_version, "15.0.0");
        let res = manifest_resources(decoded["manifest"].as_str().unwrap());
        assert_eq!(res.len(), 2);
        assert_eq!(res[1].kind, "Deployment");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_release(b"!!!").is_err());
    }

    #[test]
    fn helm_binary_name_is_enforced() {
        assert!(matches!(
            HelmCli::locate("/bin/sh"),
            Err(AppError::Security(_))
        ));
    }
}
