# Greenlight — TUI UX analysis and redesign plan

> **Status:** dated product/UX research and design history, **not a current implementation specification**. Some descriptions and proposed keys are deliberately preserved as historical evidence.
> **Original baseline:** `feat/ratatui-session-viewer` / PR #6, 2026-11-12. **Current as of 2026-11-13:** PRs #6–#9 are merged; the Ratatui TUI, basic Markdown renderer, guarded native-file restore, workspace manifests and Greenlight rename exist. See [current TUI behavior](11-tui.md), [execution contract](01-execution-contract.md), [README status](../README.md#roadmap--todo) and the [decision log](16-decision-log.md).
> **Implementation update (2026-11-14):** the P1 branch now has session workspaces, a searchable picker, in-TUI creation, keyboard help, explicit activity following, per-session memory-only drafts and compact resizing. The proposals below remain historical; current keys and limits are in [the TUI contract](11-tui.md).
> **Audience:** people using Greenlight to supervise AI coding workflows; contributors implementing the terminal interface.
> **Companion:** the prioritized checklist in [README.md](../README.md#roadmap--todo).
>
> **North star:** *At any moment, the user understands what Greenlight is doing, why it matters, what evidence exists, and what they can safely do next.*

## 1. What problem are we solving?

Greenlight's differentiator is **human-supervised, artifact-driven execution**. It is not supposed to finish every request at any cost. A justified stop is a valid result. Every step has a declared purpose, inputs, outputs, verification and permitted transitions.

The UI must translate those internal contracts into a reassuring, usable experience. The user should not need to interpret SQLite event names, guess which keyboard shortcuts are active, remember a session UUID, or ask the agent what it was just doing.

### The five questions every active screen should answer

1. **Where am I?** Project, session, branch, workflow, active step and attempt.
2. **What is happening?** Current action, meaningful progress and last verified outcome.
3. **Why?** Step objective, planned output, relevant artifact or verification evidence.
4. **What needs me?** Exact decision, its impact, and the safe/default response.
5. **What can I do?** A contextual primary action and discoverable secondary actions.

**A readable status is insufficient if it has no corresponding next action.** In particular, a session labeled **Blocked** should immediately expose why it stopped and a way to recover without memorizing `resume`, `fork` or event IDs.

## 2. Evidence and present baseline

### Observed problems from hands-on use

- The initial TUI showed **Blocked** but did not make recovery obvious. Enter on the session should lead to a useful instruction/composer flow.
- The message composer looked like the placeholder was already typed text; the placeholder needed a distinct, subdued treatment.
- Tool permissions were presented as console text inside the TUI. `y`/`g` appeared to select choices while Enter submitted them; Enter alone could deny a request, and the agent's subsequent retry looked like a successful approval.
- The event-centric display buried useful outputs. Artifacts are a primary **conversation medium between the agent and human**, not just files to retrieve through the CLI.
- Asking the model to “restart from implement” is not equivalent to safely rewinding workflow state and tracked filesystem changes.

These observations motivate an **interaction redesign**, not just visual polish.

### Original PR #6 baseline (historical snapshot, 2026-11-12)

- CLI start/resume, terminal prompts, branch-aware SQLite events, immutable artifact versions, native edit diffs and guarded restore/fork operations.
- Linux Bubblewrap isolation for shell tools; native writes remain separately permissioned and journaled.
- Ratatui session list, conversation-like event timeline, composer that can resume ready/blocked sessions, an in-TUI permission bridge, and a `/restart` flow with restore preview.
- A second tab showing the **latest branch-visible version** of each artifact as **raw Markdown/text**, with producer step and size.
- The current TUI can observe other processes but only controls its own worker. There is **no strong cross-process session lock**, model token streaming, or real-time remote interaction with another process.
- The workflow has step IDs, but the store lacks a durable first-class **attempt ID**. Event-based inference is currently necessary.

### Still missing after PRs #6–#9 (status check, 2026-11-13)

Artifact version navigation and in-TUI editing; a proper workflow graph/stepper; guided blocked-state recovery cards; event search/filtering; a shared persistent execution service and robust process ownership; summary/manager model roles; authoritative per-attempt run records; and end-to-end UX/accessibility testing. **A deliberately small Markdown preview/raw toggle and step-boundary file-hash recording are implemented**, but neither satisfies the complete review/provenance design here.

**Source of truth:** [execution contract](01-execution-contract.md), [workflow format](02-workflow-format.md), [restore limits](09-workspace-restore.md), [permission semantics](08-persistent-approvals.md), and [current TUI behavior](11-tui.md). Some older v0.1 docs describe pre-sandbox/pre-TUI behavior and should eventually be reconciled.

## 3. Users, jobs, and design principles

### Core jobs

- **Delegator:** “I know the outcome I want; take a workflow as far as it safely can, and tell me when I matter.”
- **Reviewer:** “Show me the plan, edits, tests, and relevant documents so I can make an informed decision.”
- **Debugger:** “A step failed. Explain the blocker and let me retry or go backward without losing the record.”
- **Maintainer:** “I have multiple sessions, interruptions and branches. Let me safely resume and understand what changed.”

### Principles, ordered by importance

1. **Human agency over autonomous cleverness.** A manager model may recommend; the harness validates; the person approves consequential actions.
2. **One obvious primary action per state.** Running → observe; permission required → decide; blocked → resolve; completed → inspect result.
3. **Progress is evidence-based.** “Step completed” requires the harness's verification result, not model confidence.
4. **Artifacts are first-class.** Plans, reports, tests and decisions deserve readable, navigable, versioned presentation.
5. **Show summary first, raw evidence on demand.** The user can drill into full logs, diffs, YAML and history without being forced to read them.
6. **No invisible execution or scope escalation.** A scope grant, write, restore, fork, branch switch or external network action must be legible.
7. **Safe defaults; no deceptive success.** Enter cannot silently approve; denial and retry are visually different; errors are not swallowed.
8. **A coherent keyboard model.** Focus, shortcuts, modals, confirmation and escape behavior are predictable.
9. **Scriptability remains intact.** TUI and CLI must share the same engine, security decisions and state semantics.
10. **Small terminals and slow networks are normal.** No clipped critical decision, frozen UI, hidden spinner, or opaque waiting period.

## 4. Desired information architecture

A **session workspace**, not three unconnected log panes:

```text
┌ Greenlight · project · session title ──────────────────────────┐
│ Workflow: add-feature    Status: BLOCKED    Branch: b3…        │
│ Plan ✓ ── Review plan ✓ ── Implement ✓ ── Verify ! ── Done ·   │
├ Sessions ┬ [1] Activity   [2] Artifacts   [3] Changes         ┤
│          │                                                     │
│ ▶ Feature│  Verify · BLOCKED                                   │
│   Bugfix │  Tests failed: test_add expected 5, received -1    │
│          │  Evidence: test-results.md · edit #72 · stderr     │
│          │                                                     │
│          │  Suggested next step: inspect → retry implement    │
│          │  [View evidence] [Retry] [Restart from…]            │
├──────────┴─────────────────────────────────────────────────────┤
│ Message the session manager / guide the agent                 │
│ Tell Greenlight how you want to proceed…                      │
├────────────────────────────────────────────────────────────────┤
│ Tab focus · / commands · ? shortcuts · Esc back               │
└────────────────────────────────────────────────────────────────┘
```

Conceptual layout only: not a screenshot or a promise that all controls exist today.

### Persistent regions

| Region | Must show | Must not do |
| --- | --- | --- |
| Session header | Human-readable goal/title, status, current step, active branch, running owner | Require UUIDs for ordinary navigation |
| Workflow stepper | Ordered steps and observed states; current attempt; verification outcome | Guess completion from model prose; assume a linear graph if workflow loops |
| Main tab | Activity, Artifacts, Changes (potentially History/Branches later) | Mix every raw protocol event into the default view |
| Context detail | Selected artifact, evidence, diff, test output, or decision | Destroy current scroll/selection on every refresh |
| Composer/action dock | State-specific primary action, user instruction, and optional manager Q&A | Pretend that chatting necessarily resumes or modifies the workflow |
| Decision overlay | Explicit required approval/restore confirmation, scope and consequences | Allow unrelated keys or implicit Enter to grant rights |

### Screen hierarchy

- **Home / session picker:** create a session, resume recent work, see blocked sessions first, locate by human-readable goal.
- **Session workspace:** active workflow, timeline, contextual actions, artifact and change views, composer.
- **Decision view:** modal/drawer that temporarily owns focus for a tool permission, human approval, or destructive state transition.
- **Settings/help:** keybindings, active provider/model roles, current permission scope, sandbox status, cost/usage if supported.

Prefer **one primary session workspace** over nesting multiple unrelated full-screen pages. Tabs should change the content panel, not reset the running session or composer draft.

## 5. The hot paths, in order of importance

Each path below describes a user intention and the expected product response, not an existing guarantee.

### HP-01 — Start useful work

**Trigger:** the user opens Greenlight for a project or wants a new session.

1. Show project context and available workflows with plain-language descriptions.
2. Let the user choose a workflow, set a goal, review model and sandbox requirements, and start.
3. Show the step roadmap immediately, including required human checkpoints.
4. Transition into the session; expose current step objective and forthcoming outputs.
5. If configuration, credentials or sandbox checks fail, offer a precise fix without creating a phantom running session.

**Expected:** “I know what will happen before I hand control over.”  
**Success criterion:** start requires no memorized CLI syntax; failure always identifies the exact missing prerequisite.

### HP-02 — Monitor while the agent works

**Trigger:** a workflow is running; the user is reading rather than interacting.

1. Display **current step**, short description and what artifact/verification is expected next.
2. Update activity with structured milestones: step started, tool request/result, file edit, artifact created, verification passed/failed.
3. Group consecutive repetitive tool calls; show concise human-readable titles first and expand raw details on demand.
4. Maintain scroll position when the user is inspecting an older item. Follow the tail only when they were already following it.
5. Clearly differentiate **working**, **waiting for model/network/tool**, **waiting for approval**, and **blocked**.
6. Show the final summary and a direct link to produced artifacts and diffs.

**Expected:** “I can understand the job without reading YAML or interrupting it.”  
**Success criterion:** the current action and last confirmed milestone are visible without scrolling.

### HP-03 — Approve a tool or human checkpoint

**Trigger:** the engine pauses for shell, write, web, human or verification approval.

1. Stop the normal flow and present a **typed** decision surface.
2. State *what*, *why*, *where*, and the **exact scope** of the proposed grant; show command text or native write diff.
3. Display only valid actions: allow once, allow process, project/global trust where supported, deny.
4. Make every actionable key unambiguous. An explicit key activates it; Enter has **no** hidden permission semantics.
5. Confirm the engine received the decision; distinguish **approved**, **denied**, **failed to persist approval**, and **agent tried another action**.
6. Preserve the approval context in the audit trail. Do not collapse a failed write into a green “continued” state.
7. For high-risk scope changes (especially global native writes), require deliberate confirmation and explain that future prompts may be skipped.

**Expected:** “I know exactly what this will permit now and later.”  
**Success criterion:** nobody can confuse Enter or a retry after denial with an approval.

### HP-04 — Understand and unblock a stopped step

**Trigger:** a session becomes blocked because of failed verification, unknown deviation, missing input or tool error.

1. Bring the **blocker card** to the foreground: failing step, root evidence, latest attempt, and whether the workspace changed.
2. Offer **Inspect evidence** as the first safe action (artifact, failure output, diff, step definition).
3. Offer valid recovery options based on actual state: **Resume current step**, **Retry**, **Restart from step…**, **Edit artifact then retry**, or **Stop**.
4. For a free-text request, distinguish an instruction to the coding agent from a question to the manager; label which recipient will act.
5. Never silently change the step on behalf of an LLM. Present proposed target/checkpoint, consequences and confirmation.
6. If the user denies a permission, show it as a rejection and explain if the agent is still running or subsequently blocked.

**Expected:** “I don't need to deduce which CLI command or event ID unblocks this.”  
**Success criterion:** from a blocked screen, evidence is one action away and an allowed recovery is discoverable without help.

### HP-05 — Read an artifact and respond

**Trigger:** a plan, review, investigation, test report or implementation report is created or referenced.

1. Surface a readable artifact chip/card in the activity timeline and the Artifacts tab.
2. Open a **rendered Markdown preview** by default, with raw text available instantly.
3. Show its name, producing step, version, branch, timestamp/event and whether it's required for a later gate.
4. Navigate headings and code blocks, copy text, search within the artifact, scroll without losing position.
5. Enable comment/request-revision actions later. Show precisely whether feedback becomes a **new artifact version** or **an instruction on resumption**.
6. For approval checkpoints, display the exact artifact version **being approved**, not an accidentally refreshed latest copy.

**Expected:** “The documents are how I inspect and steer the work.”  
**Success criterion:** the plan can be reviewed end-to-end without copying it to an external editor.

### HP-06 — Restart at an earlier step safely

**Trigger:** “go back to implement”, “redo the plan”, “retry from before the failed change”.

1. Show workflow steps and their **attempts**, not only a flat list of raw event IDs.
2. Explain that **restart** means a **new branch** from a selected checkpoint (preserving prior history), optionally restoring tracked native file edits.
3. Show a precise restore preview with create/modify/delete operations, diff and paths. Mention unsupported/untracked changes.
4. Default to **cancel**, never silently “fork without restore”. Refuse on drift, symlinks and unsupported paths.
5. On confirmation, record the fork/restore, show the **new branch and current step**, and let the user supply a revised instruction before running.
6. If restoration partially succeeds and then fails, stop with a prominent warning and concrete recovery steps. No automatic retry.

**Expected:** “I can change direction without rewriting history or losing code silently.”  
**Success criterion:** user can predict workspace consequences and distinguish historical data from on-disk state.

### HP-07 — Understand completion and export the result

**Trigger:** all required steps and verification have completed.

1. Present outcome, verified checks, produced artifacts, changed files and unresolved warnings.
2. Offer **Review changes**, **Read final report**, **Copy summary**, and **Start follow-up session**.
3. Explain whether file changes are already in the working tree; do not imply a Git commit exists.
4. Make the full trace accessible but secondary.

**Expected:** “I can quickly decide whether the work is ready for my normal Git/review process.”  
**Success criterion:** final screen is useful without reading the entire timeline.

### HP-08 — Resume tomorrow / switch sessions

**Trigger:** TUI reopened, user has several sessions or the process was interrupted.

1. Show friendly labels, last activity, workflow, blocked/running/completed state, branch and project.
2. Distinguish a **live owned run** from a stale `running` flag after process termination.
3. Restore safe UI preferences (tab, selection, optional drafts) without replaying an action.
4. Enforce single-owner execution per session across CLI/TUI processes; allow read-only observers.
5. Make session archive/search/filter and branch history discoverable.

**Expected:** “I don't need to reconstruct what happened from the terminal's last scrollback.”  
**Success criterion:** reopening never duplicates a run or strands a pending approval invisibly.

## 6. State model and primary action

| State seen by user | Primary action | Secondary actions | Important copy/feedback |
| --- | --- | --- | --- |
| No project/workflow | Configure / choose workflow | Help, check sandbox | Missing requirement shown explicitly |
| Ready | Start / Resume | Review workflow, change instruction | “Ready to run step X” |
| Running | Inspect progress | View artifact/diff, request interruption (future) | “Working on X”; last event visible |
| Waiting on tool/model | Observe | Expand raw request | Distinguish latency from a frozen process |
| Waiting on tool permission | Allow once / Deny | Process/project/global scope where valid | Exact command/diff; default safe |
| Waiting on human checkpoint | Review artifact, Approve / Reject | Open raw/source | Gate names exact version/evidence |
| Blocked | Inspect blocker → choose recovery | Ask manager, restart | Reason, evidence and safe next moves |
| Restore preview | Cancel / Confirm restore+fork | View diff, compare branches | No changes until deliberate confirmation |
| Restore partly failed | Inspect and recover manually | Export error details | Never show false success |
| Completed | Review outcome | Artifacts, changes, new run | Verified facts versus model claims |
| Failed process/disconnected owner | Reconnect/recover | Inspect logs | State reconciled before allowing resume |

**Important:** storage uses coarse `ready/running/blocked/completed` states today. The richer UI states are design targets; derive them from typed engine events and durable attempt/owner records rather than arbitrary log text matching.

## 7. Markdown artifacts: rendering and navigation contract

Markdown rendering is a high-value **P0/P1 boundary**: it materially improves human review, but should not block reliable control of a running agent. Prefer a single parser/renderer shared by previews and printable exports.

### Initial rendering support (MVP)

- Headings with clear hierarchy; paragraphs, emphasis, inline code, ordered/unordered/task lists, blockquotes and thematic rules.
- Fenced code blocks with monospaced layout, preserved indentation and a visible language label; optional syntax color with a plain-text fallback.
- Links shown as styled text with a safe **open URL / copy URL** action; no automatic requests.
- GFM-style tables when width permits; horizontal scrolling or a stacked/plain fallback on narrow terminals.
- Unicode-aware wrapping and horizontal scrolling for code; preserve meaningful whitespace and line numbers where helpful.
- **Preview / Raw** toggle. Never lose raw original content. Search within the current document.
- Large documents: incremental render/virtualization or bounded caching, remembered scroll, clear truncation indicator when limits are reached.
- Images: descriptive placeholder/alt text (and optional external open action); terminal image protocols are not assumed to be supported.
- Raw HTML and terminal escape sequences from model-generated artifacts are **untrusted**: sanitize or display literally, never execute HTML, shell, ANSI control codes or links automatically.
- Relative links: resolve only against known, allowed project paths if a safe file-viewer exists; otherwise show the path as text.

### Version semantics

- Default: latest artifact **visible on the selected branch**.
- Later: version picker with producing step, attempt, event ID, timestamp and branch provenance; show differences between versions and a “referenced by step” relation.
- An approval freezes the **exact version ID**; a newer version must not silently change what was approved.
- Editing a document produces a **new immutable version**, not mutation of an old blob. Show conflicts when the source changed since editing began.
- Artifacts are **registered UTF-8 content**, not necessarily project files. Don't imply that editing their preview directly writes to the workspace.

### Terminal behavior (reconciled after keyboard testing)

The shipped layout follows a **Yazi-inspired pane hierarchy**: Sessions → event/artifact list → detail. **Right/Enter** enters the next pane; **Left/Escape** returns. `i` explicitly focuses the composer, which is **never automatically focused** on opening, even for blocked sessions. From detail, Enter can focus the composer. Up/Down or `j/k` selects in lists and scrolls focused details, including approval and restart previews. **The earlier `PgUp/PgDn` suggestion was rejected** as awkward on some keyboards (notably AZERTY); those bindings were removed. Tab also navigates focus. Future document search should not hijack composer or slash-command input, and copy/return behavior remains a future usability improvement. See [current key map](11-tui.md).

## 8. Conversation, composer and keyboard UX

The current TUI is event-oriented. The redesign should use **semantic conversation cards** based on typed events:

- **User:** instruction and its destination (coding agent or manager).
- **Agent:** brief current intent/result with a link to full transcript and raw YAML.
- **Tool:** command, elapsed time, exit status and collapsed output; diff opens separately.
- **Artifact:** name, producing step, current version, quick preview.
- **Verification:** test command/check and pass/fail evidence.
- **Decision:** who decided, what was allowed and for which scope.
- **System:** blocked/restarted/completed with branch and step provenance.

The composer requires a clear mode:

| Mode | User's message means | May mutate code or workflow? |
| --- | --- | --- |
| **Instruct agent** | Resume/steer the selected ready or blocked coding step | Yes, via normal engine permissions |
| **Ask manager** (future) | Answer a question, explain evidence, propose action | No |
| **Confirm proposed action** | Validate and perform a typed, explicitly previewed operation | Only the specific confirmed operation |

Do **not** use a single opaque “chat” submit action whose recipient and side effects change with state without indication.

### Keybinding rules

- One primary global shortcut to view help, e.g. `?`; avoid shadowing typed composer text.
- Dedicated focus indicator and on-screen hint at each active panel.
- Keyboard keys operate **only in their visible context** (especially `y`, `g`, `Enter`, `Esc`, `/`).
- No auto-approval on `Enter`, even after selecting an option.
- Multiline composer, cursor movement, word delete, paste, selection/copy where supported; avoid sending on paste.
- Keep unsent drafts when switching tabs/sessions; clearly indicate which session owns a draft.
- Permission modals take focus and suppress unrelated shortcuts; no hidden global grant.
- Resize support and meaningful compact layout at **80×24**, with a one-pane fallback for narrower terminals.

## 9. Two optional model roles: summarizer and manager

There is already a **coding agent**. Consider two **additional roles**, which may point to the same provider/model initially. They are **not** extra permission bypasses.

### 9.1 Summarizer — what happened?

**Job:** produce an understandable account of each completed or blocked step and an optional workflow-level recap. The event log, artifacts, verification results and edit journal remain ground truth.

Inputs: explicit step/attempt range; linked artifact **versions**, verification events, tool outcome summaries and changes. Include source references in the prompt.

Outputs (structured and cached):
- Outcome: completed, blocked, failed verification, etc., as **reported by harness**, not decided by model.
- 1–3 sentence plain-language summary.
- Evidence: artifact version IDs, file-change event IDs, verification IDs.
- Remaining issues and proposed human review points, clearly labeled as interpretations.
- Model ID, prompt/schema version, source event cutoff, cost/latency when available.

**Rules:** generate on meaningful transitions, not after every token; lazy-load/calculate where possible. Cache by session/branch/attempt/input fingerprint; invalidate or mark **stale** when linked evidence changes. If summarization fails, the UI falls back to factual event labels. No fabricated “tests passed” claims.

**Priority:** **P1** after a usable non-LLM stepper and artifact viewer. A cheap model is appropriate but optional; users may disable it.

### 9.2 Manager — what should happen next?

**Job:** explain blockers and answer user questions using workflow definitions, recorded evidence, artifact versions, and selectively retrieved allowed code; optionally formulate a **proposed** workflow action.

Initially **read-only**:
- “Why did verify fail?”
- “Which plan was approved?”
- “What changed since the previous attempt?”
- “Should we retry verify or go back to implement?”

Later propose one of a **small typed action vocabulary**, e.g.:

```yaml
action: propose_restart
session: "<selected session>"
target_step: implement
checkpoint_event: 51
reason: "Current verification failure suggests the implementation needs changes."
evidence:
  - "verification.failed:#77"
  - "artifact:plan.md@event:42"
```

This is an *illustrative proposal schema*, not the current agent YAML protocol. The manager's output is not a privileged command. The **Rust controller** must verify that the target is an existing workflow step, the event is within visible ancestry, the session is not actively owned by another run, and the restore plan is safe. The **person** sees evidence and approves the exact restart/restore/fork before any mutation. Similar guardrails apply to retry/goto; arbitrary jump without provenance should be rejected.

The manager must **never** directly:
- execute shell/write/web tools or override sandbox policy;
- approve its own permissions or grant global trust;
- assert that an artifact version or verification exists without provenance;
- silently switch branches, overwrite files, or re-run a finished step;
- hide errors or reinterpret an unapproved scope change as success.

**Priority:** **P2**; start with Q&A and recommendations, add typed proposals only after deterministic recovery actions and approval previews work reliably.

### 9.3 Config and trust model

Proposed logical roles: `coding`, `summarizer`, `manager`, each with optional model ID, enabled flag, budgets and provider settings. **Do not assume an additional provider is required**; all roles may share a model endpoint. Model data is untrusted, can contain prompt injection from artifacts/code, and may be sent off-machine: show provider/data-sharing implications.

Use **bounded, purpose-selected evidence retrieval**, redact known secrets where possible, never expose internal permission files or API keys, and enforce read-only access for manager/summarizer at the application layer. Record question, source versions, answer, model and cost for reproducibility. Provide a manual “Show sources” control and “not enough evidence” response.

## 10. Architectural prerequisites for the redesign

The UX above will be brittle unless the engine exposes stable structured state:

1. **Typed frontend/engine contract:** events and requests with IDs, event-time, session/branch/attempt, tool kind, status, evidence refs and required decision options. No parsing of arbitrary terminal strings.
2. **Durable run/attempt records:** exact step start/end, retry count, status and previous checkpoint. Needed for trustworthy stepper, summaries and restart.
3. **Single execution ownership:** a per-session lock/lease and crash recovery, whether sessions are launched by CLI or TUI. Observers should never contend for stdin.
4. **Request lifecycle:** pending approval persisted or reconciled on restart/disconnect; a decision cannot be double-submitted or applied to the wrong tool call.
5. **Reliable restore boundary:** preview and conflict detection; journal all writes; clearly mark the current limitation that multi-file restore is not transactional.
6. **Separate display from execution:** UI must remain responsive to resize/navigation while tools/network calls are pending. No background stdout corrupting the alternate screen.
7. **Event sourcing as fact:** cached summaries and manager interpretations are additive and disposable. They never rewrite historical evidence.
8. **Artifact identity:** stable version/branch/step/attempt references to support frozen approval and historical comparison.
9. **Explicit command result types:** an attempted action can be approved, denied, cancelled, failed, completed, or partially applied. Do not treat “engine continued” as “user approved”.
10. **Visibility across terminals:** when CLI runs a session, TUI should either attach safely or show **observer mode** and the owning process.

## 11. Quality-of-life backlog — not all are equal

### Essential to the redesign (P0)

- Home view with human-readable session labels and project context.
- Evidence-first blocked-state card with contextual recovery actions.
- Persistent and unambiguous current step/attempt status.
- Reliable typed approvals with a distinct decision panel, exact scope and outcome.
- Clear separation between **viewing**, **asking**, **instructing**, and **confirming**.
- Controlled navigation/focus/help and compact terminal support.
- Robust single-session ownership and crash/reconnect behavior.
- Structured errors with one-click context (verification, tool, restore) and no invisible success.

### High-value quality of life (P1)

- Rendered Markdown artifacts with preview/raw toggle and heading navigation.
- Artifact version history, origin, references and diff; frozen approval snapshots.
- Timeline cards that expand into raw events; filter by step, errors, tool type and changes.
- Search in conversation and artifacts; jump to latest blocked/approval item.
- Stepper with attempts, retry/goto loop edges, current verification and a “why this step?” view.
- Session-friendly naming, labels and archives; branch overview and compare.
- Keep composer drafts, per-tab selection and scroll position across refresh and restart.
- Copy artifact/diff/path/command, open file in `$EDITOR`, sensible URL handling.
- Usage/cost/token/time view where provider data supports it (unknown values shown as unknown).
- Optional summarizer and source-linked step/workflow recaps.

### Useful later (P2)

- Manager Q&A grounded in artifacts/code, with cited source versions.
- Manager-generated **proposals** to retry/goto/restart with typed validation and human confirmation.
- Direct artifact editing and review comments that create new immutable versions.
- Diff review tools: per-file summary, hunk navigation, open in editor, compare branches.
- Interrupt/cancel/queue/resume controls with deterministic cleanup.
- Notifications in the TUI (and optional desktop/Discord alerts) for pending decisions and completion.
- Saved views, configurable keybindings/themes and colorblind-friendly indicators.
- Context/cost budgets, cached retrieval, summaries for long sessions.
- Better code-context search/indexing as a separate explicit, permissioned capability.

### Optional/longer term (P3)

- Multi-session dashboard and supervised parallel/subworkflow execution.
- MCP integrations and external agent tooling with the same permission model.
- Rich graph visualization and time travel across attempt history.
- Remote/mobile approval surfaces with authenticated session ownership.
- Git-aware checkpoints/patch export, without claiming that Git replaces the native journal.
- Terminal inline images and side-by-side diff rendering when the terminal supports it.

## 12. Recommended delivery sequence

**Do not replace the whole interface in a single untestable PR.** Keep the current CLI path operational throughout.

| Milestone | Scope | User-visible result | Prerequisite / exit criterion |
| --- | --- | --- | --- |
| **M0 — Initial TUI baseline (delivered; validation still incomplete)** | Compile/test, permission keyboard semantics, artifact tab, smoke tests | Basic interactive TUI merged in PR #6; lockfile committed and local Rust tests/Clippy passed | Broader terminal and failure-mode E2E tests remain in [README P0](../README.md#p0--stabilize-the-current-interactive-baseline) |
| **M1 — UX foundation** | Session home, stepper, typed event cards, focused key map, errors | “Where am I / what's next?” answered | 80×24 usable, blocked/review states obvious |
| **M2 — Review artifacts (partially delivered)** | Markdown renderer, preview/raw, version metadata, search, approval evidence | Basic Markdown rendering and raw toggle shipped in PR #8; version provenance/search/pinning still pending | Hardening of control sequences and terminal layout, then stable version identity |
| **M3 — Recovery and engine boundaries** | Durable attempts, owner lease, crash recovery, typed proposals/confirmation surfaces, restore clarity | Safe restart/retry without guessing | No concurrent double-run or silent restore |
| **M4 — Summaries** | Optional step/workflow summarizer with cache/provenance | Quick understanding of long runs | Factual fallback when model disabled/fails |
| **M5 — Manager** | Read-only evidence-based Q&A, then validated action proposals | “Why blocked / what next?” answered conversationally | Manager never mutates without harness validation + explicit user consent |
| **M6 — QoL and extension** | Editing/review loops, branch compare, streaming/interrupt, configurable UX | Mature daily-driver experience | E2E testing and observability maintained |

**Ordering guidance:** if renderer work is small, it may land before the full M1 stepper. But **execution ownership and typed decisions must be resolved before shipping manager-initiated transitions.**

## 13. Test plan and quality bar

### Deterministic tests

- Keybinding/focus matrix: composer, tabs, timeline, artifact reader, approvals, restart confirmation, help, search.
- Enter never approves a tool; `p`/`g` cannot apply to unsupported permission kinds.
- Console output never corrupts TUI after approvals, errors, network failures or resize.
- Artifact version selection remains stable across refresh; fork ancestry shows only valid versions.
- Markdown parser safety: ANSI escape injection, malicious URLs, raw HTML, giant tables, non-ASCII width, very long code lines.
- Attempt/history recovery: first step, repeated retries, nested loop, terminal crash, stale owner and wrong-branch event.
- Restore tests: manual drift, symlinks, changed files between preview/apply, partial failures and no writes on cancel.
- Read-only manager tests: malicious artifact instructions cannot become tool calls, permission grants or unconfirmed restarts.

### End-to-end acceptance scripts

1. **Happy path:** start feature → see plan artifact → approve plan → verify → completion summary.
2. **Permission path:** tool requests shell → inspect scope → deny → visible denied result → choose recovery; repeat with global grant and verify persistence explicitly.
3. **Block path:** failed test → open evidence → ask why → retry current step or restart earlier, with actual state change.
4. **Artifact path:** view Markdown plan → navigate headings/code → toggle raw → inspect older version → return with scroll retained.
5. **Crash path:** kill active worker during a pending approval → reopen → safe recovery, no duplicate execution.
6. **Narrow terminal:** resize while viewing long artifact/approval; controls remain reachable and readable.
7. **Branch path:** fork from earlier event → compare old and new artifacts/changes → prove historical branch remained intact.
8. **No-model path:** run with summarizer/manager disabled; all controls and factual status must still work.

### Usability measures (targets to validate, not measured facts)

- **Time to answer “why blocked?”**: target under 10 seconds for a user familiar with terminal apps.
- **Time from block to chosen recovery**: target under 30 seconds, excluding human review of diffs.
- **Approval ambiguity**: zero accidental approvals in keybinding usability tests.
- **Evidence discoverability**: testers can find the plan and latest failed verification without CLI commands.
- **Trust**: after a restart, the user can explain which branch and files changed.
- **Responsiveness**: navigation/resize should remain usable during slow model/tool operations; measure rather than assume.

## 14. Explicit decisions and open questions for later design

These are **decisions to resolve during implementation**, not reasons to defer documenting the overall UX:

1. Should a session composer default to **coding agent instructions**, or visibly select **Agent / Manager** before typing? Recommendation: explicit mode indicator; no silent mode switch.
2. Should approval apply on a single keypress or select then confirm? Current PR direction: direct single-key for one-off permissions; consider a second confirmation for **persistent global native-write grants**.
3. Should a restart checkpoint be “before first attempt at step” or a selectable **attempt boundary**? Recommendation: selectable attempts once durable attempt IDs exist.
4. Should artifacts support in-TUI edits or open `$EDITOR` first? Recommendation: external editor / new-version registration before an embedded editor.
5. How much untracked filesystem state should recovery detect? Recommendation: display known coverage, Git status if available, and avoid implying full repository rollback.
6. How is a session named/archived across projects and branches? Recommendation: user-editable title derived initially from goal, immutable IDs kept under the hood.
7. How should a manager select code context without exfiltrating secrets? Recommendation: explicit bounded read-only retrieval with provenance, no unrestricted repository ingestion by default.
8. When should summaries be generated and regenerated? Recommendation: on completed/blocked transitions, cached by precise evidence cutoff with optional manual refresh.
9. When multiple sessions are visible, who owns an approval? Recommendation: durable per-session owner; TUI can observe but never impersonate another runner.
10. Should the default view collapse technical tools or include everything? Recommendation: semantic summaries by default, raw logs one deliberate action away.


## 15. External harness benchmark (research snapshot: 2026-11-12)

This research compares public UX behavior and source-code contracts, **not** benchmark scores or model quality. Greenlight's strength is the explicit step/verification/artifact contract. Competitors are references for particular interactions and implementation seams—not a feature checklist to copy wholesale. Links below are first-party documentation or repository sources and should be rechecked before implementing a particular design.

### 15.1 Comparative lessons

| Harness | Relevant design | Greenlight difference | What to learn |
| --- | --- | --- | --- |
| [OpenCode](https://opencode.ai/docs/cli/tui/) | Discoverable commands, queue vs steer while running, contextual file mentions, and a side question (/btw) that does not join the ongoing agent conversation. [Permission API](https://opencode.ai/docs/build/plugins/server/) exposes pending requests and decisions. | Our composer currently resumes only idle/blocked steps; the proposed manager is not implemented. | Explicit **Ask / Instruct / Queue** semantics and a typed pending-request lifecycle. |
| [Codex CLI](https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/approval_overlay.rs) | Typed command/patch/permission/elicitation approval variants, request IDs, action-specific choices and a queue. Its [app-server protocol](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md) describes replaying pending approvals after reconnect. | Our TUI uses typed permission *modes* but process-local channels, without durable request identities, queues or replay. | **P0:** persist identity/outcome of approvals; treat cancel, deny and disconnect differently. |
| [Pi](https://pi.dev/docs/latest/session-format) | Session JSONL forms a branchable tree through entry IDs and parent IDs; [active context](https://pi.dev/docs/latest/how-pi-works) is rebuilt from the selected branch and compaction preserves original history. | Our SQLite branch history exists, but attempts/checkpoints/context inputs are not visible as navigable objects. | **P1:** step attempt browser, branch bookmarks and an exact **context inputs** view. |
| [Plandex](https://github.com/plandex-ai/plandex/blob/main/docs/docs/cli-reference.md) | Accumulates proposed edits separately from working-tree files until review/apply, supports [autonomy levels](https://github.com/plandex-ai/plandex/blob/main/docs/docs/core-concepts/autonomy.md) and multiple [model roles](https://github.com/plandex-ai/plandex/blob/main/docs/docs/models/roles.md). | Our native journal records edits applied immediately, then supports guarded restore. This is not a staging area. | **P2:** study optional proposal → review → apply → verify for multi-file changes. |
| [Aider](https://aider.chat/docs/usage/modes.html) | Explicit ask/code/architect modes; [architect/editor](https://aider.chat/docs/usage/modes.html) can use two LLMs, and [Git integration](https://aider.chat/docs/git.html) supports review and undo. | Our proposed manager is non-mutating; Aider architect mode is part of the editing pipeline. | **P1:** visibly label recipient and side effects. **P2:** evaluate multi-model roles empirically. |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/plan-mode.md) | Read-only planning mode with a deliberate transition to implementation; [session browser](https://geminicli.com/docs/cli/session-management/) has search and checkpoints. | Our hard workflow steps already separate planning/implementation, but the UI does not clearly display what plan version is *approved*. | **P1:** accepted plan evidence, review state and searchable sessions. |
| [Crush](https://github.com/charmbracelet/crush/blob/main/internal/ui/AGENTS.md) | Separate main state machine, dialogs, scroll-follow and lazy render components; [UI source](https://github.com/charmbracelet/crush/blob/main/internal/ui/model/ui.go) also caches rendered Markdown/diffs. | Our TUI is concentrated in one App source with interleaved state, event routing and rendering. | **P1:** extract testable UI components before increasing layout complexity. |
| [OpenHands](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-agent-server/openhands/agent_server/README.md) | Typed event/action/observation state, persisted session history and WebSocket event streaming. Its [server source](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-agent-server/openhands/agent_server/event_service.py) separates durable events from ephemeral token deltas. | Our UI polls SQLite and only owns the runtime worker started by the TUI. | **P1:** durable event replay plus ephemeral progress signals; do not mistake a token stream for an authoritative state transition. |
| [Cline](https://docs.cline.bot/core-workflows/task-management) | Task-centric history with execution, cost and decision metadata; [ClineCore](https://docs.cline.bot/cline-sdk/sessions) exposes reusable sessions and approval callbacks. | We have an audit trail but limited human-readable session metadata. | **P1:** session names, summaries, search and stable frontend contracts. |

### 15.2 What the upstream code changes in our architectural priorities

1. **Approval lifecycle is more than keyboard shortcuts (Codex).** An approval should have a stable ID, parent tool call, session, branch, attempt, allowed decisions, current state and final decision receipt. The runtime must reject stale or duplicate responses and permit a reattached UI to inspect pending decisions. An unavailable UI must not be interpreted as consent. A grant persisted globally deserves explicit additional confirmation.
2. **Separate UI projection and event ownership (OpenHands).** Durable step/tool/verification/artifact facts must be replayable from the store. Transient token and progress frames can update the screen quickly but should not be treated as authoritative history. Unknown event types must render safely rather than crash the viewer.
3. **UI composition matters as the view grows (Crush).** Introduce focused parts (SessionPicker, Stepper, ActivityList, ArtifactReader, ApprovalDialog, Composer, RecoveryDialog) with a single explicit focus router, renderer caches, terminal resize handling and tests. Do not keep expanding one massive draw/key-handler function.
4. **There are three distinct histories (Pi and Plandex).** Conversation/context branches, registered artifact versions, and the actual workspace are not interchangeable. The UI should show which historical checkpoint, which versioned evidence and which on-disk files an action refers to.
5. **Reasoning roles have different jobs (Aider, Plandex).** Architect/editor is an editing pipeline; summarizer can reduce context; Greenlight's manager is a human-facing read-only advisor. These can share a model endpoint, but should never share tool authority merely because they use the same model.

### 15.3 Three additional hot paths

#### HP-09 — Ask a side question without changing the run

User expectation: “Why did the previous verification fail?” must **not** secretly become an instruction to the coding agent.

- Show recipient **Ask manager (read-only)** or **Instruct coding agent** explicitly.
- Use a bounded snapshot of the selected session/branch, artifact versions, verification events and permitted code. Display evidence links and the snapshot cutoff.
- No workflow transition, tool execution, file write, permission grant, or mutation of the coding agent's subsequent prompt.
- Do not silently persist a private side question forever; make its retention behavior visible. Fall back to deterministic navigation when the manager is disabled.

**Acceptance:** the same agent work and permissions result whether or not the user asks a side question.

#### HP-10 — Steer now, queue for later, or stop

User expectation: “I found a mistake during execution” is different from “do this after verification.”

- Distinguish **Steer current turn**, **Queue for next safe boundary**, **Stop attempt** and **Keep editing draft**.
- Show whether cancellation actually succeeded. Never report a completed stop while a tool is still executing.
- Queue entries must carry session, target step/attempt, branch and delivery rules; a fork or session switch must not send instructions to the wrong branch.
- Pending approvals take precedence. A queued message cannot implicitly accept an approval or become a different kind of instruction.
- Restore drafts on safe cancellation and record what happened.

**Acceptance:** changing branch or denying a permission cannot cause a queued instruction to execute unexpectedly.

#### HP-11 — Inspect a proposed change set before applying it

User expectation: “Show me the overall patch before it touches my files.”

- Investigate an **optional staging area** for combined changes. Display file/hunk diffs, expected tests, and the exact relation to the approved plan artifact.
- Distinguish *proposed*, *approved*, *applied*, and *verified*. A tool grant is not blanket approval of the whole proposed plan.
- Recheck original file state at apply time. Surface partial apply explicitly, and only claim atomicity when actually guaranteed.
- Preserve links between the proposal, review decision, actual edit-journal entries, artifact versions and verification evidence.
- Don't force staging on exploratory workflows; this is a separate design experiment from native write approvals and restore.

**Acceptance:** the user can determine exactly which proposed files were applied and which were verified.

### 15.4 New quality-of-life and safety requirements

| Requirement | Priority | Rationale |
| --- | --- | --- |
| Approval request ID, queue, reconnect/replay and decision receipt | **P0** | Avoid stranded prompts and ambiguous consent across CLI/TUI/process restart |
| Per-attempt **context manifest** (step definition, selected artifact versions, retrieved evidence) | **P1** | Explain what the model actually saw rather than relying on its memory |
| Visible **accepted plan version** alongside latest/superseded artifacts | **P1** | A later edit must not silently change the approved evidence |
| Explicit ask/steer/queue/stop semantics and persistent per-session drafts | **P1** | Messages should not be delivered to the wrong model, phase or branch |
| Componentized TUI with lazy lists, render caching, resize/accessibility tests | **P1** | Large Markdown and event histories should remain responsive |
| Branch/attempt bookmarks, searchable sessions and context-compaction provenance | **P1** | Make navigable history understandable and reproducible |
| Optional staged changeset with conflict-checked review/apply boundary | **P2** | Better multi-file supervision, without claiming that native restore is full rollback |
| Measure role-specific costs/quality/latency before adding more models | **P2** | Separate capability and permissions from provider/model choice |

### 15.5 What *not* to copy

- **No uncontrolled chat-first autonomy.** Every approved transition, required artifact and verification gate still belongs to the harness.
- **No implicit mode switching.** Asking a question does not become an execution instruction when the session changes status.
- **No automatic Git commits by default.** Git integration is useful, but the user should own the commit policy and Greenlight's native journal remains distinct.
- **No broad unreviewed “auto allow everything” profile.** Keep deny-overrides, explicit tool scopes and clear lifetime of grants.
- **No premature server rewrite.** A stable typed frontend/engine contract, ownership and replay are prerequisites; a separate daemon is optional.
- **No claim that branch navigation restores all filesystem state.** Context history, artifact history and workspace recovery have separate limitations.

### 15.6 Additional acceptance scenarios

- Disconnect the TUI during a permission prompt; reattach to the *same* request ID and prevent double-decision or stale approval.
- Ask a read-only side question during execution; verify no changes to agent context, pending approvals or workflow state.
- Queue instructions while a tool runs, then stop/fork: no instruction should leak into a different branch/attempt.
- Show the exact approved artifact version and context manifest for an attempt after that artifact has been updated.
- Exercise proposed multi-file staging against manual drift and partial apply failures.
- Render thousands of events and a very large Markdown artifact in a narrow/resized terminal without losing scroll, focus or responsiveness.

**Research-driven order of work:** typed interaction lifecycle → readable, navigable evidence → provenance/context inspection → optional manager and summaries → staged multi-file change sets.

## 16. Critical design review — what could go wrong? (2026-11-12)

**Purpose:** challenge our own roadmap before committing to a large TUI rewrite or additional models. These are **technical findings plus design judgments**, not evidence from usability trials. Severity is about consequences if we implement the idea naively; it is not a prediction that a particular failure will occur.

**Status note:** this is the original risk analysis. Its assumptions were revisited with the project owner in [Section 17](#17-decisions-after-critique--scope-and-semantics). Where the two differ, Section 17 records the current agreed direction.

### 16.1 Risk register

| Idea | Substantial drawback / failure mode | What current code tells us | Recommendation and evidence required |
| --- | --- | --- | --- |
| **Manager model controls flow** | A proposed transition can be valid but inappropriate; an LLM can misunderstand artifact provenance, carry prompt injection from code, or accidentally become a second source of authority. Human confirmation can become rubber-stamping. | The workflow engine applies strict transitions; models are not currently a trusted policy engine ([engine.rs](../src/engine.rs)). | **Defer manager-driven transitions.** First make deterministic Retry, Resume, Inspect and Restart genuinely usable. Prototype Q&A read-only. Adopt proposals only after tests show a clear usability gain over a conventional recovery menu. |
| **Summarize every step** | Additional latency, token spend, external data transfer, and plausible-but-wrong narrative. Cached summaries may describe old artifacts; a summary can become more visible than the real verification result. | Events and required artifacts already provide factual checkpoints, but no stable attempt IDs or summary provenance. | **Do not auto-summarize every action.** Start with deterministic status cards from events; consider opt-in summaries on completed/blocked attempts, with linked evidence/version IDs and a measured cost budget. |
| **Rewind/restart as a primary action** | “Go back” can sound like full rollback, including dependencies, external APIs, Git and manual edits. A multi-file restore can partially succeed before error, leaving mixed state. | [restore.rs](../src/restore.rs) only handles journaled native edits/restores and applies files one at a time. | **Separate workflow fork, code restoration and retry.** Call out coverage before confirmation, provide detailed diff, stop on conflicts, offer an explicit *no-restore fork* path only with clear warnings. Do not present it as full time travel. |
| **Persistent approvals that replay after a crash** | A restored prompt is not a restored in-flight tool call. Without exact action identity and atomic decision consumption, an old approval could authorize a new call or be consumed twice. | [tui_actions.rs](../src/tui_actions.rs) uses thread-local channels and [tools.rs](../src/tools.rs) awaits a blocking receiver; those vanish with the process. | **P0: identify and audit live requests.** On crash, mark pending requests *expired*, require new execution/approval; **defer transparent replay** until a durable runner/action protocol exists. Exactly-once *effects* are a separate unsolved problem. |
| **One-key project/global approval** | Typing a single `g` can silently create a long-lived, broad grant; native write grants authorize **any permitted project path**, not necessarily the displayed diff. The convenience can defeat the whole purpose of human supervision. | [tools.rs](../src/tools.rs) persists project/global `write` grants; TUI sends recognized keys immediately. | **P0: add a second, explicit confirmation** for persistent scope grants (especially global writes), with scope warning. Keep single-key one-time approval. Record whether persistence succeeded; never report a grant until acknowledged. |
| **Full Markdown renderer** | GFM tables, nested syntax, ANSI/OSC escape injection, wide Unicode, links, images, huge fenced blocks and reflow create both security and performance work. It is easy to spend weeks polishing a renderer that does not improve the actual human decision. | [tui.rs](../src/tui.rs) currently shows raw UTF-8; artifacts can contain model-generated untrusted text. | Start with **safe headings, lists, emphasis, blockquotes, code and wrapping**, plus Preview/Raw. Delay syntax highlighting, full GFM and images. Preserve a raw fallback and test terminal control escapes before shipping. |
| **Complete artifact version history and context manifest** | “Which version did the agent see?” cannot be reconstructed reliably if the exact selected artifact IDs were never recorded. Showing the latest artifact next to an old approval may misrepresent the decision. More metadata joins/IDs add persistence complexity. | [store.rs](../src/store.rs) returns latest-by-name `Artifact { name, content, step }`, without exposed event/version identity; [engine.rs](../src/engine.rs) selects artifacts to build context. | First record the **resolved input manifest per attempt** and version IDs **at selection time**; only then build historical provenance and pinned approval UI. Never silently substitute a newer artifact. |
| **Artifact browser as always-live, full-text view** | Polling and parsing every artifact version creates unnecessary disk/CPU work; large histories and UI rendering can stall keyboard response. | [tui.rs](../src/tui.rs) refreshes the session and [store.rs](../src/store.rs) reads all branch-visible artifact blob bodies to pick latest by name; it currently re-reads even when the artifact tab is not selected. | Fetch **metadata/index incrementally**, load only the selected artifact body on demand, and cache by immutable version ID. Treat this as an early performance prerequisite, not a future visual nice-to-have. |
| **Immutable artifacts guarantee clean provenance** | A generated event can claim “artifact created” before the content blob/database entry exists, producing a misleading success event on I/O failure. | [store.rs](../src/store.rs) currently calls `event("artifact.created")`, then writes the blob, then inserts the artifact row without one transaction covering publication. | **P0 data integrity:** publish artifact+event coherently, or use a recoverable pending/committed state and reconciliation; test interrupted writes and storage failure. The TUI must distinguish failure from successfully registered evidence. |
| **Live steer, queue and stop** | Model completions and external tool calls may not be interruptible safely. Cancellation can leave edits and artifacts partially applied; queued instructions may target a step that completed or a branch that was forked. | Greenlight starts a fresh context for each step and the TUI currently controls one worker; there is no interrupt/queue protocol. | **Defer arbitrary mid-turn steering.** Offer “send next-step instruction” only when target and delivery boundary are explicit; design cancellation and partial-effect semantics before claiming a reliable Stop button. |
| **Linear workflow stepper** | A horizontal progress bar suggests forward-only progress, but workflows have configured goto/retry loops; a step can have multiple attempts and verification can fail after code changes. | [config.rs](../src/config.rs) supports deviation `goto`, bounded `retry`, and graph transitions. | Use a compact **current step + last transition + attempt number** summary; mark previous attempts separately. Show full graph only on demand. A progress percentage is misleading unless it represents an actual bounded plan. |
| **Staged multi-file changesets** | This changes execution topology, not just the UI: model edits must go into staging, command verification must run against staged content, and the eventual apply must handle drift/conflicts. It may duplicate Git's job and complicate sandbox rules. | Native `write` updates the working tree immediately; Bubblewrap shell views a read-only project, not a separate staged overlay. | **Research spike only, not committed roadmap.** Test whether a Git worktree/temporary workspace prototype can serve the need. Avoid implementing a second change engine unless real use shows the journal + review is inadequate. |
| **Big multi-pane redesign** | Extra tabs, overlays, modes and shortcuts can increase cognitive load. A wide desktop-like layout can fail in 80×24 terminals. Rewriting the TUI all at once risks breaking a working agent control path. | Current [tui.rs](../src/tui.rs) already combines app state, key routing, engine control and rendering in one large module. | Agree on 3–4 critical screens, one visible primary action and consistent focus. Extract UI components incrementally with terminal snapshot/interaction tests. **Do not implement every proposed panel before testing use.** |
| **Two additional LLM roles by default** | Expense, latency, duplicated context, model/provider configuration burden, failure and privacy surface. Small projects may get worse UX from unnecessary conversational layers. | Provider currently has one primary configured model; the workflow may override per step. | Keep **zero extra LLM calls by default**. Treat summarizer/manager as independently optional experiments with explicit evidence retrieval, budgets and disable/fallback paths. |

### 16.2 Where the proposed roadmap is internally inconsistent

1. **We promise a “simple, obvious next action” while proposing many modes.** Ask / Instruct / Steer / Queue / Stop / Retry / Fork / Restore are not eight equivalent buttons. The MVP should emphasize **Inspect → Resume with instruction**, with **Restart** available as an advanced, explicit operation. Only introduce Ask/Queue/Stop after validating what users actually need.
2. **We want instant one-key decisions and careful scope review.** These conflict for persistent `p`/`g` grants. Use instant `y`/`n` for a single request, but require a deliberate extra confirmation for persistent broad grants. Do not add a “remember forever” shortcut purely to save keystrokes.
3. **We prioritize crash-safe approval replay before durable execution exists.** Persisting an answer does not resume its recipient. Short-term correctness means a pending prompt **expires safely**, not that it is invisibly replayed after a restart.
4. **We market “human-supervised” but allow artifacts to outrun review.** A step can generate multiple versions of the same plan; a human may approve a snapshot then the agent produces a newer document. Review needs **version pinning** and explicit accepted/superseded state, not merely latest-by-name display.
5. **We call the TUI an interactive conversation while still running a strict step engine.** Arbitrary ongoing chat may violate the deterministic step/input/verification contract. A composer should state the recipient and operational meaning. Questions must not masquerade as execution instructions.
6. **We propose real-time summaries alongside a costly polling model.** Before adding inference work, fix the underlying UI data loading and event metadata. Most “what happened?” screens should be populated without contacting any model.

### 16.3 Re-scope into a testable first redesign

A **small but dependable** daily-driver experience should include:

- Session picker, readable status and a truthful **current step / latest attempt** indicator.
- Activity timeline grouped into step, artifact, tool, verification and error cards; raw log drill-down preserved.
- Artifact **safe Markdown preview** and raw toggle, selected content loaded on demand.
- Evidence-first blocked state with **Inspect**, **Resume with instruction**, and clearly guarded **Restart**.
- Typed approvals with correct scope, explicit outcome, persistent-grant confirmation and safe worker-loss behavior.
- Small terminal keyboard navigation, persistent selection/scroll, failure messages, deterministic tests.

**Explicitly out of this first redesign:** manager model, automatic summarizer, arbitrary mid-turn steering, queued actions, multi-file staging, rich graph visualizer, embedded Markdown editor, remote decisions and full Git-integrated rollback.

The goal is not feature parity with OpenCode or Codex. It is a **short path from “blocked” to “understand → decide → safely continue”** without any false implication that a model or a checkpoint knows more than it does.

### 16.4 Decision gates before adding complexity

| Candidate feature | Required evidence before implementation |
| --- | --- |
| Manager Q&A | Users cannot answer “why blocked?” quickly using structured evidence alone; bounded retrieval can reliably cite exact artifact/event IDs; prompt-injection and privacy tests pass. |
| Manager action proposals | The deterministic recovery interface already works; proposals improve task completion or time-to-recovery in controlled examples; every operation is validated and confirmed. |
| Summarizer | Human review of factual activity cards is too slow; a small evaluation set shows faithful, source-linked summaries; cost/latency and failure behavior meet a chosen budget. |
| Staged changesets | Existing one-file approvals + final diff review fail realistic multi-file review tasks; a prototype proves verification on staged content and safe conflict-aware apply. |
| Active steer/queue/stop | We have well-defined tool cancellation, disposition of partial writes, branch-targeting, stale queue invalidation and recoverable attempt state. |
| Persistent approval replay | A durable runner can reattach to an **existing, still-live** request; response is tied to action ID and consumed once. On process death, use expiry and re-execution, not magic continuation. |
| Full Markdown/GFM support | Core document review is already usable and terminal/layout tests show a real gap; malicious control-code inputs remain harmless. |
| Complete graph/history UI | Repeated attempts and loops have durable IDs and users need navigation beyond the current step and historical evidence list. |

### 16.5 Stop conditions and measured success

Before making a PR for a major new subsystem, write one scenario it will improve and one failure it must not cause. A feature can be postponed or removed if it adds more approvals, latency, visual complexity or persistent state than the value it delivers.

Start with four checks on the actual current TUI:

1. **Comprehension:** in a blocked session, can a user name the failing step and find evidence in under ~10 seconds without asking an LLM?
2. **Control:** can they approve once, explicitly decline, and resume with a revised instruction without mistaking one outcome for another?
3. **Recoverability:** can they explain what a restart will and will not restore after reading its preview?
4. **Performance:** can they navigate a session with thousands of events and long artifacts without keyboard lag or confusing scroll jumps?

**Change of posture:** these concerns are not arguments against good UX. They are reasons to ship the *smallest trustworthy interaction* first, and make every additional model, pane and execution feature earn its complexity.


## 17. Decisions after critique — scope and semantics

The previous section deliberately over-indexed on failure modes and was written against the original PR #6 baseline. Some descriptions (for example, raw-only artifact display) are now obsolete. The ensuing product discussion clarified what Greenlight is trying to accomplish and which concerns are **real technical boundaries** versus **premature reasons to remove a feature**. This section records the revised direction, superseding conflicting recommendations in Section 16.

### 17.1 Keep the manager as a coordinator, but implement it separately

- A **manager/coordinator** may answer questions, help interpret artifacts/code, or suggest retry/restart/step navigation. It need not interfere with an ordinary coding-agent instruction: if the user is addressing the coding agent, the manager (or a deterministic router) can **pass the instruction through unchanged**.
- The UI should make it understandable whether a response is an explanation, a forwarded instruction, or a **proposed** action. We do not need to force an extra “are you talking to the manager?” confirmation at every turn.
- The manager need not be weak or unreliable to create a state ambiguity. The real contract is that **the Rust harness validates and commits transitions, permissions and restore operations**, using the same enforcement as direct user actions.
- Keep the feature on the roadmap. Implement the TUI and deterministic workflow operations first **one feature at a time**; build the manager afterward without reopening the whole interface.

### 17.2 Rollback is intentionally based on recorded native writes

- **The existing rollback works for files edited through Greenlight's native `write` tool.** Every native edit records its before/after contents; the restore planner reconstructs eligible tracked file states from that journal.
- Shell commands in the Bubblewrap sandbox have a **read-only project view**, so the common agent workflow is far less exposed to untracked in-project script edits than a general host-shell agent. This makes the native-write strategy a reasonable design, not a fundamentally broken rollback system.
- Exceptions remain: manual/concurrent edits, files never written through the native tool, temp/build caches, external side effects and a **partial multi-file restore**. The practical impact should be measured in actual workflows, not assumed to be disastrous.
- **Keep the current rollback design** and improve its reliability/tests; do not introduce Git or a staging engine as a prerequisite. A restore confirmation should state its tracked-file coverage without sounding like full environment time travel.

### 17.3 Saved grants survive their scope; pending requests do not

These are different lifecycle concepts:

| Kind | Desired lifetime | Present implementation |
| --- | --- | --- |
| Approve once | This specific request | Supported |
| Process grant (`a`) | This process only | Supported |
| **Session grant** | A single session across its resumed runs and processes | **Not yet supported; add a durable session-scoped grant** |
| Project grant (`p`) | Subsequent uses within the project, subject to policy | Supported |
| Global grant (`g`) | Subsequent uses across projects, subject to policy | Supported |
| **Pending, unanswered request** | Only while the requesting worker/action is live | In-memory channel today; **discard on crash**, never replay to an unrelated new request |

- **Do not implement persistent pending-approval replay.** On worker death, mark the request interrupted/expired for diagnostics and require a fresh permission check if execution restarts. The saved grants remain; pending requests do not.
- Tracking a request ID can still be useful during a **live** UI session for correctness and audit. It does not require a durable, resumable worker protocol.
- Favor convenient one-off decisions, but for broad persistent write grants make the full scope unmistakable. Consider deliberate confirmation for global write trust while avoiding needless friction for `y`.

### 17.4 Prefer coding-agent-authored activity titles; summarization is optional

The target is **short, human-readable progress**, not summarizing every tool call with a second model. Examples:

- “Exploring call sites”
- “Adding temporary diagnostics”
- “Trying the first fix”
- “Verifying behavior”

A practical approach is to let the **coding model emit an optional short `activity_title`** describing the intention of its next action. Greenlight records that text alongside the structured action. If a single workflow step includes several logical phases, subsequent responses can update the title. A second model may provide a concise completed/blocked **step recap** if useful.

**Protocol constraint:** Greenlight currently expects **one YAML action per model response**. Unstructured prose before the YAML or multiple independent action blocks will break that contract. A title should therefore be an **optional validated field** on the action (or a separately typed metadata event), not an instruction to produce free-form preambles. Changing this requires a small, explicit protocol revision and tests.

**Accuracy:** a title is a *model-authored intent label*, not evidence that the step was completed or tests passed. Tool outcomes, artifact creation and verification states still come from harness events.

A lightweight summarizer model is a valid option: model choice can keep cost small, and **running it asynchronously** can avoid blocking execution. It still needs to operate on a defined evidence snapshot; a slow or failed summary must never hold up the step or silently overwrite verified state. We should compare whether the activity-title approach already provides enough clarity before adding another inference pass.

### 17.5 Markdown scope is deliberately modest

The requested artifact UX is **clear headings, bold/italic or highlighted text, readable code and perhaps simple tables**. A small palette of colors and whitespace can make this much easier to scan in a terminal.

- Implement headings, emphasis/highlights, basic lists/code, and optionally modest-width tables.
- Keep a raw-content toggle and neutralize terminal escape/control sequences from untrusted artifact text.
- Do not require complete CommonMark/GFM behavior, images, sophisticated syntax coloring or a general-purpose Markdown browser.

This remains a useful early TUI feature, not a reason to postpone the redesign.

### 17.6 Multi-file review means grouped *native* edits, not scripts

The coding agent is directed to use the native **`write` tool** for project source edits; sandbox shell commands cannot write to the project's read-only mount. All native writes are journaled.

“Staged multi-file changes” was a separate, more ambitious idea: *withhold several proposed edits from the real workspace until a combined diff is approved*. That is **not necessary** for normal recorded edits and is **not** part of the current contract.

- We can show a **combined diff of all recorded writes within a step/attempt** without changing execution semantics.
- Grouping already-applied diffs for review does not retroactively make them pre-apply approvals.
- Only investigate true pre-apply staging if we encounter a concrete use case where separate write approvals and post-edit group review are insufficient.

### 17.7 Live steering: useful, with one unavoidable API boundary

The intended use is **intervening while the LLM is working**, not arbitrarily undoing code edits or tool side effects. That's a sensible UX goal.

The technical distinction: a normal in-flight model completion request generally **cannot absorb a new user message after it has been sent**. Steering can mean:

1. **Cancel and reissue** the model request with new instructions, if the provider/client supports reliable cancellation; or
2. **Deliver at the next model-call boundary**, including immediately after the current model response and before its next action, subject to explicit handling.

We should call both behaviors what they are, show when the instruction actually took effect, and never imply that a delivered message rewrote an already-issued tool call. This is an implementation constraint, **not a reason to drop live steering**.

### 17.8 TUI redesign and performance

- Keep the **ambitious usability direction** and work iteratively. The user will identify painful interaction paths through hands-on testing. The architecture should support evolving the layout without forcing one giant rewrite.
- The artifact loading optimization is **recorded in the TODO**, but a small local project does not justify treating it as an immediate blocker. Profile first; then use metadata-first queries, selected-body lazy loading and immutable-version caches if needed.
- **Artifact publication consistency** is a tangible, bounded correctness fix: ensure an `artifact.created` success event cannot be left behind when blob creation/database insertion fails, with explicit error reporting and recovery tests. Prioritize it independently of visual polish.

### 17.9 Updated working agreement

**Build one feature at a time; retain valuable ideas without inventing needless prerequisites.** Greenlight should offer attractive workflow navigation, good artifact presentation, short activity titles, supervised manager coordination, explicit saved permissions and native-write restoration. The harness—not a model's phrasing—remains the authority for recorded facts, permission enforcement and state transitions.


## 18. Workspace recording before broader rollback

**Agreed approach:** keep native `write` journaling as the basis of restoration. Before expanding rollback to user/external edits, instrument the actual workspace and measure what the journal misses.

### Recording layers

| Layer | Captures | Can restore old contents? | Priority |
| --- | --- | --- | --- |
| **Native journal (already implemented)** | Before/after UTF-8 for native writes and explicit restores | Yes, for recorded writes and restores | Maintain and harden |
| **Step-boundary file manifest** | Relative path, existence, file type/size, content hash, exclusion policy at session start and step boundaries | **No**; hashes detect changed files, not their old contents | First experiment |
| **Optional delta snapshots** | Old/new bytes for selected files that changed between checkpoints | Potentially, with retention and privacy restrictions | Defer until measured need |

**A hash detects that a file changed; it does not identify whether a human, Git, another process or a tool made the change.** Baseline and after-step hashes cannot detect edits created and then undone between scans. Recording only after a user edit also cannot reconstruct the file's earlier bytes unless a prior snapshot exists.

### Suggested experiment

1. At session creation, capture a **baseline** for agreed project files. At each step start/end, capture a new manifest with the session/branch/attempt/event cutoff and compare it with the last recorded state.
2. Prefer a limited source scope: Git-tracked files plus an explicit allowlist for additional project files, or a clearly documented project-root scan. Exclude `.git`, internal state, credentials, build directories, caches, symlinks and oversized/binary files by policy. Don't store secret file bodies. Measure scan cost rather than assuming it's too high.
3. Compare file changes with native `file.changed` and `workspace.restored` events; classify known journaled changes, out-of-band changes, creations/deletions and excluded/unscanned paths.
4. Show user/out-of-band drift **before rollback**. Start by refusing to overwrite it or requiring an explicit reconciliation. Do not promise rollback of bytes that were never saved.
5. Independently fix native-write crash consistency: current `write` changes disk bytes before the engine records the journal entry. Consider a pre-write intent/preimage and a recoverable committed/failed state.
6. If manifests show substantial drift, evaluate snapshotting selected changed file contents with retention/deduplication and a conflict policy; only afterward consider restoring unjournaled user edits.

### Why multi-file restoration may be partial

`RestorePlan::apply_inner` validates all tracked targets, then **modifies each file and journals that operation in sequence**. File replacement can be atomic individually without making an entire set of file edits atomic. For instance, a later file might be changed concurrently, an I/O or disk-full error may happen, or the journal write may fail after a prior file was updated. The code reports such failures, but cannot undo all prior mutations automatically. A SQLite transaction alone doesn't make the filesystem atomic.

This does not invalidate native-write restoration for normal supervised workflows. It describes an engineering limit to test on a representative project before choosing whether to add a recoverable multi-file transaction mechanism.

### Exit criteria for recording work

- Detect a manual edit, a new untracked project file and a deletion between steps within the declared scan scope.
- Identify which changes are already explained by the native journal.
- Make excluded paths and missing snapshots explicit.
- Keep prior restore/fork behaviors intact, including changes recorded before the recording feature.
- Test crash after native write but before journal commit, plus partial restore errors.
- Show measured scan time/storage size for realistic small and medium repositories.

**Decision:** no whole-project rollback engine is authorized by this research task. Workspace recording goes on the TODO first; the heavier rollback ideas can remain optional.

---

**Definition of UX success:** Greenlight should feel like a reliable supervisor's workbench: the user can delegate, observe, read evidence, decide, recover, and audit every outcome without losing control of the underlying workflow.
