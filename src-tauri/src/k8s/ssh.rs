//! "Via SSH server" connections.
//!
//! The user only provides host, user and SSH credentials (private key, OpenSSH
//! certificate, passphrase and/or password). Manifex then
//!   1. verifies the server's host key (pinned fingerprint; `~/.ssh/known_hosts`
//!      is honoured, a changed key is always refused),
//!   2. logs in (certificate → public key → password),
//!   3. fetches the kubeconfig from the server (kubectl, ~/.kube/config, k3s,
//!      RKE2, kubeadm, MicroK8s – with sudo if necessary),
//!   4. opens a local TCP listener on 127.0.0.1 that forwards every connection
//!      through an SSH `direct-tcpip` channel to the API server, and
//!   5. points the Kubernetes client at that tunnel while keeping TLS
//!      verification against the API server's real name.
//!
//! The SSH session is re-established automatically if it drops; the local port
//! stays the same so the Kubernetes client keeps working.

use std::sync::Arc;
use std::time::Duration;

use kube::config::Kubeconfig;
use russh::client::{self, Handle};
use russh::keys::{
    Certificate, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate,
};
use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::AbortHandle;

use crate::error::{AppError, AppResult};
use crate::security::validate;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const EXEC_TIMEOUT: Duration = Duration::from_secs(20);

/// Secret material for an SSH profile (stored encrypted in the vault).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SshCredentials {
    /// OpenSSH / PEM / PuTTY private key.
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    /// OpenSSH user certificate (`id_xxx-cert.pub`).
    pub certificate: Option<String>,
    /// Login password and/or sudo password.
    pub password: Option<String>,
    /// Last kubeconfig fetched from the server (fallback if fetching fails).
    pub kubeconfig: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl SshTarget {
    pub fn validate(&self) -> AppResult<()> {
        let h = self.host.trim();
        if h.is_empty()
            || h.len() > 253
            || !h
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
        {
            return Err(AppError::validation("SSH host is invalid"));
        }
        if self.port == 0 {
            return Err(AppError::validation("SSH port is invalid"));
        }
        let u = self.user.trim();
        if u.is_empty()
            || u.len() > 64
            || !u
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'@'))
        {
            return Err(AppError::validation("SSH user is invalid"));
        }
        Ok(())
    }

    fn addr(&self) -> (String, u16) {
        (
            self.host
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
            self.port,
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum KnownHostState {
    /// Fingerprint matches an entry in ~/.ssh/known_hosts.
    Trusted,
    /// Host not in known_hosts – the user has to confirm the fingerprint.
    Unknown,
    /// known_hosts has a *different* key – possible man-in-the-middle.
    Changed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    pub fingerprint: String,
    pub algorithm: String,
    pub known_hosts: KnownHostState,
}

fn fingerprint_of(
    key: &PublicKeyOrCertificate,
) -> (String, String, Option<russh::keys::PublicKey>) {
    match key {
        PublicKeyOrCertificate::PublicKey { key, .. } => (
            key.fingerprint(HashAlg::Sha256).to_string(),
            key.algorithm().to_string(),
            Some(key.clone()),
        ),
        PublicKeyOrCertificate::Certificate(cert) => (
            cert.public_key().fingerprint(HashAlg::Sha256).to_string(),
            cert.algorithm().to_string(),
            None,
        ),
    }
}

pub struct Verifier {
    /// Pinned fingerprint; `None` = probe only (records the key, refuses login).
    expected: Option<String>,
    target: SshTarget,
    seen: Arc<std::sync::Mutex<Option<HostKeyInfo>>>,
}

impl client::Handler for Verifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let (fingerprint, algorithm, plain) = fingerprint_of(key);
        let known_hosts = match plain.as_ref() {
            Some(pk) => {
                match russh::keys::check_known_hosts(&self.target.addr().0, self.target.port, pk) {
                    Ok(true) => KnownHostState::Trusted,
                    Ok(false) => KnownHostState::Unknown,
                    Err(russh::keys::Error::KeyChanged { .. }) => KnownHostState::Changed,
                    Err(_) => KnownHostState::Unknown,
                }
            }
            None => KnownHostState::Unknown,
        };
        *self.seen.lock().expect("lock") = Some(HostKeyInfo {
            fingerprint: fingerprint.clone(),
            algorithm,
            known_hosts,
        });
        if known_hosts == KnownHostState::Changed {
            return Ok(false);
        }
        Ok(self.expected.as_deref() == Some(fingerprint.as_str()))
    }
}

