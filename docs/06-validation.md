# Validation

## Current reviewed branch — 2026-11-19

The P0/P1 feature and UI work is on `feat/install-and-project-layout`; this review does not merge or publish it. The reviewed implementation passed 370 Rust tests with one existing environment-dependent Bubblewrap test ignored, six installer tests, an offline build, and all-target Clippy with warnings treated as errors. The suite also covers concurrent project/global grant writers, statement/commit rollback during session/branch publication, keyboard/button parity and decision-layer ownership. It includes project-boundary and private-include regressions, corrupt-ancestry rejection, closed-input authorization handling and bounded native-write/restore snapshots. This is local evidence, not a GitHub CI or live-model quality claim.

Local HTTP/CLI fixtures exercised malformed-action repair, native source discovery with shell denied, nested artifact fences, refusal to execute ambiguous replies and exact prepared-message trace exports. Native PTY fixtures exercised permission popup dismissal/reopen and HTTP 429 retry without repeating an earlier tool, changing attempt/branch or losing usage records. Earlier incremental test counts and the completed pre-merge handoff are consolidated here; their original records remain in Git.

## Repeatable automated checks

```bash
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
python tests/test_install.py
cargo run --locked -- validate fix-bug
cargo run --locked -- validate add-feature
cargo run --locked -- validate optimize-performance
```

Use `--offline` as well when dependencies are already available. Rust tests use scripted/local providers; installer tests use local Git repositories and controlled builds. No real provider key is required for these automated checks. Rust sources use standard rustfmt; run `cargo fmt --all -- --check` alongside tests and Clippy.

## Scope and failure coverage

- Native read paths: valid project files/directories, absolute/parent traversal rejection, symlink escapes, private paths, scan bounds and oversized reads.
- Action envelopes: one explicit mapping, bounded corrections, missing required fields, nested literal fences and multiple-action rejection before dispatch.
- Permissions: explicit decisions, scope confirmation, inert Enter/repeats, Esc dismissal without an answer, stale clicks and worker-loss expiry.
- Recovery: session ownership, interrupted native writes, manual drift, partial restores and branch cutoffs.
- Evidence: pinned artifact approvals/versions, model claims versus verification, accounting provenance, unknown billing and exact request dumps.
- TUI: TestBackend layouts, literal Unicode input, drafts/focus, bounded readers, contextual actions and supported/tiny terminal states.

## Environment-dependent and manual checks

Install Bubblewrap with enabled unprivileged user namespaces, then run `greenlight --root PROJECT sandbox-check`. Identify the ignored integration test with `cargo test -- --list --ignored` and run it explicitly on a suitable host; default-suite success does not establish shell isolation on every machine. Shell execution must fail when isolation is unavailable, retain a read-only project/no network/private HOME, and expose no inherited API keys.

Use [the smoke-test guide](17-smoke-test.md) with a disposable project to assess actual terminal behavior and model/provider compatibility. Paid/live-provider runs require the user's chosen credentials and are separate from fixture verification. A successful model-authored report, workflow completion or restore never proves that the full project was validated or rewound.
