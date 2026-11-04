//! Cluster connection management: connect / test / disconnect, health probing and
//! automatic reconnection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use k8s_openapi::api::authentication::v1::SelfSubjectReview;
use kube::api::PostParams;
use kube::discovery::Discovery;
use kube::{Api, Client};
use parking_lot::RwLock;
use serde::Serialize;
use tokio::task::AbortHandle;

use super::kubeconfig;
use super::EventSink;
use crate::error::{AppError, AppResult};
use crate::persistence::clusters::{ClusterProfile, ClusterSource};
use crate::persistence::Db;
use crate::security::vault::SecretVault;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConnState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterStatus {
    pub id: String,
    pub name: String,
    pub state: ConnState,
    pub message: Option<String>,
    pub server: Option<String>,
    pub version: Option<String>,
    pub platform: Option<String>,
    pub identity: Option<String>,
    pub latency_ms: Option<u64>,
    pub since: String,
}

impl ClusterStatus {
    fn new(profile: &ClusterProfile, state: ConnState, message: Option<String>) -> Self {
        Self {
            id: profile.id.clone(),
            name: profile.name.clone(),
            state,
            message,
            server: None,
            version: None,
            platform: None,
            identity: None,
            latency_ms: None,
            since: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub version: Option<String>,
    pub platform: Option<String>,
    pub identity: Option<String>,
    pub latency_ms: u64,
    pub message: Option<String>,
}

pub struct ClusterConn {
    pub id: String,
    pub profile: RwLock<ClusterProfile>,
    pub client: Client,
    pub default_namespace: String,
    status: RwLock<ClusterStatus>,
    discovery: tokio::sync::RwLock<Option<(Arc<Discovery>, Instant)>>,
    pub access_cache: DashMap<String, (bool, Instant)>,
    health_task: RwLock<Option<AbortHandle>>,
    /// SSH tunnel + rewritten kubeconfig for "via SSH" profiles (kept alive for
    /// the lifetime of the connection).
    pub ssh: Option<SshRuntime>,
}

pub struct SshRuntime {
    pub tunnel: super::ssh::SshTunnel,
    /// kubeconfig pointing at the local tunnel (used for the Helm CLI).
    pub kubeconfig_yaml: String,
    pub context: String,
    pub source: String,
}

/// A built client plus, for SSH profiles, the session that carries it.
pub struct BuiltClient {
    pub client: Client,
    pub config: kube::Config,
    pub ssh: Option<SshRuntime>,
    /// Freshly fetched kubeconfig to persist into the vault.
    pub refreshed_secret: Option<Vec<u8>>,
}

impl ClusterConn {
    pub fn status(&self) -> ClusterStatus {
        self.status.read().clone()
    }

    pub fn name(&self) -> String {
        self.profile.read().name.clone()
    }

    pub fn identity(&self) -> Option<String> {
        self.status.read().identity.clone()
    }

    /// API discovery, cached for five minutes (or until explicitly refreshed).
    pub async fn discovery(&self, refresh: bool) -> AppResult<Arc<Discovery>> {
        if !refresh {
            if let Some((d, at)) = self.discovery.read().await.as_ref() {
                if at.elapsed() < Duration::from_secs(300) {
                    return Ok(d.clone());
                }
            }
        }
        let mut guard = self.discovery.write().await;
        let client = self.client.clone();
        let discovery = match tokio::time::timeout(
            Duration::from_secs(30),
            Discovery::new(client.clone()).run_aggregated(),
        )
        .await
        {
            Ok(Ok(d)) => d,
            _ => tokio::time::timeout(Duration::from_secs(60), Discovery::new(client).run())
                .await
                .map_err(|_| AppError::Connection("API discovery timed out".into()))??,
        };
        let d = Arc::new(discovery);
        *guard = Some((d.clone(), Instant::now()));
        Ok(d)
    }
}

impl Drop for ClusterConn {
    fn drop(&mut self) {
        if let Some(h) = self.health_task.write().take() {
            h.abort();
        }
    }
}

pub struct ClusterManager {
    db: Arc<Db>,
    vault: Arc<SecretVault>,
    sink: Arc<dyn EventSink>,
    conns: RwLock<HashMap<String, Arc<ClusterConn>>>,
    statuses: RwLock<HashMap<String, ClusterStatus>>,
}

async fn whoami(client: &Client) -> Option<String> {
    let api: Api<SelfSubjectReview> = Api::all(client.clone());
    let review = SelfSubjectReview::default();
    match tokio::time::timeout(
        Duration::from_secs(5),
        api.create(&PostParams::default(), &review),
    )
    .await
    {
        Ok(Ok(r)) => r.status.and_then(|s| s.user_info).and_then(|u| u.username),
        _ => None,
    }
}

impl ClusterManager {
    pub fn new(db: Arc<Db>, vault: Arc<SecretVault>, sink: Arc<dyn EventSink>) -> Arc<Self> {
        Arc::new(Self {
            db,
            vault,
            sink,
            conns: RwLock::new(HashMap::new()),
            statuses: RwLock::new(HashMap::new()),
        })
    }

    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    pub fn vault(&self) -> &Arc<SecretVault> {
        &self.vault
    }

    fn set_status(&self, status: ClusterStatus) {
        self.statuses
            .write()
            .insert(status.id.clone(), status.clone());
        self.sink.cluster_status(&status);
    }

    pub fn statuses(&self) -> Vec<ClusterStatus> {
        self.statuses.read().values().cloned().collect()
    }

    pub fn status(&self, id: &str) -> Option<ClusterStatus> {
        self.statuses.read().get(id).cloned()
    }

    pub fn connected_ids(&self) -> Vec<String> {
        self.conns.read().keys().cloned().collect()
    }

    pub fn get(&self, id: &str) -> AppResult<Arc<ClusterConn>> {
        self.conns
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::NotConnected(id.to_string()))
    }

    pub fn client(&self, id: &str) -> AppResult<Client> {
        Ok(self.get(id)?.client.clone())
    }

    pub fn load_secret(&self, source: &ClusterSource) -> AppResult<Option<Vec<u8>>> {
        match source.secret_id() {
            Some(sid) => Ok(Some(self.db.get_secret(&self.vault, sid)?)),
            None => Ok(None),
        }
    }

    pub async fn build_client(&self, profile: &ClusterProfile) -> AppResult<BuiltClient> {
        let secret = self.load_secret(&profile.source)?;
        build_from_source(&profile.source, secret).await
    }

    /// Tests a stored profile without registering a connection.
    pub async fn test(&self, profile: &ClusterProfile) -> AppResult<ConnectionTestResult> {
        let secret = match self.load_secret(&profile.source) {
            Ok(s) => s,
            Err(e) => {
                return Ok(ConnectionTestResult {
                    ok: false,
                    version: None,
                    platform: None,
                    identity: None,
                    latency_ms: 0,
                    message: Some(e.to_string()),
                })
            }
        };
        self.test_with_secret(profile, secret).await
    }

    /// Tests connection parameters with explicit secret material (used before a
    /// new cluster is saved, so nothing is persisted for failed attempts).
    pub async fn test_with_secret(
        &self,
        profile: &ClusterProfile,
        secret: Option<Vec<u8>>,
    ) -> AppResult<ConnectionTestResult> {
        let start = Instant::now();
        let built = build_from_source(&profile.source, secret).await;
        // keep the SSH tunnel (if any) alive until the test finished
        let (client, _ssh) = match built.map(|b| (b.client, b.ssh)) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ConnectionTestResult {
                    ok: false,
                    version: None,
                    platform: None,
                    identity: None,
                    latency_ms: start.elapsed().as_millis() as u64,
                    message: Some(e.to_string()),
                })
            }
        };
        match tokio::time::timeout(Duration::from_secs(15), client.apiserver_version()).await {
            Ok(Ok(info)) => Ok(ConnectionTestResult {
                ok: true,
                version: Some(info.git_version),
                platform: Some(info.platform),
                identity: whoami(&client).await,
                latency_ms: start.elapsed().as_millis() as u64,
                message: None,
            }),
            Ok(Err(e)) => Ok(ConnectionTestResult {
                ok: false,
                version: None,
                platform: None,
                identity: None,
                latency_ms: start.elapsed().as_millis() as u64,
                message: Some(AppError::from(e).to_string()),
            }),
            Err(_) => Ok(ConnectionTestResult {
                ok: false,
                version: None,
                platform: None,
                identity: None,
                latency_ms: start.elapsed().as_millis() as u64,
                message: Some("timed out after 15s".into()),
            }),
        }
    }

    pub async fn connect(self: &Arc<Self>, id: &str) -> AppResult<ClusterStatus> {
        if let Ok(conn) = self.get(id) {
            return Ok(conn.status());
        }
        let profile = self.db.get_cluster(id)?;
        self.set_status(ClusterStatus::new(&profile, ConnState::Connecting, None));

        let result: AppResult<(
            BuiltClient,
            k8s_openapi::apimachinery::pkg::version::Info,
            u64,
        )> = async {
            let built = self.build_client(&profile).await?;
            let start = Instant::now();
            let info =
                tokio::time::timeout(Duration::from_secs(15), built.client.apiserver_version())
                    .await
                    .map_err(|_| {
                        AppError::Connection("API server did not respond within 15s".into())
                    })??;
            Ok((built, info, start.elapsed().as_millis() as u64))
        }
        .await;

        let (built, info, latency) = match result {
            Ok(v) => v,
            Err(e) => {
                self.set_status(ClusterStatus::new(
                    &profile,
                    ConnState::Error,
                    Some(e.to_string()),
                ));
                return Err(e);
            }
        };

        let BuiltClient {
            client,
            config,
            ssh,
            refreshed_secret,
        } = built;
        if let (Some(bytes), Some(sid)) = (refreshed_secret, profile.source.secret_id()) {
            if let Err(e) = self.db.put_secret(&self.vault, sid, &bytes) {
                tracing::warn!(error = %e, "could not store refreshed kubeconfig");
            }
        }
        let identity = whoami(&client).await;
        let mut status = ClusterStatus::new(&profile, ConnState::Connected, None);
        status.server = Some(match &profile.source {
            ClusterSource::Ssh {
                host, port, user, ..
            } => {
                let port = if *port == 22 {
                    String::new()
                } else {
                    format!(":{port}")
                };
                format!("ssh://{user}@{host}{port}")
            }
            _ => config.cluster_url.to_string(),
        });
        status.version = Some(info.git_version.clone());
        status.platform = Some(info.platform.clone());
        status.identity = identity;
        status.latency_ms = Some(latency);

        let default_namespace = profile
            .default_namespace
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| config.default_namespace.clone());
        let conn = Arc::new(ClusterConn {
            id: profile.id.clone(),
            profile: RwLock::new(profile.clone()),
            client,
            default_namespace,
            status: RwLock::new(status.clone()),
            discovery: tokio::sync::RwLock::new(None),
            access_cache: DashMap::new(),
            health_task: RwLock::new(None),
            ssh,
        });

        let handle = tokio::spawn(Self::health_loop(
            Arc::downgrade(self),
            Arc::downgrade(&conn),
        ));
        *conn.health_task.write() = Some(handle.abort_handle());
        self.conns.write().insert(profile.id.clone(), conn);
        let _ = self.db.touch_cluster_connected(&profile.id);
        self.set_status(status.clone());
        tracing::info!(cluster = %profile.name, version = %info.git_version, "connected");
        Ok(status)
    }

    pub fn disconnect(&self, id: &str) {
        let removed = self.conns.write().remove(id);
        if removed.is_some() {
            if let Ok(p) = self.db.get_cluster(id) {
                self.set_status(ClusterStatus::new(&p, ConnState::Disconnected, None));
            } else {
                self.statuses.write().remove(id);
            }
        }
    }

    pub fn forget(&self, id: &str) {
        self.conns.write().remove(id);
        self.statuses.write().remove(id);
    }

    pub fn refresh_profile(&self, profile: &ClusterProfile) {
        if let Ok(conn) = self.get(&profile.id) {
            *conn.profile.write() = profile.clone();
            conn.status.write().name = profile.name.clone();
        }
        if let Some(s) = self.statuses.write().get_mut(&profile.id) {
            s.name = profile.name.clone();
        }
    }

    async fn health_loop(mgr: std::sync::Weak<Self>, conn: std::sync::Weak<ClusterConn>) {
        let mut failures: u32 = 0;
        loop {
            let (interval, notify_loss, auto_reconnect) = match mgr.upgrade() {
                Some(m) => {
                    let s = m.db.load_settings().unwrap_or_default();
                    (
                        s.health_interval_secs,
                        s.notify_connection_loss,
                        s.auto_reconnect,
                    )
                }
                None => return,
            };
            let wait = if failures == 0 {
                Duration::from_secs(interval)
            } else if auto_reconnect {
                // exponential backoff: 2, 4, 8 … max 60 seconds
                Duration::from_secs((2u64 << failures.min(5)).min(60))
            } else {
                Duration::from_secs(interval)
            };
            tokio::time::sleep(wait).await;

            let (Some(m), Some(c)) = (mgr.upgrade(), conn.upgrade()) else {
                return;
            };
            let start = Instant::now();
            let probe =
                tokio::time::timeout(Duration::from_secs(8), c.client.apiserver_version()).await;
            let ok = matches!(probe, Ok(Ok(_)));
            let prev = c.status.read().state;
            if ok {
                let latency = start.elapsed().as_millis() as u64;
                let mut st = c.status.write();
                st.latency_ms = Some(latency);
                if prev != ConnState::Connected {
                    st.state = ConnState::Connected;
                    st.message = None;
                    st.since = chrono::Utc::now().to_rfc3339();
                    let snapshot = st.clone();
                    drop(st);
                    failures = 0;
                    m.set_status(snapshot.clone());
                    if notify_loss {
                        m.sink.notify(
                            "info",
                            &format!("{} reconnected", snapshot.name),
                            "The connection to the cluster has been restored.",
                            Some(&snapshot.id),
                        );
                    }
                } else {
                    drop(st);
                    failures = 0;
                }
            } else {
                failures = failures.saturating_add(1);
                let reason = match probe {
                    Ok(Err(e)) => AppError::from(e).to_string(),
                    _ => "health probe timed out".into(),
                };
                if prev == ConnState::Connected {
                    let snapshot = {
                        let mut st = c.status.write();
                        st.state = ConnState::Reconnecting;
                        st.message = Some(reason.clone());
                        st.since = chrono::Utc::now().to_rfc3339();
                        st.clone()
                    };
                    m.set_status(snapshot.clone());
                    if notify_loss {
                        m.sink.notify(
                            "critical",
                            &format!("Lost connection to {}", snapshot.name),
                            &reason,
                            Some(&snapshot.id),
                        );
                    }
                } else {
                    c.status.write().message = Some(reason);
                }
            }
        }
    }
}

