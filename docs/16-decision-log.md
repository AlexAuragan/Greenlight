# Greenlight decision log

**Purpose:** preserve concrete product and architecture decisions made while implementing Greenlight, separately from proposed UX research and the active TODO. This is a **dated record**, not a replacement for the executable Rust contracts. If a later PR changes behavior, update the corresponding status and links here.

**Current references:** [README roadmap](../README.md#roadmap--todo), [execution contract](01-execution-contract.md), [current TUI controls](11-tui.md), [workspace restoration](09-workspace-restore.md), [agent action protocol](13-agent-action-protocol.md), and [original UX research](12-tui-ux-analysis.md). Sections 16–18 of the UX research include earlier risks and subsequent revisions; its initial PR #6 baseline is historical.

## Implemented decisions

### D-001 — Harness owns truth and control (foundational; retained 2026-11-13)

The workflow definition and Rust engine determine valid transitions, tool permissions, verification, session history and restore operations. A model can suggest an action or report a result, but its words are **not proof** that a tool ran, a test passed, or a step completed. The user retains control of consequential operations. Reason: Greenlight is a **human-supervised, artifact-driven harness**, not an agent that must finish at any cost.

### D-002 — Native writes and explicit, limited restore (implemented before PR #6; retained)

The native `write` tool records before/after content in an immutable journal. A guarded restore can reconstruct **those recorded edits**, checking for conflicts and refusing unsupported or symlinked paths. `fork` offers a separate restore preview by default; `--no-restore` forks without touching files. Shell commands run in a read-only Bubblewrap workspace and cannot edit project source through that mount. **Do not claim full-project, Git, external-service or manual-edit rollback.** Restoring multiple files is not a single atomic transaction.

**Rejected as an automatic prerequisite:** replacing the journal with Git time travel or a general staging/rollback engine. Measure genuine coverage gaps first.

### D-003 — Observe workspace state before expanding rollback (implemented PR #7; follow-up open)

Greenlight records bounded SHA-256 manifests before/after each step and exposes read-only `workspace-status`. Paths/hashes help detect drift; they do not store old file bytes or tell who made a change. The **remaining** experiment is to correlate changes against the native journal, measure scan cost, and only then decide whether selected external changes merit byte snapshots. See [workspace recording](14-workspace-recording.md).

### D-004 — Keyboard focus is Yazi-inspired and explicit (implemented PR #8)

The focus hierarchy is **Sessions → event/artifact list → detail**. Right/Enter moves inward; Left/Escape backs out. Up/Down or `j/k` selects in lists and scrolls focused detail; `i` focuses the composer directly. The composer never autofocusses, including when the selected session is blocked, so navigation shortcuts remain usable on opening. Escape from the composer returns to the previous pane.

**Discarded design:** requiring PageUp/PageDown for detail scrolling; those keys proved awkward on some keyboards, notably AZERTY, and are no longer bound. Dialog previews use the same Up/Down or `j/k` convention. The exact shipped keys are maintained in [TUI docs](11-tui.md), not the dated UX proposal.

### D-005 — Small Markdown reader first (implemented PR #8; further hardening open)

Registered Markdown artifacts are a primary human review surface. Preview headings, emphasis, lists, quotes, code and simple tables with a raw toggle, without promising full CommonMark/GFM, images or a complete document editor. The artifact tab still shows **latest visible versions**. Persisted attempt input manifests and human approvals now reference exact version IDs; version navigation/diffs and editing remain separate TODOs.

### D-006 — Permissions retain their scope, pending prompts do not survive (partially implemented)

Approve once, process, project and global grants have distinct lifetimes; persisted grants are subject to configuration `deny`. There is **no durable session-scoped grant yet**. TUI approval keys submit explicitly; Enter is not an approval. A pending permission request belongs to the live worker and must not be replayed after a crash. Project/global shell and native-write grants now require a separate scope confirmation before persistence or execution. See [approval semantics](08-persistent-approvals.md).

### D-007 — Greenlight is the canonical name (implemented PR #9)

The executable, Cargo package and current project configuration use `greenlight`, `greenlight.yaml` and `.greenlight/state`. Old config/instructions remain readable as fallbacks. Existing `.stepwright/state` migrates intact, and simultaneous old/new state directories cause an explicit error. See [migration notes](15-name-migration.md).

## Planned and intentionally deferred decisions

### D-008 — Human-readable agent activity titles before routine model summaries

Prefer an optional **coding-agent-authored** `activity_title` for each structured model action or logical phase. Treat it as *intent*, never a verified tool outcome. Preserve the **single YAML action** protocol; do not accept arbitrary prose or multiple actions to generate titles. This field is **not implemented** and currently fails strict parsing. Update the runtime parser, embedded prompt, protocol docs and tests in one change. A second, cheap summarizer may later generate evidence-linked step recaps **only when justified**; no extra inference is required by default.

### D-009 — Manager as an optional coordinator, not an alternate executor

A future manager may answer grounded questions, forward coding instructions unchanged, and propose retry/goto/restart. A clearly addressed coding instruction need not incur a mandatory manager detour. Read-only side questions must not mutate the coding-agent context; proposed transitions must be checked against real session/branch/step/restore state by the Rust harness and confirmed by the human. Separate the manager feature from initial TUI work. Provider/model reuse is allowed; an additional subscription is **not** required.

### D-010 — Live steering is a user goal, but delivery boundaries must be honest

A model request already in flight generally cannot accept new messages. Steering therefore means **cancel and reissue** (if safe and supported) or **deliver at the next model-call boundary**. Show which happened; account for partial tool effects and stale queued messages. Do not promise invisible mid-completion instruction injection. Streaming, queueing and cancellation remain future engineering work. Per-session execution ownership now uses an OS lock plus a persisted owner record, with explicit `recover-session` recovery; it does not provide durable in-flight model/tool continuation.

### D-011 — Review grouped native edits, not a hidden staging engine

A combined diff of already-journaled native edits within a step/attempt can help human review without changing when changes land. **Do not describe that as pre-apply approval.** Deferring writes into a staging overlay introduces new verification, application and drift semantics; investigate that only if a concrete workflow demonstrates a need.

### D-012 — User-owned Git policy and first-class artifacts

Greenlight must not create automatic Git commits by default; the user controls checkpoints. Registered artifacts are immutable, branch-aware UTF-8 versions and are not necessarily project files. External editing plus registration of a new artifact version is preferable to a premature embedded editor. Future approval/context views must reference the **exact historical artifact version** used, not silently substitute latest-by-name content.

## Planned decision — Token usage and pricing (2026-11-14)

Track usage for every model request, including input, cached input and output tokens. Prefer provider-reported usage; tokenizer/tiktoken fallbacks must be labelled estimates and cannot infer cache hits. Configured models should surface provider-specific prices and their provenance, with historical rate snapshots and explicit unknown values. Keep incurred spending separate from fork-inherited history. Core per-request accounting, configured flat-rate snapshots and CLI/TUI reporting are implemented in P1; automatic pricing discovery, tiers, freshness checks and budgets remain TODOs. See [usage and pricing](18-token-usage-and-pricing.md).

## Correctness issue discovered during documentation audit (2026-11-13; resolved)

**Deviation detector duplication:** the earlier inner/outer checks could evaluate one report twice. Detection now uses one shared evaluator with a retained verdict: rejected agent reports return to the same conversation, accepted reports route without rechecking, and malformed/failed detector responses block. Regression tests cover pause, retry, goto and engine-generated failures; see the [execution contract](01-execution-contract.md).

## Working rule for future features

Ship and validate **one feature at a time**, preserve old and new behavior distinctions, and update the README TODO, the relevant authoritative contract, this log and any dated research note in the same PR when a design decision changes. Use small, evidence-driven increments instead of inventing large prerequisite subsystems.
