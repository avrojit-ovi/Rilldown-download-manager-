# Rilldown browser extension

Two builds share one source tree:

- `manifest.chrome.json` — Chromium MV3 service worker (Chrome, Edge, Brave, Opera, Vivaldi)
- `manifest.firefox.json` — Firefox MV3 background script

Build both:

```bash
node extension/build.mjs
```

Outputs land in `extension/dist/chrome` and `extension/dist/firefox`. The build
also validates that every manifest reference exists; CI runs it with `--check`.

## Sideload for development

Chromium:

1. Open `chrome://extensions` and enable Developer mode.
2. Choose **Load unpacked** and select `extension/dist/chrome`.
3. Note the extension ID shown on the card. The desktop installer needs it to
   register the native host allowlist.

Firefox:

1. Open `about:debugging#/runtime/this-firefox`.
2. Choose **Load Temporary Add-on** and select `extension/dist/firefox/manifest.json`.
3. Temporary add-ons disappear when Firefox closes; a signed XPI is needed for
   persistent installation.

## Native host

The extension talks to `app.rilldown.host` over native messaging. Until the
desktop app registers that host manifest for the browser, the popup reports
"Desktop app not running". See [docs/browser-integration.md](../docs/browser-integration.md)
for the exact paths and the in-app installer that writes them.

Chromium will not silently install an extension outside enterprise policy. The
app opens the appropriate store page after registering the host, and offers a
sideload fallback with instructions.
