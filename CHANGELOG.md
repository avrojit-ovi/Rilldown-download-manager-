# Changelog

All notable changes to Rilldown are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

No changes yet.

## [0.1.0] - 2026-10-29

First public release. Windows x64 and ARM64 (NSIS), Linux x64 and ARM64
(`.deb`, AppImage).

### Added

- Segmented HTTP(S) downloads with 1–32 connections per file, dynamic splitting
  when a worker finishes, `If-Range` validator checks, capped exponential retry
  with jitter, disk-space checks, and byte-exact final files.
- Crash-safe resume: segment progress is checkpointed to SQLite, interrupted
  downloads recover as paused after a restart, and resume continues from the
  acknowledged byte ranges.
- Single-stream fallback for servers without byte ranges or a known length.
- Named queues with per-queue admission limits, a global concurrency limit, and
  a global or per-download speed ceiling enforced with a token bucket.
- Per-download proxies, cookies, referrer, custom headers, and MD5/SHA-1/SHA-256
  checksum verification.
- Automatic categories derived from the probed filename and content type.
- Desktop UI with live progress, per-download actions, batch add, search,
  category and queue filters, settings, tray menu, close-to-tray, desktop
  notifications, clipboard link detection, and English/Bangla navigation.
- Media grabbing through `yt-dlp` (HLS, DASH, supported sites) with `ffmpeg`
  stream merging; both tools are used from `PATH` when present and otherwise
  downloaded on first use into the app's private data directory.
- Browser capture for Chromium (MV3) and Firefox: extensions, a native-messaging
  host, and an authenticated loopback bridge. Native-host manifests are written
  for installed browsers and the extension is sideloaded.
- Optional signed auto-updates through GitHub Releases when the release signing
  secrets are configured.
- Workspace layout of an engine library and CLI, the media pipeline, the native
  messaging host, the Tauri shell, the React UI, and the extension sources, with
  a local mock-server test suite for transfers, resume, queues, and speed limits.

### Notes

- The v0.1.0 installers are published **unsigned**: Windows SmartScreen will warn
  on first launch, and the Linux packages are not GPG-signed. See
  [docs/release.md](docs/release.md).
- The in-app updater needs `TAURI_SIGNING_PRIVATE_KEY` (and its password) in the
  release secrets. When those are missing the workflow still publishes unsigned
  installers, but no `latest.json` is uploaded and the update check reports that
  it cannot find a manifest.
- Documented limitations of this release: no FTP/FTPS, no mirror URLs, no queue
  schedules or post-queue actions, no start-with-the-OS, no macOS builds, no
  per-connection progress bars, no category folder routing, no pinned digests
  for the downloaded media tools, and no encryption at rest for stored cookies,
  headers, or proxy URLs. See the README's “Not in 0.1.0” section.

[Unreleased]: https://github.com/avrojit-ovi/Rilldown-download-manager-/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/avrojit-ovi/Rilldown-download-manager-/releases/tag/v0.1.0
