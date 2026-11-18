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

**Design update (2026-11-15):** the [new coordinator contract](20-conversational-coordinator.md) selects a default manager route for ordinary messages when enabled; direct-agent mode is a later explicit option. The earlier bypass suggestion below is historical. Runtime behavior is unchanged until the new contract is implemented.

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

## 2026-11-14 — Optional coding-response titles are intent annotations

The coding YAML contract now accepts optional `activity_title` metadata on any existing action. Omission/null preserves previous responses; a supplied string is trimmed and bounded to 1–80 Unicode scalar values, with control/direction/selected invisible-format characters rejected. Invalid metadata follows the existing bounded protocol retry. The reference, embedded prompt, parser and tests change together.

After validation and input-approval checks, the engine records `activity.title` with the exact `model.output` event ID before dispatch. This records intent while approvals/results are pending, without implying that the action ran. Projections validate source visibility, step/order, attempt/fork boundaries, text agreement and duplicate links. Concise cards label **Agent intent** and retain an informational tone, even for a model-authored success claim. Expansion retains the complete source response; invalid links stay diagnostic. Factual execution/verifier cards remain separate, and titles do not enter selected verifier/detector evidence. Successive coding responses can carry different titles within one step; no new phase lifecycle or extra inference request is introduced. Scripted-provider and terminal-buffer tests cover denied writes, rejected verification, legacy omission, malformed metadata, branch links, evidence expansion, filtering and raw audit mode.

## 2026-11-14 — Authoritative pricing discovery and qualified billing estimates

The exact default DeepInfra transport now resolves missing per-model prices from its public model catalog without transmitting credentials or inference/project data. Explicit overrides win as a complete block. Public fetches are bounded, do not redirect, use a private atomic local cache and retain old snapshots on failure. Unknown model IDs, other provider endpoints, prose tables, discounts and explicit-cache storage/granularity billing stay unpriced. `pricing_discovery: false` disables automatic network lookup. Provider retrieval dates are distinct from effective dates; request records snapshot age warnings, with no historical repricing.

Explicit context tiers replace every rate for the entire request above strictly increasing boundaries, selected only with reported input totals. Cache creation is a separate input subset; ordinary input pricing excludes both cache reads and creation. Native 5-minute/1-hour breakdowns require matching explicit rates and exact reconciliation; unknown TTLs are not guessed. Original ledgers/configs remain readable. Tests cover units, multipliers, unrecognized billing, cached/failed refreshes, override precedence, model IDs, tiers, TTL splits and freshness. Live smoke uses only public catalog GETs; no paid model calls are required.

## 2026-11-15 — Workspace audit explains hash agreement without expanding restore

New step manifests record scan time, included files and bytes hashed. Interval-scoped correlation loads only branch-visible native write/restore blobs between exact manifest IDs, compares every before/after hash and reports journal-explained, unexplained or coverage-unknown paths. Net-zero journal chains remain visible; gaps, mismatched final states, oversized/excluded coverage and missing rows cannot become explained by matching a filename. Completed intervals cannot cross attempt/fork/manifest boundaries. Live comparisons capture a journal cutoff before scanning and disclose non-atomic observation. Missing required blobs produce an explicit error; unrelated old missing bodies do not invalidate a new interval.

The engine records `workspace.correlation` after scanning and before the step outcome when there is evidence/warnings to show. CLI `workspace-status` and its JSON output recompute the last complete interval and current drift without appending history, invoking models or restoring files; TUI cards expand the saved report. `workspace-benchmark` measures scans without a session. Seven debug-build scans of Greenlight and Tokio 1.54.1's cached source tree show repeat medians around 18 ms / 91 ms for 90 / 582 included files. These small/medium local samples do not measure controlled cold caches, monorepos, full step costs or restoration; [the measurement table](14-workspace-recording.md#measured-scan-overhead) records limits. Scripted engine, chain/coverage/fork tests, terminal-buffer tests and a real CLI human-only workflow check preserve zero paid inference calls and unchanged files/history during status inspection. Coordinator implementation remains deferred for joint design.

## 2026-11-15 — Coordinator contract and prompt candidate, before runtime changes

