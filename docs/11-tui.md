# Greenlight interactive TUI

Greenlight's Ratatui interface is a session conversation and a controller for **the existing Engine**. Both CLI and TUI use the same persisted histories, approval policy, workspace edit journal and fork/restore implementation.

The first [manager Q&A stage](21-manager-qa.md) implements `/ask`, context preview, saved exchanges and validated **unexecuted** forwarding proposals. With `manager.enabled: true`, ordinary messages ask the manager; `/resume` still explicitly runs a ready/blocked session. Default disabled preserves existing ordinary-message resume behavior. Executing manager routes and the [coordinator contract's](20-conversational-coordinator.md) live instruction inbox remain unimplemented.

```bash
cargo run -- --root /path/to/project tui
cargo run -- --root /path/to/project tui SESSION_ID
```

## Session workspace

The compact header shows the project and labeled state/current step. The full goal, workflow and attempt metadata/roadmap are under Details (`c`). The stepper there is an overview: loops can revisit steps and unvisited branches stay pending. It does not imply a linear execution plan. Long workflows window around the current step.

Press `c` or enter `/context` for a scrollable snapshot of the objective, required outputs, verification verdict, exact pinned input event IDs and approval references. An existing attempt uses its saved definition and manifest; a ready session shows the configuration for its next attempt. Missing legacy metadata stays explicit. Model prose never supplies a verification verdict.

Blocked sessions show a reason/evidence card after any diagnostic is dismissed. `e` opens its exact event, `H` returns to activity, `b` reopens the card, `2` opens artifacts, `i` composes resume instructions, and `r` opens the guarded restart flow. These controls never automatically retry. A live external owner is observation-only; interrupted ownership needs `/recover`, and unfinished native writes need CLI `recover-writes`. Context and evidence remain scrollable with arrows or `j/k`; Escape closes context.

Activity uses concise rows; Enter or a row click opens the selected event's evidence in the available pane, and Enter/Left/Escape returns to the list. Sidebar entries lead with the session goal. The session picker supports search and workflow creation as described below.

## Finding and creating sessions

The session sidebar is hidden by default. Sessions, Left/Esc from the list, or `f` opens it; rows show the goal and status/current step. Blocked sessions sort first; each group sorts by latest event across that session's branches. Sorting preserves the selected session by ID as background activity arrives.

From navigation press `f` to search goals, workflows, current steps, statuses or full session IDs. Matching is case-insensitive and accepts Unicode. Use arrows to select, Enter to keep the filter, or Escape to clear it. To clear an applied filter, press `f`, then Escape. The sidebar shows matching/total counts. A worker's approval switches back to its owning session and clears a filter that might hide it.

Press `n` to choose a workflow from the project's `.greenlight/workflows/` (or legacy root `workflows/`). Arrows or `j/k` select; Enter opens the goal editor. Invalid workflows show their validation error. Enter a nonempty goal and press **Enter to create and immediately start the session** through the existing engine and approval policy. Escape cancels before creation. The form revalidates workflow files on submission and retains other sessions' drafts. Creation is disabled while this TUI has an active worker; no separate `/resume` is needed for a new goal.

Goal and composer editors support Left/Right, Up/Down across wrapped rows, Home/End, Delete and Backspace, including Unicode graphemes. Ctrl+J, Alt+Enter or Shift+Enter inserts a newline (terminal support for modified Enter varies). Editable text is blue; labels/hints retain their UI colors. Active goal, composer, session search and activity-filter fields use a real terminal blinking caret and scroll to keep it visible.

The header shows STARTING immediately, then the persisted execution status. TUI-owned model calls request OpenAI-compatible SSE streaming with final usage. A small **Live model output** preview (normally five rows including its frame) shows the latest 64 KiB of unvalidated response text as it arrives, separately from recorded actions/results. Only a complete response enters normal action validation; truncated/malformed streams fail without executing their partial action or automatically retrying uncertain billable requests. Final provider input/cached-input/output usage remains in the central ledger; absent reports retain the usual estimates/unknowns. Providers that return ordinary JSON instead of SSE still complete normally, but cannot show partial text. Error diagnostics and approval prompts take precedence over the preview. `P` hides/shows the preview and `Z` expands/collapses it; both are also in More. Streaming is ephemeral, not an extra event per token; completed responses use the existing history records.


## Main interaction

Open a blocked or ready session. The composer never automatically captures shortcuts. Blocked sessions with recorded failures open a scrollable diagnostic first; inspect it and press Escape to return to navigation. From Sessions use **Enter** to focus the conversation/artifact list. Enter or clicking a row explicitly opens evidence; at wide widths the activity inspector can remain beside the list and follows deliberate selection. At narrow widths it uses a full-width reader. Right or Tab focuses the workspace actions instead of implicitly opening evidence. Press **i** or Compose to open the editor. Enter in the detail pane returns to its list. Type instructions and press Enter in the composer to resume the agent. While the agent works, its activity appears in the conversation timeline; the TUI refreshes automatically. Direct resume waits for the current worker to stop; read-only manager questions remain available through `/ask`.

Example instruction: `Fix the existing add() bug and rerun verification.`

The default conversation hides routine tool-permission decision records. Press `v` or use `/events` to include them in full history. Tool failures and human workflow checkpoints remain visible; the recorded audit history is unchanged.

The TUI executes resumed sessions in a background worker thread with a separate Tokio runtime and database connection. It does **not** fork a CLI process or bypass the Engine. All shell commands still use the mandatory Bubblewrap sandbox.

When the Engine requests permission to execute a shell command, write a file, use a tool, or approve a human verification gate, it sends the **same** permission prompt to the TUI. The foreground TUI captures a decision and sends it back over channels. The TUI now shows a separate color-coded choice menu. Press **`y`** to approve once, **`a`** for the process, **`p`** for the project or **`g`** globally (where offered), or **`n`** to deny. **A decision key submits immediately; Enter does not approve or deny.** Project/global choices open a separate persistent-grant confirmation: review the full scope, then press a fresh `y` to save and allow, or `n`/Escape to cancel and deny. Repeated key events cannot authorize a decision. Escape denies. Only `y/n` are valid for human verification. The available choices are enforced from typed permission metadata rather than inferred from tool text. Proposed native file diffs and required human review artifacts are shown in the approval pane.

## Semantic activity cards

The default activity list uses compact cards: a title/status, step and short summary; IDs remain in expanded/raw evidence. Execution cards describe recorded facts; optional **Agent intent** cards explicitly label model-authored annotations. Small panes show a single title line; Enter/Right expands the selected card, and Enter/Left/Escape returns. Opening evidence pauses activity following, including when inspecting the latest item. End/G in the list resumes it.

- New tool/command-step outputs link to their exact request and expand both arguments and result/error. Shell cards can show the recorded exit code. Other tools say **returned**, not “success”; file contents or model prose cannot supply an exit code or verification verdict. Old outputs without saved correlation say **unlinked**. Auxiliary `tool.finished` records and routine permission decisions stay hidden in the default list.
- Coding responses may include an optional bounded `activity_title`, such as **Exploring call sites**. A separate **Agent intent** card appears before the requested action, even if approval is pending or the action later fails. Its informational style does not assert success. Enter expands the exact model response and saved annotation; `F` searches the title. Titles never grant permissions or count as verification evidence. Without a supplied title, the existing recorded-fact cards remain unchanged. See [validation and history semantics](13-agent-action-protocol.md#4-optional-activity-title-implemented).
- Verification cards distinguish a failed individual check, recorded command exit, model-authored reviewer report, selected-evidence references and the aggregate policy verdict. A rejected check may coexist with a later aggregate pass under an any-of policy. A model report alone is not displayed as a verified pass.
- File cards identify native write/restore records; `d` opens the matching journal diff. Artifact cards identify a creation event; `o` opens that exact version, even after a newer same-name version exists. Its browser supports provenance and adjacent-version diffs as usual.
- Provider and partial-restore failures use their recorded structured fields. Unknown or malformed records retain raw evidence; invalid tool links become visible diagnostics rather than silently attaching unrelated output.

`v` or `/events` switches to the original raw audit timeline, including request/correlation records; it never deletes events or changes approvals. Expanding a raw row shows that event's original payload. Expanded card/diff evidence uses the same terminal-safe bounded reader, including background preparation for large bodies. Card projections are cached by session, branch and latest immutable event ID; redraws do not repeatedly parse reports. No extra model calls generate titles or summaries.

## Reading state, filtering and export

Activity and artifact tabs retain independent selection, reading/list focus and scroll positions by session and branch. Selections use event IDs and artifact names/version IDs, so a new row does not move an inspected item. Activity retains its follow/pause state and any opened journal diff. Help/context/diagnostic panels have separate temporary scroll; closing them returns to the saved tab position. If a selected record is filtered out or unavailable, the tab uses a visible fallback with its offset reset. A newer same-name artifact resets that artifact's offset rather than applying an older body's position. Positions, filters and drafts are **in memory only** and disappear when the TUI exits. Raw/Markdown visibility settings retain their existing behavior; this is not a persistent bookmarks system.

From activity navigation press **F** to filter event kinds, steps, card titles and summaries with a case-insensitive literal substring (including Unicode). Typing filters cached rows without reading the database or cloning their bodies; normal background polling still updates the source rows. Enter keeps the filter; Escape clears it. To clear an applied filter, press F then Escape. Filters are remembered per session/branch. This searches displayed metadata/summaries, not complete hidden payloads; use CLI history export for deep source searches. Lowercase `f` remains session search.

Press **C** from a reading/navigation surface to copy its **full selected text**, beyond preview clipping. Artifact content is the original stored text; artifact-version content uses the selected creation ID; provenance/diff panels export their current report. Activity exports the complete expanded evidence (or raw audit record in raw mode). Help, context, diagnostics and restore previews export their displayed source. Clipboard work runs outside the input loop and confirms success only after the utility accepts the complete input and exits successfully.

Copy uses `wl-copy`, then `xclip`, then `xsel`, with a two-second limit per utility. At least one utility and a working Wayland/X11 desktop session must be available. It targets **the machine running Greenlight**; SSH/cloud sessions do not automatically copy to your local terminal's clipboard. There is no OSC 52 fallback. Missing/failed utilities produce a scrollable diagnostic with CLI-export guidance. The harness sends original text, including control bytes, to the clipboard utility; it does not send the escaped/clipped preview or log document contents.

Press **E** to open the selected text as a private read-only temporary `.txt` snapshot using `VISUAL`, then `EDITOR`, then `vi`. The TUI pauses and hands over the terminal until the editor exits, then redraws and restores input. Editor errors also restore the terminal and show a diagnostic. Editor commands support quoted program paths/arguments via `shlex`; they are launched as argv, not interpolated shell commands. Common Code/Codium/Cursor/Sublime commands receive `--wait` when absent, and gvim/mvim receive `-f`; other GUI editors must be configured to wait for the file to close. Snapshots are cleaned up on normal return, including editor errors. Forced process termination can leave a private temporary file.

Edits to that snapshot are **not imported** into stored artifacts, history, project files or the composer. Save a separate file and explicitly use `put-artifact` if a new artifact version is intended. Editor handoff is refused while this TUI's own worker runs so approval dialogs remain available; copy remains usable during runs. These helpers are explicit local user actions, not sandboxed model-tool calls. C/E are literal text in composer/search/filter inputs and inactive during approvals. Native approval keys and permission semantics are unchanged.

## Failure diagnostics

Worker failures/disconnects, blocked-step reasons, malformed known structured events, missing artifact bodies and partial restores appear in a red attention panel. Up/Down or j/k scroll the complete message; Escape dismisses it without changing execution state. Polling preserves diagnostic scroll position and does not repeatedly reopen an acknowledged unchanged error. Raw event evidence remains available; unknown event types remain readable rather than being rejected. These panels do not interpret model prose as a verified outcome.

Restore failures include files changed versus files journaled. A failed restart does not fork, and a restore-then-fork failure is explicitly distinguished. Inspect workspace/history before retrying; a partial restore is not automatically rolled back. Artifact read failures preserve history access.

## Provider failures and retry waits

Provider errors show actionable guidance for credentials, account credit/quota, rate limiting, permissions, invalid requests and provider/network problems. Final failures block the session and appear in the existing diagnostic/history panels. Errors during verification stop the run directly; they cannot trigger a verification-failed workflow retry or detector call.

Eligible rate-limit rejections or connection failures can retry with at most three requests. The activity list records each failure and retry delay. Press `x` in navigation (including a reading/help/diagnostic panel) to cancel a retry wait and stop future provider requests from this worker. It does not interrupt an in-flight HTTP request or stop a tool already running; a returned action may still finish before the next provider-call boundary. `x` stays literal text in the composer/search, and is inactive during approval dialogs. A fresh resume creates a new worker with cancellation cleared. See [provider failures](19-provider-failures.md) for retry/billing limits.

## Token usage

The footer reports input/cached-input/output token totals for recorded requests across the selected session’s branches. `~` marks estimates and `?` marks incomplete counts. Enter `/usage` in the composer for live-refreshed counts and estimated cost subtotals by model/attempt; arrows scroll and Escape closes the view. Unknown rates or cache hits do not become zero charges. Use CLI `usage SESSION_ID --json` for per-request provenance. Historical calls before the ledger was introduced are not reconstructed.

## Artifacts tab

Press **2** in navigation mode to open the **Artifacts** tab; press **1** to return to the conversation. The left pane lists the most recent version of each artifact on the selected session's active branch, along with its producing step and byte size. The right pane renders headings, emphasis, inline/fenced code, lists, blockquotes and simple pipe tables. Press **m** in navigation mode to toggle between rendered Markdown (the default) and a literal Markdown-source preview; the original stored artifact is never rewritten. Other CommonMark features, hyperlinks and complex table layouts are not yet supported. **j/k**, arrows and **g/G** select a saved artifact; **Up/Down** or **j/k** scroll the artifact when the detail pane is focused. The selected artifact remains selected when newer versions are registered.

Both preview modes visibly escape terminal controls (including ANSI/OSC escape bytes, C1 controls and bidirectional formatting controls) as `\u{xxxx}` text. CRLF is normalized to newline, a lone carriage return is escaped, and tabs display as four spaces. International text, combining marks and emoji joiners remain intact. Simple table padding uses Ratatui terminal widths; unusually wide cells wrap normally, and alignment beyond the padding budget is not guaranteed. Longer code fences require a matching closing run of at least the opening length with no trailing content.

Preview processing stops at the first of 256 KiB of source/sanitized text, 4,096 source lines or 8,192 sanitized bytes on one line. Both modes show an explicit truncation notice. Tables recognize up to 32 columns, with at most 240 cells of padding width per row; excessive-column tables stay literal. These are display limits independent of the workflow's artifact-size policy. To retrieve the full stored artifact, use `greenlight artifact SESSION NAME > artifact.md`; the CLI output is unfiltered, so redirect it to a file rather than relying on a terminal preview. The literal toggle preserves Markdown syntax, but is not an unfiltered terminal or unlimited export.

Artifacts are inputs to later workflow steps and an important human-review surface. This tab deliberately uses the same branch-aware `Store::artifacts` lookup as the CLI and does not alter artifact content. The default list shows **latest versions**; press `v` for historical versions. To register a manually edited version, use `greenlight put-artifact SESSION_ID NAME --file PATH` from another terminal; direct artifact editing remains a follow-on feature.

## Preview responsiveness

Artifact/version content and expanded activity evidence over 8 KiB or 64 lines are prepared on a background preview worker. While it parses, wraps and prepares the visible viewport, the panel shows **Preparing document preview…** and keyboard input remains active. Moving to another version, toggling Markdown/literal mode or resizing cannot paint a result from the old selection. The worker reuses prepared text and wrapped line counts; an unchanged viewport is reused on redraw. Its input/result queues each hold at most one pending item. Preview limits and terminal escaping are unchanged, and the worker has no engine/store or approval authority.

Short previews use the bounded synchronous path. Storage refresh/blob reads, explicit diff generation and approval-request rendering retain their existing paths; this change does not promise asynchronous execution of every TUI operation. If the preview worker fails, the panel reports that it is unavailable and CLI export/navigation remain available.

## Artifact versions and provenance

In the Artifacts tab, select an artifact and press `v`. The version browser shows saved creation-event IDs, producing steps and UTC timestamps, newest first. It only includes versions visible in the active branch's ancestry at each fork cutoff. Parent changes made after the fork and sibling/session versions remain excluded.

Use arrows or `j/k` to select a version. Enter or a row click opens its content at the available width; Enter again or Left returns to the version list. `o` opens content, `m` toggles literal/Markdown content, `p` opens provenance, and `d` compares the selected version with its immediate older visible version. In reading mode arrows/j/k, PgUp/PgDn and Home/End scroll. Escape closes the browser and returns to latest artifacts. Controls are read-only and preserve composer drafts. The inspected creation ID stays pinned when newer versions arrive; changing session/branch closes the browser. Incoming approvals take priority and close it.

Provenance shows the exact creation event, source branch (including inheritance), associated recorded attempt/definition when the creation falls in that step's attempt interval, selected-input consumers and human approval events referring to that exact artifact event. An interval association does not identify whether a human or model authored the content. Unknown/legacy metadata is explicit. Historical approvals describe recorded review, not a fresh authorization or approval of later replacements. Malformed manifests/approval payloads are reported rather than reconstructed from latest names.

Missing blobs show an error for content/diffs while metadata/provenance remains available. The first visible version has no predecessor. Identical bodies are labelled as identical; differing versions use the existing newline-aware unified diff. Unified diff generation is limited to 256 KiB per input; full originals, never clipped previews, are compared within the limit. Result previews retain terminal escaping and display limits.

Export a specific visible version with `greenlight artifact SESSION NAME --event EVENT_ID > version.md`. Without `--event`, the CLI still exports the latest visible version. The event must belong to that artifact, session and active branch ancestry. Exports do not modify history or trigger execution.

## Restart from a historical step

Type `/restart` in the composer. Use `j/k` to select an earlier **executed** workflow step and press Enter. Greenlight selects the checkpoint immediately before the first recorded attempt at that step, constructs a **guarded workspace restoration plan** from its native edit history, and displays the file changes. Press `y` to apply and fork at that event; `n` or Escape cancels with no modification. This is a real workflow branch, not a text instruction asking the model to pretend it is at a different step.

The restore planner refuses mismatched file snapshots, manual drift, symlinks, and unsupported paths. Its guarantees apply only to recorded native writes/restores, not manual edits or untracked shell operations. Restoration applies individual files; if an error occurs partway through, the error is displayed and the user must inspect the workspace/history. Never retry blindly after a partial restore. Restart does not automatically execute the new branch; send a message or `/resume` from the composer.

## Keys

The focus model uses explicit evidence opening: Enter or click opens details, Enter/Left/Escape returns, and move or scroll in the currently focused pane rather than switching into an implicit editor. For an AZERTY-friendly keyboard, **Up/Down** or **j/k** handle detail scrolling and approval/restart previews; **PageUp/PageDown optionally scroll eight lines**, and Home/End jump to the beginning/end of reading panels. Navigation opens without focusing the composer, even for blocked sessions.

- **Enter/click** opens selected evidence; **Left/Esc** returns or opens/closes Sessions. **Right** focuses the action row. Focused detail content scrolls independently of selection.
- **i** or Compose opens the composer; **Enter** in the composer sends. Enter in detail returns to the list. **Escape** from the composer returns to the previous pane.
- **Ctrl+J** inserts a newline into the composer; **Escape** returns to the previously focused pane.
- **1 / 2** switch between Conversation and Artifacts tabs while in navigation mode; **m** toggles rendered Markdown/raw text in Artifacts.
- **Tab/Shift+Tab** enters/cycles workspace action controls; Enter activates an ordinary focused action, Esc returns. More opens a keyboard/mouse action menu. No function keys are required.
- **j/k**, Up/Down and **g/G** or Home/End navigate sessions, events and artifacts when a list is focused; Up/Down or j/k scroll line by line when detail is focused.
- **d** displays a journal-backed unified diff for a selected native write or restore; **v** toggles concise/full event visibility.
- **Up/Down** (or **j/k**) also scroll permission requests and restoration previews when those dialogs are open.
- **F** filters activity; **C** copies the full selected text; **E** opens a temporary review snapshot in the editor. All are uppercase navigation keys.
- **h/?** opens contextual keyboard Help; **/help** opens it from the composer. Help owns input above approvals: y/a/p/g/n cannot decide there, and Esc closes only Help. Expired requests never regain controls. Within Help, Tab reaches Close and Toggle mouse; **M** toggles capture directly. While editing, letters and `?` remain literal text.
- **/restart** chooses the step to restart; **/resume** resumes without extra instructions; **/recover** recovers the selected session's interrupted execution owner without running it; **/events** toggles raw event visibility; **/help** lists shortcuts.
- **q** quits from navigation mode when no agent is running. The TUI intentionally prevents quitting while its own worker is active, to avoid silently abandoning a pending prompt.

## Drafts, following and terminal size

Unsent composer text belongs to the selected session ID. Switching sessions, changing filters, opening help, cancelling restart and returning to an approval's owning session retain each session's draft separately. A new session starts with an empty draft. Drafts remain **in memory only**; exiting the TUI discards them. Long messages scroll to keep the editing caret visible.

Activity initially follows incoming events. Opening evidence or navigating activity pauses following, including when reading the latest event. Background polling preserves that event and its scroll position. In the activity list, End or `G` selects the latest visible event and resumes following; Home or `g` selects the first and pauses. The activity title indicates following/paused. Artifact and diagnostic scrolling remains independent of new activity.

The compact header is used at every size. At narrow widths common header actions use a second navigation row; short button labels fit their available cells. Below 72 columns or 22 rows the open session picker/workspace uses full width; close Sessions to return to the workspace. Wrapped-content scroll bounds are recomputed at the current size, so overscrolling and resizing do not leave a blank reading panel. Below 40×14 it shows a resize notice and retains drafts/selections. Approval requests require at least 40×20; until then decision keys are disabled and Escape denies. Restoring the terminal size restores the pending request. Enter remains inert during approvals at every size.

The wrapped-line calculation uses Ratatui's `unstable-rendered-line-info` feature on the version recorded in `Cargo.lock`; dependency upgrades must retain the resize/scroll tests.

## Limitations

Initial TUI control is intentionally serial: only one runner per TUI, streaming previews for this TUI’s worker, no direct interruption of an in-progress model call, and no interactive control of another process that happens to be running a session. Read-only observation of external sessions remains possible. The underlying CLI remains available. A per-session OS lock prevents concurrent CLI/TUI execution of the same session; competing resumes are rejected without changing the owner's state.

If a worker disconnects, its pending approval dialog is discarded and the TUI reports the loss. Use `/recover` (or CLI `recover-session SESSION_ID`) after inspecting history. Recovery refuses a live owner, preserves saved grants, and never replays an unanswered approval or automatically resumes work. If a native write was interrupted, run `recover-writes` afterward before resuming. Completed sessions remain completed even if the worker died during final ownership cleanup.

Approval choices and restoration actions retain their existing security semantics. The TUI requires an interactive terminal and uses Ratatui's terminal restoration. Raw events may contain private project information.

The Ratatui/Crossterm dependency versions are locked in `Cargo.lock`. The [decision log](16-decision-log.md) captures the keyboard rationale, and the [smoke-test guide](17-smoke-test.md) covers the related manual exercise.

## Approval command emphasis and former turn-limit diagnostics

Shell permission and persistent-grant confirmation panes show the exact command in a separate bold cyan block, with scope/location in yellow and sandbox context in gray. The command comes from typed request metadata; model text cannot change available approval choices or grant matching. Multiline commands, arguments and operators remain visible in the scrollable request. Nonprinting control/format characters are escaped in the command preview; the engine receives the original unchanged command.

A recorded `step hit limit of 16 model turns` diagnostic means an older engine stopped that attempt after 16 coding responses. Responses can request tools, register artifacts or need protocol correction; this count is not exclusively tool calls. The fixed model-turn cutoff has been removed, including for existing configurations containing legacy `max_turns`/`max_turns_per_attempt`. Existing historical diagnostics remain evidence of the old attempt. After upgrading, dismiss with Esc and use `i`, `/resume`, Enter to retry the blocked step as a new attempt. Explicit cancellation, provider failures, verification, configured deviation retry bounds and the workflow transition guard still apply.

## Completed-run results and recorded source edits

When a selected run completes (or a completed session is first loaded), the Activity view opens a result panel. `s` reopens it; `H`, Escape or Enter returns to activity. The overview separates a three-line, attributed conclusion excerpt, final-step verification scope and the recorded native-edit count. It does not dump the report body into the overview. Read final report opens the exact historical version; Review edits opens recorded diffs; More → Full result preserves the complete summary/report evidence. Step summaries/reports are labelled model-authored. Completion means control flow reached the end, not that performance improved or the user's objective was independently validated. `verification: none` displays “no checks required”; its aggregate pass is not a substantive test. Artifacts replaced after completion do not rewrite the report for that run.

Press **3** for **Edits**. This lists paths recorded by the selected session's active branch ancestry and shows their net journal diff: first recorded pre-edit contents to latest recorded state, including previous explicit restores. Newly created files are labelled added; already reverted files remain in history with no net change. Artifact-only actions are not source edits. Missing snapshots and history gaps are explicit errors, not a claim that no edits occurred. Enter toggles between the file list and scrollable diff; copy/editor snapshot actions also work here.

- `u`: preview reverting the selected file.
- `U`: preview reverting all recorded files.
- `y`: apply a fresh preview; `n`/Escape cancels. Enter and held/repeated confirmation keys never authorize a revert.

Revert restores the original contents recorded before the session first touched each selected file, preserving any pre-existing manual work in that snapshot. It deletes files originally created by the model. It uses the existing guarded restore engine: execution must be idle, no writes may need recovery, history/branch and current contents must still match the preview, and sensitive/symlinked paths remain forbidden. Per-file revert checks only the selected path's history/current state; unrelated manual drift does not prevent that file's revert. Revert-all preflights all tracked paths before writing. Manual edits are never force-overwritten.

Reverting changes workspace files and appends restoration records; it does not fork/resume the workflow or erase its artifacts/history. The result then states that its report/checks describe the earlier run, not the restored workspace. This covers tracked native UTF-8 writes, not arbitrary shell/manual changes, Git operations, service state or external effects. Multi-file restore retains the existing partial-failure/crash limitations in [workspace restoration](09-workspace-restore.md); failures report what happened rather than claiming atomic undo.

## Mouse, paste and the first redesign slice

Mouse capture is enabled by default for this TUI process. Click tabs, workspace actions, More entries, session/activity/artifact rows or the explicit approval-choice rows; keyboard paths use the same routing and guards. Press and release must target the same current control. Resizes, request replacement and layer changes invalidate stale targets. Wheel events navigate/scroll the active region (or the hovered listed row's region). Click inside a focused editor to place its caret.

Open Help (`h`/`?`) and use Toggle mouse or `M` to release capture for terminal text selection; the setting is memory-only. Bracketed paste inserts literal text into the focused editor/search field, respects its size limit and never submits. Terminals must support bracketed-paste delimiters to distinguish a paste from keystrokes. Capture and cursor settings are restored on exit and suspended for the external editor.

The composer and sidebar are not permanently allocated. Activity/artifact/native-edit inspection opens deliberately; the workspace action row replaces the old full static key legend. Blue input, cyan focus, neutral model output, scoped green outcomes, amber attention and red errors/destructive actions have separate roles.

This implements the compact workspace/interaction foundations from [the plan](34-ui-implementation-plan.md), not every mockup. Edits All-files Tab cycling and Keep-file/Keep-all review markers, explicit-purpose composer modes, and the fully redesigned result/recovery/artifact/checkpoint surfaces remain pending. Existing typed approvals, native restore guards and accounting contracts remain authoritative.

## Rendered presentation and contextual controls

The TUI uses a shared dark background/panel palette and rounded, framed action buttons, with a filled primary action. The bottom row depends on the current screen: results expose Read final report / Review edits / More; artifacts expose Open selected / Versions / More, or Back / Versions / More while reading. Sessions, Details and Help remain in the header, reachable through the same keyboard focus cycle. Artifact lists use one row per document, with producer and size columns when width allows. Full metadata and version references remain inspectable.

[Actual Ratatui captures](screenshots/README.md) show the current code at explicit cell dimensions. Earlier [design drawings](mockups/README.md) are visual direction rather than terminal screenshots. Text snapshots guard the default 80×24 result and artifact list against accidental return to the older layout.
