//! manifex-agent – exposes the Kubernetes API of a (private) cluster to Manifex.
//!
//! The agent runs inside the cluster and terminates TLS on its own endpoint.
//! Clients authenticate with a bearer token (compared in constant time). Accepted
//! requests are forwarded to the in-cluster API server using the agent's
//! ServiceAccount token, optionally impersonating a configured user/groups so the
//! effective permissions can be scoped with ordinary RBAC. Protocol upgrades
//! (exec / attach / port-forward WebSockets) are proxied transparently.
//!
//! Client-supplied `Authorization` and `Impersonate-*` headers are always stripped,
//! so a client can never escalate beyond the agent's configured identity.
//!
//! Configuration (environment):
//!   MANIFEX_AGENT_LISTEN            default 0.0.0.0:8443
//!   MANIFEX_AGENT_TLS_CERT / _KEY   default /etc/manifex-agent/tls/tls.crt|tls.key
//!   MANIFEX_AGENT_TOKEN_FILE        default /etc/manifex-agent/auth/token
//!   MANIFEX_AGENT_IMPERSONATE_USER  optional
//!   MANIFEX_AGENT_IMPERSONATE_GROUPS optional, comma separated
//!   MANIFEX_AGENT_UPSTREAM          default https://$KUBERNETES_SERVICE_HOST:$KUBERNETES_SERVICE_PORT
//!   MANIFEX_AGENT_UPSTREAM_CA       default /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
//!   MANIFEX_AGENT_UPSTREAM_TOKEN_FILE default /var/run/secrets/kubernetes.io/serviceaccount/token

use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue, AUTHORIZATION, CONNECTION, HOST, UPGRADE};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

type Body = BoxBody<Bytes, hyper::Error>;
type Upstream = Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    Incoming,
>;

struct Config {
    listen: SocketAddr,
    tls_cert: String,
    tls_key: String,
    token_file: String,
    impersonate_user: Option<String>,
    impersonate_groups: Vec<String>,
    upstream: Uri,
    upstream_ca: String,
    upstream_token_file: String,
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let default_upstream = match (
            std::env::var("KUBERNETES_SERVICE_HOST"),
            std::env::var("KUBERNETES_SERVICE_PORT"),
        ) {
            (Ok(h), Ok(p)) if h.contains(':') => format!("https://[{h}]:{p}"),
            (Ok(h), Ok(p)) => format!("https://{h}:{p}"),
            _ => "https://kubernetes.default.svc".into(),
        };
        let upstream: Uri = env("MANIFEX_AGENT_UPSTREAM", &default_upstream)
            .parse()
            .map_err(|e| format!("invalid upstream: {e}"))?;
        if upstream.scheme_str() != Some("https") {
            return Err("upstream must use https".into());
        }
        Ok(Self {
            listen: env("MANIFEX_AGENT_LISTEN", "0.0.0.0:8443")
                .parse()
                .map_err(|e| format!("invalid listen address: {e}"))?,
            tls_cert: env("MANIFEX_AGENT_TLS_CERT", "/etc/manifex-agent/tls/tls.crt"),
            tls_key: env("MANIFEX_AGENT_TLS_KEY", "/etc/manifex-agent/tls/tls.key"),
            token_file: env("MANIFEX_AGENT_TOKEN_FILE", "/etc/manifex-agent/auth/token"),
            impersonate_user: std::env::var("MANIFEX_AGENT_IMPERSONATE_USER")
                .ok()
                .filter(|s| !s.is_empty()),
            impersonate_groups: std::env::var("MANIFEX_AGENT_IMPERSONATE_GROUPS")
                .map(|g| {
                    g.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
            upstream,
            upstream_ca: env(
                "MANIFEX_AGENT_UPSTREAM_CA",
                "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt",
            ),
            upstream_token_file: env(
                "MANIFEX_AGENT_UPSTREAM_TOKEN_FILE",
                "/var/run/secrets/kubernetes.io/serviceaccount/token",
            ),
        })
    }
}

struct State {
    cfg: Config,
    client: Upstream,
}

fn load_certs(path: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    rustls_pemfile::certs(&mut BufReader::new(&data[..]))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{path}: {e}"))
}

fn tls_acceptor(cfg: &Config) -> Result<TlsAcceptor, String> {
    let certs = load_certs(&cfg.tls_cert)?;
    let key_data = std::fs::read(&cfg.tls_key).map_err(|e| format!("{}: {e}", cfg.tls_key))?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(&key_data[..]))
        .map_err(|e| e.to_string())?
        .ok_or("no private key found")?;
    let mut sc = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|e| e.to_string())?;
    sc.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(sc)))
}

fn upstream_client(cfg: &Config) -> Result<Upstream, String> {
    let mut roots = rustls::RootCertStore::empty();
    for c in load_certs(&cfg.upstream_ca)? {
        roots.add(c).map_err(|e| e.to_string())?;
    }
    let cc = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(cc)
        .https_only()
        .enable_http1()
        .build();
    Ok(Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(60))
        .build(https))
}

fn text(status: StatusCode, msg: &'static str) -> Response<Body> {
    let body = format!("{{\"kind\":\"Status\",\"apiVersion\":\"v1\",\"status\":\"Failure\",\"message\":\"{msg}\",\"code\":{}}}", status.as_u16());
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(
            Full::new(Bytes::from(body))
                .map_err(|never| match never {})
                .boxed(),
        )
        .expect("valid response")
}