Ordinary messages are designed to go through a manager that answers grounded questions, forwards an original message to a specific agent step, or clarifies ambiguity. The manager returns a validated JSON decision and original-message reference; the harness delivers the saved text verbatim. The [contract](20-conversational-coordinator.md) and [candidate system prompt](prompts/manager-v1.md) specify source-pinned bounded context, separate Q&A history, per-request usage, fresh-state routing checks, next-coding-call delivery, existing guarded restart/restore, and explicit pending/uncertain statuses. A step ending with queued guidance holds further progression for review rather than dropping the instruction or steering the next step. Only the lock owner consumes the future durable inbox; a prompt alone cannot add live steering or bypass approvals. Current run-wide resume instructions cannot implement this scope by themselves. P1's design deliverable is drafted; runtime implementation stays unchecked in P2, staged as read-only Q&A, idle routing, then durable live delivery. No source code, model calls or current execution semantics changed in this design increment.

## 2026-11-15 — First coordinator runtime stage is read-only Q&A

The [first manager runtime](21-manager-qa.md) compiles the documented prompt and sends bounded source-pinned evidence to the selected existing provider/model through the central usage ledger. CLI ask/context/history and TUI `/ask`, `/manager-context`, `/manager` and cancellation work independently of coding execution. The default ordinary-message route is opt-in (`manager.enabled`); explicit asks are per-request opt-in. Original receipt precedes inference, manager history never enters coding context, and every provider dispatch—including invalid decisions/failures—is accounted with purpose `manager`, outside coding attempt attribution. Captured transport metadata uses the actual engine configuration, not a later reloaded provider setting.

Tagged JSON validation checks IDs, supplied citations, agent-step destinations, duplicate/unknown fields and bounds. Answers require source references; missing/unsafe/truncated sources stay explicit. Read-only control records may be appended while execution is owned elsewhere but never become executing instructions. Forward decisions are retained as `forward_unavailable` or stale-state `needs_review`; they do not resume, restore, retry, queue or advance anything. TUI approval routing retains priority. Scripted-provider tests and a real local HTTP worker exercise live-owner observation, original text, persistence, prompt/source integrity, stale definitions, context bounds, failure accounting and display escaping without paid model calls. Guarded idle delivery is the next separate implementation stage.

## 2026-11-15 — Pre-merge review: composite verification preserves fatal call errors

Review reproduced an authentication failure inside `verification: any` that continued to an alternate reviewer and completed the step; a failed verification-usage update also produced additional calls via fallback/deviation handling. Central model-call errors now carry a typed fatal marker while preserving their original error causes. Any-of composites and agent/human/command verification boundaries propagate that marker immediately. Ordinary rejected verdicts still use the configured any-of alternatives. Regression tests distinguish fatal provider/ledger errors from a genuine `passed: false`, assert no fallback/detector/workflow retry and retain the interrupted usage record. This is a merge-readiness fix, not a change to provider retry budgets or workflow verification policy.


## 2026-11-15 — Enter toggles detail reading for keyboard accessibility

Enter opens activity/artifact detail and returns from the focused detail pane to its list. The artifact-version reader follows the same Enter read/list toggle. Visible hints and help now advertise Enter rather than requiring the Left arrow; Left/Escape remain available. Composer focus stays explicit through i or Tab, and Enter submits only when the composer is already focused. Repeat Enter events are ignored to prevent held-key navigation/submission. Navigation tests cover ordinary/compact panes, version readers, draft/selection preservation and unchanged history.

## 2026-11-15 — User-local installation and self-contained projects

P1 was merged to master at da32034. The installer fetches a managed Git checkout, builds a native release with a revision stamp, validates the executable and replaces the user-local binary atomically. Rerunning updates the remembered ref; build/fetch failure preserves the previous executable. Project initialization embeds templates under `.greenlight/` without overwriting existing files. Runtime discovery finds initialized ancestors within Git boundaries; explicit roots and initialization select exactly the requested directory. Legacy configuration/workflows/state remain supported. Project-local secrets/settings are excluded from agent tools and workspace restore. Updates do not modify project settings or history. See [installation and projects](22-installation-and-projects.md).

## 2026-11-16 — Goal submission starts work, editable inputs and live response preview

Real-project testing exposed a misleading new-session flow: submitting a goal only created a ready session. Enter now creates and starts the normal worker directly, retaining explicit tool/human approvals and failure diagnostics. Goal/composer editing shares grapheme-aware cursor navigation, insertion/deletion, wrapping and caret-following scrolling; all focused editable fields use the terminal blinking bar caret, with green user text distinct from UI hints. TUI-owned provider calls request SSE and usage, showing an ephemeral bounded preview; incomplete streams cannot reach action parsing or trigger uncertain automatic retries. Completed output and usage still follow the existing persistence/verification boundaries. Local HTTP tests gate a response midway to prove preview visibility before action execution, then check input/cached/output accounting.