fn ssh_config() -> Arc<client::Config> {
    Arc::new(client::Config {
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    })
}

/// Connects without credentials only to learn the server's host key.
pub async fn probe_host_key(target: &SshTarget) -> AppResult<HostKeyInfo> {
    target.validate()?;
    let seen = Arc::new(std::sync::Mutex::new(None));
    let verifier = Verifier {
        expected: None,
        target: target.clone(),
        seen: seen.clone(),
    };
    // The handshake is aborted by the verifier after the key was recorded.
    let _ = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect(ssh_config(), target.addr(), verifier),
    )
    .await;
    let info = seen.lock().expect("lock").clone();
    info.ok_or_else(|| {
        AppError::Connection(format!(
            "could not reach SSH server {}:{}",
            target.host, target.port
        ))
    })
}

fn parse_key(creds: &SshCredentials) -> AppResult<Option<Arc<PrivateKey>>> {
    let Some(pem) = creds
        .private_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
    else {
        return Ok(None);
    };
    let key = russh::keys::decode_secret_key(
        pem.trim(),
        creds.passphrase.as_deref().filter(|p| !p.is_empty()),
    )
    .map_err(|e| match e {
        russh::keys::Error::KeyIsEncrypted => {
            AppError::validation("the SSH key is encrypted – enter its passphrase")
        }
        other => AppError::validation(format!("cannot read SSH key: {other}")),
    })?;
    Ok(Some(Arc::new(key)))
}

/// Inspects a private key without exposing it (for the "choose key file" UI).
pub fn describe_key(pem: &str) -> AppResult<(bool, String)> {
    let key = PrivateKey::from_openssh(pem.trim());
    match key {
        Ok(k) => Ok((k.is_encrypted(), k.algorithm().to_string())),
        Err(_) => match russh::keys::decode_secret_key(pem.trim(), None) {
            Ok(k) => Ok((false, k.algorithm().to_string())),
            Err(russh::keys::Error::KeyIsEncrypted) => Ok((true, "encrypted".into())),
            Err(e) => Err(AppError::validation(format!(
                "not a supported SSH private key: {e}"
            ))),
        },
    }
}

