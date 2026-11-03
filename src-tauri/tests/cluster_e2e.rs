//! End-to-end tests against a real Kubernetes cluster.
//!
//! These tests are skipped unless `MANIFEX_E2E_KUBECONFIG` points to a kubeconfig
//! (and optionally `MANIFEX_E2E_CONTEXT` names the context). They create and delete
//! a temporary namespace. CI runs them against a k3s cluster.
//!
//!   MANIFEX_E2E_KUBECONFIG=~/.kube/config cargo test --test cluster_e2e -- --nocapture

use std::sync::Arc;
use std::time::Duration;

use manifex_lib::k8s::connection::{ClusterManager, ConnState};
use manifex_lib::k8s::exec::{ExecEvent, ExecManager, ExecTarget};
use manifex_lib::k8s::logs::{LogManager, LogMessage, LogOptions};
use manifex_lib::k8s::ops::{self, ApplyMode, DeleteOptions};
use manifex_lib::k8s::resources::builtin;
use manifex_lib::k8s::summary::Health;
use manifex_lib::k8s::watch::{WatchKey, WatchManager, WatchMessage};
use manifex_lib::k8s::{kubeconfig, NullSink};
use manifex_lib::persistence::clusters::{ClusterProfile, ClusterSource};
use manifex_lib::persistence::Db;
use manifex_lib::security::vault::SecretVault;
use parking_lot::Mutex;

fn e2e_config() -> Option<(String, String)> {
    let path = std::env::var("MANIFEX_E2E_KUBECONFIG").ok()?;
    let kc = kubeconfig::read_kubeconfig(std::path::Path::new(&path)).expect("readable kubeconfig");
    let ctx = std::env::var("MANIFEX_E2E_CONTEXT").ok().or(kc.current_context.clone()).expect("context");
    Some((path, ctx))
}

async fn wait_until<F: FnMut() -> bool>(timeout: Duration, mut f: F) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    false
}

const MANIFEST: &str = r#"
apiVersion: v1
kind: Namespace
metadata:
  name: NS
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: settings
  namespace: NS
data:
  greeting: hello
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: echo
  namespace: NS
