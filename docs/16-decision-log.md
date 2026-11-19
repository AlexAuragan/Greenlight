# Greenlight decisions

These are settled product/runtime decisions, consolidated on 2026-11-19. Current behavior is described by [runtime documentation](README.md); unfinished work belongs in the [README roadmap](../README.md#roadmap--todo). Completed planning documents and superseded studies are recoverable from Git rather than duplicated here.

## D-001 — The harness owns execution facts and control

Rust owns tool enablement, permissions, transitions, verification and execution ownership. Model intent, prose and completion claims are not evidence that a command ran, a check passed or the user's goal was achieved. A justified stop is valid; the objective is not unconditional completion.

## D-002 — Native journal and explicit, limited restoration

Native writes retain immutable before/after content and durable intents before atomic file replacement. Recovery reconciles interruptions explicitly. Restore/fork/revert recheck journal coverage and current files; they do not claim arbitrary manual/shell/external changes or atomic multi-file rollback. Git checkpoints remain user-owned. See [edit history](07-edit-history.md) and [restore](09-workspace-restore.md).

## D-003 — Measure workspace drift before expanding rollback

Step-boundary hashes, scan metrics and native-journal correlation are implemented. They distinguish explained, unexplained and coverage-unknown changes without identifying the editor or storing arbitrary file bodies. Broader byte snapshots/rollback remain conditional on measured need. See [recording](14-workspace-recording.md).

## D-004 — Compact, explicit interaction and semantic colors

Keep the session sidebar hidden and composer closed by default. Enter/click explicitly opens an inspector, which then follows deliberate selection. Preserve scoped memory-only drafts and reading positions. Use arrows, j/k and Tab; require no function keys. `h` opens Help where its layer permits it. Page/navigation keys remain optional alternatives, not requirements.

Blue identifies user input, cyan focus/active controls, neutral text model output, green recorded positive outcomes, amber attention and red errors/destructive actions. Active view and keyboard focus are distinct. Keep controls contextual, their shortcuts truthful, and important decision consequences visible. See [the TUI guide](11-tui.md).

## D-005 — Bounded documents and exact version evidence

The Markdown subset, raw view, version history/provenance and adjacent-version diffs are implemented. Preserve original source; escape terminal controls and label clipped previews. Copy/export can retain the full selected evidence. Pin human approvals and consumer context to immutable artifact versions; do not substitute latest-by-name content into historical claims. Full CommonMark, images and editable artifact import are separate scope.

## D-006 — Saved permission scope differs from pending decisions

Once, process, project and global grants have distinct lifetimes and remain subject to configured deny. Broad project/global grants require explicit confirmation. A durable session-scoped grant is still TODO. Unanswered approvals belong to a live worker, expire on loss/recovery and are never replayed. Execution uses OS locks and owner records; observation does not imply control. See [approvals](08-persistent-approvals.md).

## D-007 — Project-local Greenlight state with guarded legacy support

Initialize `.greenlight/` per project; local settings, workflows, instructions and private state stay separate from the installed binary. Native read/write scope is the selected project root. Legacy config/instructions and `.stepwright/state` remain supported with explicit conflict checks. Initialization/upgrades preserve user-edited project files. See [projects](22-installation-and-projects.md) and [migration](15-name-migration.md).

## D-008 — Activity titles are optional intent, not outcomes

Validated coding-agent `activity_title` annotations are implemented. They link to the original response and remain distinct from factual tool/verification cards. They add no inference call, execution authority or separate phase lifecycle. Summarization is optional future work, not a prerequisite for readable activity. See [the action protocol](13-agent-action-protocol.md).

## D-009 — Manager questions do not execute routes

Grounded read-only Q&A, bounded snapshots, citations and unexecuted forward proposals are implemented. The manager can reuse the provider/model and requires no second subscription. Direct step instructions and manager questions have explicit recipients. Project TODO progress and pipeline progress are separate evidence. Executing targeted delivery/restart and a durable owner-bound inbox remain governed by the unfinished [coordinator contract](20-conversational-coordinator.md). See [current manager behavior](21-manager-qa.md).

## D-010 — Live preview is not live instruction delivery

Streaming previews are ephemeral and unvalidated; only a complete response reaches action dispatch. Stopping future provider requests is implemented; an in-flight request or tool may finish. Durable live steering, mid-run queues and cross-process continuation remain TODO. Never acknowledge delivery or cancellation that did not occur.

## D-011 — Review applied edits without inventing staging

All files/per-file views, Tab cycling, Keep markers and guarded revert are implemented. Keep records review of a native revision; files are already applied. It neither stages Git changes nor grants permissions, certifies correctness or protects an outdated revision after further edits. Drift/stale previews block destructive application. More grouped review is future work, not an automatic new staging engine.

## D-012 — User-owned Git and first-class artifacts

Do not auto-commit by default. Artifacts are immutable, branch-aware documents and are not necessarily source files. External editor review uses a read-only temporary snapshot; edits are not imported automatically. Explicit registration creates a new artifact version. See [persistence](03-persistence.md) and [TUI exports](11-tui.md#reading-state-filtering-and-export).

## D-013 — Inspect recorded conversations without inventing reasoning

Branch-scoped dumps preserve original events and exact new prepared messages with content-addressed deduplication. Bounded previews and complete local Markdown/JSON exports distinguish parser rejection, tool failure, nonzero exits and missing outcomes. Legacy gaps and ephemeral buffers remain explicit; private reasoning is never reconstructed. See [dumps](35-conversation-dump.md).

## D-014 — Dismissing a tool-permission popup sends no decision

A modal tool/grant popup captures its valid decision keys; Esc hides it and leaves the worker waiting. `[a] Review permission` reopens it. Background clicks, unrelated shortcuts and permission keys outside the popup cannot answer it. Human checkpoints retain their separate evidence review and decline controls; Help/reader layers never authorize a decision. See [current controls](11-tui.md).

## D-015 — Retrying a provider request differs from continuing a step

Live workflow Retry request retains the prepared messages, call frame, attempt and branch without replaying tools. Stop for now ends that continuation; Esc hides it. Honor remaining provider cooldowns and preserve per-dispatch usage and uncertain billing. Continue step for an already-stopped run starts a fresh attempt and may run earlier tools again. Tool-operation retry and durable/manager continuations remain TODO. See [provider recovery](19-provider-failures.md).

## D-016 — Discover within the project and repair envelopes without guessing

Directory reads reuse the existing read policy and return bounded project-relative paths/substring matches. Reject absolute paths, parent traversal and symlink escapes; omit private state/settings, known secrets and caches. Regular file reads have a bounded size/allocation. See [file discovery](36-native-file-discovery.md).

Accept one unambiguous fenced action with ordinary surrounding commentary, preserve literal content and original raw output, and reject multiple actions or unfinished thought/analysis envelopes. Missing required fields enter protocol correction before dispatch. Repair never rewrites commands, grants tools or bypasses workflow/host policy.

## Accounting and pricing

Input, cached-input and output usage is recorded per physical request, with source/unknown labels and immutable rate snapshots. Retries incur separate records; inherited branch evidence does not create another charge. tiktoken estimates are qualified; absent rates/cache data are not zero. DeepInfra discovery, explicit overrides, context tiers and cache-creation rates are implemented. Invoice reconciliation, broader provider adapters and budgets remain separate. See [accounting](18-token-usage-and-pricing.md).
