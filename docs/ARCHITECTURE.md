# Architecture

Manifex is a native desktop application built with **Tauri 2**. A Rust core owns
every privileged operation (credentials, Kubernetes API traffic, files, processes);
the React frontend renders state and sends intents over Tauri's IPC bridge.

```
┌──────────────────────────── Webview (React + TypeScript) ────────────────────────────┐
│  App shell · Sidebar · Command palette · Views · Detail panel · Monaco · xterm.js     │
│  zustand (UI state)   react-query (request cache)   useWatch (live collections)       │
└───────────────▲───────────────────────────────┬──────────────────────────────────────┘
                │ events / Channels             │ invoke (typed, validated)
┌───────────────┴───────────────────────────────▼──────────────────────────────────────┐
│ Rust core (src-tauri/src)                                                             │
│  commands/     IPC surface – input validation, audit logging, RBAC-aware operations   │
│  desktop/      tray, macOS menu, extra windows, native notifications (TauriSink)      │
│  k8s/          Kubernetes integration layer (kube-rs, tokio)                          │
│     connection   ClusterManager: connect/test, health probes, auto-reconnect          │
│     kubeconfig   discovery, context extraction, TLS policy, synthetic kubeconfigs     │
│     watch        shared watches → snapshot + batched deltas (150 ms)                  │
│     summary      kubectl-compatible status → compact table rows                       │
│     ops          apply (SSA / replace), delete, scale, restart, rollouts, drain …     │
│     logs/exec    streaming logs, interactive TTY sessions (WebSocket)                 │
│     metrics      metrics.k8s.io + capacity / requests / limits                        │
│     helm         native release decoding + Helm CLI (no shell)                        │
│     rbac         SelfSubjectAccessReview / RulesReview                                │
│     schema       OpenAPI v3 → JSON schema for the YAML editor (incl. CRDs)            │
│     insights     cross-cluster search, cluster comparison, application grouping       │
│     alerts       Warning-event / node watchers, rules, de-duplication                 │
│  persistence/  SQLite (profiles, encrypted secrets, settings, rules, audit log)       │
│  security/     SecretVault (OS keychain + XChaCha20-Poly1305), input validation       │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

## Layers

| Layer | Responsibility | Key types |
|---|---|---|
| Frontend | UI, interaction, visualisation, transient UI state | `useApp`, `useWatch`, `ResourceView`, `DetailPanel`, `EditorPanel` |
| Rust core | Native integration, process management, credential handling, IPC | `AppState`, `commands::*`, `desktop::*` |
| Kubernetes integration | API communication, connections, watch streams, resource operations | `ClusterManager`, `WatchManager`, `ops`, `ExecManager`, `LogManager` |
| Persistence | Local settings, cluster profiles, preferences, secure metadata | `Db`, `SecretVault` |

## Data flow for live lists

1. A view calls `useWatch(cluster, resource, namespace)`.
2. The frontend invokes `watch_resources` with a Tauri `Channel`.
3. `WatchManager` looks up (or starts) the **shared** upstream `watch` for that key
   (`kube::runtime::watcher` with pagination, bookmarks and backoff).
4. Each object is summarised in Rust into a compact `Row` (status logic mirrors
   kubectl). Full objects never cross the bridge during a watch.
5. The subscriber receives a snapshot followed by batches of upserts/deletes every
   150 ms. The table is virtualised (`@tanstack/react-virtual`), so only visible
   rows are rendered.
6. When the last subscriber leaves, the upstream watch is kept warm for 60 s (fast
   back-and-forth navigation) and then stopped – no permanent background traffic.

## Connections

`ClusterSource` variants:

| Source | Credentials | Notes |
|---|---|---|
| `kubeconfig` | stay in the user's kubeconfig | Path + context are referenced, never copied. Exec/OIDC plugins work. |
| `storedKubeconfig` | encrypted vault | Single context, files inlined, encrypted. |
| `token` | encrypted vault | Bearer token, optional CA / TLS server name / proxy. |
| `certificate` | encrypted vault | Client certificate + key. |
| `agent` | encrypted vault | HTTPS endpoint of a Manifex agent (see `agent/`) or another authenticating Kubernetes API gateway. |
| `ssh` | encrypted vault (key, certificate, passphrase, password, last kubeconfig) | Logs in to an SSH server (OpenSSH certificate → public key → password, host key pinned), fetches the kubeconfig there (kubectl, ~/.kube/config, k3s, RKE2, kubeadm, MicroK8s, with `sudo` if needed) on every connect and tunnels the API through `direct-tcpip` channels from a local 127.0.0.1 port. TLS is still verified against the API server's real name. |

Health: every connected cluster is probed (`/version`) every *n* seconds
(configurable). On failure the state becomes `reconnecting`, a notification is
raised, and probes continue with exponential back-off (2 s → 60 s). Watches use the
same client and resume automatically (resourceVersion / re-list).

## Multi-window & multi-cluster

All windows share one Rust core: connections, watch caches and terminal sessions
are global, so opening a cluster in a second window costs no extra API traffic.
Every command takes an explicit `cluster_id`; there is no implicit "current
context" in the core, which makes acting on the wrong cluster structurally
impossible. The UI shows the active cluster (with environment colour) in the title
bar, the status bar and in every confirmation dialog.

## Persistence

SQLite in the platform app-data directory (`manifex.db`, mode 0600, WAL):

- `clusters` – profiles (no secrets)
- `secrets` – nonce + ciphertext only
- `settings` – JSON preferences
- `alert_rules` – user-defined alert rules
- `audit_log` – every mutating action with identity and outcome

Schema migrations are versioned through `PRAGMA user_version`.

## Frontend structure

```
src/
  lib/           ipc (typed bridge), types, formatting, YAML tools, deep links
  store/         zustand application store
  hooks/         useWatch, useCanI, useTheme
  components/    ui primitives, layout (title bar, sidebar, palette, …), table
  features/      clusters, overview, resources, editor, logs, terminal, helm,
                 monitoring, apps, access, custom, audit, settings
```
