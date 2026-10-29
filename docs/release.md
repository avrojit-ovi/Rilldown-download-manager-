# Releasing Rilldown

## Targets

| Target | Runner | Bundles |
|---|---|---|
| Windows x64 | `windows-latest` | NSIS `.exe` |
| Windows ARM64 | `windows-11-arm` | NSIS `.exe` |
| Linux x64 | `ubuntu-22.04` | `.deb`, AppImage |
| Linux ARM64 | `ubuntu-24.04-arm` | `.deb`, AppImage |

macOS is not a 0.1.0 target. The code compiles on macOS, but no macOS bundle is
produced or tested.

## Updater signing key

Tauri's updater signs update artifacts with a minisign key. The public key is
committed in `src-tauri/tauri.conf.json`. The private key must never be
committed.

This checkout's key was generated with:

```bash
npx tauri signer generate --ci -p "" -w ~/.tauri/rilldown.key -f
```

- Private key: `~/.tauri/rilldown.key` (back it up; losing it breaks updates)
- Public key: `~/.tauri/rilldown.key.pub` (already in the config)

Add the private key and its password to the GitHub repository secrets:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/rilldown.key
# Only if you set a password:
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Alternatively use the repository's **Settings → Secrets and variables → Actions**
page. Without `TAURI_SIGNING_PRIVATE_KEY`, a Tauri build with
`createUpdaterArtifacts: true` fails during signing.

## Local build

```bash
# signed build for the host platform (requires the signing key env vars)
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/rilldown.key)"
npm run build

# unsigned build, no updater artifacts
npx tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

`npm run build` first compiles `rilldown-native-host`, stages it as
`src-tauri/binaries/rilldown-native-host-<triple>`, builds the UI and extension,
then invokes Tauri. The staged sidecar is required by Tauri's external-binary
bundler.

## CI release

`.github/workflows/release.yml` builds all four targets on a `v*` tag (or manual
dispatch), signs updater artifacts, and publishes a GitHub Release with the
installers attached. To keep a build back, delete the release or re-run the
tag; there is no manual publish step.

Signing only happens when both `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are set. Otherwise the workflow builds
unsigned installers with `createUpdaterArtifacts` disabled, so a missing or
mismatched key never blocks a release — it only means no `latest.json` is
published and the in-app updater stays off for that build.

The updater endpoint is:

```
https://github.com/avrojit-ovi/Rilldown-download-manager-/releases/latest/download/latest.json
```

That URL resolves once the release is the latest published one. If the workflow
ran without the signing secrets, the in-app update check reports that it cannot
find an update manifest.

`.github/workflows/windows-build.yml` is a manual, unsigned build for testing.
It uploads NSIS installers and executables as workflow artifacts and does not
create a release, so it works without the signing secrets.

## Windows testing

There is no WebView2 runtime under Wine, so the desktop window cannot be
exercised meaningfully in a Linux Wine prefix. What can be tested locally:

- The Windows engine CLI and native host can be cross-compiled and run under
  Wine; they exercise the transfer engine, SQLite, framing, and bridge client.
- The NSIS installer can be inspected (and, on a real Windows machine, installed).
- The Tauri window, tray, WebView2 integration, updater installer, and native
  host manifest registry writes require a real Windows 10/11 machine or VM.

The release is unsigned unless an Authenticode certificate is configured.
SmartScreen will show “Windows protected your PC”; choose **More info → Run
anyway** after verifying the SHA-256 of the artifact. Signing is a matter of
adding the certificate to the build; no placeholder signatures are produced.

## SHA-256 checksums

The release workflow does not currently publish a checksums file. Generate one
from the downloaded artifacts:

```bash
sha256sum Rilldown_*.exe Rilldown_*.deb Rilldown_*.AppImage > SHA256SUMS
```

Attach it to the release so users can verify a download before running an
unsigned installer.
