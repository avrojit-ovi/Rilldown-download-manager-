# Browser integration

Rilldown captures links through a WebExtension and a native-messaging host. The
extension never opens the download database; it sends a narrow JSON request to
`rilldown-native-host`, which forwards it to the running app over an authenticated
loopback bridge.

## Components

| Piece | Role |
|---|---|
| `extension/` | Chromium MV3 and Firefox MV3 builds from one source tree |
| `crates/native-host` | stdin/stdout framing, request validation, bridge client |
| `src-tauri/src/browser_host.rs` | writes native-host manifests, reports state |
| `src-tauri/src/bridge.rs` | 127.0.0.1 HTTP listener with a bearer token |

## Identifiers

These are fixed, not discovered at runtime:

- Native host name: `app.rilldown.desktop`
- Chromium extension id: `bkcbdfegbgmhalklgbhjgdefopbglmif`
- Firefox add-on id: `rilldown@rilldown.com`

The Chromium id is pinned by the public `key` in
`extension/manifest.chrome.json`. Do not regenerate that key unless you also
update `CHROME_EXTENSION_ID` in `src-tauri/src/browser_host.rs` and the extension
manifest, then reinstall the native host.

## Build the extension

```bash
node extension/build.mjs
```

Outputs:

- `extension/dist/chrome` — load this folder unpacked in Chrome/Chromium/Edge/Brave.
- `extension/dist/firefox` — load `manifest.json` temporarily from
  `about:debugging#/runtime/this-firefox`, or build a signed XPI for distribution.

The desktop bundle includes `extension/dist` as a resource, so **Settings →
Browser integration → Open extension folder** points at the Chrome build.

## Install the native host

Open **Settings → Browser integration** and press **Install bridge**. Rilldown
writes a manifest only for browsers whose profile directory already exists:

| Browser | Linux manifest directory | macOS manifest directory |
|---|---|---|
| Chrome | `~/.config/google-chrome/NativeMessagingHosts/` | `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/` |
| Chromium | `~/.config/chromium/NativeMessagingHosts/` | `~/Library/Application Support/Chromium/NativeMessagingHosts/` |
| Brave | `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/` | `~/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/` |
| Edge | `~/.config/microsoft-edge/NativeMessagingHosts/` | `~/Library/Application Support/Microsoft Edge/NativeMessagingHosts/` |
| Firefox | `~/.mozilla/native-messaging-hosts/` | `~/Library/Application Support/Mozilla/NativeMessagingHosts/` |

On Windows, Rilldown writes the host manifest into its data directory and adds
the per-user registry keys:

```
HKCU\Software\Google\Chrome\NativeMessagingHosts\app.rilldown.desktop
HKCU\Software\Chromium\NativeMessagingHosts\app.rilldown.desktop
HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\app.rilldown.desktop
HKCU\Software\Microsoft\Edge\NativeMessagingHosts\app.rilldown.desktop
```

The manifest is a Chromium-style `allowed_origins` file:

```json
{
  "name": "app.rilldown.desktop",
  "description": "Rilldown download bridge",
  "path": "/absolute/path/to/rilldown-native-host",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://bkcbdfegbgmhalklgbhjgdefopbglmif/"]
}
```

Firefox gets `allowed_extensions: ["rilldown@rilldown.com"]` instead.

The host binary must exist beside the app. Release bundles ship it as a Tauri
sidecar; a development build is at `target/debug/rilldown-native-host`. If the
binary is missing, the installer reports that instead of writing a broken
manifest.

## Sideloading

Chromium cannot install an extension silently outside enterprise policy. After
installing the bridge, open `chrome://extensions`, enable Developer mode, choose
**Load unpacked**, and select the Chrome folder. The fixed id means the host
allowlist already matches.

Firefox: `about:debugging#/runtime/this-firefox` → **Load Temporary Add-on** →
`extension/dist/firefox/manifest.json`. Temporary add-ons disappear when Firefox
closes; publish a signed XPI for persistent use.

## How a capture flows

1. The extension sends a `download` request with the URL and an optional
   referrer over native messaging.
2. The native host validates the request, reads `<data-dir>/bridge.json`, and
   POSTs it to `127.0.0.1:<port>/v1/downloads` with the bearer token.
3. The app builds a normal `DownloadSpec` using the saved settings (folder,
   connections, proxy, user agent) and starts it through the engine.
4. The host returns `{ok: true, download: {id, filename, state}}` or a structured
   error.

`status` checks the bridge each time, so the popup tells the truth after the app
is closed or restarted. `ping` only proves that the native host process started;
it is not used as a connection indicator.

## Sandboxed browsers

Snap and Flatpak browsers run in a mount namespace and cannot see the normal
per-user manifest paths. Rilldown 0.1.0 writes only the native paths above and
does not claim support for sandboxed browsers. If you use Snap/Flatpak Firefox
or Chromium, install the extension in a non-sandboxed build or use the app's
normal add-download flow.

## Security

- The bridge binds only to `127.0.0.1`, uses an ephemeral port, and requires a
  256-bit token published in `<data-dir>/bridge.json` with mode `0600`.
- Tokens rotate on every app start. Stale tokens get HTTP 401.
- Request bodies are capped at 128 KiB; the native host caps frames at 1 MiB.
- The host reads the token from disk. The extension never sees it.
- Cookies/headers sent with a capture are stored in the local download record so
  the transfer can resume; they are not encrypted at rest.
- No telemetry or remote logging is involved.
