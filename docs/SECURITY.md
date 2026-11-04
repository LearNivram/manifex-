# Security model

## Credentials

- **No credentials in the frontend.** Tokens, client keys and kubeconfig contents
  never cross the IPC bridge back to the webview. The frontend only sends new
  credentials once (when a cluster is added) and receives profile metadata.
- **Envelope encryption with the OS credential store.** A random 256-bit master key
  is created on first use and stored in Windows Credential Manager, macOS Keychain
  or the Linux Secret Service (via `keyring`). Each secret is encrypted with
  XChaCha20-Poly1305; the secret id is authenticated as associated data so
  ciphertexts cannot be swapped between entries. Only ciphertext is written to
  SQLite. If the OS store is unavailable, Manifex refuses to persist secrets.
- **Kubeconfigs are never persisted in plaintext.** Discovered kubeconfigs are
  referenced by path; importing "into the vault" stores an encrypted single-context
  copy.
- Key material is zeroised on drop.

## SSH connections

- The server's host key is pinned (SHA256 fingerprint) when the cluster is added:
  entries in `~/.ssh/known_hosts` are trusted automatically, unknown hosts must be
  confirmed by the user, and a key that differs from known_hosts or from the pinned
  fingerprint is always refused. No credentials are sent before the key is trusted.
- Remote commands are fixed constants (reading well-known kubeconfig paths, with
  `sudo -S` only when a password was given); nothing user-supplied is interpolated.
- The tunnel listens on 127.0.0.1 only, on a random port, for the lifetime of the
  connection. Kubernetes TLS verification is unchanged (`tls-server-name` = the API
  server's original name).

## Transport

- Only `https://` API endpoints are accepted.
- TLS certificate verification is **always** enforced. Contexts that request
  `insecure-skip-tls-verify` are rejected with an actionable message (provide a CA).
- Proxies (HTTP(S)/SOCKS5) are supported per cluster.

## Tauri hardening

- Strict CSP (`script-src 'self'`, no remote content, `connect-src ipc:`).
- `freezePrototype` is disabled on purpose: xterm.js and Monaco assign properties
  that shadow `Object.prototype` members at module load (the JavaScript "override
  mistake"), which throws once the prototype is frozen. The strict CSP and the
  absence of remote content keep prototype-pollution attack surface minimal.
- Capabilities grant only core window operations, notifications, the updater,
  process restart and deep links to application windows. **No** `fs`, `shell` or
  `http` plugins are enabled for the webview.
- File dialogs are opened by the Rust core (`pick_kubeconfig_file`,
  `save_text_file`); the webview never receives arbitrary file access.
- Deep links are validated against an allow-list of routes and safe characters.

## Process execution

- No shell is ever spawned. The only external binaries are Kubernetes credential
  plugins configured in the user's kubeconfig (handled by kube-rs) and the Helm CLI.
- Helm is executed with an explicit argument vector; every argument (release,
  chart, version, namespace, repository) is validated, and the binary must be named
  `helm`. Values and temporary kubeconfigs are written to private (0600) temp files
  which are deleted immediately after the command.
- Container shells are selected from an allow-list (`auto`, `bash`, `sh`, `ash`,
  `zsh`, `powershell`, `cmd`).

## Input validation

All IPC inputs are validated in `security::validate` (RFC 1123 names, namespaces,
API groups/versions, selectors, URLs, Helm arguments) before they reach URL
construction, the database or a child process.

## Kubernetes RBAC

- Manifex acts with the user's own identity – it never escalates privileges.
- `SelfSubjectAccessReview` checks drive the UI: actions the user may not perform
  are disabled (and marked with a lock). Results are cached for 60 s.
- Forbidden list/watch requests render an "Access denied" state instead of errors.
- The API server remains the final authority.

## Dangerous operations

- Deletes, drains, scale-to-zero, Helm uninstall/rollback require confirmation and
  always show the target cluster and its environment.
- For clusters marked **production**, the user must type the object name. This is
  enforced again in the Rust core (`check_production_confirmation`), not only in
  the UI.
- Server-side dry-run and diff are available before every apply.
- Edits use optimistic concurrency (`resourceVersion`) – stale edits are rejected.

## Auditability

Every mutating action (apply, replace, delete, scale, restart, rollout, cordon,
drain, CronJob actions, exec sessions, Helm operations, cluster add/remove) is
recorded in the local audit log with timestamp, cluster, authenticated identity,
target object, outcome and error detail. The log can be filtered and exported as
CSV (formula-injection safe) or JSON.

## Reporting vulnerabilities

Please report security issues privately to the maintainers instead of opening a
public issue.