spec:
  replicas: 1
  selector:
    matchLabels: { app: echo }
  template:
    metadata:
      labels: { app: echo }
    spec:
      terminationGracePeriodSeconds: 1
      containers:
      - name: main
        image: busybox:1.36
        command: ["sh", "-c", "i=0; while true; do echo tick $i; i=$((i+1)); sleep 1; done"]
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_cluster_workflow() {
    let Some((path, context)) = e2e_config() else {
        eprintln!("MANIFEX_E2E_KUBECONFIG not set – skipping");
        return;
    };
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let ns = format!("manifex-e2e-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    // --- discovery of kubeconfig contexts
    let discovered = kubeconfig::inspect_path(std::path::Path::new(&path));
    assert!(discovered.error.is_none(), "{:?}", discovered.error);
    assert!(discovered.contexts.iter().any(|c| c.name == context));

    // --- connection
    let db = Arc::new(Db::open_in_memory().unwrap());
    let vault = Arc::new(SecretVault::with_key([9u8; 32]));
    let mgr = ClusterManager::new(db.clone(), vault, Arc::new(NullSink));
    let profile = ClusterProfile {
        id: "e2e".into(),
        name: "e2e".into(),
        source: ClusterSource::Kubeconfig { path: path.clone(), context: context.clone() },
        group: None,
        favorite: false,
        color: None,
        environment: "development".into(),
        default_namespace: None,
        alerts_enabled: false,
        sort_order: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        last_connected_at: None,
    };
    db.insert_cluster(&profile).unwrap();
    let test = mgr.test(&profile).await.unwrap();
    assert!(test.ok, "connection test failed: {:?}", test.message);
    let status = mgr.connect("e2e").await.expect("connect");
    assert_eq!(status.state, ConnState::Connected);
    println!("connected to {} as {:?}", status.version.unwrap_or_default(), status.identity);
    let conn = mgr.get("e2e").unwrap();
    let client = conn.client.clone();

    // --- discovery
    let disco = conn.discovery(false).await.unwrap();
    let resources = manifex_lib::k8s::resources::list_api_resources(&disco);
    assert!(resources.iter().any(|r| r.kind == "Deployment" && r.group == "apps"));

    // --- server-side apply (dry run, then real)
    let manifest = MANIFEST.replace("NS", &ns);
    let dry = ops::apply_manifests(&conn, &manifest, None, ApplyMode::Apply, true, false).await.unwrap();
    assert_eq!(dry.len(), 3);
    assert!(dry.iter().all(|r| r.error.is_none() || r.kind != "Namespace"), "{dry:?}");
    let applied = ops::apply_manifests(&conn, &manifest, None, ApplyMode::Apply, false, false).await.unwrap();
    assert!(applied.iter().all(|r| r.error.is_none()), "{applied:?}");
    assert_eq!(applied[2].action, "created");

    // strict validation rejects unknown fields
    let bad = ops::apply_manifests(
        &conn,
        &format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: bad\n  namespace: {ns}\nbogusField: 1\n"),
        None,
        ApplyMode::Apply,
        true,
        false,
    )
    .await
    .unwrap();
    assert!(bad[0].error.is_some(), "strict field validation should reject unknown fields");

    // --- watch pods
    let watches = WatchManager::new();
    let messages: Arc<Mutex<Vec<WatchMessage>>> = Arc::default();
    let sink_msgs = messages.clone();
    let key = WatchKey {
        cluster_id: "e2e".into(),
        resource: builtin::pods(),
        namespace: Some(ns.clone()),
        label_selector: Some("app=echo".into()),
        field_selector: None,
    };
    let sub = watches
        .subscribe(client.clone(), key.clone(), move |m: WatchMessage| {
            sink_msgs.lock().push(m);
            true
        })
        .unwrap();
    let running = wait_until(Duration::from_secs(180), || {
        watches.cached_rows(&key).map(|rows| rows.iter().any(|r| r.status == "Running" && r.health == Health::Ok)).unwrap_or(false)
    })
    .await;
    assert!(running, "pod did not become Running; messages: {}", messages.lock().len());
    let pod_name = watches.cached_rows(&key).unwrap()[0].name.clone();
    assert!(messages.lock().iter().any(|m| matches!(m, WatchMessage::Snapshot { .. })));

    // --- logs
    let logs = LogManager::default();
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink_lines = lines.clone();
    let log_id = logs
        .start(
            client.clone(),
            &ns,
            &pod_name,
            LogOptions { follow: true, tail_lines: Some(10), ..Default::default() },
            move |m| {
                if let LogMessage::Lines { lines } = m {
                    sink_lines.lock().extend(lines);
                }
                true
            },
        )
        .await
        .unwrap();
    assert!(wait_until(Duration::from_secs(20), || lines.lock().iter().any(|l| l.starts_with("tick"))).await);
    logs.stop(&log_id);

    // --- exec
    let exec = Arc::new(ExecManager::default());
    let output: Arc<Mutex<Vec<u8>>> = Arc::default();
    let out = output.clone();
    let exited: Arc<Mutex<Option<ExecEvent>>> = Arc::default();
    let ex = exited.clone();
    let session = exec
        .start(
            client.clone(),
            ExecTarget { cluster_id: "e2e", namespace: &ns, pod: &pod_name, container: Some("main"), shell: "sh", cols: 80, rows: 24 },
            move |d| {
                out.lock().extend(d);
                true
            },
            move |e| *ex.lock() = Some(e),
        )
        .await
        .unwrap();
    exec.resize(&session, 120, 40).await.unwrap();
    exec.write(&session, b"echo manifex-$((40+2))\n".to_vec()).await.unwrap();
    let ok = wait_until(Duration::from_secs(15), || String::from_utf8_lossy(&output.lock()).contains("manifex-42")).await;
    assert!(ok, "exec output: {:?} exit: {:?}", String::from_utf8_lossy(&output.lock()), exited.lock());
    exec.write(&session, b"exit 3\n".to_vec()).await.unwrap();
    assert!(wait_until(Duration::from_secs(15), || exited.lock().is_some()).await);
    println!("exec exit: {:?}", exited.lock());

    // --- scale, restart, history, rollback
    ops::scale(client.clone(), &builtin::deployments(), &ns, "echo", 2).await.unwrap();
    ops::restart(client.clone(), &builtin::deployments(), &ns, "echo").await.unwrap();
    let got_two_revisions = {
        let mut ok = false;
        for _ in 0..60 {
            let h = ops::rollout_history(client.clone(), &ns, "echo").await.unwrap();
            // wait until the controller has marked the newest revision as current
            if h.len() >= 2 && h[0].current {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        ok
    };
    assert!(got_two_revisions, "restart should create a second revision");
    let history = ops::rollout_history(client.clone(), &ns, "echo").await.unwrap();
    let old = history.iter().find(|r| !r.current).unwrap();
    ops::rollback(client.clone(), &ns, "echo", old.revision).await.unwrap();
    let view = ops::get_object(client.clone(), &builtin::deployments(), Some(&ns), "echo").await.unwrap();
    assert!(!view.yaml.contains("managedFields"));
    assert_eq!(view.object["spec"]["replicas"], 2);

    // --- replace with optimistic concurrency
    let mut cm = ops::get_object(client.clone(), &builtin::configmaps(), Some(&ns), "settings").await.unwrap().object;
    cm["data"]["greeting"] = serde_json::json!("hi");
    let replaced = ops::apply_manifests(&conn, &serde_yaml::to_string(&cm).unwrap(), None, ApplyMode::Replace, false, false)
        .await
        .unwrap();
    assert_eq!(replaced[0].action, "replaced", "{replaced:?}");
    let stale = ops::apply_manifests(&conn, &serde_yaml::to_string(&cm).unwrap(), None, ApplyMode::Replace, false, false)
        .await
        .unwrap();
    assert!(stale[0].error.is_some(), "stale resourceVersion must conflict");

    // --- cleanup
    watches.unsubscribe(&sub);
    ops::delete_object(client.clone(), &builtin::namespaces(), None, &ns, &DeleteOptions::default()).await.unwrap();
    mgr.disconnect("e2e");
    assert!(mgr.get("e2e").is_err());
}

#[tokio::test]
async fn refuses_plain_http_and_insecure_tls() {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let vault = Arc::new(SecretVault::with_key([1u8; 32]));
    db.put_secret(&vault, "tok", b"abc").unwrap();
    let mgr = ClusterManager::new(db, vault, Arc::new(NullSink));
    let profile = ClusterProfile {
        id: "x".into(),
        name: "x".into(),
        source: ClusterSource::Token {
            server: "http://127.0.0.1:6443".into(),
            ca_pem: None,
            tls_server_name: None,
            proxy_url: None,
            secret_id: "tok".into(),
        },
        group: None,
        favorite: false,
        color: None,
        environment: "production".into(),
        default_namespace: None,
        alerts_enabled: false,
        sort_order: 0,
        created_at: String::new(),
        last_connected_at: None,
    };
    let r = mgr.test(&profile).await.unwrap();
    assert!(!r.ok);
    assert!(r.message.unwrap().contains("https"));
}

/// Connects through a running manifex-agent (see `agent/`). Skipped unless
/// MANIFEX_E2E_AGENT_URL, MANIFEX_E2E_AGENT_CA and MANIFEX_E2E_AGENT_TOKEN_FILE are set.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_connection_with_exec() {
    let (Ok(url), Ok(ca), Ok(token_file)) = (
        std::env::var("MANIFEX_E2E_AGENT_URL"),
        std::env::var("MANIFEX_E2E_AGENT_CA"),
        std::env::var("MANIFEX_E2E_AGENT_TOKEN_FILE"),
    ) else {
        eprintln!("agent e2e variables not set – skipping");
        return;
    };
    let db = Arc::new(Db::open_in_memory().unwrap());
    let vault = Arc::new(SecretVault::with_key([5u8; 32]));
    let token = std::fs::read_to_string(&token_file).unwrap();
    db.put_secret(&vault, "agent-tok", token.trim().as_bytes()).unwrap();
    let mgr = ClusterManager::new(db.clone(), vault, Arc::new(NullSink));
    let profile = ClusterProfile {
        id: "agent".into(),
        name: "via-agent".into(),
        source: ClusterSource::Agent {
            endpoint: url,
            ca_pem: Some(std::fs::read_to_string(&ca).unwrap()),
            proxy_url: None,
            secret_id: "agent-tok".into(),
        },
        group: None,
        favorite: false,
        color: None,
        environment: "staging".into(),
        default_namespace: None,
        alerts_enabled: false,
        sort_order: 0,
        created_at: String::new(),
        last_connected_at: None,
    };
    db.insert_cluster(&profile).unwrap();
    let status = mgr.connect("agent").await.expect("connect through agent");
    assert_eq!(status.state, ConnState::Connected);
    println!("agent identity: {:?}", status.identity);
    let client = mgr.get("agent").unwrap().client.clone();

    // a running pod in kube-system to exec into
    let pods = ops::list_objects(client.clone(), &builtin::pods(), Some("kube-system"), None, Some("status.phase=Running"), None).await.unwrap();
    let pod = pods
        .iter()
        .find(|p| p["metadata"]["name"].as_str().unwrap_or("").starts_with("local-path-provisioner"))
        .expect("local-path-provisioner pod");
    let name = pod["metadata"]["name"].as_str().unwrap().to_string();

    let exec = Arc::new(ExecManager::default());
    let output: Arc<Mutex<Vec<u8>>> = Arc::default();
    let out = output.clone();
    let session = exec
        .start(
            client,
            ExecTarget { cluster_id: "agent", namespace: "kube-system", pod: &name, container: None, shell: "sh", cols: 80, rows: 24 },
            move |d| {
                out.lock().extend(d);
                true
            },
            |_| {},
        )
        .await
        .expect("exec through agent");
    exec.write(&session, b"echo via-agent-$((6*7))\n".to_vec()).await.unwrap();
    let ok = wait_until(Duration::from_secs(15), || String::from_utf8_lossy(&output.lock()).contains("via-agent-42")).await;
    assert!(ok, "exec output through agent: {:?}", String::from_utf8_lossy(&output.lock()));
    exec.close(&session);
}
