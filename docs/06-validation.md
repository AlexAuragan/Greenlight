# Validation

## Isolated offline verification (GL-027) — 2026-11-23

Implemented on `docs/sandbox-verification-plan`, pending merge. Full parallel validation passes **568 Rust tests** in 8.22 seconds, with five live-environment/profiling tests ignored. Strict all-target Clippy, formatting, offline build and six installer tests pass. No paid provider calls or host namespace configuration changes were used.

Fifteen added passing regressions cover private/generated input filtering, symlink/size/cancellation refusal, executable flags, generated edits versus check status, selected journal imports and repeated selections, full-input/manual drift, branch scope, unsupported binary/deletion/mode changes, edit-retention overflow, copy/cache retention during quarantine, independent recipe-scoped approval, historical fingerprint binding and changed-recipe refusal, CLI failure/no-provider dispatch, cleanup propagation through any-of verification/cache, and host-generated diffs staying out of final-report selection. The added successful synthetic-Git/formatter live integration is ignored on this host.

A production CLI smoke check against a temporary project/seeded session also returned exit 1 for unavailable preflight in 0.054 seconds; `check-show` read its exact evidence, `check-import --yes` refused it, and live source stayed unchanged.

The real profile test on this cloud host reports exact Bubblewrap startup denial before authorization or source capture; it does not run the configured check or modify live source. Successful namespace isolation, synthetic Git/pre-commit/venv execution and cold/warm timing still require a suitable host (GL-035). Separate dependency provisioning is GL-034; CPU/PID/disk quotas remain GL-023. See [runtime behavior and controls](54-isolated-verification.md).

## Sandbox lifecycle foundation (GL-033) — 2026-11-23

Implemented on `docs/sandbox-verification-plan`, pending merge. The full parallel Rust suite passes **553 tests** in 8.70 seconds, with four existing live-integration/profiling tests ignored and no manual helper termination during this final run. Strict all-target Clippy, formatting, offline build and all six installer tests pass. No paid provider requests or host namespace configuration changes were used.

Seven added regressions cover session-escaped pipe holders after startup failure, future-drop cleanup without killing unrelated children, parent-process exit without Rust destructors, bounded cleanup with a paused supervisor, unused-command records, locked/unconfirmed job evidence and project-owner release/recovery quarantine. Existing output-cap, cancellation, partial-launch, per-stage exit and successful-head tests now run through the dedicated supervisor as well.