/// Builds a Kubernetes client for any cluster source. SSH sources establish an
/// SSH session, refresh the kubeconfig from the server and tunnel the API.
pub async fn build_from_source(
    source: &ClusterSource,
    secret: Option<Vec<u8>>,
) -> AppResult<BuiltClient> {
    if let ClusterSource::Ssh {
        host,
        port,
        user,
        host_key_fingerprint,
        context,
        ..
    } = source
    {
        let bytes = secret
            .ok_or_else(|| AppError::Vault("SSH credentials missing for this cluster".into()))?;
        let mut creds: super::ssh::SshCredentials = serde_json::from_slice(&bytes)
            .map_err(|_| AppError::Vault("SSH credentials are corrupt".into()))?;
        let target = super::ssh::SshTarget {
            host: host.clone(),
            port: *port,
            user: user.clone(),
        };
        let session =
            super::ssh::establish(&target, &creds, host_key_fingerprint, context.as_deref())
                .await?;
        let refreshed_secret = match &session.fetched_kubeconfig {
            Some(fresh) if creds.kubeconfig.as_deref() != Some(fresh.as_str()) => {
                creds.kubeconfig = Some(fresh.clone());
                Some(serde_json::to_vec(&creds)?)
            }
            _ => None,
        };
        let kubeconfig_yaml = kubeconfig::kubeconfig_to_yaml_with_secrets(&session.kubeconfig)?;
        let config =
            kubeconfig::build_config_from(session.kubeconfig.clone(), session.context.clone())
                .await?;
        let client = Client::try_from(config.clone())?;
        return Ok(BuiltClient {
            client,
            config,
            ssh: Some(SshRuntime {
                tunnel: session.tunnel,
                kubeconfig_yaml,
                context: session.context,
                source: session.source,
            }),
            refreshed_secret,
        });
    }
    let config = kubeconfig::build_config(source, secret).await?;
    let client = Client::try_from(config.clone())?;
    Ok(BuiltClient {
        client,
        config,
        ssh: None,
        refreshed_secret: None,
    })
}
