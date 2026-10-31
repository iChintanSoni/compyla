<!-- Every PR closes an issue. Keep the description to what changed and why; the issue holds the background. -->

Closes #

## What and why

## How it was checked

<!-- Tests added or changed, fixtures, and anything checked by hand (e.g. opened in PowerPoint). -->

## Checklist

- [ ] Linked issue above (`Closes #…`); labels and milestone set on it
- [ ] Tests: unit or integration tests, and conformance fixtures for new behavior
- [ ] Ran locally: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `node spec/tools/check.mjs --schemas`, `node spec/tools/oracle.mjs`
- [ ] Spec: behavior changes recorded in `docs/markdown-spec.md` with a C-number, registry and fixtures updated — or N/A
- [ ] Output changed? Opened in real viewers (which), no repair prompt, accessibility checked — or N/A
- [ ] Docs: wiki, README, roadmap or milestone updated — or N/A
- [ ] Independent review done; every finding fixed here or filed as an issue
- [ ] CI green on the final commit
