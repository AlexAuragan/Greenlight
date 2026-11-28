# Greenlight decisions

These are settled product/runtime decisions, consolidated on 2026-11-19. Current behavior is described by [runtime documentation](README.md); unfinished work belongs in the [README roadmap](../README.md#roadmap--todo). Completed planning documents and superseded studies are recoverable from Git rather than duplicated here.

## D-001 — The harness owns execution facts and control

Rust owns tool enablement, permissions, transitions, verification and execution ownership. Model intent, prose and completion claims are not evidence that a command ran, a check passed or the user's goal was achieved. A justified stop is valid; the objective is not unconditional completion.

## D-002 — Native journal and explicit, limited restoration

Native writes retain immutable before/after content and durable intents before atomic file replacement. Recovery reconciles interruptions explicitly. Restore/fork/revert recheck journal coverage and current files; they do not claim arbitrary manual/shell/external changes or atomic multi-file rollback. Git checkpoints remain user-owned. See [edit history](07-edit-history.md) and [restore](09-workspace-restore.md).

## D-003 — Measure workspace drift before expanding rollback

Step-boundary hashes, scan metrics and native-journal correlation are implemented. They distinguish explained, unexplained and coverage-unknown changes without identifying the editor or storing arbitrary file bodies. Broader byte snapshots/rollback remain conditional on measured need. See [recording](14-workspace-recording.md).

## D-004 — Compact, explicit interaction and semantic colors

Keep the session sidebar hidden and composer closed by default. Enter/click explicitly opens an inspector, which then follows deliberate selection. Preserve scoped memory-only drafts and reading positions. Use arrows and Tab; `k` stops execution. Require no function keys. `h` opens Help where its layer permits it. Page/navigation keys remain optional alternatives, not requirements.

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


## 2026-11-19 — Explicit stop key and arrow navigation

Use arrows for list/reader navigation; remove j/k aliases, retaining semantic controls such as h Help and m Markdown/raw. Reserve k for Stop and keep q exclusively Quit. Text editors and decision layers own their input. Stop cancels local provider work, retry waits, web requests and sandboxed commands, releases hidden unanswered prompts without inventing an approval decision, and checks again before dispatching a returned model action. Already-applied edits remain and synchronous journaled writes finish safely. Remote processing/billing and partial command effects can remain unknown. Continue starts a new attempt.

Historical tool reruns remain GL-001: normal failures go back to the model; an explicit user rerun should append a linked user-requested execution/result using the original arguments without another approval prompt, while preserving project/tool boundaries. Do not automatically replay tools.


## 2026-11-19 — User-requested tool executions

Implement GL-001 as an explicit rerun of a selected historical call, not an automatic pause/retry loop. Stop execution first. The Run tool again action authorizes only that exact invocation without another approval or saved grant; current tool/project boundaries remain. Append a new request/result and user provenance linked to the original. Reuse execution ownership and native journaling, never replay after crashes automatically, and provide bounded results to a fresh attempt. Completed sessions return to blocked review after reruns; no new verification/completion is implied. See [tool rerun contract](37-tool-reruns.md).


## 2026-11-19 — Durable provider continuations

Provider calls publish versioned immutable checkpoint blobs through branch-visible continuation state events. Save exact messages, model, attempt, execution phase, completed verification checks and manager interaction/snapshot identity; keep credentials and unanswered approvals out of checkpoints. Retry explicitly, reload credentials on restart, retain Retry-After and physical-request accounting, and validate project hashes, configuration, inputs, branch, attempt and checkpoint identity under ownership. Preserve failed/corrupt checkpoints without silently starting fresh work. Resume coding/verification/detector phases without replaying earlier tools/checks; manager recovery is independently locked and read-only. Fresh instructions intentionally discard the workflow checkpoint and create a new attempt. See [provider continuations](38-provider-continuations.md).


## 2026-11-20 — DeepInfra capacity responses and inspectable API errors

Provider support identified the reported 429s as shared model-pool Model busy responses, not the account concurrency ceiling. The earlier parser missed that phrase and discarded response text, so old exact wording cannot be recovered. Recognize busy/capacity responses and allow up to five sequential attempts with 1/2/4/8-second jittered exponential backoff, respecting Retry-After. DeepInfra unknown non-billing 429s use the ordinary bounded retry budget; other providers retain explicit classification. Never auto-retry quota/billing or usage-bearing/uncertain processed responses. Keep the concise safe advice, while separately persisting bounded redacted body/request-ID diagnostics for Provider details, individual requests and conversation exports. Add opt-in service_tier: priority without changing defaults or applying standard-tier price estimates to priority billing.


## 2026-11-20 — GL-002 recovery review

Revalidate saved requests after waiting, not just before cooldown: project files or session state can change during a pause. Apply the same guard to manual and automatic retries, retain the old checkpoint on refusal, and show a saved-cooldown waiting card. Re-read persisted session identity/step/goal during validation. Report malformed checkpoint metadata without terminating the TUI or hiding raw history; validate workflow and manager slots independently. A child-process regression exercises dotenv credential reload, exact messages, unchanged attempt identity and no repeated tools.


## 2026-11-21 — Recovery branch pre-merge validation

Review GL-001/002, stop controls and provider capacity/diagnostic changes together. A native TUI smoke fixture caught diagnostics appended after the decision-help delimiter, which the popup intentionally trims. Keep diagnostics before that final delimiter and test that provider-supplied decision-like text remains evidence rather than a control. Retain stable backlog IDs; GL-004 approval-save serialization was already completed.


## 2026-11-21 — GL-003 descriptor-anchored file access

Replace check-then-open path IO with a small Linux directory-capability layer using openat/O_NOFOLLOW and pinned file descriptors. Inspect O_PATH handles before reading regular files; publish mutations relative to pinned parents, verify published inode identity, retain write intents on uncertainty, and refuse changed parent bindings around journaled writes/restores. Route includes, discovery/workspace scans, state blobs, exports, approval saves and locks through that layer. Preserve SQLite descriptor paths with a narrow VFS pathname adapter; default SQLite canonicalization would otherwise undo the anchor. Bubblewrap receives project/cache descriptors using bind-fd options. Existing privacy/approval/sandbox policy remains; no unsafe fallback. Native race/SQLite tests pass, while this cloud environment blocks the optional live sandbox test at its read-only UID-map interface. GL-027 usability work remains deferred.


## D-017 — Exclusive project execution (2026-11-22)

One workflow/tool rerun or workspace control scope owns a project at a time. Ownership spans inference, verification, transitions and approval/input/retry waits. Read-only inspection and manager questions remain available. A busy result never queues work; goals/drafts remain reviewable. Durable owner records require explicit crash recovery, and restore plus fork publication shares one lease. See [project coordination](41-project-execution-coordination.md). Parallel orchestration remains separate work.

## D-018 — External file bodies remain opt-in future work (2026-11-22)

GL-011 confirms that hashes cannot recover external preimages, but after-step changed-file capture cannot recover those preimages either. Keep bounded hash auditing, native-write snapshots and overwrite protection; do not introduce implicit whole-project body retention or broader restore claims. A future implementation requires a concrete recovery case, explicit narrow opt-in, aggregate quotas, privacy/cache exclusions, stable captures and reference-aware retention. See the [evaluation and measured baseline](45-file-body-snapshot-evaluation.md). No runtime/schema/provider behavior changes in this decision.

## D-019 — Auto-approve a bounded project-inspection language (2026-11-22)

GL-012 uses a static supported argv/option subset, fixed system executables and private filtered per-command views inside the existing sandbox. It does not turn shell strings into prefix/wildcard grants. Preserve explicit policy/grant/rerun semantics; reject private/outside/parent paths from automatic classification, and retain exact approval for shell syntax/unsupported forms. Temporary inspection views do not become persistent recovery snapshots. Metadata-dependent commands remain approved through the original path rather than reporting staged metadata as original. See [inspection permissions](46-project-inspection-permissions.md); safe pipelines remain GL-031.

## 2026-11-23 — Merge supervised review and inspection stack

Merged GL-009 through GL-017, the GL-031 design and GL-032 implementation into `master` after review and 546 passing serial tests. Preserve existing grant/deny precedence and explicit user control; summaries, grouped review and role/budget defaults remain optional. Corrected artifact review feedback for post-publication ownership-cleanup failure. GL-027 and GL-033 remain deferred sandbox work, and live namespace validation is still host-blocked.

## 2026-11-28 — Close the functional sandbox-validation slice

Accept the user-host v4 direct-fetch smoke as supplied compatible-host evidence, keeping the earlier cloud DNS failures and v3 Python-wheel failure intact. GL-035 completion means reviewed functional containment/offline dependency/result-integrity coverage, not every proposed performance metric. Move interactive/non-verification profiling into GL-025 and supported exclusive whole-tree memory/write accounting into GL-023, with explicit acceptance details in TODO. The current non-delegated cloud cannot supply trustworthy exclusive peak accounting; child RSS/block counters/final cache footprints remain narrower observations. Original Git semantics remain GL-020. See the [closure rationale and evidence](57-sandbox-validation-review.md).