## 2026-11-16 — Clear approval command previews and remove fixed agent turn caps

Real workflow testing reached the bundled 16-response ceiling during baseline investigation. The coding loop now continues until completion, deviation, failure or cancellation rather than enforcing a response count. Old per-step/workflow/session turn fields remain readable but are ignored, so existing projects benefit without editing configuration; fresh templates omit them. Provider retry limits, workflow transition guards, verification and approval boundaries retain their behavior. Tests exercise more than 16 responses with protocol corrections, denied/unavailable tools and artifact writes, and verify cancellation before dispatch.

Shell approval requests carry the exact command as typed display metadata through initial and persistent confirmation prompts. The TUI emphasizes commands and scope without inferring approval authority from text. Argument-varying executable grants are recorded as a joint-design TODO, retaining exact-string matching until shell composition, options, paths and executable identity have an agreed policy.

## 2026-11-16 — Completion evidence and native-edit review/revert

A real optimization run completed with a measurement-failure report and a vacuous verification policy. The TUI now opens a result panel with the agent-reported conclusion, exact final report event, recorded native-edit count and verification scope. Policy acceptance with no checks is displayed separately from substantive validation. The new 3 Edits tab aggregates branch-visible native writes/restores per file and previews selected/all-file reversions through the existing guarded restore engine. Pre-edit snapshots preserve pre-existing user work; new files can be removed, manual drift cannot be force-overwritten, and stale history/branch previews are rejected under ownership. Workspace-only undo appends evidence without rewriting workflow history or resuming execution; post-run restoration marks result reports historical. Existing restore crash/partial-failure limits remain explicit. No summarizer/model calls are added.

## 2026-11-16 — Distinguish invalid action syntax from unavailable capabilities

A model emitted tool names as action names, then published artifacts claiming the tools did not exist. The compiled coding-agent prompt now contains independent valid YAML examples and explicitly separates action names from tool names. Per-step context includes scoped tool examples. Parser/engine format errors supply corrective syntax and explain that the rejected response never dispatched a tool. Accepted requests retain normal workflow/permission/sandbox enforcement; tool-error feedback no longer asserts that a failed execution necessarily never started or encourages working around a denial. Historical model-authored availability notes remain non-authoritative evidence, not capability settings. Scripted-provider tests verify error feedback reaches the next request, a corrected read returns real file contents, and workflow/host denials still prevent the read. Every embedded example is checked against the actual parser, without paid calls.

## 2026-11-17 — UI inventory and visual redesign research, before implementation

The user requested a design-only pass before further feature work. The [screen/component audit and redesign draft](23-ui-audit-and-redesign.md) inventories existing surfaces and user goals, distinguishes current behavior from proposed components, and reviews official Ratatui documentation/source against the pinned version. It proposes semantic color roles, less default metadata, selective inspectors/readers, real overlays, buttons and a focus-aware action dock, with explicit decisions left for joint review. Official documentation was read online through GitHub because direct documentation-site access was blocked; no environment policy was changed. This records research and recommendations, not acceptance of a new UI contract or authorization to implement it. Application code and behavior remain unchanged.

## 2026-11-17 — First UI design choices recorded after user document edits

The user's `ef9672a` document edit selects lowercase `h` for Help, retaining `?`, with uppercase `H` suggested as the Activity/history replacement. Follow-up review selects a collapsible session sidebar hidden by default, activity/file details opened explicitly with Enter/click (inspector when wide, full-width reader when narrow), and an on-demand composer that retains its memory-only draft when closed. The same discussion settles semantic roles (blue user input, cyan focus, neutral model body, green recorded positive outcomes, amber attention, red errors/destructive actions), default-on mouse clicks/scrolling with complete keyboard operation and a capture-off option, and read-only Help over approvals with decision keys disabled until Help closes; Esc returns to the still-valid approval without denying it. Exact shades, final Activity remapping, inspector-follow behavior and detailed screen composition remain open. The [UI draft](23-ui-audit-and-redesign.md) separates these settled choices from proposals. This remains documentation-only: current runtime keys, layout, mouse handling and permission behavior are unchanged.

