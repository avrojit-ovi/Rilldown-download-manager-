## What changed

<!-- One or two sentences. Link the issue this closes, if any. -->

## Why

<!-- The problem this solves, or the behaviour it fixes. -->

## How it was verified

<!-- Paste the commands you ran and their result. Remove the ones that do not apply. -->

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

## Checklist

- [ ] New engine behaviour has a test that fails without this change.
- [ ] User-facing changes are reflected in the README, `docs/`, or the changelog.
- [ ] No competitor code, icons, strings, or installer assets were copied.
- [ ] Commits follow Conventional Commits and one logical change each.
