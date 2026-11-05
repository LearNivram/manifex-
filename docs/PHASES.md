# Development phases

Each phase lists what was built and how it was verified. "e2e" refers to
`src-tauri/tests/cluster_e2e.rs`, run against a real k3s v1.33 cluster;
"app" refers to manual verification of the compiled desktop application against
the same cluster (screenshots in `docs/screenshots`).

| # | Phase | Delivered | Verification |
|---|---|---|---|
| 1 | Architecture & foundations | Tauri 2 + Rust core + React/Vite/Tailwind; layered modules (commands / desktop / k8s / persistence / security); error model; SQLite with migrations | `cargo test --lib`, `npm run typecheck` |
| 2 | Desktop core & UI shell | Window management (min sizes, state restore, multiple windows), tray, macOS menu, global shortcut, single instance, deep links, notifications; app shell with sidebar, title bar, status bar, command palette, notification centre, theming | app |
| 3 | Cluster connections | Kubeconfig discovery/import (path reference or encrypted vault copy), token / certificate / agent connections, proxy support, connection tests, health probes, auto-reconnect with back-off, TLS policy (no insecure-skip) | unit tests (kubeconfig parsing, TLS policy, vault crypto), e2e (connect, refuse http), app |
| 4 | Kubernetes API integration | Shared watches with snapshot + batched deltas, discovery, kubectl-compatible status summaries, RBAC checks, OpenAPI schemas | unit tests (status logic for pods/deployments/nodes/…), e2e (watch → Running pod) |
| 5 | Resource & service management | Generic resource views for all kinds incl. Gateway API & CRDs, detail panel, delete (propagation), scale, restart, pause, rollout history/rollback, cordon/drain, CronJob trigger/suspend, bulk delete, production confirmations | e2e (scale, restart, rollback, replace conflict), app (delete with typed confirmation) |
| 6 | Terminal & YAML editor | xterm.js exec sessions (tabs, shells, resize, copy); Monaco editor with cluster schemas, lint markers, format, server dry-run + diff, apply (SSA) / replace (optimistic locking) | e2e (exec with exit code, SSA, strict validation), app (interactive shell, dry-run diff + save) |
| 7 | Monitoring, logging & events | Capacity overview, live node/pod metrics charts, top pods; log streaming (multi-pod, filter, export); events; alert engine (pod/workload/node + custom rules) with native notifications | e2e (log streaming), unit tests (alert classification, rule matching, de-duplication), app |
| 8 | Helm & applications | Native release decoding (history, values, manifest, notes, resources); Helm CLI install/upgrade/rollback/uninstall, repos, search; application grouping by `app.kubernetes.io/*` labels | unit tests (release payload decoding), app (release list + rollback) |
| 9 | UI / UX | Command palette with cross-cluster search & actions, keyboard shortcuts, cluster favourites/groups/colours, comparison view, permission matrix, custom resources browser, error boundaries, dark/light themes | app, vitest (formatting, YAML tools, watch merging, deep-link parsing) |
| 10 | Security, tests & performance | Vault (OS keychain + XChaCha20-Poly1305), input validation, restrictive capabilities & CSP, audit log + export, RBAC-aware actions; virtualised tables, shared watch cache with idle teardown, row summarisation in Rust; clippy `-D warnings` | 40 Rust unit tests, 15 frontend tests, 3 e2e tests, CI |
| 11 | Installer, updater, releases | Bundles for Windows/macOS/Linux, updater plugin + UI, release workflow with signed updater artifacts; `manifex-agent` with Dockerfile and manifests | CI workflow, `docs/RELEASE.md` |

## Follow-up: connection "via SSH server" (0.2.0)

Verified against a real OpenSSH server (port 2222) in front of k3s with a user
`deploy` that has **no** kubeconfig of its own and can read the root-only
`/etc/rancher/k3s/k3s.yaml` only through `sudo` with a password:

- login with an OpenSSH **user certificate** (sshd: `Accepted certificate ID
  "deploy-cert" … signed by ED25519 CA`) and with password only,
- kubeconfig discovered via `sudo -S`, API reachable through the tunnel, exec
  (WebSocket) through SSH, wrong pinned host key refused (e2e
  `ssh_connection_fetches_kubeconfig_and_tunnels`),
- full UI flow in the app: default key + certificate detected, fingerprint
  confirmation, test, save, automatic reconnect after app restart.

## Follow-up: agent, updates & image versions (0.3.0)

Verified against k3s and the SSH test server:

- one-click install of the agent via the SSH connection (NodePort), generated CA
  verified by the client, "edge-server (agent)" connection created and connected,
  `edit` role enforced (creating a ClusterRoleBinding through the agent is denied),
- image change of `shop/api` from `nginx:1.27` to `nginx:1.27.3` through the
  agent with production confirmation, revision 2 + change-cause,
- agent self-update 0.2.9 → 0.3.0 through the agent connection itself without
  losing the connection, removal (e2e `agent_one_click_install_update_and_set_image`).

Found and fixed: the alert engine retried forbidden node watches about twice per
second (now paused for 60 s on 401/403), the overview showed "0 nodes · all good"
without permission, and the Updates panel did not refresh after an install.

## Findings fixed during verification

Running the real application against a cluster surfaced issues that unit tests
could not:

- `AttachedProcess` closes the exec WebSocket when dropped – sessions now own it.
- Tauri's `freezePrototype` breaks xterm.js / Monaco at module load ("override
  mistake") – disabled, documented in SECURITY.md.
- monaco-yaml's worker used Monaco's pre-0.55 initialisation protocol – replaced by
  a synchronous shim.
- A failing lazy chunk crashed the whole window – views, detail panel and editor are
  now isolated by error boundaries.
- A self-signed CA certificate used as server certificate is (correctly) rejected
  by rustls – the agent manifest documents CA + server certificates.