## 2026-11-17 — Running-screen design pass and static visual studies

The user selects a compact running header (project/session, status/current step; full goal, roadmap and technical metadata under Details), a small expandable/hideable live-output preview, and an inspector that follows deliberate selection after it has been explicitly opened. The [focused running-screen design](24-running-screen-design.md) documents these choices, keeps unsupported live-agent messaging explicit, and adds three static PNG/SVG studies for default, inspector-open and compact-width presentation. The studies are fictional documentation assets, not working prototypes or terminal-cell fit tests. Application code, dependencies, input handling and execution behavior remain unchanged.

## 2026-11-17 — Approval-dialog studies with every valid grant visible

Following approval of the running-screen visual direction, the user selects every valid grant choice immediately visible rather than a scope-options menu. The [focused approval design](25-approval-dialog-design.md) adds four static PNG/SVG studies: exact shell command, pending native-write diff, global write confirmation and read-only Help above approval. Scope wording follows the current engine: shell reuse matches the exact full command, native-write reuse covers all permitted native writes, and the in-memory grant lasts for the current worker. Persistent confirmation cancellation denies the current request; closing Help returns to a still-valid approval without denying it. Typed modes still determine which choices are valid. This is documentation-only; application behavior and permission policy are unchanged.

## 2026-11-17 — Completed-workflow result design proposal

The user authorizes the next design-only pass. The [completed-screen proposal](26-completed-screen-design.md) adds three fictional PNG/SVG studies for unavailable measurement, scoped passing checks and a historical result after workspace restore. It separates neutral workflow completion, agent-reported conclusions, final-step verification and branch-visible native edits; it adds no automatic outcome classifier or model calls. Proposed actions lead to exact report versions, guarded edit review, recorded activity, explicit read-only manager questions and reviewed restart paths. This records a proposal for review, not user acceptance of every detail or application implementation.

## 2026-11-17 — Completed direction approved; attention/recovery proposed

The user approves the completed-screen visual direction and requests the next design pass. The [attention/recovery proposal](27-attention-and-recovery-design.md) adds four fictional PNG/SVG studies separating blocked-step instructions, terminal provider errors, guarded owner recovery and CLI native-write reconciliation. Action wording follows current contracts: recovery does not resume, restart restores/forks before explicit continuation, and provider retry policy remains unchanged. Exact interactions remain proposals for review. No application code or runtime behavior changes.

## 2026-11-17 — Session creation and composer proposal

The user requests the next design-only pass. The [creation/composer proposal](28-session-creation-and-composer-design.md) adds four fictional PNG/SVG studies for workflow selection, goal entry, read-only manager questions and idle-step resume instructions. It preserves explicit Start/Send effects and separates manager proposals from executable workflow delivery. Mode-specific drafts, literal-message handling and richer input controls are labeled proposed implementation work, not existing capabilities or newly settled user decisions. No application or routing changes.

## 2026-11-17 — Creation direction approved; Edits/revert and consistency reviewed

The user approves the overall creation/composer direction and requests the next pass. The [Edits/revert proposal](29-edits-and-revert-design.md) adds four fictional PNG/SVG studies for quiet file browsing, opened diff, reverse-diff confirmation and preparation refusal on manual drift. It preserves native-journal scope, first pre-edit snapshots, fresh lock/history checks and partial-failure distinctions. The [consistency review](30-ui-consistency-review.md) checks shared rules across the studies and explicitly lists remaining reader/navigation/detail surfaces and terminal validation. No application changes or blanket design-complete claim.

## 2026-11-17 — Edits review navigation selected; artifact-reader design proposed

The user requests an All files overview, Tab cycling through edited files and acceptance individually or together. The [Edits amendment](29-edits-and-revert-design.md) interprets acceptance as Keep of already-applied edits, separate from permission, Git staging/commit and guarded revert. Review markers, revision invalidation and focus escape are proposed implementation details, not existing tracking. Two static studies illustrate this revised direction. The [artifact-reader proposal](31-artifact-reader-and-history-design.md) adds four studies for exact-version content, versions, adjacent document comparison and provenance, retaining branch visibility and bounded preview/diff limits. No runtime implementation or model requests.

## 2026-11-17 — Supporting UI surfaces proposed

