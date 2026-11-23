# Remaining work

Updated 2026-11-23. This is the canonical open backlog; completed work is recorded in Git, the [decision log](docs/16-decision-log.md) and [runtime documentation](docs/README.md). IDs remain stable when priorities change. New findings use new IDs, rather than renumbering existing references.

P0/P1 feature work, installer, TUI redesign and recovery improvements merged into master on 2026-11-19. This does not mean no defects remain. Implement one feature at a time, preserve audit evidence and distinguish model claims from recorded verification.

## Supervised workflows and review

No open items in this section; completed supervised-workflow features are merged into `master`.

## Sandbox verification — next

- [ ] **GL-027** — Make project verification practical inside the sandbox: report masked Git metadata and unavailable host services clearly, support Python runtimes/virtualenvs without exposing host HOME, route Python build/bytecode caches to writable sandbox storage, and distinguish unavailable checks from test failures. Keep the project read-only and network isolation intact. Next phase of the approved [shared design](docs/53-sandbox-and-verification-design.md): capability feedback, pre-commit and an isolated writable verification copy. This profile is not implemented yet and adds no current runtime authority.

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

GL-007 defines the coordination contract; GL-028 implements project exclusion and recovery. GL-021 parallel orchestration needs a new explicit concurrency design rather than bypassing this lease. GL-012/GL-032 already implement bounded read-only inspection and safe pipe-only classification. Broader argument-varying executable grants and verification execution profiles still require scoped design; unsupported commands retain exact approval and the existing sandbox boundary. Keep docs, embedded prompts and the decision log synchronized with implementation.

## Completed recovery and code improvements

