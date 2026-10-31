# Contributing to Compyla

Compyla turns markdown into Office documents (docx, pptx, xlsx) and pdf, against a written spec.
Issues and discussions are open to everyone. Code contributions from outside aren't open yet; this
file describes how the work is done, for maintainers and for anyone reading along.

## Where things live

| What | Where |
|------|-------|
| The spec (normative) | [`docs/markdown-spec.md`](docs/markdown-spec.md), with its change log (C-numbers) |
| Architecture decisions | [`docs/architecture.md`](docs/architecture.md) (D-numbers) |
| Phases and status | [`docs/roadmap.md`](docs/roadmap.md), the [milestones](https://github.com/iChintanSoni/compyla/milestones) and the [project board](https://github.com/users/iChintanSoni/projects/9) |
| Diagnostics registry, IR and report schemas | [`spec/`](spec) |
| Conformance suites | [`spec/conformance/`](spec/conformance) ([README](spec/conformance/README.md)) |
| Guides and the contributor handbook | the [wiki](https://github.com/iChintanSoni/compyla/wiki) |

The spec and the decisions stay in the repository, so they are reviewed in pull requests, checked by
CI and versioned with the code. The wiki holds guides that point back to them.

## Issues

Every piece of work is an issue: a bug, a feature, a spec change, a real-viewer finding, a
conformance gap, a task or a docs fix. [Pick the form that fits](https://github.com/iChintanSoni/compyla/issues/new/choose);
questions and early ideas go to [Discussions](https://github.com/iChintanSoni/compyla/discussions).

Triage gives each issue a `type:`, an `area:`, a `priority:`, and a milestone (the phase), and puts
it on the project board. A review finding that isn't fixed in its pull request becomes an issue, so
nothing is left only in a comment.

## The flow for every change

1. **Start from an issue.** Branch from `main`.
2. **Spec first.** A behavior change starts in `docs/markdown-spec.md`: the text, a C-numbered entry
   in the change log, and the diagnostics registry if codes change.
3. **Implement it with tests:** unit and integration tests, and conformance fixtures that pin the
   behavior (generated binaries come from scripts in `spec/tools/`, never hand-made).
4. **Run everything locally:**
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace            # the render suite needs uv
   node spec/tools/check.mjs --schemas
   node spec/tools/oracle.mjs        # needs the prototype built: pnpm install && pnpm build
   ```
5. **Check output in real viewers** when it changes: open it in PowerPoint, Excel or Word, with no
   repair prompt and a clean accessibility check.
6. **Open a pull request** that closes the issue, and fill in its checklist.
7. **Independent review.** Fix every finding, or file it as an issue. Fix commits are reviewed too.
8. **CI green** on the final commit. `main` is protected: changes arrive only through pull requests
   with passing checks, and history stays linear (rebase merges).
9. **Merge**, then move the issue to Done and pick the next.

Nothing moves to the next milestone while a finding, a doubt or an offer from the current one is
still open.

## Engineering rules

- **Determinism:** the same input gives the same bytes on every platform (markdown-spec.md §13).
- **Hostile input:** every reader enforces its limits on the bytes themselves (§12).
- **One core:** the Rust core is shared by every host; hosts stay thin.
- **Commits:** an imperative subject of at most 72 characters; a body saying what changed and why.

## Security

Never report a vulnerability in a public issue: see [SECURITY.md](SECURITY.md).

## Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
