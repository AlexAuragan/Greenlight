# Code review — 2026-11-19

Current disposition (2026-11-28): the sandbox stack passed final checks and merged into `master`; see the closure section below. Earlier dated review notes retain their original scope/status.

## GL-035 measurement/evidence review — 2026-11-28

Reviewed fixture measurement forwarding, uncertain-cleanup retention, recorded report claims and host capability assumptions. Fixed discarded shell observations and unconditional test-link removal on cleanup-incomplete. The [acceptance matrix](57-sandbox-validation-review.md) separates successful provisioned subsets from the native run's unavailable tools, direct DNS failure and remaining UI/aggregate-accounting measurements. It also corrects the assumption that the outer wrapper's read-only cgroup mount describes the actual benchmark path. This is a scoped review; it does not approve merging the whole sandbox stack or close GL-035.

This review prioritizes readable control flow, shared safety policies and concrete failure cases. It is not an exhaustive security audit or a redesign of the engine/TUI.

## Changes

- Normalize Rust formatting across the project and document editor conventions.
- Flatten provider retry error handling and project-instruction fallback; inline one-use workflow digest and fence-closing logic. Retain helpers that encapsulate shared policy, schemas or transport boundaries.
- Apply project privacy and canonical root checks to workflow definitions, step files, prompt includes and project instructions. Outside/private symlink targets cannot become model instructions through these loaders.
- Reject cyclic and cross-session branch ancestry instead of looping or accepting corrupt lineage.
- Distinguish closed CLI input and absent affirmative authorization from an explicit human denial.
- Share exact UTF-8 snapshots between native writes, recovery and restore, bounded to 20 MiB per existing file. Oversized files require manual handling; replacement content retains its 100 KB limit. Snapshot errors happen before mutation.
- Sync trace blob directory entries before publishing their references.

## Deferred findings

[GL-028](../TODO.md) implements runtime project-level coordination on its review branch, following the [GL-007 contract](41-project-execution-coordination.md). GL-003 descriptor anchoring and GL-005 read budgets/caching are merged into `master`; see [anchored file access](39-anchored-file-access.md) and [resource budgets](40-resource-budgets.md). GL-001/GL-002 reruns and durable continuations, GL-004 approval-store locking and GL-006 typed actions are implemented. These changes preserve explicit side-effect and recovery semantics.

## Readability conventions

Use standard rustfmt. Prefer early exits and local variables over nested success/error branches. Keep one-use logic local when it makes the caller easier to read; retain a helper when it names a substantial operation or enforces a shared boundary. Avoid hiding a single expression behind a forwarding function. Keep persisted schemas, factual verification and model-authored claims distinct.

Validation results are recorded in [validation](06-validation.md). Existing terminal snapshots must remain unchanged for this cleanup.

## Design-pattern follow-up

Use the [architecture pattern notes](04-architecture.md#patterns-used-to-keep-the-implementation-local) for the implemented command registry, decision input ownership, transactions and serialized approval updates. Tests exercise concurrent processes, injected SQL/commit failures and keyboard/direct-action parity. Existing 80×24 result/artifact captures remain unchanged.

## Coordination and coordinator branch review — 2026-11-22

Reviewed the six commits from current `master` to `cd9e421`, focusing on ownership and recovery boundaries rather than adding another abstraction layer. The project observation guard is released before session/native-write locks and executing transactions; the execution lease remains held during provider/tool/approval waits. Owner capability and token checks remain required for mutations, while live producers only append validated control metadata.

Guidance retains immutable originals and linked clarification turns, binds to one branch/attempt, and records inclusion with the prepared request before HTTP dispatch. Withdrawal cannot retract an included request. A saved retry reuses its frozen context; undelivered instructions hold progression for explicit review instead of rewriting the recorded outcome. Restart confirmation revalidates the proposal/checkpoint and atomically publishes the new branch/binding after guarded restoration, with explicit partial-failure reporting.

TUI checks cover explicit read-only versus Conversation recipients, cancellation, stale state, session navigation, filtered activity, new drafts and approval priority. No additional blocking issue was found in the reviewed paths. Historical validation notes were clarified to avoid implying that later GL-008 slices are still missing. Current checks and known implementation limits are recorded in [validation](06-validation.md); this review does not merge or publish the branch.

## Sandbox stack closure — 2026-11-28

Reviewed supervisor lifetime/control pipe and quarantine, bounded pipeline results, pinned direct HTTPS transport/staging, recipe binding and copied-source/native import guards. No new blocker was found in these scoped paths. The final parallel suite exposed a fixture lock-release timing assumption; explicit unlock makes the intended recovery state deterministic without changing runtime recovery. Removed one duplicated edit-overflow assignment and a permanent TODO-status claim from benchmark errors. Final functional evidence, measured limits and deliberate GL-023/GL-025 follow-ups are recorded in the [acceptance review](57-sandbox-validation-review.md) and [validation results](06-validation.md). This is not an exhaustive application security audit.