The production binary's `sandbox-check` on a fresh temporary project fails closed in 0.027 seconds with the actual `bwrap: setting up uid map: Read-only file system` diagnostic. Its job record confirms cleanup completed. This validates the host's startup-denial/cleanup path, **not successful namespace isolation**. Live successful isolation tests remain ignored here and must run on a suitable Linux host. Writable verification workspaces/capability feedback (GL-027) and resource quotas (GL-023) remain open. See [sandbox lifecycle](10-shell-sandbox.md#process-lifecycle-and-cleanup).

## Reviewed feature stack merged — 2026-11-23

The pending summary/inspection/supervised-review stack and its review fix are merged into `master`. Review covered role/budget dispatch and retry boundaries, session grant scope, immutable artifact publication and provenance, branch-visible journal/evidence readers, restart/restore guards, and structured inspection classification/execution. One edge case was fixed: an artifact version or comment can commit before project-ownership cleanup fails, so the UI now avoids falsely claiming that nothing was saved and retains the recovery draft. A deterministic injected-cleanup regression covers both versions and comments.

Final validation: 546 Rust tests pass in a full serial run (20.66 seconds), with four existing integration/profiling tests ignored. Strict all-target Clippy, formatting, offline build and six installer tests pass. No paid provider calls were used. Merge was conflict-free and preserves the reviewed source tree. Earlier per-feature entries below retain their original branch/status descriptions as historical records.

Known deferred limitations remain explicit: GL-027 sandbox practicality and GL-033 startup-helper/process-tree cleanup. This cloud host cannot establish successful live Bubblewrap namespace isolation; passing scripted/lifecycle tests is not that proof. Existing restoration partial-failure and estimate-based token/cost limits remain as documented.

## Model roles and session budgets (GL-017) — 2026-11-23

Implemented on `feat/optional-summaries`, pending merge. A full serial run passes 545 Rust tests in 21.21 seconds (four existing integration/profiling tests ignored), alongside strict all-target Clippy, formatting, offline build and six installer tests. Twelve new tests cover workflow/verification/manager/summary role precedence, first-request token/unpriced-cost denial without phantom dispatches, retry and side-purpose budget sharing, independent-connection reservation contention, cross-branch accounting, cached-input subset semantics, maximum context-rate reservations, unknown/foreign-currency/tier cost handling, old ledger JSON, empty/invalid/default configuration, exact saved-message continuation after budget-only changes and TUI budget blockers/status. Scripted providers make no paid calls. See [model roles and session budgets](52-model-roles-and-session-budgets.md).

The parallel run reached the known GL-033 Bubblewrap helper stall in the unrelated project-inspection test. Terminating only its orphaned test helper allowed all 545 tests to finish in 188.62 seconds; that intervention is not evidence of automatic sandbox cleanup. The subsequent full serial run needed no intervention. GL-033 remains deferred; no sandbox bypass was added.

## Optional grouped native edit review (GL-016) — 2026-11-23

Implemented on `feat/optional-summaries`, pending merge. The full suite passes 533 Rust tests (four existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. Seven new tests cover multi-file attempt/step grouping and retry separation, discontinuous-chain per-write fallback, rerun/restore/unattributed provenance, fork/source-branch and session isolation, frozen read-only snapshots, missing/empty/over-budget evidence, real TUI keys and narrow/normal rendering, full selected-group export, cached idle refresh, explicit reopening, scope-change closure and permission precedence. A lookup index is added on existing attempt columns; no new journal/staging schema or body snapshots are introduced. No paid provider calls were used. See [grouped native edit review](51-grouped-native-edit-review.md).

The earlier full default-parallel run passed 533 tests in 8.40 seconds. A final run stalled in the existing project-inspection test: a pre-namespace Bubblewrap helper survived failed UID-map setup with stdio still open. After explicitly terminating only that owned test helper, the run completed with 533 passes in 118.83 seconds. This was manual test cleanup, not proof of automatic containment. GL-033 tracks the deferred process-tree/capture-drain defect; no runtime sandbox bypass or cleanup workaround was added to this feature.

## Diff hunks and verification evidence (GL-015) — 2026-11-23

Implemented on `feat/optional-summaries`, pending merge. The full suite passes 526 Rust tests (four existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. Seven new tests cover exact UTF-8 hunk ranges, real Previous/Next/full-file actions and narrow/normal rendering, selected/full export scope and file-cycle reset, private TUI export without history changes, historical artifact bodies and correlated tool requests, branch/session isolation, stale selection, missing blobs, event/reference/JSON-escaping limits, empty/legacy histories and symlink export-directory refusal. The CLI help exposes `verification-evidence SESSION`. No provider calls or fresh verification commands were used. See [diff hunks and evidence](50-diff-hunks-and-verification-evidence.md).

## Artifact comments and user revisions (GL-014) — 2026-11-23

Implemented on `feat/optional-summaries`, pending merge. The full suite passes 519 Rust tests (four existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. Seven new tests cover original preservation and user/source attribution, version/branch-scoped comments, stale/latest and active-owner rejection, no-op/size bounds, rollback of failed attribution publication, real TUI shortcut/actions with an external-editor fixture, symlink/failed-editor rejection and recoverable conflict drafts. No paid provider calls were used. See [artifact review](49-artifact-review.md).

Initial suite attempts stalled in the existing project-inspection policy test; the isolated test and a subsequent full default-parallel suite passed. No sandbox fallback or production change was made to bypass that test. The previously recorded host restriction on live Bubblewrap integration remains.

## Session-scoped saved permissions (GL-013) — 2026-11-23

Implemented on `feat/optional-summaries`, pending merge. The full suite passes 512 Rust tests (four existing integration/profiling tests ignored). Strict all-target Clippy, formatting, offline build and six installer tests pass. New and extended checks cover same-session reload, session/project separation, exact shell matching, configured deny precedence, confirmed/cancelled session grants, disconnected confirmation saving nothing, and session rows in 80×24/120×36 permission popups. Session grants reuse anchored reads, scope locks and atomic saves; they do not change sandbox authority. No paid provider calls were used. See [saved approvals](08-persistent-approvals.md).

## Safe read-only pipelines (GL-032) — 2026-11-23

Implemented on `feat/optional-summaries`, pending merge. The full suite passes 510 Rust tests, with four integration/profiling tests ignored. Strict all-target Clippy, formatting, offline build and six installer tests pass. New coverage checks complete-plan classification, stdin-only roles, malformed/unsafe whole-command fallback, stable shared snapshots, per-stage statuses, finite head draining without guessing from exit 141, bounded stdout/combined stderr, cancellation/deadline cleanup, partial launch and early namespace failures. Direct trusted commands are test fixtures only; production always uses Bubblewrap.

The explicitly attempted live pipeline integration fails at `bwrap: setting up uid map: Read-only file system`. This host cannot establish live namespace isolation; no host fallback was added or used. Live acceptance remains required on a suitable Linux host. No paid provider calls were made. See the [runtime guide](48-safe-inspection-pipelines.md).

## Project inspection permissions (GL-012) — 2026-11-22

Implemented on the existing `feat/optional-summaries` branch, pending merge. The normal suite passes 500 Rust tests, with three integration/profiling tests ignored. Strict all-target Clippy, formatting, offline build and six installer tests pass. New checks cover argv/option classification and injection rejection, path/private/symlink scope, filtered snapshot stability and limits, literal coreutils argv semantics, fixed-executable sandbox construction, output bounds, policy opt-out/deny, exact grants, explicit reruns and early cancellation. Test-only direct coreutils execution checks argv semantics; production never has an unsandboxed fallback.

The explicitly attempted new Bubblewrap integration fails before executing the inspection command: `bwrap: setting up uid map: Read-only file system`. This cloud host cannot validate live namespace isolation, consistent with the previously deferred sandbox limitation. The failure remains closed; it was not bypassed or converted into a passing result. No paid provider calls were used. See [policy and host reproduction](46-project-inspection-permissions.md).

## File-body snapshot evaluation (GL-011) — 2026-11-22

This increment changes documentation/backlog decisions only. Eight existing workspace-audit tests and six restore tests pass, including external-change classification, journal-chain gaps, branch/retry cutoffs and refusal to overwrite manual drift. A three-run read-only workspace benchmark recorded 443 included files, 26,608,001 bytes hashed and a 57,075-byte manifest, with median scan time 0.511585 seconds. The raw aggregate measurements and their limits are retained in the [snapshot evaluation](45-file-body-snapshot-evaluation.md). No new file bodies, schema/configuration changes or provider requests were introduced. Broader recovery remains unimplemented; the completed item is the evaluation and policy decision.

## Historical restart controls (GL-010) — 2026-11-22

GL-010 is implemented alongside GL-009 on `feat/optional-summaries`, not merged. Validation passes 490 Rust tests (two existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. Checks cover every visible retry checkpoint, unchanged canonical manager targets, inherited fork cutoffs, selecting and restoring a second run, preservation of a manually created file, branch hash comparisons, missing scans and cross-session rejection.

A CLI smoke test verifies `restart-points` emits the existing fork command's exact `--event` flag, lists inherited and current runs, then compares recorded branch scans after a manual file addition. Comparison leaves the selected branch/status and manual file unchanged. Its local fixture provider is shut down before restart/comparison inspection; those operations make no inference requests.

## Optional summaries (GL-009) — 2026-11-22

`feat/optional-summaries` adds explicitly requested background TUI/CLI summaries; it is not merged. Validation passes 488 Rust tests (two existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. New checks cover live-owner coexistence, pending-approval preservation, model selection and central accounting, strict output/citation validation, stale startup and branch visibility, and preservation of an existing coding checkpoint even when using the same Engine. Background dispatch/failure attention cannot replace the actual workflow's provider diagnostic.

A CLI/local HTTP smoke test generates one summary on the selected summarizer model, checks its captured source references and ledger purpose, then shuts down the fake provider and reopens the saved summary offline. The workflow's status, branch and step remain unchanged. Tests use scripted/local providers and no paid inference. Activity-title versus summary comparisons document request/latency/evidence differences; they do not claim measured real-model quality or cost improvements.

## Coordination merge validation — 2026-11-22

Final review of the GL-008/GL-028/GL-029/GL-030 stack against remote `master` found no additional blockers in the reviewed ownership, routing, persistence and recovery paths. The feature branch had no divergence from `master`. Merge validation passes 483 Rust tests (two existing integration/profiling tests ignored), six installer tests, strict all-target Clippy, formatting and offline build. Both native terminal scenarios pass: exact original guidance delivery and cross-client project exclusion through a provider retry. Tests use local/scripted providers, with no paid requests.

The stack is merged into `master`; TODO and current runtime status headers reflect this. Earlier sections retain the historical pre-merge evidence and its limits. Filesystem/SQLite publication remains explicitly non-atomic across their boundary, saved manager retries require explicit route acceptance, and sandbox integration remains deferred as documented.

## Pre-merge branch review — 2026-11-22

Reviewed `feat/project-execution-coordination` through `cd9e421` against the current remote `master` (`dfc666d`): six commits ahead, no divergence. The review focused on project/session lock order, observation/startup contention, claim/cleanup transactions, manager snapshot and branch revalidation, linked original-message preservation, live inbox withdrawal/inclusion boundaries, saved-request retries, guarded restore/fork publication and TUI draft/approval isolation. No additional blocking code issue was found in those paths; this is not an exhaustive security audit.

Fresh validation passes 483 Rust tests, with the existing Bubblewrap integration and manual profiling tests ignored, strict all-target Clippy, formatting, offline build and six installer tests. Native terminal checks use local fake HTTP providers, covering explicit guidance delivery and cross-client exclusion through provider retry; no paid provider requests or new sandbox validation are involved.

GL-008, GL-028, GL-029 and GL-030 are implemented on this branch, pending merge. At the time of this pre-merge review, the earlier entries below described the evidence and limitations at each historical slice; they do not override the current runtime documentation. Filesystem restore and SQLite fork publication remain separate operations: a publication failure after restoration is reported explicitly and starts no worker. Exact saved manager retries remain read-only, and older binaries/external editors do not participate in the new coordination protocol.

## GL-008 initial idle-delivery slice (historical) — 2026-11-22

The existing `feat/project-execution-coordination` branch now also contains explicit idle manager delivery, retaining one pending branch. The suite passes 467 Rust tests (two existing integration/profiling tests ignored), strict all-target Clippy, formatting, offline build and six installer tests. Tests use scripted/local providers and no paid API calls.

New coverage checks exact whitespace/Unicode originals in only the target step; separate FIFO IDs for identical text; stale branch/workflow/artifact rejection; withdrawal; no permission/input-gate bypass; non-executing restart proposals; unresolved/unreachable guidance; atomic attempt binding and prepared-envelope inclusion; saved retries without duplication; withdrawal preventing a saved request from replaying the instruction; and Help-safe TUI controls.

A native PTY/local HTTP scenario asks from the CLI, verifies that asking queued nothing, selects the manager proposal in the TUI and uses **g** to start the blocked current target. Its persisted coding envelope contains the exact original once; the initial attempt did not contain it. Reaccepting via CLI starts nothing. Three central dispatch records account for initial coding, manager inference and routed coding. At this initial slice, confirmed restart binding and live delivery were not yet implemented; the current branch validation above supersedes that limitation.

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
