# Remaining work

Updated 2026-11-19. This is the canonical open backlog; completed work is recorded in Git, the [decision log](docs/16-decision-log.md) and [runtime documentation](docs/README.md). IDs remain stable when priorities change. New findings use new IDs, rather than renumbering existing references.

P0/P1 feature work, installer, TUI redesign and recovery improvements merged into master on 2026-11-19. This does not mean no defects remain. Implement one feature at a time, preserve audit evidence and distinguish model claims from recorded verification.

## Next: recovery and correctness

- [ ] **GL-002** — Persist suspended provider-request continuations across TUI/process restarts, including manager side questions, with exact-message checkpoints, credential/config reload semantics and validation of intervening workspace changes. In-process workflow Retry request preserves messages; ordinary Resume/Continue step starts a fresh step attempt.
- [ ] **GL-003** — Close concurrent path-swap races with descriptor-anchored file access across tools, include loading, journal snapshots, restore and private blob storage. Current canonicalization/lstat checks reject static escapes but do not make later opens race-free.
- [ ] **GL-005** — Bound expanded includes, artifact/trace reads and history/export memory. Profile refresh/diff work, load lightweight event metadata and selected immutable bodies on demand, and avoid full-history scans on interactive paths. Define configurable limits and legacy-file behavior before changing schemas.
- [ ] **GL-007** — Define project-level execution coordination before parallel workflows: session ownership and per-write locking do not prevent concurrent sessions reasoning over stale project state. Session creation, branch switching and fork publication are now transactional; project-level execution coordination remains open.

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

GL-002 needs durable message checkpoints and workspace validation. GL-007 precedes parallel orchestration. Command-level grants require joint design with the user; retain exact-command grants until then. Keep docs, embedded prompts and the decision log synchronized with implementation.

## Completed recovery and code improvements

- [x] **GL-001** — Implemented on `feat/stop-execution-key` (awaiting merge): user-requested historical tool reruns: exact arguments, explicit one-call authorization, linked new history/results, existing project/tool boundaries and write journal, execution ownership/recovery, and bounded evidence in the next attempt. No automatic replay. See [tool reruns](docs/37-tool-reruns.md).

- [x] **GL-004** — Serialize approval reload–merge–save with a stable per-scope OS lock; sync atomic replacements. Concurrent project/global process tests cover preservation of all grants.
- [x] **GL-006** — Share typed action definitions, shortcut labels and the Help reference; dispatch buttons directly without synthetic keys. Keep contextual navigation/text editing explicit and guard decision-layer ownership. Keyboard/button parity, literal input and stale-mouse regressions pass.

Session/branch publication now commits its records and audit event together, updating the caller’s session only after commit. Fault-injection tests cover statement and commit failures. IDs remain reserved; do not reuse them.
