# Code review — 2026-11-19

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

[GL-028](../TODO.md) retains runtime project-level coordination, following the [GL-007 design proposal](41-project-execution-coordination.md). GL-003 descriptor anchoring and GL-005 read budgets/caching are implemented on their review branches; see [anchored file access](39-anchored-file-access.md) and [resource budgets](40-resource-budgets.md). GL-001/GL-002 reruns and durable continuations, GL-004 approval-store locking and GL-006 typed actions are implemented. These changes preserve explicit side-effect and recovery semantics.

## Readability conventions

Use standard rustfmt. Prefer early exits and local variables over nested success/error branches. Keep one-use logic local when it makes the caller easier to read; retain a helper when it names a substantial operation or enforces a shared boundary. Avoid hiding a single expression behind a forwarding function. Keep persisted schemas, factual verification and model-authored claims distinct.

Validation results are recorded in [validation](06-validation.md). Existing terminal snapshots must remain unchanged for this cleanup.

## Design-pattern follow-up

Use the [architecture pattern notes](04-architecture.md#patterns-used-to-keep-the-implementation-local) for the implemented command registry, decision input ownership, transactions and serialized approval updates. Tests exercise concurrent processes, injected SQL/commit failures and keyboard/direct-action parity. Existing 80×24 result/artifact captures remain unchanged.
