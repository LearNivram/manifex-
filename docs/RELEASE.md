# Releases, signing & auto-update

## Versioning

The version lives in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json` – bump all three together. Tags `vX.Y.Z` trigger the
release workflow.

## Updater keys (one-time)

The Tauri updater verifies every update with a minisign key pair.

```bash
npx tauri signer generate -w ~/.tauri/manifex.key
```

1. Put the **public key** into `src-tauri/tauri.conf.json` →
   `plugins.updater.pubkey` (replace `REPLACE_WITH_UPDATER_PUBLIC_KEY`).
2. Store the **private key** and its password as repository secrets
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
   Never commit the private key.

The updater endpoint is
`https://github.com/<owner>/<repo>/releases/latest/download/latest.json`; the
release workflow uploads `latest.json` and signatures automatically
(`includeUpdaterJson: true`). Updater artifacts are only produced for release
builds (`src-tauri/tauri.release.conf.json`), so local builds do not need the key.

The public key of the project's key pair is already in `tauri.conf.json`. Without
the `TAURI_SIGNING_PRIVATE_KEY` secret the workflow still builds installers but
skips the updater artifacts. The in-app updater only sees **published** releases
(`releases/latest` ignores drafts).

## Agent image

`.github/workflows/agent-image.yml` builds `agent/` for linux/amd64 and
linux/arm64 and pushes `ghcr.io/<owner>/manifex-agent:<agent version>` and
`:latest` (on changes under `agent/` or manually). Make the package **public**
(GitHub → Packages → manifex-agent → Package settings → Change visibility), so
clusters can pull it without credentials. The app installs the image tag that
matches its own version.

## Code signing (recommended)

- **macOS:** set `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY` and, for notarisation, `APPLE_ID`, `APPLE_PASSWORD`
  (app-specific password), `APPLE_TEAM_ID`.
- **Windows:** configure `bundle.windows.certificateThumbprint` / signing command
  or use Azure Trusted Signing in the workflow.
- **Linux:** AppImage/deb/rpm are signed through the updater signature; distro
  repositories can additionally sign packages.

## Pipeline

`.github/workflows/release.yml` builds on macOS (universal binary), Linux
(ubuntu-22.04 for broad glibc compatibility) and Windows, creates a **draft**
GitHub release with all installers and `latest.json`, and leaves publishing to a
human.

Installer formats:

| Platform | Formats |
|---|---|
| Windows | NSIS `.exe` (per-user), `.msi` |
| macOS | `.dmg`, `.app` (universal) |
| Linux | `.AppImage`, `.deb`, `.rpm` |

## In-app updates

The Updates panel in the status bar (and Settings → Updates) checks the endpoint, downloads with progress, verifies the
signature and restarts. With "Check for updates automatically" enabled the main
window checks once at start-up and shows a notification when a new version exists.

## Releasing without certificates

The pipeline also works with no secrets configured: macOS builds are ad-hoc
signed (`signingIdentity: "-"`), Windows installers are unsigned, and updater
artifacts are skipped. Users then confirm the first launch (macOS: right-click →
Open; Windows SmartScreen: More info → Run anyway). Add the secrets above to get
signed, notarised builds and in-app updates.

```bash
git tag v0.1.0 && git push origin v0.1.0   # starts .github/workflows/release.yml
```
