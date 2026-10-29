<div align="center">

# Rilldown

**A free, open-source download manager and accelerator for Windows and Linux — segmented HTTP(S) transfers, crash-safe resume, browser capture, and HLS/DASH video downloads in a native Rust + Tauri app.**

[![CI](https://github.com/avrojit-ovi/Rilldown-download-manager-/actions/workflows/ci.yml/badge.svg)](https://github.com/avrojit-ovi/Rilldown-download-manager-/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/avrojit-ovi/Rilldown-download-manager-?display_name=tag)](https://github.com/avrojit-ovi/Rilldown-download-manager-/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux-4c8bf5)

[Download](#download) · [Why Rilldown](#why-rilldown) · [Comparison](#how-rilldown-compares-with-other-download-managers) · [Build from source](#build-from-source) · [Documentation](#documentation)

</div>

---

Rilldown is a desktop **download manager** for people who move large files and
expect them to arrive intact. It splits each transfer across up to 32
connections, keeps a byte-level checkpoint in SQLite so an interrupted download
resumes instead of restarting, verifies checksums you supply, and captures links
from Chrome, Edge, Brave and Firefox through a native-messaging host rather than
a resident web server.

It is written in Rust with a Tauri 2 shell and a React/TypeScript interface, so
there is no Electron runtime, no bundled browser engine, and no telemetry. It is
MIT licensed and runs on Windows (x64 and ARM64) and Linux (x64 and ARM64). macOS
is not a 0.1.0 target.

Rilldown is an original, clean-room implementation. It takes the ideas that make
classic download managers useful — segmented transfers, resume, queues, browser
capture — and rebuilds them around a memory-safe engine and integrations that
tell you the truth about what they can and cannot do.

## Download

Installers are published on the [releases page](https://github.com/avrojit-ovi/Rilldown-download-manager-/releases/latest).
The latest version is **0.1.0**.

| Platform | Architecture | File | Install |
|---|---|---|---|
| Windows 10/11 | x64 | `Rilldown_0.1.0_x64-setup.exe` | Run the installer |
| Windows 11 | ARM64 | `Rilldown_0.1.0_arm64-setup.exe` | Run the installer |
| Debian / Ubuntu | x64 | `Rilldown_0.1.0_amd64.deb` | `sudo apt install ./Rilldown_0.1.0_amd64.deb` |
| Debian / Ubuntu | ARM64 | `Rilldown_0.1.0_arm64.deb` | `sudo apt install ./Rilldown_0.1.0_arm64.deb` |
| Any Linux | x64 | `Rilldown_0.1.0_amd64.AppImage` | `chmod +x` and run |
| Any Linux | ARM64 | `Rilldown_0.1.0_aarch64.AppImage` | `chmod +x` and run |

Two things are worth knowing before you install:

- **The 0.1.0 installers are unsigned.** Windows SmartScreen will warn on first
  launch and Linux packages are not GPG-signed. Download only from this
  repository's releases page. Signing is described in
  [docs/release.md](docs/release.md).
- **The in-app updater needs a signing key** to be configured in the release
  pipeline. The published 0.1.0 build has no `latest.json`, so update checks
  report that no update manifest is available.

## Why Rilldown

Most download managers ask you to trust a black box. Rilldown's differences are
the ones you can inspect:

- **A native engine, not a browser wrapper.** The download engine is a
  UI-independent Rust crate with its own CLI, covered by integration tests
  against an in-process HTTP range server. The desktop app is a client of it.
- **Byte-exact results.** Every completed transfer is checked against the
  expected length, and MD5, SHA-1 or SHA-256 checksums you supply are verified
  before the file is published — a mismatch leaves no file behind.
- **Validator-aware resume.** Segments store their `ETag`/`Last-Modified`, and
  resumes send `If-Range`. If the file changed on the server, Rilldown restarts
  instead of gluing mismatched bytes together.
- **Crash-safe by construction.** Progress is checkpointed to SQLite every
  400 ms. Kill the process mid-transfer and it comes back as *paused*, resuming
  from the acknowledged byte ranges.
- **Honest networking.** Capped exponential retry with jitter, a token-bucket
  speed limiter (global or per download), HTTP/2, `Content-Disposition`
  filenames, 10-hop redirects, and a single-stream fallback when a server offers
  no ranges.
- **Browser capture without a listener loop.** A native-messaging host launched
  by the browser talks to the app over a token-authenticated loopback bridge,
  and the app writes the host manifests for the browsers it finds.
- **Local-first and quiet.** No account, no telemetry, no analytics, no cloud
  queue. Everything lives in a local SQLite database next to your downloads.
- **Media when you need it.** `yt-dlp` handles HLS, DASH and supported sites;
  `ffmpeg` merges the streams with `-c copy`, so nothing is re-encoded.

## Features

**Transfer engine**

- 1–32 connections per file with dynamic segment splitting as workers finish
- `If-Range` / `ETag` / `Last-Modified` validation and byte-exact finalisation
- Single-stream fallback for servers without ranges or a known length
- Sparse pre-allocation, disk-space checks, `.part` files, duplicate-name handling
- Capped exponential retry with jitter; failed segments resume, not restart
- Per-download and global speed limits enforced with a token bucket
- Proxy (HTTP/SOCKS5), cookies, referrer, arbitrary headers, per-download credentials
- MD5 / SHA-1 / SHA-256 verification; a mismatch fails the download cleanly

**Queues and organisation**

- Named queues with per-queue admission limits and a global concurrency cap
- Automatic categories derived from the probed filename and content type
- Batch URL list, search, category and queue filters
- Clipboard link detection (opt-in, prompts before adding)

**Desktop app**

- Live progress and speed, pause/resume/cancel/retry per download
- System tray, close-to-tray, single instance, desktop notifications
- Dark, light and system themes; English and Bangla navigation
- Settings for limits, categories, browser integration and update checks

**Browser and media**

- Chromium (MV3) and Firefox WebExtension sources, sideloaded
- Native-messaging host with in-app installation and a fixed Chromium extension ID
- `yt-dlp` + `ffmpeg` used from `PATH` when present, otherwise fetched on first
  use into a private data directory

## How Rilldown compares with other download managers

Rilldown is not the right tool for every job, and this table says so. Competitor
columns summarise public documentation and observable behaviour **as of October
2026**; projects change, so check their own documentation before you choose.

Legend: **Yes** = supported, **Partial** = limited or third-party, **No** = not
offered, **—** = not applicable.

| Download manager | Platforms | Licence / price | Multi-connection | Browser integration | Stream / video capture | ARM64 builds | CLI |
|---|---|---|---|---|---|---|---|
| **Rilldown** | Windows, Linux | **MIT, free** | **Yes** (1–32, dynamic split) | **Yes** (Chromium + Firefox ext, native host) | **Yes** (yt-dlp + ffmpeg) | **Yes** (x64 + ARM64) | **Yes** (engine CLI) |
| Internet Download Manager | Windows | Proprietary, paid | Yes (up to 32) | Yes (extension) | Yes (site grabber) | No (x64 installer) | No |
| Free Download Manager | Windows, Linux, macOS | Proprietary freeware, optional premium | Yes | Yes | Yes | Partial | No |
| JDownloader 2 | Windows, Linux, macOS (Java) | Open source, free | Yes | Yes (MyJDownloader) | Yes (plugins) | Runs on any JVM | Partial (headless / remote API) |
| Xtreme Download Manager (XDM) | Windows, Linux, macOS | GPL-3.0 | Yes | Yes | Yes | Partial | No |
| AB Download Manager | Windows, Linux, macOS, Android | Apache-2.0 | Yes | Yes | No | Yes | No |
| Neat Download Manager | Windows, macOS | Proprietary freeware | Yes | Yes | Yes | No | No |
| Motrix | Windows, Linux, macOS | Open source (MIT) | Yes (aria2) | No (clipboard / tray) | No | Yes | No |
| Gopeed | Windows, Linux, macOS, Android, Web/Docker | GPL-3.0 | Yes | Yes (extension) | No (HTTP, BitTorrent, magnet, ED2K) | Yes | REST API |
| aria2 | Windows, Linux, macOS | GPL-2.0 | Yes | No (third-party wrappers) | No | Yes | Yes (CLI-first) |
| uGet | Linux, Windows | Open source | Yes (aria2 / curl backends) | Partial | No | Partial | No |
| Persepolis | Linux (older Windows/macOS builds) | GPL-3.0 | Yes (aria2) | Yes (extension) | No | Partial | No |
| DownThemAll! | Firefox | Open source, free | Yes | In-browser only | No | — | No |
| pyLoad | Self-hosted server (Python) | Open source | Yes | Yes (extension) | Partial (plugins) | Yes (server) | Yes (API) |

### Pick the right tool for the job

- **You want IDM-class segmented downloads on both Windows and Linux, in an
  open-source app with no telemetry** → Rilldown.
- **You need BitTorrent, magnet links or ED2K** → Gopeed, Motrix or aria2.
  Rilldown does not speak these protocols.
- **You need FTP/FTPS** → IDM or Free Download Manager. Rilldown is HTTP(S) only.
- **You are on macOS or Android** → AB Download Manager, FDM or Gopeed. macOS is
  not a 0.1.0 target.
- **You need time-of-day or weekday queue schedules** → IDM or AB Download
  Manager. Rilldown queues are admission-based only.
- **You download from file hosters with container links (DLC, CCF)** →
  JDownloader 2, which has no equivalent here.
- **You want scripting and headless orchestration** → aria2, or Rilldown's engine
  CLI if you specifically want its checkpointing and checksum verification.
- **You need signed, notarised macOS builds or an Authenticode-signed Windows
  installer today** → a vendor that ships signed binaries. Rilldown's 0.1.0
  installers are unsigned.

The deeper, capability-by-capability table — including range safety, checksum
support, tray behaviour and updater details — is in
[docs/feature-matrix.md](docs/feature-matrix.md).

## Quick start

1. Install the package for your platform from [Download](#download).
2. Add a link with **Add download**, paste a URL, or copy a link and accept the
   clipboard prompt.
3. Choose a queue, connections (1–32) and an optional speed limit, then start.
4. Watch it in the list; pause and resume at any time, including after a restart.

Optional next steps:

- **Browser capture:** open **Settings → Browser integration**, install the
  native host, then load the unpacked extension from the folder the app opens.
  See [docs/browser-integration.md](docs/browser-integration.md).
- **Video:** paste a page URL and use **Media** to inspect the streams `yt-dlp`
  can see before downloading.
- **Updates:** the 0.1.0 build cannot self-update until release signing is
  configured; check the releases page manually.

## Build from source

Prerequisites: Rust stable (the toolchain pinned in `rust-toolchain.toml`),
Node.js 22+, and npm.

Ubuntu 22.04+:

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf
```

Windows 10/11: install the MSVC build tools and WebView2 (preinstalled on current
Windows) and use Rust's `stable-x86_64-pc-windows-msvc` toolchain.

```bash
npm install
npm run dev          # Vite + the Tauri window
npm run build        # installer bundles for the host platform
```

`npm run build` compiles the native host, stages it under `src-tauri/binaries/`,
builds the UI and the extension, then invokes Tauri. Updater artifacts need
`TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` when the key
is encrypted). Without a key, build unsigned installers:

```bash
npx tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

### Verify a change

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

The engine integration tests run against an in-process range server and cover
segmented transfer, single-stream fallback, retries, pause/resume, checksums,
queues and speed limiting. The UI tests cover the shell, dialogs and the event
store.

## What 0.1.0 does not do

Published limitations, stated up front so nobody discovers them mid-download:

- No FTP/FTPS, BitTorrent, magnet links, ED2K, or mirror URLs (the field is
  reserved but unused).
- No time/weekday queue scheduling, post-queue actions, or start-with-the-OS.
- No macOS build. The UI is portable; only Windows and Linux are release targets.
- No per-connection progress bars — the engine emits segment snapshots, but the
  list shows a connection count.
- No automatic routing of downloads into per-category folders; categories are
  labels and filters.
- No pinned-digest verification for the `yt-dlp`/`ffmpeg` binaries it fetches.
- Cookies, headers and proxy URLs attached to a download are stored unencrypted
  in the local SQLite database so the download can resume.
- No silent extension install and no enterprise policy installer; the extension
  is sideloaded.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — process model, transfer algorithm, data
  model, security boundaries and the honest limitations.
- [docs/feature-matrix.md](docs/feature-matrix.md) — capability-by-capability
  comparison against IDM, AB Download Manager and Neat Download Manager.
- [docs/browser-integration.md](docs/browser-integration.md) — native-host
  installation, extension IDs, and troubleshooting.
- [docs/release.md](docs/release.md) — signing keys, update manifests, release
  targets, and what unsigned installers mean.
- [CHANGELOG.md](CHANGELOG.md) — what changed in each release.
- [CONTRIBUTING.md](CONTRIBUTING.md) — local setup, quality gates and the
  clean-room policy.
- [SECURITY.md](SECURITY.md) — how to report a vulnerability privately.

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
.github              CI, release workflow, issue and PR templates
```

## FAQ

**Is Rilldown free?** Yes. It is MIT licensed, with no paid tier, no account and
no telemetry.

**Does it work with Chrome, Edge, Brave and Firefox?** Yes, through a bundled
native-messaging host and sideloaded extensions. Chromium browsers do not allow
silent extension installation, so you load the unpacked extension once.

**Can it download YouTube or HLS/DASH streams?** It can download anything
`yt-dlp` supports, merged with `ffmpeg`. Respect the terms of service and
copyright of the site you are downloading from.

**Will it resume after a crash or reboot?** Yes. Progress is checkpointed to
SQLite every 400 ms and interrupted downloads return as paused.

**Is there a macOS build?** Not in 0.1.0. Windows and Linux are the release
targets.

**Is it faster than IDM?** No benchmark is claimed. Rilldown uses the same
segmented-transfer approach, and actual speed depends on the server, the segment
count and your connection. What Rilldown adds is verifiable behaviour: range
safety, checksum verification and crash-safe checkpoints.

**Why do I get a SmartScreen warning on Windows?** The 0.1.0 installers are not
Authenticode-signed. Obtain them only from this repository's releases page.

## Contributing

Issues and pull requests are welcome. Start with
[CONTRIBUTING.md](CONTRIBUTING.md); it covers the build, the quality gates that
CI enforces, and the clean-room policy (no code or assets from other download
managers). Security reports go through [SECURITY.md](SECURITY.md), not public
issues.

## License

MIT — see [LICENSE](LICENSE).

Rilldown is not affiliated with, endorsed by, or derived from Internet Download
Manager, Free Download Manager, JDownloader, XDM, AB Download Manager, Neat
Download Manager, Motrix, Gopeed, aria2, uGet, Persepolis, DownThemAll! or
pyLoad. All product names and trademarks belong to their respective owners and
are used here only to describe interoperability and differences.