The user requests continued design work. The [supporting-surface review](32-navigation-checkpoints-details-and-help-design.md) adds six fictional PNG/SVG studies for empty home, session sidebar, exact-input human checkpoint, run details, usage and contextual Help. It distinguishes human input approval from result verification/tool grants/Keep marks, preserves usage uncertainty, and proposes candidate-versus-open session navigation with safe owner routing. Major surface families now have focused coverage; exact key/focus/compact behavior and minor states remain implementation-validation work. No application code, runtime Help, model calls or policy changes.

## 2026-11-17 — Denser layouts and no function keys

The user requests a more compact UI with arrows, j/k and Tab and rejects function-key shortcuts. Asked about the h conflict, they explicitly retain h for Help rather than h/j/k/l navigation. The [compact revision](33-compact-layout-and-navigation.md) replaces the proposed F6 escape with Enter to focus Edits actions (without activation), Esc to return, and local Tab routing. Two new static studies demonstrate denser Help and file review. Older studies remain historical; no runtime input/layout changes.

## 2026-11-17 — Compact direction approved; implementation handoff drafted

The user approves the denser direction and requests continuation. The [consolidated implementation plan](34-ui-implementation-plan.md) separates explicitly selected choices from detailed proposals and defines a first compact-workspace/theme/action/focus/Help/mouse/paste slice with acceptance evidence. Later slices retain independent Keep-review storage, explicit composer routing and existing approval/restore/accounting contracts. This turn consolidates planning only; no runtime changes, model calls or future-test claims.

## 2026-11-17 — First compact TUI slice implemented

The user authorizes implementation after the design handoff. The workspace now uses a compact header, optional sidebar/composer, explicitly opened readers, restrained live preview and shared semantic colors/actions. A More menu keeps additional actions reachable. h/? opens Help above pending decisions, with isolated keys and preserved return state; H accesses activity. Mouse buttons/rows share guarded routes, press/release targets are freshness-bound, editors accept literal bracketed paste and caret clicks, and terminal capture is cleaned up/suspended for editor handoff. Existing provider/permission/restore/accounting behavior remains authoritative. Keep markers, Edits file cycling and the remaining full-surface designs are later slices. Verification uses fixtures rather than paid model calls.

## 2026-11-17 — Correct result/artifact fidelity using actual Ratatui captures

The user reports a major mismatch between the approved drawings and the first code slice, supplies terminal screenshots and authorizes correction before more features. The result view now presents an attributed excerpt, scoped checks and native edits instead of a full report dump; exact report versions and complete result evidence remain separately accessible. Artifact rows are compact, actions are contextual, the full goal moves to Details and the shared dark theme/real button widgets match the visual direction. Actual TestBackend cell captures and 80×24 text baselines make the review concrete. Original drawings were static illustrations, not Ratatui proof. Permission, restore and provider policies are unchanged.

## 2026-11-17 — Distinguish primary actions, tab selection and keyboard focus

The user reports that repeated Tab presses leave Read final report permanently blue and make header stops appear inactive. The shared button renderer now uses primary accent without permanent fill; keyboard focus alone adds cyan fill, bold text and a visible marker for both framed and one-line controls. Main-tab selection has a separate renderer so it cannot mask focus, including when Activity appears as an action in More. A render regression walks all six result actions forward/backward at three sizes and verifies one visible marker/fill at every stop. Actual captures demonstrate report and header focus.

## 2026-11-17 — Display existing navigation shortcut badges

The user requests visible `[1]`/`[2]`/`[3]` view labels and clarifies that Sessions/Details/Help should display their existing letter keys, not new numeric assignments. Labels now show `[f] Sessions`, `[c] Details`, `[h] Help`, with reduced tab padding to keep all three view labels visible at 40 columns. Input routing and existing shortcuts are unchanged. Actual render captures and text baselines are regenerated and the 93 TUI tests pass.

## 2026-11-17 — Creation and explicit-recipient composer UI implemented

The user continues the UI pass after shortcut/focus review. Creation receives workflow roadmaps, a full goal editor and Start/Back/Clear/Cancel controls; cancelling retains the memory-only goal. Step, manager and command messages have explicit UI modes with separate session/branch-scoped drafts. Step instructions are passed verbatim through the guarded worker path even with manager routing enabled; slash-prefixed message content is literal. Control commands are entered separately through navigation `/`. Sending rechecks target/owner/write prerequisites and retains drafts on refusal; manager questions are saved before inference and never execute routes. More exposes saved manager exchanges and same-scope instruction restoration. Actual Ratatui captures plus local fixture-provider/PTY tests cover the new surfaces.