/// Opens an authenticated SSH session to a host whose key must match `fingerprint`.
pub async fn connect(
    target: &SshTarget,
    creds: &SshCredentials,
    fingerprint: &str,
) -> AppResult<Handle<Verifier>> {
    target.validate()?;
    let seen = Arc::new(std::sync::Mutex::new(None));
    let verifier = Verifier {
        expected: Some(fingerprint.to_string()),
        target: target.clone(),
        seen: seen.clone(),
    };
    let connected = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect(ssh_config(), target.addr(), verifier),
    )
    .await
    .map_err(|_| AppError::Connection(format!("SSH connection to {} timed out", target.host)))?;
    let mut handle = match connected {
        Ok(h) => h,
        Err(e) => {
            let info = seen.lock().expect("lock").clone();
            return Err(match info {
                Some(i) if i.known_hosts == KnownHostState::Changed => AppError::Security(format!(
                    "the host key of {} differs from ~/.ssh/known_hosts – refusing to connect (possible man-in-the-middle)",
                    target.host
                )),
                Some(i) if i.fingerprint != fingerprint => AppError::Security(format!(
                    "the host key of {} changed (now {}) – refusing to connect. Re-verify the server and update the cluster.",
                    target.host, i.fingerprint
                )),
                _ => AppError::Connection(format!("SSH connection failed: {e}")),
            });
        }
    };

    let user = target.user.trim().to_string();
    let mut authenticated = false;
    let mut attempts: Vec<&str> = Vec::new();
    if let Some(key) = parse_key(creds)? {
        if let Some(cert_text) = creds
            .certificate
            .as_deref()
            .filter(|c| !c.trim().is_empty())
        {
            let cert = Certificate::from_openssh(cert_text.trim())
                .map_err(|e| AppError::validation(format!("cannot read SSH certificate: {e}")))?;
            attempts.push("certificate");
            authenticated = handle
                .authenticate_openssh_cert(user.clone(), key.clone(), cert)
                .await
                .map_err(|e| AppError::Connection(e.to_string()))?
                .success();
        }
        if !authenticated {
            attempts.push("public key");
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            authenticated = handle
                .authenticate_publickey(user.clone(), PrivateKeyWithHashAlg::new(key, hash))
                .await
                .map_err(|e| AppError::Connection(e.to_string()))?
                .success();
        }
    }
    if !authenticated {
        if let Some(pw) = creds.password.as_deref().filter(|p| !p.is_empty()) {
            attempts.push("password");
            authenticated = handle
                .authenticate_password(user.clone(), pw)
                .await
                .map_err(|e| AppError::Connection(e.to_string()))?
                .success();
        }
    }
    if !authenticated {
        return Err(AppError::Connection(if attempts.is_empty() {
            "no SSH credentials provided".into()
        } else {
            format!(
                "SSH login as '{user}' was rejected (tried: {})",
                attempts.join(", ")
            )
        }));
    }
    Ok(handle)
}

