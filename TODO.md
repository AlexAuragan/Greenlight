# Remaining work

Updated 2026-11-22. This is the canonical open backlog; completed work is recorded in Git, the [decision log](docs/16-decision-log.md) and [runtime documentation](docs/README.md). IDs remain stable when priorities change. New findings use new IDs, rather than renumbering existing references.

P0/P1 feature work, installer, TUI redesign and recovery improvements merged into master on 2026-11-19. This does not mean no defects remain. Implement one feature at a time, preserve audit evidence and distinguish model claims from recorded verification.

## Supervised workflows and review

- [ ] **GL-008** — Implement the remaining [coordinator contract](docs/20-conversational-coordinator.md): targeted idle delivery and guarded retry/goto/restart proposals first, then a durable live-owner inbox. Preserve original message bytes, consistent evidence snapshots, attempt/branch scope, ownership, permissions and verification. Show Queued / Included / Restart proposed delivery states; define side-question history and retention, and keep deterministic commands/approvals separate.
- [ ] **GL-009** — Consider an inexpensive **optional summarizer** for long steps or completed workflows, run asynchronously and display evidence-linked output when ready; compare it against coding-agent-authored short activity titles. Never block execution on summarization.
- [ ] **GL-010** — Add richer restart controls: choose a specific historical attempt/checkpoint, inspect changed files, compare branches, understand untracked/manual edits.
- [ ] **GL-011** — Evaluate **snapshotting changed file bodies** at step boundaries only after hash manifests reveal a useful gap; define size/privacy/caches policy and whether external edits should be restored or simply protected from overwrite.
- [ ] **GL-012** — **Design command-level shell grants together before implementation:** allow selected executables (e.g. `ls`) across approved argument/path variations without approving a whole shell string by prefix. Define argument/option restrictions, path and executable identity, wrappers/interpreters, pipelines/AND/OR/sequences, redirections, substitutions and environment assignments; validate every executed component or request approval for the whole compound command. Regression case: granting `cat` must never auto-approve `cat something.txt | ./harmful_script.sh` (nor hidden substitutions). Keep exact-command grants until this design is agreed.
- [ ] **GL-013** — Add real **session-scoped saved permissions**, separate from one-time/process/project/global grants; no persistence of unanswered approval prompts.
- [ ] **GL-014** — Support artifact review comments and safe new-version editing (external editor first, optionally embedded later).
- [ ] **GL-015** — Extend diff review with hunk navigation and exportable verification evidence.
- [ ] **GL-016** — Add **opt-in grouped review** of multiple already-journaled native writes (combined diff by step/attempt). Don't build a separate staging engine unless a concrete workflow requires pre-apply group approval.
- [ ] **GL-017** — Introduce model-role configuration and usage/cost budgets without assuming multiple provider accounts.

## Deferred sandbox follow-up

- [ ] **GL-027** — Make project verification practical inside the sandbox: report masked Git metadata and unavailable host services clearly, support Python runtimes/virtualenvs without exposing host HOME, route Python build/bytecode caches to writable sandbox storage, and distinguish unavailable checks from test failures. Keep the project read-only and network isolation intact. Deferred by the user after the clock-hover test.

## Later: scale and integrations

- [ ] **GL-018** — Revisit full-workspace / multi-file atomic rollback only if measured drift and restoration failures justify the extra complexity; document the current restore's partial-error handling.
- [ ] **GL-019** — Extend the implemented model preview/future-request stop controls with safe in-flight cancel/interrupt/queue semantics and durable continuation across process restarts.
- [ ] **GL-020** — Safe Git-aware checkpoints and patch export; do not imply native restore captures arbitrary workspace changes.
- [ ] **GL-021** — Parallel/subworkflow orchestration with explicit session ownership and approval routing.
- [ ] **GL-022** — Provider flexibility, context compaction, bounded code retrieval/indexing and opt-in MCP tool integrations.
- [ ] **GL-023** — More complete sandbox resource limits and secret-handling defenses, plus security regression tests.
- [ ] **GL-024** — Add optional desktop notifications, authenticated remote decisions and customizable keybindings/themes. Outbound Discord notifications are already supported.
- [ ] **GL-025** — Branch graph/time-travel visualization, saved views, accessibility/contrast testing and performance profiling.
- [ ] **GL-026** — Improve approval-decision provenance (request identity and UI action/key source) without persisting or replaying unanswered approvals.

## Dependencies and design gates

GL-007 defines the coordination contract; GL-028 implements project exclusion and recovery. GL-021 parallel orchestration needs a new explicit concurrency design rather than bypassing this lease. Command-level grants require joint design with the user; retain exact-command grants until then. Keep docs, embedded prompts and the decision log synchronized with implementation.

## Completed recovery and code improvements

- [x] **GL-028** — Implemented on `feat/project-execution-coordination` (awaiting merge): one workflow/control lease per project, atomic project/session execution ownership, stable recovery lock order, one lease through restore/fork publication, legacy/interrupted maintenance recovery, truthful busy feedback and retained rejected drafts/goals. Read-only observers/manager questions remain available. See [project execution coordination](docs/41-project-execution-coordination.md).

- [x] **GL-029** — Fixed on the same branch: activity rows could be tagged with a newer database revision read after their history snapshot, indefinitely hiding a new blocker. Capture the refresh generation before reads and include the visible history cutoff; deterministic regression and repeated original engine-backed diagnostic checks retain the assertion.

- [x] **GL-007** — Defined the [project execution coordination proposal](docs/41-project-execution-coordination.md): operation scope, waiting/stop semantics, lock order, interrupted/legacy owners, read-only exceptions, UI feedback and acceptance scenarios. Implemented by GL-028; the design document now describes runtime behavior.

- [x] **GL-005** — Merged into `master`: configurable source/include, blob, history/usage and dump budgets; labelled recent-history fallback with exact selected bodies; immutable history/selected-artifact caches and revision-based UI reuse. No schema changes or evidence pruning. See [resource budgets and refresh profile](docs/40-resource-budgets.md).

- [x] **GL-003** — Merged into `master`: descriptor-relative reads, includes, discovery, snapshots/writes/restore, private blobs/exports/locks, anchored SQLite/WAL paths and sandbox bind sources. Path-swap regressions and limits are documented in [anchored file access](docs/39-anchored-file-access.md).

- [x] **GL-002** — Completed: durable exact-message provider continuations for coding, verification, detectors and manager side questions; original attempt/check progress, credential reload, cooldown and ownership recovery, immutable checkpoints, and workspace/config/input validation. See [saved provider requests](docs/38-provider-continuations.md).

- [x] **GL-001** — Completed: user-requested historical tool reruns: exact arguments, explicit one-call authorization, linked new history/results, existing project/tool boundaries and write journal, execution ownership/recovery, and bounded evidence in the next attempt. No automatic replay. See [tool reruns](docs/37-tool-reruns.md).

- [x] **GL-004** — Serialize approval reload–merge–save with a stable per-scope OS lock; sync atomic replacements. Concurrent project/global process tests cover preservation of all grants.
- [x] **GL-006** — Share typed action definitions, shortcut labels and the Help reference; dispatch buttons directly without synthetic keys. Keep contextual navigation/text editing explicit and guard decision-layer ownership. Keyboard/button parity, literal input and stale-mouse regressions pass.

Session/branch publication now commits its records and audit event together, updating the caller’s session only after commit. Fault-injection tests cover statement and commit failures. IDs remain reserved; do not reuse them.
