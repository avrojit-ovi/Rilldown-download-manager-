# Rilldown architecture

This document describes the code that ships in 0.1.0. Anything labelled planned
is not implemented yet.

## 1. Goals and non-goals

Goals:

- A UI-free Rust engine that is testable, embeddable, and usable from a CLI.
- Crash-safe, resumable, dynamically segmented transfers over HTTP(S).
- A Tauri 2 desktop shell for Windows and Linux.
- Browser capture through native messaging, with a secured loopback bridge.
- Honest integrations: no silent extension installs, no invented status.

Non-goals:

- A torrent client.
- A media transcoder. Media streams are remuxed by `ffmpeg` when needed, not
  re-encoded by Rilldown.
- Cloud sync or user accounts.
- Copying behavior, assets, or code from other download managers.

## 2. Process model

```
┌──────────────────────────────┐        ┌───────────────────────────┐
│ Tauri window (WebView)       │        │ Browser extension         │
│ React UI                     │        │ (Chromium MV3 / Firefox)  │
└──────────────┬───────────────┘        └─────────────┬─────────────┘
               │ Tauri commands/events                │ native messaging
┌──────────────▼───────────────┐        ┌─────────────▼─────────────┐
│ rilldown-app (Rust)          │        │ rilldown-native-host      │
│ - window, tray, dialogs      │        │ - length-prefixed JSON    │
│ - settings, host installer   │        │ - narrow request set      │
│ - engine supervisor          │        └─────────────┬─────────────┘
└──────────────┬───────────────┘                      │ loopback HTTP + token
               │ in-process API                       │ (bridge.json, 0600)
┌──────────────▼──────────────────────────────────────▼─────────────┐
│ rilldown-engine                                                    │
│ - download manager, queues, admission gates, speed limiter         │
│ - probe/segment workers (tokio)                                    │
│ - SQLite store (single connection behind a mutex)                  │
│ - broadcast event stream                                           │
└──────────────────────────────┬─────────────────────────────────────┘
                               │
                    ┌──────────▼──────────┐
                    │ filesystem          │
                    │ sparse .part + file │
                    └─────────────────────┘
```

The engine is the only component that touches download state. The native host
never opens the database; it forwards one request at a time to the running app
over the loopback bridge.

## 3. Crate responsibilities

### `crates/engine`

- `Engine` — add/pause/resume/cancel/remove, list, queues, settings, global
  limits, event subscription.
- `DownloadSpec` / `DownloadRecord` / `DownloadProgress` / `EngineEvent` — the
  serializable contracts used by the shell, CLI, and tests.
- `Store` — SQLite persistence and versioned migrations.
- `scheduler::ConcurrencyGate` — dynamic admission limits that can be raised or
  lowered while transfers wait.
- `throttle::RateLimiter` — a token bucket shared by every connection of a
  download, plus an optional engine-wide bucket.
- `rilldown` binary — a CLI over the same API.

The engine has no Tauri or UI dependency.

### `crates/media`

- URL/content-type detection and an HLS master-playlist parser used by tests and
  browser capture.
- Tool discovery for `ffmpeg` and `yt-dlp`: `PATH` first, then the managed tools
  directory under the app data directory.
- First-use downloads from pinned release URLs, archive extraction, executable
  permissions, and version probing.
- Media inspection (`yt-dlp -J`) and grabbing with structured progress events
  and cancellation.

### `crates/native-host`

- Browser native-messaging framing: a 4-byte native-endian length followed by
  UTF-8 JSON, with a 1 MiB ceiling.
- A narrow method set: `ping`, `status`, `download`/`add_url`.
- Reads `bridge.json` on every request; never caches or logs the token.
- Returns structured errors when the app is not running.

### `src-tauri`

- Window, tray, single instance, notifications, clipboard monitor.
- Tauri commands wrapping the engine, settings, browser host installer, tools,
  media jobs, and updater.
- Loopback bridge: random port, 256-bit bearer token, 128 KiB body cap, token
  file written with mode `0600`.
- Native-host manifest installation for Chrome, Chromium, Brave, Edge, and
  Firefox.
- Updater commands backed by the Tauri updater plugin and a signed manifest on
  GitHub Releases.

### `ui`

- React 19 + TypeScript + Vite + Tailwind v4.
- Zustand stores for view state and mirrored engine/media state.
- i18next with English and Bangla resources; newer dialog copy falls back to
  English when a Bangla key is not present.
- Vitest + Testing Library.

## 4. Download state machine

```
Queued ──► Probing ──► Downloading ──► Verifying ──► Completed
  │           │             │              │
  │           └─────────────┴──────────────┴──► Failed
  ├──────────────────────────────────────────► Cancelled
  └──────────────────────────► Paused ◄───────┘
```

Notes:

- A queued download waits for both a global and a queue admission slot. The
  slot is held only while the transfer runs.
- Pausing cancels the workers after the current chunk and checkpoints segment
  progress. Resuming re-probes and validates the remote ETag/length.
- A changed validator or length discards partial data and restarts rather than
  mixing bytes.
- A checksum failure keeps the partial file for inspection and marks the
  download failed; retrying restarts from zero.
- Terminal states are idempotent.

## 5. Transfer algorithm

1. **Probe.** `HEAD` first; fall back to `GET` with `Range: bytes=0-0`. Record
   the final URL, length, content type, `Accept-Ranges`, `ETag`,
   `Last-Modified`, and `Content-Disposition`. A zero-length `HEAD` without
   range support is confirmed with a range request before being treated as an
   empty file.