/// Runs a fixed command (never user supplied) and returns (exit code, stdout).
async fn exec(
    handle: &Handle<Verifier>,
    command: &str,
    stdin: Option<&str>,
) -> AppResult<(u32, String)> {
    let run = async {
        let mut ch = handle
            .channel_open_session()
            .await
            .map_err(|e| AppError::Connection(e.to_string()))?;
        ch.exec(true, command)
            .await
            .map_err(|e| AppError::Connection(e.to_string()))?;
        if let Some(input) = stdin {
            ch.data(format!("{input}\n").as_bytes())
                .await
                .map_err(|e| AppError::Connection(e.to_string()))?;
        }
        ch.eof()
            .await
            .map_err(|e| AppError::Connection(e.to_string()))?;
        let mut out = Vec::new();
        let mut code = None;
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    if out.len() < 8 * 1024 * 1024 {
                        out.extend_from_slice(&data);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok::<_, AppError>((
            code.unwrap_or(255),
            String::from_utf8_lossy(&out).to_string(),
        ))
    };
    tokio::time::timeout(EXEC_TIMEOUT, run)
        .await
        .map_err(|_| AppError::Connection(format!("remote command timed out: {command}")))?
}

/// Where kubeconfigs are commonly found. Commands are constants; nothing from the
/// user is interpolated.
const SOURCES: &[(&str, &str)] = &[
    ("kubectl config", "kubectl config view --raw 2>/dev/null"),
    ("~/.kube/config", "cat ~/.kube/config 2>/dev/null"),
    ("k3s", "cat /etc/rancher/k3s/k3s.yaml 2>/dev/null"),
    ("RKE2", "cat /etc/rancher/rke2/rke2.yaml 2>/dev/null"),
    ("kubeadm", "cat /etc/kubernetes/admin.conf 2>/dev/null"),
    ("MicroK8s", "microk8s config 2>/dev/null"),
];
const SUDO_SOURCES: &[(&str, &str)] = &[
    ("k3s (sudo)", "/etc/rancher/k3s/k3s.yaml"),
    ("RKE2 (sudo)", "/etc/rancher/rke2/rke2.yaml"),
    ("kubeadm (sudo)", "/etc/kubernetes/admin.conf"),
];

fn usable(text: &str) -> Option<Kubeconfig> {
    let kc = Kubeconfig::from_yaml(text).ok()?;
    let has_server = kc
        .clusters
        .iter()
        .any(|c| c.cluster.as_ref().and_then(|c| c.server.as_ref()).is_some());
    (has_server && !kc.contexts.is_empty()).then_some(kc)
}

/// Finds a kubeconfig on the server. Returns (source label, YAML).
pub async fn fetch_kubeconfig(
    handle: &Handle<Verifier>,
    sudo_password: Option<&str>,
) -> AppResult<(String, String)> {
    for (label, cmd) in SOURCES {
        if let Ok((0, out)) = exec(handle, cmd, None).await {
            if usable(&out).is_some() {
                return Ok((label.to_string(), out));
            }
        }
    }
    for (label, path) in SUDO_SOURCES {
        // passwordless sudo first, then with the provided password
        let mut attempts = vec![(format!("sudo -n cat {path} 2>/dev/null"), None)];
        if let Some(pw) = sudo_password.filter(|p| !p.is_empty()) {
            attempts.push((format!("sudo -S -p '' cat {path} 2>/dev/null"), Some(pw)));
        }
        for (cmd, stdin) in attempts {
            if let Ok((0, out)) = exec(handle, &cmd, stdin).await {
                if usable(&out).is_some() {
                    return Ok((label.to_string(), out));
                }
            }
        }
    }
    Err(AppError::NotFound(
        "no kubeconfig found on the server (checked kubectl, ~/.kube/config, k3s, RKE2, kubeadm, MicroK8s). \
         If it requires root, provide the user's password so Manifex can use sudo."
            .into(),
    ))
}

/// API server host/port of a kubeconfig context.
pub fn api_endpoint(kc: &Kubeconfig, context: &str) -> AppResult<(String, u16)> {
    let ctx = kc
        .contexts
        .iter()
        .find(|c| c.name == context)
        .and_then(|c| c.context.as_ref())
        .ok_or_else(|| AppError::NotFound(format!("context '{context}'")))?;
    let server = kc
        .clusters
        .iter()
        .find(|c| c.name == ctx.cluster)
        .and_then(|c| c.cluster.as_ref())
        .and_then(|c| c.server.clone())
        .ok_or_else(|| AppError::validation("cluster has no server address"))?;
    let uri: http::Uri = server
        .parse()
        .map_err(|_| AppError::validation("invalid API server address"))?;
    if uri.scheme_str() != Some("https") {
        return Err(AppError::Security(
            "plain HTTP API endpoints are not supported".into(),
        ));
    }
    let host = uri
        .host()
        .ok_or_else(|| AppError::validation("API server address has no host"))?;
    Ok((
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .to_string(),
        uri.port_u16().unwrap_or(443),
    ))
}

/// Rewrites a context's cluster to go through the local tunnel, keeping TLS
/// verification against the original API server name.
pub fn rewrite_for_tunnel(
    mut kc: Kubeconfig,
    context: &str,
    local_port: u16,
) -> AppResult<Kubeconfig> {
    let (orig_host, _) = api_endpoint(&kc, context)?;
    let cluster_name = kc
        .contexts
        .iter()
        .find(|c| c.name == context)
        .and_then(|c| c.context.as_ref())
        .map(|c| c.cluster.clone())
        .ok_or_else(|| AppError::NotFound(format!("context '{context}'")))?;
    let cluster = kc
        .clusters
        .iter_mut()
        .find(|c| c.name == cluster_name)
        .and_then(|c| c.cluster.as_mut())
        .ok_or_else(|| AppError::validation("cluster missing"))?;
    cluster.server = Some(format!("https://127.0.0.1:{local_port}"));
    let loopback = orig_host == "127.0.0.1" || orig_host == "::1";
    if cluster.tls_server_name.is_none() && !loopback {
        cluster.tls_server_name = Some(orig_host);
    }
    // The tunnel is local – any proxy configured for the original address no longer applies.
    cluster.proxy_url = None;
    Ok(kc)
}

/// A running local port forward through SSH.
pub struct SshTunnel {
    pub local_port: u16,
    task: AbortHandle,
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Starts forwarding 127.0.0.1:<random port> → `remote_host:remote_port` (as seen
/// from the SSH server). The session reconnects on demand.
pub async fn start_tunnel(
    target: SshTarget,
    creds: SshCredentials,
    fingerprint: String,
    initial: Handle<Verifier>,
    remote_host: String,
    remote_port: u16,
) -> AppResult<SshTunnel> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let local_port = listener.local_addr()?.port();
    let session: Arc<AsyncMutex<Arc<Handle<Verifier>>>> =
        Arc::new(AsyncMutex::new(Arc::new(initial)));
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, peer)) = listener.accept().await else {
                continue;
            };
            let _ = socket.set_nodelay(true);
            let session = session.clone();
            let (target, creds, fingerprint, remote_host) = (
                target.clone(),
                creds.clone(),
                fingerprint.clone(),
                remote_host.clone(),
            );
            tokio::spawn(async move {
                let open = |h: Arc<Handle<Verifier>>| {
                    let remote_host = remote_host.clone();
                    async move {
                        h.channel_open_direct_tcpip(
                            remote_host,
                            remote_port as u32,
                            "127.0.0.1",
                            peer.port() as u32,
                        )
                        .await
                    }
                };
                let handle = session.lock().await.clone();
                let channel = match open(handle.clone()).await {
                    Ok(c) => c,
                    Err(_) => {
                        // Session dropped – reconnect once, then retry.
                        let mut guard = session.lock().await;
                        if guard.is_closed() || Arc::ptr_eq(&guard, &handle) {
                            match connect(&target, &creds, &fingerprint).await {
                                Ok(h) => *guard = Arc::new(h),
                                Err(e) => {
                                    tracing::warn!(error = %e, "SSH tunnel reconnect failed");
                                    return;
                                }
                            }
                        }
                        let h = guard.clone();
                        drop(guard);
                        match open(h).await {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::warn!(error = %e, "SSH tunnel channel failed");
                                return;
                            }
                        }
                    }
                };
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
            });
        }
    });
    Ok(SshTunnel {
        local_port,
        task: task.abort_handle(),
    })
}

