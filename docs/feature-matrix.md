# Feature matrix

Rilldown 0.1.0 against three well-known download managers. Competitor columns
summarize public documentation and observable behavior as of October 2026;
vendor features change, so verify before making a purchasing decision.

The README carries a wider comparison against fourteen download managers
(IDM, FDM, JDownloader 2, XDM, AB Download Manager, Neat, Motrix, Gopeed, aria2,
uGet, Persepolis, DownThemAll! and pyLoad). This file keeps the deeper
capability-by-capability detail.

Legend: **Yes** = supported, **Partial** = limited, **No** = not offered,
**—** = not applicable.

| Capability | IDM | AB Download Manager | Neat Download Manager | Rilldown 0.1.0 |
|---|---|---|---|---|
| Open source | No | Yes (Apache-2.0) | No | **Yes (MIT)** |
| Windows support | Yes | Yes | Yes | **Yes**, x64 + ARM64 (unsigned installers) |
| Linux support | No | Yes | Yes | **Yes**, x64 + ARM64 (`.deb`, AppImage) |
| macOS support | No | Yes | Yes | **No** (not a 0.1.0 target) |
| HTTP/HTTPS multi-connection | Yes (up to 32) | Yes | Yes | **Yes** (1–32) |
| Dynamic segment splitting | Yes | Yes | Yes | **Yes** |
| Resume after crash/restart | Yes | Yes | Yes | **Yes** |
| Range/validator safety (`ETag`, `If-Range`, length) | Yes | Partial | Partial | **Yes** |
| Unknown-length single-stream fallback | Yes | Yes | Yes | **Yes** |
| FTP / FTPS | Yes | No | No | **No** |
| HTTP/2 | Partial | Yes | Partial | **Yes** (`reqwest` h2) |
| Redirect handling | Yes | Yes | Yes | **Yes** (10-hop, HTTP(S) only) |
| Named queues + concurrency limits | Yes | Yes | Partial | **Yes** |
| Time/weekday scheduler + post-queue actions | Yes | Yes | Partial | **No** |
| Global + per-download speed limit | Yes | Yes | Yes | **Yes** (token bucket) |
| Aggregate queue speed limit | Yes | Yes | Partial | **No** (field reserved) |
| Proxy (HTTP/SOCKS5) | Yes | Yes | Yes | **Yes** (per download / settings) |
| Cookies, referrer, custom headers | Yes | Yes | Partial | **Yes** |
| Saved credentials in OS keychain | Yes (Windows) | Partial | Partial | **No** (stored in local SQLite) |
| Auto-retry with backoff | Yes | Yes | Yes | **Yes** (capped + jitter) |
| Mirror / alternate URLs | Yes | Partial | No | **No** (field reserved) |
| Smart filename detection | Yes | Yes | Yes | **Yes** (`Content-Disposition`, extension fixups) |
| Duplicate filename handling | Yes | Yes | Yes | **Yes** |
| Pre-allocation | Yes | Yes | Yes | **Yes** (sparse `set_len`) |
| Checksum verification | Partial | Partial | No | **Yes** (MD5/SHA-1/SHA-256) |
| Categories + filtering | Yes | Yes | Partial | **Yes** (labels/filters; no folder routing) |
| Batch URL list | Yes | Yes | Partial | **Yes** |
| Clipboard monitoring | Yes | Yes | Yes | **Yes** (opt-in, prompt) |
| Drag-and-drop URLs | Yes | Yes | Yes | **No** |
| Site grabber | Yes | No | No | **Yes** via yt-dlp |
| HLS / DASH capture | Yes | Partial | Partial | **Yes** via yt-dlp |
| ffmpeg muxing | Yes | Partial | Partial | **Yes** (copy, no re-encode) |
| yt-dlp integration | No | No | No | **Yes** |
| Browser extension (Chromium) | Yes | Yes | Yes | **Yes**, sideloaded MV3 |
| Browser extension (Firefox) | Yes | Yes | Yes | **Yes**, MV3 source |
| Native messaging host | Yes | Yes | Yes | **Yes** |
| In-app native-host setup | Yes | Partial | Yes | **Yes** |
| Silent extension install | No (uses policy) | Partial | Partial | **No** (store/sideload only) |
| Enterprise policy installer | Yes | Partial | Partial | **No** (documented manual path) |
| System tray | Yes | Yes | Yes | **Yes** |
| Start with OS / single instance | Yes | Yes | Yes | **Single instance yes; autostart no** |
| Desktop notifications | Yes | Yes | Yes | **Yes** |
| Dark/light/system theme | No | Yes | Partial | **Yes** |
| English + Bangla UI | Partial | Yes | Partial | **English fully; Bangla for the shell and core actions** |
| Per-connection progress display | Yes | Yes | Yes | **Partial** (engine emits segments; UI shows connection count) |
| Signed auto-update | Yes | Yes | Yes | **Yes**, when release secrets are configured |
| Telemetry | Vendor-specific | Opt-in | Vendor-specific | **None** |

## Notes

- “Unsigned installers” means Windows SmartScreen will warn until an Authenticode
  certificate is configured in CI. The Linux packages are not GPG-signed.
- IDM's browser integration historically installs with broad permissions;
  Rilldown does not install an extension silently.
- AB Download Manager is Apache-2.0 and is used only as a UX reference. No code
  or assets were copied.
- “Field reserved” means the database/spec carries the value but the engine does
  not act on it yet. The UI does not offer it as a working feature.