## Attention and recovery slice — 2026-11-17

Implemented concise cause/impact/next-action screens for blocked steps, typed provider failures, retry waits, interrupted ownership, unfinished native writes and external-owner observation. Explicit actions open exact evidence, full diagnostics, step instructions or read-only manager questions. Owner recovery retains the backend lock guard and never resumes; write recovery is guidance only. Partial restore failures warn that some files may already have changed. Manager failures and stale coding failures do not mislabel the current step. Full diagnostics retain complete evidence while default previews are bounded.

Verification: 305 tests passed, one existing test ignored; 106 TUI tests passed. Actual Ratatui captures at 80×24, 120×36 and 40×14 are indexed in [screenshots](screenshots/README.md). This completes the attention/recovery presentation slice, not the remaining run-details, checkpoint or Edits Keep-review work.


## Run details and usage implementation — 2026-11-17

Run details now separates the full goal, current state, roadmap and ownership from Technical details. Technical details labels current configuration separately from requested/returned models linked to a recorded attempt, exposes exact recorded definitions and retains unknown legacy metadata. Usage presents session-wide ledger counts with source/unknown labels, separate currency subtotals, partial-cost coverage and uncertain-response guidance. Request selection is identity-based across refresh; missing requests stay unavailable. Full accounting retains existing model/attempt and pricing-snapshot provenance. Back, keyboard/mouse traversal, Help and copy snapshots remain read-only; scope changes close the previous run's panel.

Actual [Ratatui captures](screenshots/README.md) cover 80×24, 120×36 and 40×14. Verification: 311 full-suite tests passed, one existing test ignored; 112 TUI tests passed; offline build and Clippy completed (five existing test-code warnings). The local fixture-provider PTY check covers run/usage/request browsing, nested Back/Help and unchanged request/history counts. No paid model calls. Session-picker refinements, richer checkpoints, Edits file cycling/Keep tracking and the final consistency pass remain separate work.


## Human checkpoint implementation — 2026-11-17

Human checkpoints and human step-result verification now transport typed review metadata from the engine. The TUI separates the review question and document selection from exact-version read-only evidence. Tab cycles input documents; Enter/e opens evidence and never approves. Decision keys are inactive in the reader and Help. The frontend rechecks owner/scope/attempt, the pinned manifest and displayed document versions before sending approval; stale/missing evidence removes approving controls while retaining Decline. The existing engine transaction still reselects and validates the complete checkpoint input set, including newly matching inputs. Verification evidence is labeled separately and continues to record only `verification.human`, without inventing pinned-input approval provenance.

Actual [Ratatui captures](screenshots/README.md) cover wide, narrow and unsupported-size states. Verification: 318 full-suite tests passed, one existing test ignored; 119 TUI tests are included. Offline build and Clippy completed with five existing test-code warnings. A local fixture-provider PTY check exercises two exact document versions, inert reader/Help decisions, Tab selection and a single approval recording both input versions, with no extra inference during review. Tool grant semantics, decline transitions and approval persistence are unchanged. Edits file cycling/Keep tracking, session navigation refinements and final consistency checks remain separate work.


## Edits review implementation — 2026-11-18

Implemented All files and file views, scoped Tab/Shift+Tab cycling, quiet overview row selection, explicit diff opening and separate action focus. Keep file/all persists review intent in `edits.kept` events keyed by session/branch/path/native revision; later writes/restores invalidate the earlier mark and forked branches require their own review. Keep checks fresh snapshots and disk contents under idle-session/native-write guards, records all requested revisions in one event and never mutates source, Git or authorization state. A saved marker describes a recorded revision, not ongoing disk validation; unkept edits are already applied and are not reverted on exit. Keep all refuses conflicts rather than accepting a partial set.

Revert retains the existing restore guards and explicit decision. Reverse diffs use bounded colored readers with prefixes, stale previews expire, Help captures decisions and undersized confirmations cannot activate. Actual [Ratatui captures](screenshots/README.md) cover wide/narrow lists, file diffs, saved review, drift and reverse-diff previews. Verification: 324 full-suite tests passed, one existing test ignored; 125 TUI tests passed after final presentation refinements. Offline build and Clippy completed (five existing test-code warnings). A local fixture-provider PTY check covers Tab cycling, inert focus transfer, Keep one/all and guarded single-file revert without extra inference. Session navigation refinements and the final consistency pass remain separate work.