2. **Plan.** With ranges and a length, split `[0, len)` into up to
   `min(connections, len / 64 KiB)` segments. Otherwise use one streaming
   response and mark the record non-resumable.
3. **Transfer.** All workers write into one sparse `.rilldown/<id>.part` file at
   absolute offsets. Requests carry `If-Range` when a validator exists.
   Responses must be `206` with the requested start (or a whole-file `200` when
   the segment covers the file).
4. **Dynamic split.** When a worker finishes, the board halves the largest
   unfinished segment if both halves keep at least 64 KiB of work. The worker
   takes the new half. This keeps connections busy without tiny requests.
5. **Checkpoint.** A ticker persists the record and segment rows every 400 ms;
   pause/cancel/error paths checkpoint synchronously.
6. **Finalize.** Optional MD5/SHA-1/SHA-256 verification, `fsync`, then an
   atomic rename from the sparse part file to the destination.
7. **Retry.** Network errors, incomplete ranges, and retryable HTTP statuses use
   capped exponential backoff with jitter. A retried segment resumes at its
   current offset.

## 6. Persistence

SQLite in WAL mode with a single connection behind a mutex. Each checkpoint is a
short synchronous transaction; async workers never hold the database lock across
an await point. `PRAGMA user_version` tracks ordered SQL migrations.

Tables: `downloads`, `segments`, `queues`, `categories`, `settings`. Timestamps
are UTC milliseconds. Download headers and mirrors are stored as JSON and parsed
defensively. Foreign keys are enabled and deleting a download cascades to its
segments.

Crash safety: at most the last checkpoint window is re-fetched on resume.

## 7. Queues and rate limiting

Every queue has `max_concurrent` (0 means “global limit only”) and an optional
stored speed limit. Queue speed limits are persisted for the UI but are not yet
applied as an aggregate queue ceiling; per-download and global limits are
enforced. Admission uses `ConcurrencyGate`, which supports live limit changes and
wakes waiters. The global limiter is a shared token bucket; each download may add
its own bucket. Both are consulted per chunk, so effective throughput is the
lower limit.

Schedules and post-queue actions are not implemented.

## 8. Security boundaries

- **Loopback bridge.** Binds `127.0.0.1` on an ephemeral port. The token is 256
  random bits, compared in constant time, and published only in
  `<data-dir>/bridge.json` with mode `0600`. Requests without
  `Authorization: Bearer …` are rejected with 401. Request bodies are capped at
  128 KiB. The bridge has no CORS headers and no non-loopback listener. It does
  not check `Origin`, because the native host is not a browser origin.
- **Native messaging.** The browser enforces the manifest allowlist. Host name
  and Chromium extension id are fixed (`app.rilldown.desktop`,
  `bkcbdfegbgmhalklgbhjgdefopbglmif`); Firefox uses
  `rilldown@rilldown.com`. The host enforces the 1 MiB frame limit and reads the
  bridge token from disk rather than accepting it from the extension.
- **Downloads.** URLs are restricted to HTTP(S); redirects are limited to 10 and
  must stay on HTTP(S). Filenames are sanitized, path traversal is rejected, and
  completed files are created with a collision-safe name.
- **Secrets.** Cookies, headers, proxy URLs, and the bridge token live in local
  user files. They are not sent anywhere except the download request they belong
  to. They are not encrypted at rest, and the OS keychain is not used yet.

## 9. Media pipeline

The media dialog asks `yt-dlp` for metadata (`-J`), presents the formats, and
starts a supervised process with a machine-readable progress template. Progress
is parsed into events and mirrored into the UI's media list. Cancellation kills
the process tree and removes the partial path file. `ffmpeg` is passed via
`--ffmpeg-location` so the managed copy is used when there is no system install.
Both tools are downloaded on first use into `<data-dir>/tools`; the URLs are
pinned release channels, not user input.

Media jobs are kept in memory for the session; completed files are ordinary
files on disk. There is no media history persisted across restarts.

## 10. Testing

- Unit tests: filename handling, range planning, dynamic splitting, retry
  backoff, token buckets, categories, SQLite round-trips, host framing and
  bridge JSON, media playlist parsing, progress parsing.
- Engine integration tests: an in-process axum server with deterministic
  payloads, range support, flaky responses, slow streams, unknown lengths, and
  content disposition. Assertions cover byte-exact output, resume, checksum
  failure, queue admission, and measured speed limiting.
- UI tests: shell rendering, dialogs, filters, event-store updates, and error
  handling with a mocked Tauri boundary.
- Manual/platform checks: tray, notifications, single instance, browser host
  registration, and installer behavior. Windows installers are produced by the
  release workflow; the GUI cannot be meaningfully exercised under Wine because
  WebView2 is not available there.

## 11. Packaging and updates

Targets: Windows x64 and ARM64 (NSIS), Linux x64 and ARM64 (`.deb` and AppImage).
Windows and Linux installers are unsigned unless certificates are configured.
The updater uses a minisign key generated by `tauri signer`; the public key is in
`src-tauri/tauri.conf.json` and the private key must live only in GitHub Actions
secrets. The release workflow builds all four targets, signs updater artifacts,
uploads them to a draft GitHub Release, and writes `latest.json`.

The sidecar binary (`rilldown-native-host`) is staged with
`scripts/prepare-sidecar.mjs` because Tauri requires target-triple-suffixed
external binaries.