/// Everything needed to run a cluster through SSH: the tunnel (kept alive by the
/// connection) and the rewritten kubeconfig.
pub struct SshSession {
    pub tunnel: SshTunnel,
    pub kubeconfig: Kubeconfig,
    pub context: String,
    pub fetched_kubeconfig: Option<String>,
    pub source: String,
}

/// Logs in, refreshes the kubeconfig from the server (falling back to the last
/// stored one) and opens the tunnel for `context` (or the current context).
pub async fn establish(
    target: &SshTarget,
    creds: &SshCredentials,
    fingerprint: &str,
    context: Option<&str>,
) -> AppResult<SshSession> {
    let handle = connect(target, creds, fingerprint).await?;
    let (source, yaml, fresh) = match fetch_kubeconfig(&handle, creds.password.as_deref()).await {
        Ok((source, yaml)) => (source, yaml, true),
        Err(e) => match creds.kubeconfig.clone() {
            Some(stored) => ("stored copy".to_string(), stored, false),
            None => return Err(e),
        },
    };
    let kc = Kubeconfig::from_yaml(&yaml)?;
    let context = match context.filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None => kc
            .current_context
            .clone()
            .or_else(|| kc.contexts.first().map(|c| c.name.clone()))
            .ok_or_else(|| AppError::validation("the server's kubeconfig has no contexts"))?,
    };
    let (remote_host, remote_port) = api_endpoint(&kc, &context)?;
    let tunnel = start_tunnel(
        target.clone(),
        creds.clone(),
        fingerprint.to_string(),
        handle,
        remote_host,
        remote_port,
    )
    .await?;
    let kubeconfig = rewrite_for_tunnel(kc, &context, tunnel.local_port)?;
    Ok(SshSession {
        tunnel,
        kubeconfig,
        context,
        fetched_kubeconfig: fresh.then_some(yaml),
        source,
    })
}