## Session navigation and consistency pass — 2026-11-18

The browser now has separate candidate and opened-session identities. Filtering, row selection and cancellation preserve the active workspace and its scoped drafts; Enter/Open selected explicitly switches after an identity check. Wide terminals keep a sidebar beside the opened workspace; narrow terminals use a picker that preserves a visible selected row. Candidate identity survives background sorting, vanished candidates cannot be opened by stale position, and vanished opened sessions never fall back to another run. Pending local approvals close browsing and restore the execution owner.

Empty discovery, no matches, unavailable setup, discovery errors and history-load errors have distinct states. Keyboard-accessible retry is read-only. The consistency pass checks local focus/Tab contracts, literal editor input, mouse target freshness, approval/Help precedence, inactive workspace highlighting, small-terminal controls and existing result/artifact/recovery/checkpoint/Edits regressions. Empty home no longer advertises unavailable workspace tabs or run Details. Actual [Ratatui captures](screenshots/README.md) replace proportional studies as the review reference; creation captures are refreshed for the quieter empty-project header.

Verification: 330 full-suite tests passed, one existing test ignored; 131 TUI tests passed. Offline build and Clippy completed (five existing test-code warnings). The local fixture-provider PTY smoke verifies two sessions, explicit Open, cancelled/no-match browsing, preserved scoped drafts, keyboard editing/streaming/Help/capture cleanup and unchanged request/history counts during browsing. No paid calls. The major planned UI slices and focused consistency checks are ready for user acceptance; this does not implement manager live routing, new permission matching, or P2 work. Full Help text remains partly hand-maintained; generating every entry from one registry and minor visual refinements remain follow-up opportunities.


## 2026-11-18 — Make the completed-run overview discoverable

User testing found that leaving the initial result screen made its conclusion difficult to find again. The completed Activity footer now exposes `[s] Overview` beside the ordinary controls; the same read-only action appears in More from other workspace tabs and directly in artifact readers. Narrow layouts preserve the full shortcut label by moving Compose into More. Keyboard/mouse regression checks navigate away and return to the same saved result without requests or history writes. The 132 TUI tests, Clippy (existing test warnings) and offline build pass. Actual captures are indexed in [screenshots](screenshots/README.md).


## 2026-11-18 — Footer shortcut badges and active Overview

User testing requested visible shortcuts on all four Activity buttons and a persistent selected Overview control. Activity now labels Enter/Open selected, i/Compose, s/Overview and period/More. Overview retains its button with bold cyan text and a double border, distinct from the filled keyboard-focus style; it is not marked active while reading a report. Report/Edits/reader controls expose their actual contextual keys, with compact badges preserving reachability on narrow screens. Contextual shortcuts use their shared action callbacks and remain literal in editors.

Verified with 332 full-suite tests passed (one existing ignored), 133 TUI tests, offline build, Clippy (five existing test warnings) and a local-provider PTY check covering More/full-result shortcuts, editing, streaming and terminal cleanup. Actual cell captures and the two rendering baselines were regenerated and then tested without the baseline-update flag. No model calls occur when switching these read-only views.

### D-013 — Inspect complete recorded conversations without inventing missing reasoning (2026-11-18)

Expose branch-scoped conversation dumps through the CLI and the TUI's More menu. Preserve original events and exact new prepared message values with content-addressed deduplication. Offer bounded previews plus complete local Markdown/JSON exports. Parser rejection, tool-operation failure, nonzero exits and missing outcomes remain distinct. Legacy missing payloads, incomplete dispatches and ephemeral live buffers are explicit; private reasoning is never reconstructed. Dump actions do not execute or change workflow state. See [conversation dumps](35-conversation-dump.md).

### D-014 — Dismissing a tool-permission popup never denies the request (2026-11-18)

Tool approvals and persistent-grant confirmations are modal popups. Their valid permission keys are the only keyboard decisions; Esc hides the layer and sends nothing. The worker keeps waiting on the same request. An amber [a] Review permission control reopens it without granting approval. Background clicks and unrelated shortcuts cannot act through the modal, and permission keys outside it never answer the request. Human evidence checkpoints retain their separate review controls. This supersedes the earlier tool-approval Esc-as-deny behavior that could turn navigation into an unintended denial.