/// Reads the expected token on every request so rotation needs no restart.
fn authorized(cfg: &Config, req: &Request<Incoming>) -> bool {
    let Ok(expected) = std::fs::read_to_string(&cfg.token_file) else {
        return false;
    };
    let expected = expected.trim();
    if expected.len() < 16 {
        return false; // refuse weak / missing tokens
    }
    let Some(given) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    given.as_bytes().ct_eq(expected.as_bytes()).into()
}

const HOP_BY_HOP: &[&str] = &[
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
];

fn is_upgrade(req: &Request<Incoming>) -> bool {
    req.headers()
        .get(CONNECTION)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
        })
        .unwrap_or(false)
        && req.headers().contains_key(UPGRADE)
}

async fn handle(
    state: Arc<State>,
    mut req: Request<Incoming>,
    peer: SocketAddr,
) -> Result<Response<Body>, hyper::Error> {
    if req.uri().path() == "/healthz" {
        return Ok(text(StatusCode::OK, "ok"));
    }
    if !authorized(&state.cfg, &req) {
        eprintln!("{peer} {} {} -> 401", req.method(), req.uri().path());
        return Ok(text(StatusCode::UNAUTHORIZED, "Unauthorized"));
    }
    let Ok(sa_token) = std::fs::read_to_string(&state.cfg.upstream_token_file) else {
        return Ok(text(
            StatusCode::BAD_GATEWAY,
            "agent service account token unavailable",
        ));
    };
    let upgrade = is_upgrade(&req);

    // Build the upstream URI.
    let pq = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let base = state.cfg.upstream.to_string();
    let uri: Uri = match format!("{}{}", base.trim_end_matches('/'), pq).parse() {
        Ok(u) => u,
        Err(_) => return Ok(text(StatusCode::BAD_REQUEST, "invalid request URI")),
    };

    // Sanitise headers: drop credentials, impersonation and hop-by-hop headers.
    let headers = req.headers_mut();
    headers.remove(AUTHORIZATION);
    headers.remove(HOST);
    let to_remove: Vec<HeaderName> = headers
        .keys()
        .filter(|k| {
            let n = k.as_str();
            n.starts_with("impersonate-")
                || HOP_BY_HOP.contains(&n)
                || (!upgrade && (n == "connection" || n == "upgrade"))
        })
        .cloned()
        .collect();
    for k in to_remove {
        headers.remove(k);
    }
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", sa_token.trim())).expect("token is ascii"),
    );
    if let Some(u) = &state.cfg.impersonate_user {
        if let Ok(v) = HeaderValue::from_str(u) {
            headers.insert("impersonate-user", v);
        }
        for g in &state.cfg.impersonate_groups {
            if let Ok(v) = HeaderValue::from_str(g) {
                headers.append("impersonate-group", v);
            }
        }
    }
    *req.uri_mut() = uri;

    if upgrade {
        let client_upgrade = hyper::upgrade::on(&mut req);
        let (parts, body) = req.into_parts();
        let up_req = Request::from_parts(parts, body);
        let mut resp = match state.client.request(up_req).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{peer} upstream error: {e}");
                return Ok(text(StatusCode::BAD_GATEWAY, "upstream unavailable"));
            }
        };
        if resp.status() == StatusCode::SWITCHING_PROTOCOLS {
            let upstream_upgrade = hyper::upgrade::on(&mut resp);
            tokio::spawn(async move {
                match tokio::try_join!(client_upgrade, upstream_upgrade) {
                    Ok((c, u)) => {
                        let mut c = TokioIo::new(c);
                        let mut u = TokioIo::new(u);
                        let _ = tokio::io::copy_bidirectional(&mut c, &mut u).await;
                    }
                    Err(e) => eprintln!("upgrade failed: {e}"),
                }
            });
            let (parts, _) = resp.into_parts();
            return Ok(Response::from_parts(
                parts,
                Empty::new().map_err(|never| match never {}).boxed(),
            ));
        }
        let (parts, body) = resp.into_parts();
        return Ok(Response::from_parts(parts, body.boxed()));
    }

    match state.client.request(req).await {
        Ok(resp) => {
            let (parts, body) = resp.into_parts();
            Ok(Response::from_parts(parts, body.boxed()))
        }
        Err(e) => {
            eprintln!("{peer} upstream error: {e}");
            Ok(text(StatusCode::BAD_GATEWAY, "upstream unavailable"))
        }
    }
}

#[tokio::main]
async fn main() {
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };
    let acceptor = tls_acceptor(&cfg).unwrap_or_else(|e| {
        eprintln!("TLS configuration error: {e}");
        std::process::exit(2);
    });
    let client = upstream_client(&cfg).unwrap_or_else(|e| {
        eprintln!("upstream configuration error: {e}");
        std::process::exit(2);
    });
    let listener = TcpListener::bind(cfg.listen).await.unwrap_or_else(|e| {
        eprintln!("cannot listen on {}: {e}", cfg.listen);
        std::process::exit(2);
    });
    eprintln!(
        "manifex-agent listening on {} → {}",
        cfg.listen, cfg.upstream
    );
    let state = Arc::new(State { cfg, client });
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((tcp, peer)) = accepted else { continue };
                let acceptor = acceptor.clone();
                let state = state.clone();
                tokio::spawn(async move {
                    let Ok(tls) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(tcp)).await.map_err(|_| ()).and_then(|r| r.map_err(|_| ())) else {
                        return;
                    };
                    let svc = service_fn(move |req| handle(state.clone(), req, peer));
                    let conn = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(tls), svc).with_upgrades();
                    if let Err(e) = conn.await {
                        eprintln!("{peer} connection error: {e}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("shutting down");
                return;
            }
        }
    }
}
