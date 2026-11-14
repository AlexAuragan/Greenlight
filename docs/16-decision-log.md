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


## 2026-11-14 — Session workspace uses saved execution facts

The TUI workspace projects persisted attempts, saved definitions, pinned input manifests, aggregate verification events and OS ownership. It does not infer success from model text or replace recorded input versions with latest artifacts. The stepper shows the latest outcome per step and attempt counts; loops and unvisited branches are not flattened into assumed progress. Missing historical metadata remains explicit. Blocker cards link to exact evidence and reuse existing guarded resume, recovery and restore controls.


## 2026-11-14 — Picker creation is separate from execution

The TUI session picker searches persisted goal/workflow/status/step/ID fields and sorts blocked sessions ahead of recorded recency. Recency uses the session's latest event across branches, not model-authored summaries. Selection follows session IDs across refreshes. The workflow picker validates files again on submission and creates a ready session; the existing resume path starts execution separately with normal permissions. Incoming approvals clear search and restore their owner before rendering, so filtering cannot hide or mislabel a pending request.


## 2026-11-14 — Explicit following, session drafts and compact navigation

Reading evidence pauses activity following, even when the inspected event was the latest. End/G in the activity list explicitly resumes it. Polling preserves the selected event and read offset; scroll limits use Ratatui's actual wrapped line count rather than byte counts. Arrows and j/k remain the primary AZERTY-friendly controls; PageUp/PageDown now provide an optional eight-line scroll alongside Home/End. This extends the earlier intentionally minimal key set.

Composer drafts are keyed by session ID and kept in memory only. Filtering, reordering, restart cancellation and approval-owner selection cannot move a draft onto another session. New sessions start empty, and quitting discards drafts. Compact terminals prioritize the active pane. An undersized approval view disables decision keys while keeping Escape denial available. TestBackend acceptance tests cover resizing, background events, Unicode drafts and approval ownership. The locked Ratatui version's rendered-line-info feature supplies layout-consistent scroll bounds.


## 2026-11-14 — Bounded, terminal-safe document previews

Markdown and literal artifact previews share control escaping and display limits. Escape/C1 bytes and bidirectional formatting controls become visible code-point text; CRLF is normalized and tabs use four spaces. Stored artifact bytes are unchanged. Full CLI exports remain available, while preview truncation is explicit at the first source/output-byte, line-count or per-line limit. Table padding uses terminal widths, caps columns/padding, and parses each cell once. Fences track their opening run length. This hardens the lightweight renderer without expanding its CommonMark/GFM scope. Tests cover real terminal buffers, CJK/combining/emoji alignment, adversarial markup, sparse tables, control expansion and oversized documents in both display modes.


## 2026-11-14 — Immutable artifact history and provider-error backlog

The artifact browser follows branch-visible creation IDs and fork cutoffs. Metadata discovery does not require blob bodies; opening content/diffs reads exact versions. Provenance uses recorded attempt intervals and exact input/approval references, never latest-name reconstruction or an inferred author. Incoming activity keeps the inspected ID pinned; changing ancestry closes the browser. Adjacent-version diffs compare full originals within a per-input size limit. CLI `artifact --event` provides the same visibility checks for export.

Provider failure management is added as an **open P1 TODO**, not implemented by this artifact change. Today errors surface largely as generic HTTP/request errors. The follow-up must classify authentication, credit/quota and rate limiting from status plus provider codes, give safe actionable CLI/TUI guidance, and bound eligible retries while preserving usage/uncertain-billing records. In particular, a 429 is not sufficient evidence to retry an exhausted account.


## 2026-11-14 — Safe provider diagnostics and conservative physical-request retries

The provider-failure TODO is now implemented for coding, verification and deviation-detector requests. Categories use status plus known error codes/types and selected billing phrases; diagnostics never copy provider error messages, raw bodies, request URLs or credential values. Unknown variants fall back to safe guidance. Model discovery uses the same classifications without automatic retry.

Only explicit 429 rate rejections without usage and pre-response connection failures retry automatically. Timeouts, service errors, ambiguous 429s, credentials and billing errors stop; inference may already have been processed. A maximum of three physical requests and two waits of <=15 seconds bound retries. Retry-After seconds/dates are respected; longer waits require manual resume. Without a header, exponential delay includes bounded jitter. Every physical request has its own durable usage record before sending. Provider failures in verification propagate directly instead of entering ordinary verification-failed retry/detector rules. TUI x cancels backoff and future provider requests, while in-flight calls/tools retain their existing behavior. Cancellation resets with a new worker. No model-generated message determines these transitions.


## 2026-11-14 — TUI component boundaries and bounded background previews

The controller now delegates focus/draft state, keyboard precedence, recorded activity, layout/help, approval decisions and guarded recovery to separate tested modules. App/store refresh and Engine-worker orchestration remain in the foreground controller; this is an incremental decomposition of the existing engine-backed TUI.

Artifact/version previews above 8 KiB or 64 lines parse, wrap and rasterize on a separate preview thread. The UI copies a ready viewport or shows a loading/error message. Capacity-one job/result queues and a bounded UTF-8 source prefix constrain work; document revision/mode/size/scroll keys prevent stale painting after navigation or resizing. Parsed text and line counts are reused. Rendering cannot authorize tools, mutate history or resolve approvals. Tests compare complete terminal buffers against synchronous rendering, route input while a job is deliberately held, and exercise stale/discarded viewport scheduling. Store I/O, explicit diff construction and approval rendering keep their existing synchronous behavior.


## 2026-11-14 — Semantic cards preserve exact evidence and conservative outcomes

Default activity uses card projections cached by immutable history generation. New tool and command-step requests receive explicit request/outcome correlations, with output and correlation committed atomically after execution. The outcome event remains the selection anchor. The UI validates branch visibility, order, step/attempt boundaries and recorded exit codes; older/unlinked outputs are not paired by proximity. Malformed links retain diagnostic/raw access. Publication atomicity does not imply atomic external effects.

Cards distinguish shell exit status from generic tool return, model reviewer reports from aggregate verification, and individual check failures from final policy outcomes. Native file cards use journal diffs; artifact cards open exact creation IDs. Raw audit mode remains available, routine permission chatter remains hidden by default, and inspection performs no model calls or workflow mutations. Large expanded evidence uses the existing bounded preview worker.


## 2026-11-14 — Reading positions, local exports and activity filtering

Reading state is keyed by session/branch and tab, using immutable event/version identities. It retains offsets, follow state and journal-diff snapshots in memory, and isolates modal-reader scrolling. New artifact bodies invalidate old offsets. Activity filters use cached shared event rows plus semantic metadata rather than re-reading history or cloning large payloads on each keypress.

Uppercase C/E avoid collision with approval decision keys. C exports full selected text through a bounded background clipboard helper on the host machine; absence/failure of clipboard utilities is explicit. E hands the terminal to a user-configured editor on a private temporary read-only snapshot, then restores the TUI. No snapshot changes are imported; commands use argv/quoting without shell interpolation. Own-worker execution blocks editor handoff to keep approval input available. Existing drafts remain memory-only, and no export operation creates workflow/approval events. Tests use fake local helpers and a real PTY smoke exercise, without touching the actual desktop clipboard or making model requests.
