# manifex-agent

A small in-cluster component that lets Manifex manage clusters which are not
directly reachable (private API servers, clusters behind NAT/bastions, or when you
do not want to hand out kubeconfigs).

- Terminates TLS itself (`tls.crt` / `tls.key`).
- Authenticates clients with a bearer token (constant-time comparison, re-read on
  every request so it can be rotated without restart).
- Forwards to the in-cluster API server with its ServiceAccount token – optionally
  impersonating a configured user/groups – so permissions are governed by ordinary
  Kubernetes RBAC (read-only `view` by default).
- Strips client-supplied `Authorization` and `Impersonate-*` headers (no privilege
  escalation through the agent).
- Proxies protocol upgrades, so `exec`, `attach` and `port-forward` work.

See [`deploy/manifex-agent.yaml`](deploy/manifex-agent.yaml) for installation and
the environment variables documented at the top of [`src/main.rs`](src/main.rs).

```bash
cargo build --release          # binary: target/release/manifex-agent
docker build -t manifex-agent . # distroless, non-root image
```
