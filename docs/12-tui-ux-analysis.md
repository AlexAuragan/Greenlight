# Greenlight — TUI UX analysis and redesign plan

> **Status:** product/UX proposal, not an implemented specification.
> **Baseline:** `feat/ratatui-session-viewer` / PR #6, 2026-11-12.
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

### What exists on the PR #6 branch (implementation still being validated)

- CLI start/resume, terminal prompts, branch-aware SQLite events, immutable artifact versions, native edit diffs and guarded restore/fork operations.
- Linux Bubblewrap isolation for shell tools; native writes remain separately permissioned and journaled.
- Ratatui session list, conversation-like event timeline, composer that can resume ready/blocked sessions, an in-TUI permission bridge, and a `/restart` flow with restore preview.
- A second tab showing the **latest branch-visible version** of each artifact as **raw Markdown/text**, with producer step and size.
- The current TUI can observe other processes but only controls its own worker. There is **no strong cross-process session lock**, model token streaming, or real-time remote interaction with another process.
- The workflow has step IDs, but the store lacks a durable first-class **attempt ID**. Event-based inference is currently necessary.

### Not implemented / not yet validated

Rich terminal Markdown rendering; artifact version navigation and in-TUI editing; a proper workflow graph/stepper; a guided blocked-state recovery panel; event search/filtering; a shared persistent execution service and robust process ownership; summary/manager model roles; authoritative per-attempt run records; and end-to-end UX/accessibility testing.

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

### Terminal behavior

`Tab` moves focus. `j/k` selects a document when the list is focused. `PgUp/PgDn` scrolls the reader, `/` inside a document should open **search** only if the input focus makes that behavior unambiguous; otherwise use a visible command. `Enter` on a document opens/focuses the reader, not the agent composer. Offer a visible way to copy text and return to the previous focus.

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
| **M0 — Stabilize PR #6** | Compile/test, permission keyboard semantics, artifact tab, smoke tests | Existing functionality trustworthy | Local `cargo test`, Clippy, TUI interaction smoke, generated lockfile committed |
| **M1 — UX foundation** | Session home, stepper, typed event cards, focused key map, errors | “Where am I / what's next?” answered | 80×24 usable, blocked/review states obvious |
| **M2 — Review artifacts** | Markdown renderer, preview/raw, version metadata, search, approval evidence | Plans/reports readable without editor | No unsafe escape sequences, version identity stable |
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

---

**Definition of UX success:** Greenlight should feel like a reliable supervisor's workbench: the user can delegate, observe, read evidence, decide, recover, and audit every outcome without losing control of the underlying workflow.
