# Security policy

Rilldown downloads files from the network, launches helper processes, and exposes
a loopback bridge to browser extensions. Those boundaries are treated as
untrusted, and reports about them are welcome.

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | Yes |
| Older or unreleased builds (`main`, local builds) | No |

Only the latest published release on the
[releases page](https://github.com/avrojit-ovi/Rilldown-download-manager-/releases)
is supported. If you believe a release is affected, reproduce on that release
before reporting.

## Reporting a vulnerability

Do not open a public issue, pull request, or discussion for a security problem.

Report it privately through GitHub Security Advisories:

1. Open <https://github.com/avrojit-ovi/Rilldown-download-manager-/security/advisories/new>.
2. Describe the issue, the affected version, and the platform (Windows x64 or
   ARM64, Linux x64 or ARM64), and the bundle you installed (NSIS, `.deb`,
   AppImage).
3. Include reproduction steps or a proof of concept, and the impact you expect.

A proof of concept that needs local code execution, a compromised browser
profile, or an already-privileged account is still useful — say which
preconditions it assumes.

This is a small, single-maintainer project: expect a best-effort response rather
than a contractual timeline. Advisories are used for fixes that need a CVE or a
coordinated disclosure. Credit is given in the advisory unless you ask otherwise.

## In scope

- **Download engine** (`crates/engine`): URL scheme and redirect validation,
  range arithmetic and segment planning, filename sanitization and path
  traversal, `.part` file handling, checksum verification, and SQLite state
  parsing.
- **Loopback bridge** (`src-tauri`): bearer-token generation, comparison, file
  permissions of `bridge.json`, request body limits, and the listener binding.
- **Native messaging host** (`crates/native-host`): frame parsing and size
  limits, host-name and extension-id handling, and reading the bridge token from
  disk instead of the message stream.
- **Browser extensions** (`extension`): handling of page-provided URLs and the
  messages sent to the host.
- **Media pipeline** (`crates/media`): argument construction for `yt-dlp` and
  `ffmpeg`, progress parsing, and cleanup of partial files on cancel.
- **Update path**: handling of the updater manifest and signed artifacts when
  the release signing secrets are configured.

## Out of scope

These are known, documented limitations rather than vulnerabilities:

- SmartScreen or “unknown publisher” warnings on the unsigned Windows
  installers. See [docs/release.md](docs/release.md).
- Secrets (cookies, custom headers, proxy URLs, the bridge token) stored
  unencrypted in the local SQLite database and data directory, as described in
  [ARCHITECTURE.md](ARCHITECTURE.md#8-security-boundaries). An attacker who can
  already read your user profile can read them; the OS keychain is not used yet.
- `yt-dlp` and `ffmpeg` being fetched at runtime from their official release
  channels without a pinned digest, as listed in README's “Not in 0.1.0”.
- Anything that requires the extension to be installed by the user, the app to
  be running as the user, or physical access to an unlocked machine.
- Missing hardening with no demonstrated impact, and reports produced only by an
  automated scanner.
- Third-party tools themselves (`yt-dlp`, `ffmpeg`, Chromium, Firefox, WebView2,
  the OS WebKitGTK build). Report those upstream.

## Handling secrets

Never include signing keys, cookies, tokens, or personal download data in a
report. If a proof of concept needs a token, use a value you generated yourself
on a throwaway profile.
