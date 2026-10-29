# Rilldown

A local-first download manager for Windows and Linux, written in Rust with a
Tauri 2 shell and a React UI.

Rilldown is an original, clean-room implementation. It takes the ideas that make
classic download managers useful — segmented transfers, resume, queues, browser
capture — and rebuilds them around a memory-safe engine, a calm desktop shell,
and integrations that tell you the truth about what they can and cannot do.

## What works in 0.1.0

- **Segmented HTTP(S) transfers.** 1–32 connections per file, dynamic splitting
  when a worker finishes, `If-Range` validator checks, capped exponential retry
  with jitter, disk-space checks, and byte-exact final files.
- **Crash-safe resume.** Segment progress is checkpointed to SQLite. A restart
  recovers interrupted downloads as paused; resume continues from the
  acknowledged byte ranges.
- **Single-stream fallback** for servers without byte ranges or a known length.
- **Queues and limits.** Named queues with per-queue admission limits, a global
  concurrency limit, and a global or per-download speed ceiling enforced with a
  token bucket.
- **Proxies, cookies, referrer, headers, and checksums** per download.
- **Automatic categories** from the probed filename and content type.
- **Desktop UI.** Live progress, per-download actions, batch add, search,
  category/queue filters, settings, tray menu, close-to-tray, desktop
  notifications, clipboard link detection, and English/Bangla navigation.
- **Media grabbing.** `yt-dlp` handles HLS, DASH, and supported sites; `ffmpeg`
  merges streams. Both tools are used from `PATH` when available, otherwise
  downloaded on first use into Rilldown's private data directory.
- **Browser capture.** Chromium and Firefox extensions, a native-messaging host,
  and an authenticated loopback bridge. The app writes the native-host manifests
  for installed browsers; the extension is sideloaded (Chromium does not allow
  silent installs outside enterprise policy).
- **Signed auto-updates** through GitHub Releases when the release secrets are
  configured.

## Not in 0.1.0

- FTP/FTPS, and mirror URLs (the fields exist but the engine does not use mirrors).
- Time/weekday queue schedules and post-queue actions.
- Start with the operating system.
- macOS builds. The UI is portable but only Windows and Linux are release
  targets.
- Per-connection progress bars in the UI. The engine emits segment snapshots, but
  the list shows the connection count rather than 32 parallel bars.
- Auto-routing downloads into category folders. Categories are labels and
  filters; files still go to the chosen download directory.
- Checksum-pinned tool downloads. `yt-dlp` and `ffmpeg` are fetched over HTTPS
  from their official release channels into a `0700`-style private data folder,
  but Rilldown does not verify a pinned digest for them.
- Encrypted secrets. Cookies, headers, and proxy URLs that you attach to a
  download are stored in the local SQLite database so the download can resume.

## Repository layout

```
crates/engine        Core download engine (UI-free library + CLI)
crates/media         Stream detection plus yt-dlp/ffmpeg management and grabbing
crates/native-host   Native messaging host for browser extensions
src-tauri            Tauri 2 desktop application and loopback bridge
ui                   React + TypeScript + Tailwind frontend
extension            Chromium (MV3) and Firefox WebExtension sources
docs                 Architecture, feature matrix, browser and release notes
scripts              Icon generation, sidecar staging
```

## Build

Prerequisites:

- Rust stable (the pinned toolchain in `rust-toolchain.toml`)
- Node.js 22+ and npm
- Tauri 2 system dependencies

Ubuntu 22.04+:

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf
```

Windows 10/11: install the MSVC build tools and WebView2 (preinstalled on
current Windows). Rust's `stable-x86_64-pc-windows-msvc` toolchain is required.

Then:

```bash
npm install
npm run dev          # Vite + the Tauri window
npm run build        # installer bundles for the host platform
```

`npm run build` builds the native host, stages it under
`src-tauri/binaries/`, builds the UI and extension, then invokes Tauri. The
updater artifacts need `TAURI_SIGNING_PRIVATE_KEY` (and optionally
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Without a key, use
`npx tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'`.

The Windows installers are unsigned unless you configure an Authenticode
certificate. Windows SmartScreen will warn on first launch; that is expected and
documented in [docs/release.md](docs/release.md).

## Verify

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix ui run check
npm --prefix ui run lint
npm --prefix ui test -- --run
npm --prefix ui run build
node extension/build.mjs --check
```

The engine integration tests use an in-process range server and cover segmented
transfer, single-stream fallback, retries, pause/resume, checksums, queues, and
speed limiting. The UI tests cover the shell, dialogs, and event store.

## Browser extensions

Build both targets:

```bash
node extension/build.mjs
```

Then in Rilldown open **Settings → Browser integration**, install the native
host bridge, and load the unpacked extension from the folder the app opens.
Exact paths, the fixed Chromium extension id, and troubleshooting live in
[docs/browser-integration.md](docs/browser-integration.md).

## Design and release notes

- [ARCHITECTURE.md](ARCHITECTURE.md) — process model, transfer algorithm, data
  model, security boundaries, and the honest limitations.
- [CONTRIBUTING.md](CONTRIBUTING.md) — local setup, quality gates, clean-room
  policy.
- [docs/feature-matrix.md](docs/feature-matrix.md) — Rilldown against IDM,
  AB Download Manager, and Neat Download Manager.
- [docs/release.md](docs/release.md) — signing keys, update manifests, and the
  four release targets.

## License

MIT. Rilldown is not affiliated with, endorsed by, or derived from Internet
Download Manager, AB Download Manager, or Neat Download Manager. See
[CONTRIBUTING.md](CONTRIBUTING.md#clean-room-policy).
