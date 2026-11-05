# Changelog

## 0.3.0

- **One-click Manifex agent.** "Install Manifex agent…" (cluster menu or the
  Updates panel) deploys the agent into any connected cluster: namespace
  `manifex-system`, service account with the chosen role (view / edit / admin /
  cluster-admin), Service (LoadBalancer or NodePort), a private CA + server
  certificate and an access token. Manifex waits for the rollout, reports image
  pull / scheduling / crash problems clearly and creates a ready-to-use
  "<cluster> (agent)" connection – no kubeconfig or SSH needed afterwards.
- **Updates panel in the status bar.** It shows the Manifex app version (update &
  restart in place) and the agent version of every connected cluster, with
  one-click agent updates – also through the agent connection itself (zero
  downtime rolling update) – and removal.
- **Change image versions of workloads.** Deployments, StatefulSets, DaemonSets
  and CronJobs have an "Images / versions" editor in the detail panel: change the
  tag (or the full image) per container and roll it out. The change is recorded
  as `kubernetes.io/change-cause`, so it is visible in the rollout history and can
  be rolled back.
- The updater signing key is configured and the release workflow publishes
  signed update artifacts when the `TAURI_SIGNING_PRIVATE_KEY` secret is set.
- Container image `ghcr.io/learnivram/manifex-agent` (amd64 + arm64) built by
  `.github/workflows/agent-image.yml`; the agent runs as numeric non-root user 65532.
- Watches that fail with 401/403 (for example nodes with the agent's `edit` role)
  pause for 60 s instead of retrying several times a second; the overview shows
  "No permission" instead of an empty, healthy tile.

## 0.2.0

- New connection method **"Via SSH server"**: enter host, user and your SSH key /
  OpenSSH certificate (default keys in ~/.ssh are detected automatically) plus
  passphrase / password. Manifex verifies the host key, logs in, finds the
  kubeconfig on the server (with sudo if needed), tunnels the Kubernetes API
  through SSH and reconnects automatically.
- Helm, exec, logs and port-heavy features work through the SSH tunnel.
- Monitoring charts render without stretched labels.

## 0.1.0

First release of Manifex – native Kubernetes management for Windows, macOS and
Linux. See README.md for the feature overview and docs/PHASES.md for the scope of
each development phase.
