//! Kubeconfig discovery, inspection and conversion into client configurations.

use std::path::{Path, PathBuf};

use base64::Engine;
use kube::config::{
    AuthInfo, Cluster, Context, KubeConfigOptions, Kubeconfig, NamedAuthInfo, NamedCluster,
    NamedContext,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::persistence::clusters::ClusterSource;
use crate::security::validate;

const MAX_KUBECONFIG_BYTES: u64 = 4 * 1024 * 1024;
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInfo {
    pub name: String,
    pub cluster: String,
    pub user: Option<String>,
    pub namespace: Option<String>,
    pub server: Option<String>,
    /// Authentication mechanism detected for the user entry.
    pub auth: String,
    /// The kubeconfig requests disabled TLS verification. Manifex refuses such
    /// connections; the UI shows a warning and asks for a CA certificate.
    pub insecure: bool,
    pub already_added: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredKubeconfig {
    pub path: String,
    pub contexts: Vec<ContextInfo>,
    pub current_context: Option<String>,
    pub error: Option<String>,
}

/// Credentials used for direct (non-kubeconfig) connections; deserialised from the
/// vault, never sent back to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateCredentials {
    pub cert_pem: String,
    pub key_pem: String,
}

/// Candidate kubeconfig paths: `$KUBECONFIG` entries, `~/.kube/config` and other
/// kubeconfig-looking files directly inside `~/.kube`.
pub fn candidate_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(var) = std::env::var_os("KUBECONFIG") {
        for p in std::env::split_paths(&var) {
            if !p.as_os_str().is_empty() {
                out.push(p);
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        let kube_dir = home.join(".kube");
        out.push(kube_dir.join("config"));
        if let Ok(entries) = std::fs::read_dir(&kube_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.starts_with('.') || !path.is_file() {
                    continue;
                }
                let looks_like = name.ends_with(".yaml")
                    || name.ends_with(".yml")
                    || name.ends_with(".conf")
                    || name.ends_with(".kubeconfig")
                    || name.contains("config");
                if looks_like {
                    out.push(path);
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.into_iter()
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(p.canonicalize().unwrap_or_else(|_| p.clone())))
        .collect()
}

pub fn read_kubeconfig(path: &Path) -> AppResult<Kubeconfig> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_KUBECONFIG_BYTES {
        return Err(AppError::validation("kubeconfig file is too large"));
    }
    Ok(Kubeconfig::read_from(path)?)
}

pub fn auth_kind(auth: Option<&AuthInfo>) -> String {
    let Some(a) = auth else { return "none".into() };
    if let Some(exec) = &a.exec {
        let cmd = exec.command.clone().unwrap_or_default();
        let base = Path::new(&cmd)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("plugin")
            .to_string();
        return format!("exec:{base}");
    }
    if let Some(p) = &a.auth_provider {
        return format!("provider:{}", p.name);
    }
    if a.client_certificate.is_some() || a.client_certificate_data.is_some() {
        return "certificate".into();
    }
    if a.token.is_some() || a.token_file.is_some() {
        return "token".into();
    }
    if a.username.is_some() {
        return "basic".into();
    }
    "none".into()
}

pub fn list_contexts(kc: &Kubeconfig) -> Vec<ContextInfo> {
    kc.contexts
        .iter()
        .filter_map(|nc| {
            let ctx = nc.context.as_ref()?;
            let cluster = kc
                .clusters
                .iter()
                .find(|c| c.name == ctx.cluster)
                .and_then(|c| c.cluster.as_ref());
            let auth = ctx
                .user
                .as_ref()
                .and_then(|u| kc.auth_infos.iter().find(|a| &a.name == u))
                .and_then(|a| a.auth_info.as_ref());
            Some(ContextInfo {
                name: nc.name.clone(),
                cluster: ctx.cluster.clone(),
                user: ctx.user.clone(),
                namespace: ctx.namespace.clone(),
                server: cluster.and_then(|c| c.server.clone()),
                auth: auth_kind(auth),
                insecure: cluster
                    .and_then(|c| c.insecure_skip_tls_verify)
                    .unwrap_or(false),
                already_added: false,
            })
        })
        .collect()
}

pub fn inspect_path(path: &Path) -> DiscoveredKubeconfig {
    let display = path.to_string_lossy().to_string();
    match read_kubeconfig(path) {
        Ok(kc) => DiscoveredKubeconfig {
            path: display,
            contexts: list_contexts(&kc),
            current_context: kc.current_context.clone(),
            error: None,
        },
        Err(e) => DiscoveredKubeconfig {
            path: display,
            contexts: vec![],
            current_context: None,
            error: Some(e.to_string()),
        },
    }
}

fn read_file_b64(path: &str) -> AppResult<String> {
    let bytes = std::fs::read(path).map_err(|e| AppError::Io(format!("{path}: {e}")))?;
    Ok(B64.encode(bytes))
}

/// Produces a self-contained kubeconfig with exactly one context. File references
/// (CA, client certificate, key, token file) are inlined so the result can be
/// stored in the encrypted vault independent of the original file.
pub fn extract_single_context(kc: &Kubeconfig, context: &str) -> AppResult<Kubeconfig> {
    let nctx = kc
        .contexts
        .iter()
        .find(|c| c.name == context)
        .ok_or_else(|| AppError::NotFound(format!("context '{context}'")))?;
    let ctx = nctx
        .context
        .clone()
        .ok_or_else(|| AppError::validation("context is empty"))?;
    let mut cluster = kc
        .clusters
        .iter()
        .find(|c| c.name == ctx.cluster)
        .cloned()
        .ok_or_else(|| {
            AppError::validation(format!("cluster '{}' missing in kubeconfig", ctx.cluster))
        })?;
    if let Some(c) = cluster.cluster.as_mut() {
        if let Some(path) = c.certificate_authority.take() {
            if c.certificate_authority_data.is_none() {
                c.certificate_authority_data = Some(read_file_b64(&path)?);
            }
        }
    }
    let mut users = Vec::new();
    if let Some(user_name) = &ctx.user {
        let mut user = kc
            .auth_infos
            .iter()
            .find(|a| &a.name == user_name)
            .cloned()
            .ok_or_else(|| {
                AppError::validation(format!("user '{user_name}' missing in kubeconfig"))
            })?;
        if let Some(a) = user.auth_info.as_mut() {
            if let Some(path) = a.client_certificate.take() {
                if a.client_certificate_data.is_none() {
                    a.client_certificate_data = Some(read_file_b64(&path)?);
                }
            }
            if let Some(path) = a.client_key.take() {
                if a.client_key_data.is_none() {
                    a.client_key_data = Some(SecretString::from(read_file_b64(&path)?));
                }
            }
            if let Some(path) = a.token_file.take() {
                if a.token.is_none() {
                    let t = std::fs::read_to_string(&path)
                        .map_err(|e| AppError::Io(format!("{path}: {e}")))?;
                    a.token = Some(SecretString::from(t.trim().to_string()));
                }
            }
        }
        users.push(user);
    }
    Ok(Kubeconfig {
        clusters: vec![cluster],
        auth_infos: users,
        contexts: vec![nctx.clone()],
        current_context: Some(context.to_string()),
        api_version: Some("v1".into()),
        kind: Some("Config".into()),
        ..Default::default()
    })
}

/// Serialises a kubeconfig *including* secret fields (only ever written into the
/// encrypted vault or a short-lived 0600 temp file for the Helm CLI).
pub fn kubeconfig_to_yaml_with_secrets(kc: &Kubeconfig) -> AppResult<String> {
    let mut value = serde_json::to_value(kc)?;
    // SecretString fields are skipped/redacted by serde; put them back explicitly.
    if let Some(users) = value.get_mut("users").and_then(|u| u.as_array_mut()) {
        for (idx, named) in kc.auth_infos.iter().enumerate() {
            let Some(auth) = &named.auth_info else {
                continue;
            };
            let Some(user_obj) = users
                .get_mut(idx)
                .and_then(|u| u.get_mut("user"))
                .and_then(|u| u.as_object_mut())
            else {
                continue;
            };
            if let Some(t) = &auth.token {
                user_obj.insert("token".into(), t.expose_secret().into());
            }
            if let Some(k) = &auth.client_key_data {
                user_obj.insert("client-key-data".into(), k.expose_secret().into());
            }
            if let Some(p) = &auth.password {
                user_obj.insert("password".into(), p.expose_secret().into());
            }
        }
    }
    Ok(serde_yaml::to_string(&value)?)
}

fn ensure_secure(kc: &Kubeconfig, context: &str) -> AppResult<()> {
    let ctx = kc
        .contexts
        .iter()
        .find(|c| c.name == context)
        .and_then(|c| c.context.as_ref())
        .ok_or_else(|| {
            AppError::NotFound(format!("context '{context}' not found in kubeconfig"))
        })?;
    let cluster = kc
        .clusters
        .iter()
        .find(|c| c.name == ctx.cluster)
        .and_then(|c| c.cluster.as_ref());
    if let Some(c) = cluster {
        if c.insecure_skip_tls_verify == Some(true) {
            return Err(AppError::Security(
                "this context sets insecure-skip-tls-verify. Manifex never disables certificate \
                 verification – add the cluster CA (certificate-authority-data) to the kubeconfig \
                 or connect via a direct connection with a CA certificate."
                    .into(),
            ));
        }
        if let Some(server) = &c.server {
            if server.starts_with("http://") {
                return Err(AppError::Security(
                    "plain HTTP API endpoints are not supported".into(),
                ));
            }
        }
    }
    Ok(())
}

pub fn pem_to_b64(pem: &str) -> AppResult<String> {
    let trimmed = pem.trim();
    if !trimmed.contains("-----BEGIN") {
        return Err(AppError::validation("expected a PEM encoded certificate"));
    }
    Ok(B64.encode(trimmed.as_bytes()))
}

fn synthetic_kubeconfig(
    server: &str,
    ca_pem: Option<&str>,
    tls_server_name: Option<&str>,
    proxy_url: Option<&str>,
    auth: AuthInfo,
) -> AppResult<Kubeconfig> {
    validate::https_url("server", server)?;
    if let Some(p) = proxy_url {
        validate::proxy_url(p)?;
    }
    let ca = ca_pem
        .filter(|s| !s.trim().is_empty())
        .map(pem_to_b64)
        .transpose()?;
    Ok(Kubeconfig {
        clusters: vec![NamedCluster {
            name: "manifex".into(),
            cluster: Some(Cluster {
                server: Some(server.trim().to_string()),
                certificate_authority_data: ca,
                tls_server_name: tls_server_name
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                proxy_url: proxy_url.filter(|s| !s.is_empty()).map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }],
        auth_infos: vec![NamedAuthInfo {
            name: "manifex".into(),
            auth_info: Some(auth),
            ..Default::default()
        }],
        contexts: vec![NamedContext {
            name: "manifex".into(),
            context: Some(Context {
                cluster: "manifex".into(),
                user: Some("manifex".into()),
                ..Default::default()
            }),
            ..Default::default()
        }],
        current_context: Some("manifex".into()),
        ..Default::default()
    })
}

/// Resolves a cluster source plus its secret material into a kubeconfig and the
/// context name to use.
pub fn resolve_kubeconfig(
    source: &ClusterSource,
    secret: Option<Vec<u8>>,
) -> AppResult<(Kubeconfig, String)> {
    let secret_str = || -> AppResult<String> {
        let bytes = secret
            .clone()
            .ok_or_else(|| AppError::Vault("credentials missing for this cluster".into()))?;
        String::from_utf8(bytes).map_err(|_| AppError::Vault("credentials are corrupt".into()))
    };
    match source {
        ClusterSource::Ssh { .. } => Err(AppError::internal(
            "SSH profiles are resolved through an SSH session (k8s::ssh)",
        )),
        ClusterSource::Kubeconfig { path, context } => {
            Ok((read_kubeconfig(Path::new(path))?, context.clone()))
        }
        ClusterSource::StoredKubeconfig { context, .. } => {
            Ok((Kubeconfig::from_yaml(&secret_str()?)?, context.clone()))
        }
        ClusterSource::Token {
            server,
            ca_pem,
            tls_server_name,
            proxy_url,
            ..
        } => {
            let auth = AuthInfo {
                token: Some(SecretString::from(secret_str()?)),
                ..Default::default()
            };
            Ok((
                synthetic_kubeconfig(
                    server,
                    ca_pem.as_deref(),
                    tls_server_name.as_deref(),
                    proxy_url.as_deref(),
                    auth,
                )?,
                "manifex".into(),
            ))
        }
        ClusterSource::Agent {
            endpoint,
            ca_pem,
            proxy_url,
            ..
        } => {
            let auth = AuthInfo {
                token: Some(SecretString::from(secret_str()?)),
                ..Default::default()
            };
            Ok((
                synthetic_kubeconfig(
                    endpoint,
                    ca_pem.as_deref(),
                    None,
                    proxy_url.as_deref(),
                    auth,
                )?,
                "manifex".into(),
            ))
        }
        ClusterSource::Certificate {
            server,
            ca_pem,
            tls_server_name,
            proxy_url,
            ..
        } => {
            let creds: CertificateCredentials = serde_json::from_str(&secret_str()?)
                .map_err(|_| AppError::Vault("certificate credentials are corrupt".into()))?;
            let auth = AuthInfo {
                client_certificate_data: Some(pem_to_b64(&creds.cert_pem)?),
                client_key_data: Some(SecretString::from(
                    B64.encode(creds.key_pem.trim().as_bytes()),
                )),
                ..Default::default()
            };
            Ok((
                synthetic_kubeconfig(
                    server,
                    ca_pem.as_deref(),
                    tls_server_name.as_deref(),
                    proxy_url.as_deref(),
                    auth,
                )?,
                "manifex".into(),
            ))
        }
    }
}

/// Builds a kube client configuration. TLS verification is always enforced.
pub async fn build_config(
    source: &ClusterSource,
    secret: Option<Vec<u8>>,
) -> AppResult<kube::Config> {
    let (kc, context) = resolve_kubeconfig(source, secret)?;
    build_config_from(kc, context).await
}

/// Builds a client configuration from an in-memory kubeconfig. TLS verification
/// is always enforced.
pub async fn build_config_from(kc: Kubeconfig, context: String) -> AppResult<kube::Config> {
    ensure_secure(&kc, &context)?;
    let options = KubeConfigOptions {
        context: Some(context),
        ..Default::default()
    };
    let mut config = kube::Config::from_custom_kubeconfig(kc, &options).await?;
    config.accept_invalid_certs = false;
    config.connect_timeout = Some(std::time::Duration::from_secs(10));
    // Each request (except streaming endpoints) is bounded by explicit timeouts at
    // call sites; streams (watch/logs/exec) must remain open indefinitely.
    config.read_timeout = None;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
apiVersion: v1
kind: Config
current-context: dev
clusters:
- name: dev-cluster
  cluster:
    server: https://127.0.0.1:6443
    certificate-authority-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCk1JSUIKLS0tLS1FTkQgQ0VSVElGSUNBVEUtLS0tLQo=
- name: insecure-cluster
  cluster:
    server: https://10.0.0.1:6443
    insecure-skip-tls-verify: true
users:
- name: dev-user
  user:
    token: abc123
- name: exec-user
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1beta1
      command: /usr/local/bin/aws
      args: ["eks", "get-token"]
contexts:
- name: dev
  context:
    cluster: dev-cluster
    user: dev-user
    namespace: apps
- name: insecure
  context:
    cluster: insecure-cluster
    user: exec-user
"#;

    #[test]
    fn lists_contexts_with_auth_kind() {
        let kc = Kubeconfig::from_yaml(SAMPLE).unwrap();
        let ctxs = list_contexts(&kc);
        assert_eq!(ctxs.len(), 2);
        assert_eq!(ctxs[0].auth, "token");
        assert_eq!(ctxs[0].namespace.as_deref(), Some("apps"));
        assert_eq!(ctxs[1].auth, "exec:aws");
        assert!(ctxs[1].insecure);
    }

    #[test]
    fn refuses_insecure_contexts() {
        let kc = Kubeconfig::from_yaml(SAMPLE).unwrap();
        assert!(ensure_secure(&kc, "dev").is_ok());
        assert!(matches!(
            ensure_secure(&kc, "insecure"),
            Err(AppError::Security(_))
        ));
    }

    #[test]
    fn extracts_single_context_and_keeps_secrets() {
        let kc = Kubeconfig::from_yaml(SAMPLE).unwrap();
        let single = extract_single_context(&kc, "dev").unwrap();
        assert_eq!(single.contexts.len(), 1);
        assert_eq!(single.clusters.len(), 1);
        let yaml = kubeconfig_to_yaml_with_secrets(&single).unwrap();
        assert!(
            yaml.contains("abc123"),
            "token must survive serialisation into the vault"
        );
        let back = Kubeconfig::from_yaml(&yaml).unwrap();
        assert_eq!(back.current_context.as_deref(), Some("dev"));
        assert_eq!(
            back.auth_infos[0]
                .auth_info
                .as_ref()
                .unwrap()
                .token
                .as_ref()
                .unwrap()
                .expose_secret(),
            "abc123"
        );
    }

    #[test]
    fn synthetic_requires_https() {
        let auth = AuthInfo::default();
        assert!(synthetic_kubeconfig("http://x:6443", None, None, None, auth.clone()).is_err());
        assert!(synthetic_kubeconfig("https://x:6443", None, None, None, auth).is_ok());
    }
}