/// Reads a private key file and an adjacent `<file>-cert.pub` certificate.
pub fn read_key_file(path: &std::path::Path) -> AppResult<(String, Option<String>)> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > 64 * 1024 {
        return Err(AppError::validation(
            "this does not look like an SSH private key (file too large)",
        ));
    }
    let key = std::fs::read_to_string(path)?;
    describe_key(&key)?;
    let mut cert_path = path.as_os_str().to_owned();
    cert_path.push("-cert.pub");
    let cert = std::fs::read_to_string(std::path::PathBuf::from(cert_path))
        .ok()
        .filter(|c| c.contains("-cert-v01@openssh.com"));
    Ok((key, cert))
}

/// Default keys in ~/.ssh (like the OpenSSH client).
pub fn default_keys() -> Vec<std::path::PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    ["id_ed25519", "id_ecdsa", "id_rsa"]
        .iter()
        .map(|n| home.join(".ssh").join(n))
        .filter(|p| p.is_file())
        .collect()
}

pub fn validate_fingerprint(fp: &str) -> AppResult<()> {
    if !fp.starts_with("SHA256:")
        || fp.len() > 100
        || fp.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AppError::validation("invalid host key fingerprint"));
    }
    Ok(())
}

pub fn validate_context(ctx: &str) -> AppResult<()> {
    validate::display_name("context", ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KC: &str = r#"
apiVersion: v1
kind: Config
current-context: default
clusters:
- name: default
  cluster:
    server: https://10.0.0.5:6443
    certificate-authority-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCk1JSUIKLS0tLS1FTkQgQ0VSVElGSUNBVEUtLS0tLQo=
- name: local
  cluster:
    server: https://127.0.0.1:6443
users:
- name: u
  user:
    token: abc
contexts:
- name: default
  context: { cluster: default, user: u }
- name: local
  context: { cluster: local, user: u }
"#;

    #[test]
    fn rewrites_server_and_keeps_tls_name() {
        let kc = Kubeconfig::from_yaml(KC).unwrap();
        assert_eq!(
            api_endpoint(&kc, "default").unwrap(),
            ("10.0.0.5".into(), 6443)
        );
        let r = rewrite_for_tunnel(kc.clone(), "default", 40000).unwrap();
        let c = r
            .clusters
            .iter()
            .find(|c| c.name == "default")
            .unwrap()
            .cluster
            .clone()
            .unwrap();
        assert_eq!(c.server.as_deref(), Some("https://127.0.0.1:40000"));
        assert_eq!(c.tls_server_name.as_deref(), Some("10.0.0.5"));
        let r = rewrite_for_tunnel(kc, "local", 40001).unwrap();
        let c = r
            .clusters
            .iter()
            .find(|c| c.name == "local")
            .unwrap()
            .cluster
            .clone()
            .unwrap();
        assert_eq!(
            c.tls_server_name, None,
            "loopback servers keep their own SAN"
        );
    }

    #[test]
    fn detects_usable_kubeconfigs() {
        assert!(usable(KC).is_some());
        assert!(usable("apiVersion: v1\nkind: Config\nclusters: []\ncontexts: []\n").is_none());
        assert!(usable("not yaml: [").is_none());
    }

    #[test]
    fn validates_targets() {
        let ok = SshTarget {
            host: "srv-01.example.com".into(),
            port: 22,
            user: "deploy".into(),
        };
        assert!(ok.validate().is_ok());
        let bad = SshTarget {
            host: "srv; rm -rf /".into(),
            port: 22,
            user: "deploy".into(),
        };
        assert!(bad.validate().is_err());
        let bad_user = SshTarget {
            host: "srv".into(),
            port: 22,
            user: "a b".into(),
        };
        assert!(bad_user.validate().is_err());
        assert!(validate_fingerprint("SHA256:abc").is_ok());
        assert!(validate_fingerprint("MD5:aa").is_err());
    }
}