- [x] **GL-033** — **Implemented on the review branch (2026-11-23), not yet merged**: a dedicated per-invocation supervisor/subreaper owns Bubblewrap startup descendants, including session-escaped helpers. Single commands and pipelines share bounded capture, cancellation and a five-second cleanup wait. Persistent job records quarantine unconfirmed cleanup and block owner release/recovery; no host fallback or new permissions. See [sandbox lifecycle](docs/10-shell-sandbox.md#process-lifecycle-and-cleanup).

- [x] **GL-017** — **Merged into `master` (2026-11-23)**: optional coding/verification/manager/summary role defaults on one provider, atomic session-wide request/input/output/estimated USD reservations across branches and retries, fail-closed unknown cost accounting, budget blockers/status and budget-only saved-request continuation. Estimates are not invoice caps; workers retain startup policy. See [roles and budgets](docs/52-model-roles-and-session-budgets.md).

- [x] **GL-016** — **Merged into `master` (2026-11-23)**: opt-in grouped native journal readers by step/attempt, separate rerun/restore/unattributed groups, combined continuous diffs with per-write fallback on gaps, frozen branch-visible snapshots and full selected-group C/E exports. No staging or group approval/undo. See [grouped review](docs/51-grouped-native-edit-review.md).

- [x] **GL-015** — **Merged into `master` (2026-11-23)**: native diff hunk readers with Previous/Next/full-file controls, plus TUI/CLI exports of bounded branch-visible verification records and exact referenced artifacts/tool evidence. No new verification, hunk staging or permissions. See [diff hunks and evidence](docs/50-diff-hunks-and-verification-evidence.md).

- [x] **GL-014** — **Merged into `master` (2026-11-23)**: external-editor human artifact revisions, exact-version branch-visible review comments, user/source provenance, immutable originals, guarded publication and retained conflict drafts. Comments do not approve work or automatically message the agent. See [artifact review](docs/49-artifact-review.md).

- [x] **GL-013** — **Merged into `master` (2026-11-23)**: confirmed session-only saved exact-shell/native-write grants, retained across restarts and branches, isolated by project/session identity, with CLI inspection and TUI decision rows. Unanswered/dismissed prompts save nothing. See [saved approvals](docs/08-persistent-approvals.md).

- [x] **GL-032** — **Merged into `master` (2026-11-23)**: fully validated 2–4-stage read-only pipelines, stdin-only filters, shared filtered view, isolated fixed executables, bounded output, per-stage outcomes and whole-pipeline cancellation. Live Bubblewrap validation remains host-blocked. See [runtime guide](docs/48-safe-inspection-pipelines.md).

- [x] **GL-031** — **Merged into `master` (2026-11-23)**: narrow pipe-only language, source/filter roles, complete preflight, shared snapshot/budgets, per-stage isolation, failure/SIGPIPE semantics and cancellation/launch-failure acceptance checks. Runtime support is implemented by GL-032. See [safe pipeline design](docs/47-safe-inspection-pipeline-design.md).

- [x] **GL-012** — **Merged into `master` (2026-11-23)**: supported read-only argv forms auto-approve under project inspection policy, using trusted executables and bounded filtered private views inside Bubblewrap. Private/outside paths, symlinks and shell syntax cannot broaden automatic authority. Exact grants, denies, user reruns and opt-out remain intact. Supported pipe-only pipelines are now implemented by GL-032; see [project inspection](docs/46-project-inspection-permissions.md). Live namespace validation remains host-blocked as documented.

- [x] **GL-011** — **Merged into `master` (2026-11-23)**: confirmed the external-preimage gap, measured the current hash baseline, and defined privacy/size/cache/retention and external-edit protection requirements. Decision: no automatic external-body capture or restore expansion; a future implementation needs explicit opt-in and a concrete recovery case. See [snapshot evaluation](docs/45-file-body-snapshot-evaluation.md).

- [x] **GL-010** — **Merged into `master` (2026-11-23)**: every visible historical run/checkpoint in the restart picker, native restore diffs plus live/manual-change coverage, and read-only recorded branch comparisons in TUI/CLI. Existing guarded restore/fork, branch ancestry and manager binding policies remain authoritative. See [historical restarts](docs/44-historical-restart-and-branches.md).

- [x] **GL-009** — **Merged into `master` (2026-11-23)**: opt-in TUI background summaries and CLI generation, bounded captured evidence/citations, selectable same-provider model, separate usage accounting and saved branch-visible readers. Activity titles remain the immediate feedback; summaries never gate workflow execution or replace verification. See [optional summaries](docs/43-optional-summaries.md).

- [x] **GL-008** — **Merged into `master` (2026-11-22)**: explicit validated proposal acceptance, exact original bytes scoped to one attempt, next-visit reservations, FIFO/withdrawal, atomic prepared-request inclusion, guarded restart binding, owner-scoped live delivery with completion-review holds, linked clarification replies and default Conversation routing. See [idle manager delivery](docs/42-idle-manager-delivery.md). Preserve ownership, input/permission/verification gates and separate side-question accounting.

- [x] **GL-030** — Merged into `master` (2026-11-22): a short descriptor-anchored observation guard serializes project-status probes with execution-lock acquisition. Starting workers wait for probes rather than reporting false busy; real workers still exclude competitors through the original execution lease. Cross-process regression freezes the failing interleaving; transaction-order, recovery and original TUI startup checks remain intact.

- [x] **GL-028** — Merged into `master` (2026-11-22): one workflow/control lease per project, atomic project/session execution ownership, stable recovery lock order, one lease through restore/fork publication, legacy/interrupted maintenance recovery, truthful busy feedback and retained rejected drafts/goals. Read-only observers/manager questions remain available. See [project execution coordination](docs/41-project-execution-coordination.md).

- [x] **GL-029** — Merged into `master` (2026-11-22): activity rows could be tagged with a newer database revision read after their history snapshot, indefinitely hiding a new blocker. Capture the refresh generation before reads and include the visible history cutoff; deterministic regression and repeated original engine-backed diagnostic checks retain the assertion.

- [x] **GL-007** — Defined the [project execution coordination proposal](docs/41-project-execution-coordination.md): operation scope, waiting/stop semantics, lock order, interrupted/legacy owners, read-only exceptions, UI feedback and acceptance scenarios. Implemented by GL-028; the design document now describes runtime behavior.

- [x] **GL-005** — Merged into `master`: configurable source/include, blob, history/usage and dump budgets; labelled recent-history fallback with exact selected bodies; immutable history/selected-artifact caches and revision-based UI reuse. No schema changes or evidence pruning. See [resource budgets and refresh profile](docs/40-resource-budgets.md).

- [x] **GL-003** — Merged into `master`: descriptor-relative reads, includes, discovery, snapshots/writes/restore, private blobs/exports/locks, anchored SQLite/WAL paths and sandbox bind sources. Path-swap regressions and limits are documented in [anchored file access](docs/39-anchored-file-access.md).

- [x] **GL-002** — Completed: durable exact-message provider continuations for coding, verification, detectors and manager side questions; original attempt/check progress, credential reload, cooldown and ownership recovery, immutable checkpoints, and workspace/config/input validation. See [saved provider requests](docs/38-provider-continuations.md).

- [x] **GL-001** — Completed: user-requested historical tool reruns: exact arguments, explicit one-call authorization, linked new history/results, existing project/tool boundaries and write journal, execution ownership/recovery, and bounded evidence in the next attempt. No automatic replay. See [tool reruns](docs/37-tool-reruns.md).

- [x] **GL-004** — Serialize approval reload–merge–save with a stable per-scope OS lock; sync atomic replacements. Concurrent project/global process tests cover preservation of all grants.
- [x] **GL-006** — Share typed action definitions, shortcut labels and the Help reference; dispatch buttons directly without synthetic keys. Keep contextual navigation/text editing explicit and guard decision-layer ownership. Keyboard/button parity, literal input and stale-mouse regressions pass.

Session/branch publication now commits its records and audit event together, updating the caller’s session only after commit. Fault-injection tests cover statement and commit failures. IDs remain reserved; do not reuse them.
