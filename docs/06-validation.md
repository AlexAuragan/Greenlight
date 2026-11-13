# Validation status and smoke-test plan

## Evidence recorded through 2026-11-13

- The original v0.1 workflow files and includes were checked syntactically. That is historical validation, **not** a standing claim that every future workflow change is tested.
- After merging PRs #6–#8, the project owner ran `cargo test` on the integrated `master`: **57 passed, 0 failed, 1 ignored**. The ignored Bubblewrap integration test requires installed Bubblewrap and enabled Linux user namespaces. The same checkout passed `cargo clippy -- -D warnings`.
- PR #9 (Greenlight rename) was reported locally as **“All good”** after testing, but full command output was not captured in this record; it is not a claim of an independently observed fresh-checkout or CI result.
- Earlier manual smoke sessions exercised an agent-created artifact, native file edit journal, detected manual drift, guarded restore refusal and a successful history-based restoration on a disposable calculator project. This is evidence from manual testing, **not** a repeatable automated regression suite.
- There were no GitHub-hosted CI check runs reported for those PRs. A complete Linux/Bubblewrap end-to-end run, formal terminal-resize test, injected crash/recovery test and network/provider regression test remain outstanding.

## Repeatable validation on a Rust/Linux machine

```bash
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --locked -- -D warnings
cargo run -- validate fix-bug
cargo run -- validate add-feature
cargo run -- validate optimize-performance
```

If `cargo fmt --check` reports differences, run `cargo fmt` to normalize source formatting. This does not alter the workflow YAML files.

With `DEEPINFRA_API_KEY` configured:

```bash
cargo run -- models
cargo run -- start fix-bug --goal 'Investigate why my test suite fails'
```

For a first or repeated live session, use a disposable project fixture and keep tool policies at `ask`. Record which approvals, artifacts, workspace changes, blocked states and restores actually occurred; model output varies across runs. See the [calculator smoke-test exercise](17-smoke-test.md). Do not claim full end-to-end validation from Rust unit tests alone.

## Security checks before wider use

- Install Bubblewrap and run `cargo run -- sandbox-check`; the sandbox must refuse to start when user namespaces are unavailable.
- Verify sandboxed commands cannot write project source files and do not inherit `DEEPINFRA_API_KEY`, the host home directory or its SSH credentials.
- Check that pre-approved shell commands still run with network isolation; use native write for project edits.
- Project source files remain readable; masking known secret filenames is not comprehensive secret scanning. Avoid embedding secrets in source.
- Test Ctrl-C during a sandboxed process and verify descendants terminate.
- A successful restore does not certify files modified by processes outside the harness.
