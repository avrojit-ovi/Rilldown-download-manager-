# Contributing to Rilldown

Thanks for helping. Rilldown is MIT licensed and aims to stay small enough to
audit, fast enough to replace a commercial download manager, and honest about
what it supports.

## Local setup

1. Install Rust stable and Node.js 22+.
2. Install the Tauri 2 system dependencies for your OS (see README).
3. `npm install`
4. `npm run dev`

The engine can be developed without the desktop shell:

```bash
cargo test -p rilldown-engine
cargo run -p rilldown -- --help
```

## Quality gates

A change is ready when all of these pass locally:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run lint
npm test
npm run ui:build
node extension/build.mjs --check
```

New engine behavior needs a test that would fail without the change. Transfers,
resume, and range handling are tested against the local mock server rather than
the public internet.

## Commit style

Conventional Commits, imperative mood, one logical change per commit:

```
feat(engine): split the largest segment when a worker finishes
fix(ui): keep the detail panel readable at 200% zoom
docs: record the Snap native-host path
```

## Clean-room policy

- Use public documentation, help centers, and observable behavior only.
- Do not decompile, patch, trace, or instrument proprietary binaries.
- Do not copy code, icons, strings, installer artwork, or store listing assets.
- AB Download Manager is an Apache-2.0 project; if any of its code or assets are
  ever reused, record the license and attribution. The default is no reuse.
- Add dependencies only when their license is compatible with MIT and record
  anything that is copyleft or requires notices.

## Security

Report vulnerabilities privately to the maintainers rather than in a public
issue. Never commit tokens, signing keys, cookies, or user download data.
Filename handling, redirect validation, and native-messaging input are treated
as untrusted boundaries and need tests.
