# Changelog

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
