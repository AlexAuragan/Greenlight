# Validation

## GL-008 idle-delivery slice — 2026-11-22

The existing `feat/project-execution-coordination` branch now also contains explicit idle manager delivery, retaining one pending branch. The suite passes 467 Rust tests (two existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. Tests use scripted/local providers and no paid API calls.

New coverage checks exact whitespace/Unicode originals in only the target step; separate FIFO IDs for identical text; stale branch/workflow/artifact rejection; withdrawal; no permission/input-gate bypass; non-executing restart proposals; unresolved/unreachable guidance; atomic attempt binding and prepared-envelope inclusion; saved retries without duplication; withdrawal preventing a saved request from replaying the instruction; and Help-safe TUI controls.

A native PTY/local HTTP scenario asks from the CLI, verifies that asking queued nothing, selects the manager proposal in the TUI and uses **g** to start the blocked current target. Its persisted coding envelope contains the exact original once; the initial attempt did not contain it. Reaccepting via CLI starts nothing. Three central dispatch records account for initial coding, manager inference and routed coding. Confirmed restart binding and live delivery are still unimplemented; GL-008 remains open.

## GL-028 / GL-029 — 2026-11-22

`feat/project-execution-coordination` implements the project lease and fixes the refresh-generation race; this validation does not merge the branch. The suite passes 454 Rust tests with the existing Bubblewrap integration and manual profiler ignored. Strict all-target Clippy, formatting, offline build and six installer tests pass. The original engine-backed diagnostic acceptance test also passed 20 consecutive runs with its assertion unchanged; a deterministic cache-generation regression covers the previously missing blocker.

New tests cover competing stores/threads/processes, independent projects, no loser instruction/status/usage changes, crash and maintenance-unwind recovery, legacy live/multiple owners, atomic claim/cleanup failure, retained drafts/goals and Help-safe owner navigation. Existing continuation, manager, native-intent and restore checks remain enabled.

A native PTY/local HTTP scenario holds a TUI at an HTTP 429 decision, rejects a competing CLI start with zero extra provider calls, retains its goal as a Ready session, and lets a second TUI browse that session and open the owner. The same token remains held through the paused request and explicit retry; earlier tools are not repeated, the original attempt is retained, and cleanup removes the project row. These fixtures use a fake local key and make no paid provider requests. Sandbox isolation was not reconfigured or revalidated on a different host.

## Merge validation — 2026-11-22

The GL-003/GL-005 implementation and GL-007 design stack is merged into `master`. Formatting, strict all-target Clippy, offline build and six installer tests pass. The first Rust suite run passed 436 tests and failed one diagnostic-presence assertion in the engine-backed workflow UI test; that test passed in isolation and the subsequent complete suite passed 437 tests (two ignored). GL-029 records the intermittent failure for investigation; no runtime change or assertion weakening was made to obtain the rerun.

## Current follow-up — 2026-11-21

`perf/bounded-reads-and-refresh` implements GL-005 on top of the unmerged GL-003 descriptor-access branch. Local validation passes 437 Rust tests, six installer tests, formatting, an offline build and all-target Clippy with warnings denied. Two default-suite tests are ignored: the environment-dependent Bubblewrap integration and the manual refresh/diff profiler. The profiler was run explicitly and passed; its fixture and timings are in [resource budgets](40-resource-budgets.md).

New regressions exercise source/include byte caps, unchanged row/body reuse, external commits and rollback isolation, sparse oversized artifact metadata, aggregate/count export limits, over-limit trace refusal and labelled history previews with one exact selected body. A native PTY/local HTTP fixture still passes HTTP 429, Escape-without-decision and exact-request retry, with one tool execution, one attempt/branch and three accounted dispatches. These checks make no paid provider requests. No merge is part of this validation.

## Earlier review — 2026-11-19

The P0/P1 feature and UI work is on `feat/install-and-project-layout`; this review does not merge or publish it. The reviewed implementation passed 379 Rust tests with one existing environment-dependent Bubblewrap test ignored, six installer tests, an offline build, and all-target Clippy with warnings treated as errors. The suite also checks running-output column proportions, one-line step/event rows, mouse-to-output selection, narrow/expanded layouts and stream isolation across sessions. It checks credential-source precedence, masking and frozen worker credentials (including actual HTTP Authorization after an environment change), plus OpenAI-compatible HTTP request shape, safe detail-wrapped provider rejection reasons and legacy failure compatibility, plus concurrent project/global grant writers, statement/commit rollback during session/branch publication, keyboard/button parity and decision-layer ownership. It includes project-boundary and private-include regressions, corrupt-ancestry rejection, closed-input authorization handling and bounded native-write/restore snapshots. This is local evidence, not a GitHub CI or live-model quality claim.

Local HTTP/CLI fixtures exercised malformed-action repair, native source discovery with shell denied, nested artifact fences, refusal to execute ambiguous replies and exact prepared-message trace exports. A provider wire audit exercised nine sequential model POSTs with one request in flight, plus exact physical request/usage counts for unknown, explicit retryable and capacity 429 responses. Native PTY fixtures exercised permission popup dismissal/reopen and HTTP 429 retry without repeating an earlier tool, changing attempt/branch or losing usage records. Earlier incremental test counts and the completed pre-merge handoff are consolidated here; their original records remain in Git.

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
