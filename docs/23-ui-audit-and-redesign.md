# Greenlight UI review: screens, components and interaction design

**Status: DRAFT FOR JOINT REVIEW — no UI implementation authorized by this document.**

**Date:** 2026-11-17, Europe/Paris. **Audited baseline:** `5a982be` on `feat/install-and-project-layout`.

**Scope:** reduce visual noise, establish a color system, improve discoverability and choose a larger set of presentation tools before writing UI code.

This is a source-based inventory of the current application, informed by the user's real-session screenshots. It is not a claim that every state has been visually exercised in every terminal. Proposed controls and wireframes below are not implemented screens. This review updates the visual/interaction direction in the [earlier dated UX research](12-tui-ux-analysis.md); it does not replace execution, approval or restore contracts. [Current TUI behavior](11-tui.md) remains the operational reference until changes land.

## Review decisions — 2026-11-17

User edits from commit `ef9672a` and the subsequent layout discussion establish the choices below. These are **design decisions**, not implemented keybindings or application behavior.

| Topic | Agreed direction | Still open |
| --- | --- | --- |
| Help key | Lowercase `h` opens Help in navigation; keep `?` as an alias | Uppercase `H` is the user's suggested replacement for the existing Activity/history binding; final remapping remains to review |
| Session navigation | Collapsible sidebar, **hidden by default**, opened through a visible Sessions control | Pinning and persistence across application launches have not been decided |
| Activity/file details | **Enter or click explicitly opens details**; use an inspector when width permits and a full-width reader on small terminals. Once open, it follows deliberate selection changes | Focus transfer, return controls and item-specific reading positions remain to validate |
| Composer | **Open on demand** through a visible Compose action; closing preserves the draft | Recipient/mode presentation and precise sizing remain to review; existing drafts are memory-only, not durably saved |
| Semantic colors | Blue user input, cyan focus/active controls, neutral model body, green recorded positive outcomes, amber attention, red errors/destructive actions | Exact shades, optional attribution accents and theme fallbacks remain to validate |
| Mouse | **Enabled by default** for clicks/scrolling, with complete keyboard operation and a discoverable way to disable capture | Hit regions, terminal text-selection interaction and supported-terminal behavior require implementation/testing |
| Help over approvals | `h` opens read-only Help above the pending decision; decision keys are disabled while Help is open; Esc closes Help and returns to the approval | Exact Help content, focus restoration and request-expiry presentation remain to design/test |
| Approval choices | **Every valid grant choice visible immediately**, including process/project/global where valid; persistent grants retain a separate confirmation | Exact layout and labels remain to validate; see [approval studies](25-approval-dialog-design.md) |
| Running header | **Compact:** project/session, status and current step; full goal/roadmap/technical metadata under Run details | Exact row count, sizing and narrow-layout action placement remain to validate |
| Live output | **Small preview**, expandable and hideable while generating | Exact preview height, expanded-reader presentation and truncation treatment remain to validate |

List browsing should therefore remain quiet while details are closed. The normal workspace does not reserve space for the sidebar, inspector or composer until the person opens them. The other recommendations below remain proposals unless explicitly marked settled.

## Review map

For a first pass, read sections 1, 4, 5 and 10. Sections 2 and 7 are the detailed checklists for our screen-by-screen discussion; section 3 is the researched toolkit.

1. [Diagnosis and user goals](#1-diagnosis-and-user-goals)
2. [Every current screen and presentation state](#2-every-current-screen-and-presentation-state)
3. [What Ratatui actually permits](#3-what-ratatui-actually-permits)
4. [A smaller information hierarchy](#4-a-smaller-information-hierarchy)
5. [Color, typography and status rules](#5-color-typography-and-status-rules)
6. [Contextual actions, buttons and help](#6-contextual-actions-buttons-and-help)
7. [Element-by-element review](#7-element-by-element-review)
8. [Feedback and missing components](#8-feedback-and-missing-components)
9. [Conceptual layouts](#9-conceptual-layouts)
10. [Decisions to discuss before implementation](#10-decisions-to-discuss-before-implementation)
11. [Acceptance criteria and possible delivery order](#11-acceptance-criteria-and-possible-delivery-order)
12. [Research sources and audit references](#12-research-sources-and-audit-references)

## 1. Diagnosis and user goals

The main problem is competing information, not a shortage of widgets. Adding colors and windows without reducing what is visible would make the interface busier.

The current header can spend seven rows on goal, workflow, status, step, attempt identifiers, stepper, next-action copy, tabs and shortcuts. A sidebar repeats goal, workflow, status and step across four rows per session. A context box repeats the current objective and verification. The composer remains visible even during dialogs that do not accept messages. Pane titles and the footer repeat navigation instructions. Activity exposes record numbers, technical event names and internal payloads alongside user-facing outcomes.

There **is already conditional footer content**, so it is not entirely static. However, many branches still append a permanent shortcut list, and the footer is selected mostly by screen/modal flags rather than the precise focused item and action availability. Helpful actions can be truncated; a visible hint need not be usable from the current overlay. The proposed replacement is therefore a focus-aware action dock, not another footer with more shortcuts.

We already have dialogs in the **input-routing** sense. Approvals, help, restart and diagnostics take over input. Most are drawn **in place of the main reading pane**, not as floating windows over a recognizable workspace. This makes a change in interaction mode look like a change of document.

Every ordinary screen should answer three questions without opening help:

- **What is happening?** One clear state, with a short factual explanation.
- **Does it need me?** A visible primary action when a decision or recovery is needed.
- **What can I do here?** A small set of relevant, enabled actions.

The longer questions—exact attempt, event, branch, payload, usage breakdown and permission provenance—belong in an inspector, report or help surface. Their records must remain available; moving them out of the default view must not delete evidence.

### Translate the user's requests into design requirements

| Request | Design requirement | Recommended direction |
| --- | --- | --- |
| More color | Assign semantic roles consistently; retain meaning without color | Named theme roles, visible focus, text labels and limited accents |
| Remove static information | Give content back its space; retain useful discoverability | Compact header, no permanent shortcut wall, technical metadata in Details |
| More UI tools | Use the right presentation for each job | Real overlays for short tasks, drawers for evidence, full readers for long content, action buttons |
| Layer information | Show the next useful level deliberately | Summary → selected item → evidence → raw audit record |
| Graphs/icons/Greenlight motif | Convey state without inventing progress or success | Small status light plus text; observed stepper; charts only for supported data |
| Better feedback | Show receipt, work, result and failure separately | Explicit action lifecycle and durable important notices |
| Better nudging | Put the next action beside its reason | One primary action and up to two secondary actions; overflow for the rest |

## 2. Every current screen and presentation state

These are logical surfaces, not independent application windows. Several share the same renderer and can coexist with the sidebar/composer. Variants are listed when their information or available actions materially differ.

### 2.1 Session entry and primary workspace

| ID / current surface | What it shows and does now | What the person wants to do here | Current friction / proposed presentation |
| --- | --- | --- | --- |
| S01 — No sessions / nothing selected | Empty session list, generic workspace/context placeholders and composer | Understand the project, start a workflow, discover help | Make this a deliberate empty state with **New session**, workflow explanation and Help; do not show an inert message box |
| S02 — Session list / selected session | Goal, workflow, status, step and last recorded UTC activity; selection opens its workspace | Locate ongoing work, see which session needs a decision, switch or create | One/two-line rows; attention badge and human title; move repeated metadata to selected-session details |
| S03 — Find sessions | Search editor, match count and search instructions; live filtering, Up/Down selects | Find by goal/workflow/status; clear filter; open a result | Search inside the session switcher or sidebar, not a replacement for the entire content pane |
| S04 — New session: workflow selection | Valid/invalid workflow list; title, validation errors or no-workflow help | Choose the right workflow; inspect what it does; back out | Short focused dialog with workflow description, **Continue** and **Cancel** |
| S05 — New session: goal | Multiline goal editor; workflow title, hints and validation error; Enter creates and starts | State the goal, review the chosen workflow, start confidently | Dedicated form with one active editor, **Start workflow** and **Cancel**; hide the unrelated session composer underneath |
| S06 — Ready session | Ready status, current step, next-run context; composer can submit instructions or `/resume` | Start the step, add instructions, inspect workflow | A **Start step** action is more discoverable than requiring a slash command |
| S07 — Starting / waiting for a response | STARTING before a persisted run; local live pane initially says starting/waiting | Know submission was accepted; distinguish preparation from a frozen application | Immediate acknowledgment followed by truthful preparing/request/wait labels; one restrained activity indicator |
| S08 — Running activity | Header/stepper, context box, semantic cards; optional agent intent and local live preview | Understand current activity, inspect latest outcome, read older evidence, stop future requests | State-aware overview plus short timeline; expand raw streaming only when useful; keep reading position |
| S09 — Tool/model/provider retry wait | Live preview and/or recorded failure/retry cards, notice text and cancel control | Understand what is delayed, whether another request will happen, how to stop retries | Visible bounded retry notice and **Stop requests**; show a real countdown only when the scheduled deadline is known |
| S10 — Observing another process | Persisted running activity and owner information, but no local runner's stream or approval channel | Observe without accidentally trying to control somebody else's run | Explicit **Observing another process** badge; explain why local steering/approval actions are unavailable |
| S11 — Blocked overview | Blocker card/reason, step objective and recovery instructions | Identify the reason, inspect evidence, decide whether to retry or restart | Cause-first warning card with supported recovery actions; distinguish a known failure from missing human evidence |
| S12 — Interrupted owner / unresolved write state | Workspace action points to owner recovery or CLI write recovery; sometimes a diagnostic | Understand what stopped, recover ownership or reconcile writes before doing more work | Separate recovery state with exact next action; do not present ordinary Resume as ready to work |
| S13 — Completed result | Automatic result panel, reported conclusion, historical report excerpt, edit count and verification scope | Know what actually happened, what remains unverified, inspect edits or undo | Outcome-first summary; retain distinction between workflow completion and demonstrated improvement; avoid stealing focus from someone already reading |

### 2.2 Reading, review and inspection

| ID / current surface | What it shows and does now | What the person wants to do here | Current friction / proposed presentation |
| --- | --- | --- | --- |
| S14 — Concise activity list / filter | Cards with event numbers, summaries, following/paused state; `F` filters metadata/summaries | Browse relevant milestones, find an error, resume following | Human labels by default; small filter chip/search row; technical IDs and raw payloads on expansion |
| S15 — Activity detail / recorded tool or write evidence | Enter replaces the list with expanded evidence, linked request/output, annotations or journal diff | Read what was requested and what actually happened; return to the same item | Inspector drawer on wide terminals; full reader with Back on narrow terminals; show model intent separately from outcomes |
| S16 — Full/raw history | More protocol/audit rows, technical kinds and exact payloads | Diagnose protocol or accounting failures; export exact evidence | Clearly named **Audit history** in More/Details, not a second meaning of an unexplained key |
| S17 — Artifacts: list + document | Filename, producer step, bytes and a Markdown/raw document reader; empty/error variants | Find the useful report, read it at a comfortable width, inspect the relevant version | Group or highlight required/final outputs; let the reader take the full content width; metadata under Details |
| S18 — Artifact version browser | Version list, timestamps, event IDs, inherited marker and selected body | Compare versions and know what a particular attempt used | Versions drawer or explicit reader mode; human-readable version context first, exact record identifiers on demand |
| S19 — Artifact provenance / adjacent-version diff | Producer/consumer/approval references or immutable version differences | Establish which evidence was reviewed; distinguish document revisions from source edits | A labeled Details/Compare mode tied to the selected version, with a clear Back target |
| S20 — Edits: files + recorded net diff | Branch-visible native-write/restore history aggregated by file; added/modified/reverted/gap labels | See whether code really changed; inspect an edit; choose one or all to revert | Recorded-file list and colored diff reader; visible coverage note; Revert actions bound to the selected file/session |
| S21 — Exact step context | Saved attempt definition/input references or next-run context, outputs and verification details | Check what the agent was instructed to do and what evidence it received | Technical inspector; normal step purpose remains short in the workspace |
| S22 — Usage report | Session counts/costs with estimates/unknowns and per-request detail | Understand cost, cache usage and unavailable pricing | Optional compact usage badge opens a report; do not fill every workspace footer with counters |
| S23 — Manager context preview / manager exchanges | `/manager-context` shows bounded evidence; `/manager` filters saved manager messages/results | Ask about status, inspect what was shared, read an answer without resuming coding | Separate **Ask manager** mode and labeled read-only exchanges; keep executing forwards explicitly unavailable |
| S24 — Keyboard help | Long scrollable help text replacing main content | Discover actions for this context, then return without losing focus | `h` (user-selected key): proposed focused popup with **This screen / Navigation / All actions**, instead of the full manual at once |

### 2.3 Decisions, confirmations and temporary surfaces

| ID / current surface | What it shows and does now | What the person wants to do here | Current friction / proposed presentation |
| --- | --- | --- | --- |
| S25 — Shell/write/web tool approval | Exact command, proposed native diff or request, isolation/scope text and valid direct-key choices | Understand the operation and scope; allow once or deny; review broader scope deliberately | True decision overlay with owner title, request body and separate action row; long command/diff can expand to full reader |
| S26 — Human step / human verification gate | Human prompt plus selected artifact/version evidence; only approve/deny | Read the actual evidence before deciding | Review dialog linked to exact artifacts, not a generic green success prompt; keep the gate pending during reading |
| S27 — Persistent grant confirmation | Exact command/write scope and project/global location; fresh yes/no decision | Understand future consequences and cancel without saving | A separate confirmation surface with scope always visible; preserve typed mode and deliberate confirmation |
| S28 — Restart selection | Executed-step candidates and historical checkpoint IDs | Choose where to restart and understand what that means | Step names, attempt context and plain consequence text; checkpoint IDs in inspection details |
| S29 — Restart restore/fork preview | Recorded file diff and restore/fork warning; yes confirms, no cancels | Review changes, safely cancel, restart deliberately | A dedicated review surface with explicit **Restore and restart branch** wording; not the same operation as workspace-only undo |
| S30 — Revert one/all preview | Target scope, guarded reverse diff, history-preservation explanation; explicit confirmation | Check which files change and confirm/cancel undo | File count and target at top; exact diffs; separate warning for conflicts/partial failure; never hide scope behind More |
| S31 — Diagnostic / recovery failure | Provider/store failures, malformed recorded payloads, missing bodies or partial restores with evidence; ordinary action-format `protocol.error` is an activity record | Know the cause, preserve data, find a supported next action | Concise persistent error/attention banner plus readable details; choose modal only when a decision or recovery truly blocks work |
| S32 — Document preparation / missing or truncated preview | “Preparing document preview…”, export fallback or truncation notice | Know reading is loading, recover the full source, keep navigating | Local loading/error/limit state inside the reader; do not take over the whole workspace |
| S33 — Copy / external editor handoff | Notice while copying; temporary editor snapshot suspends/resumes the terminal | Know what was copied and where; return safely from review | Small success receipt; persistent failure; label **Review snapshot in editor**, not **Edit source**—changes are not imported |
| S34 — Terminal too small | Resize message; drafts/selections retained; stricter minimum for approvals | Know why interaction is limited, resize or back out safely | Clear minimum and safe actions; critical decisions must not be offered while content is clipped |

A pre-TUI startup failure is a separate terminal surface: missing project configuration, invalid root/configuration or lack of a TTY can stop launch before any workspace exists. Keep its instructions actionable. A future configuration page would require deliberately changing that startup path; no such page exists now.

### 2.4 Variants that must not be lost during redesign

These modify the surfaces above rather than adding another permanent page:

- Empty versus filtered-empty lists; invalid workflow versus no workflow directory.
- Agent composer versus manager composer; no session, active worker, completed session and draft-retained send refusal.
- Local starting/waiting/receiving/response-complete/stop-requested state versus externally observed activity. The manager worker is separate and does not share the coding stream.
- Authentication, credit/quota, permission, request, rate-limit/retry, connection, timeout and protocol failures; tool denial is not an overall workflow failure by itself.
- Approval requested, answer submitted, answer recorded/applied or failed, request expired after worker loss; project/global confirmation remains distinct.
- Artifact Markdown/raw, inherited versions, missing blobs, malformed records, display truncation and background preparation failure.
- Edit history gaps, net-zero/reverted paths, post-write manual drift, stale previews, branch changes, live/interrupted ownership, pending writes and partial restoration.
- A finished run with no checks, scoped checks, missing evidence, model-reported measurement failure or later workspace restoration. None should be collapsed into an undifferentiated green “success”.

**Not existing screens:** settings editor, command palette, real mouse buttons, native file tree, benchmark dashboard, independent app windows, durable free-form `ask_user` waiting and live instruction delivery. The human-question tool was discussed separately but is not implemented at this baseline. A new drawing surface cannot supply that backend behavior.

## 3. What Ratatui actually permits

### 3.1 Research result and version boundary

Greenlight pins **Ratatui 0.30.2** with Crossterm 0.29. The official versioned popup example and widget source confirm the main proposal can be built with this stack. The website's current documentation is useful guidance, but newer examples/APIs must be checked against the pinned version before implementation.

Direct requests to `ratatui.rs` and `docs.rs` were blocked by this cloud environment's destination policy. Research continued online through the **official documentation repository and versioned Ratatui source on GitHub**, which are reachable. The website snapshot read was `cceec13d0152da3a66305cf9230a124ae010bd74`; the pinned Ratatui tag is `ratatui-v0.30.2`, peeled commit `e665c36cb14752a61cd777fbd06dbef8474f2add`. No network policy/environment configuration was changed. Sources are linked in section 12.

Ratatui primarily **draws cells into terminal rectangles**. It does not automatically give an application a desktop window manager, focus system, clickable button behavior or accessible modal event routing. Those require application state and backend input handling. `List`/`Table` selection rendering does not mean they process key presses for us.

### 3.2 Presentation toolbox: strengths, limitations and recommendation

| Method / primitive | Support and strengths | Limitations / responsibility | Recommendation for Greenlight |
| --- | --- | --- | --- |
| Styled text, spans, backgrounds, borders | Core `Style`, `Span`, `Line`, `Text`, `Block`; precise hierarchy and focus | Overusing color/borders competes with content; terminal palettes vary | **Use consistently**, with semantic theme roles rather than ad hoc colors |
| Tabs | Core `Tabs`; compact navigation between a few stable contexts | Input handling remains ours; too many tabs hide actions | **Keep three main sections**; current tab labels are manually drawn spans, not the core Tabs widget |
| Lists and tables | Core selectable `List`/`Table`; good for sessions, versions, files and compact usage records | Multiple columns become unreadable at narrow widths | **Use**, adapting rows/columns to available width |
| Split panes | `Layout` and `Rect`; list alongside reader/inspector | Every extra pane consumes a large share of 80×24 | **Use selectively**; optional sidebar and one inspector, not three permanent narrow columns |
| Centered floating popup | Render underlying scene, then `Clear`, then a bordered surface in a centered/clamped rectangle; official example exists | Must own focus, clear background correctly and restore focus/scroll on close | **Add** for Help, New session and short choices |
| Modal decision dialog | Same drawing primitives as a popup | Modality, action validity, expiry and input capture are application behavior | **Add deliberately** for approvals/confirmations; full-screen review when evidence is long |
| Drawer / inspector | Render a side/bottom rectangle; no special window API needed | Avoid shrinking the primary reader below a useful width | **Add** on wide layouts, use full-screen detail on narrow ones |
| Full-screen reader / page | A widget can occupy the whole content area | Requires clear title, Back action and saved selection | **Use** for long artifacts, logs, diffs and review evidence |
| Buttons | No built-in `Button` in the pinned core widget list; official custom-widget recipe demonstrates one | Need enabled/focused/pressed state, activation rules and mouse hit-testing for the agreed default-on interaction | **Add a small shared action-button component**; a component library is optional, not required |
| Menus / command palette | Compose an input and list in a popup; third-party options exist | Filtering, command validity and input-mode conflicts remain ours | **Add a small More menu** first; evaluate searchable palette if discoverability still needs it |
| Inline expandable sections | Compose normal widgets and store expanded/collapsed state | Expansion must not reset the surrounding document or selection | **Use** for metadata and secondary details; not for the critical scope of an approval |
| Scrollbar / scroll position | Core `Scrollbar` and state | Does not provide scrolling behavior automatically; wrapped offsets still matter | **Add** to long readers and modals; indicate that more evidence exists |
| Toast / transient receipt | A styled rectangle plus timer; no special core toast system needed | Easy to overwrite, cover content or disappear before being read | **Use only for minor success receipts**; errors, decisions and unsaved-state problems persist |
| Loading indicator / spinner | Small tick-driven custom widget, or optional third-party throbber | Animation is not proof of progress; respect reduced-motion preferences | **Use one restrained indicator** per active operation |
| Gauge / line gauge | Core `Gauge`/`LineGauge`; useful for known quantities | No trustworthy percentage when duration/work is unknown | **Use only with a meaningful denominator**; do not show “75% done” because step 3 of 4 is active |
| Stepper / workflow route | Custom text/list or Canvas; current observed stepper already exists | Loops, retries and branches make a linear completion bar misleading | **Refine the existing stepper**, with route details on demand |
| Status icon / traffic light | Unicode/ASCII characters, colored spans or core Canvas | A lamp cannot encode both execution state and outcome validity by color alone | **Add a compact status light plus words**, with ASCII fallback; see section 5 |
| Charts / bar charts / sparklines | Core `Chart`, `BarChart`, `Sparkline` | Data must have known units/source/scope; narrative model claims are not a benchmark dataset | **Defer default charts**; consider measured comparisons or usage reports when their data supports them |
| File/version tree | Custom list or third-party tree widget | Useful at scale; unnecessary hierarchy can hide a small list | **Defer** until real session/file counts justify it |
| Text editor / input field | Current shared editor exists; third-party textarea options are documented | IME, graphemes, wrapping, paste and key reporting vary across terminals | **Keep current editing behavior** initially; assess a dependency only against concrete missing behavior |
| Images | Third-party `ratatui-image` supports Kitty/Sixel/iTerm2 and character-cell fallback | Terminal/multiplexer compatibility, sizing and protocol detection; not a core universal feature | **Technically possible, not needed for this redesign**; never make an essential action depend on an image |
| Rich transitions / effects | Third-party animation libraries are documented | Additional dependency, movement and accessibility cost | **Defer**; do not animate critical approval content or add loading theatrics |
| Independent OS windows | Outside the normal model of a single terminal Ratatui app | New terminals/processes need lifecycle/ownership design | **Do not add**; use internal modal/drawer/full-reader surfaces |

### 3.3 Mouse and buttons: the missing half

The current event loop processes `Event::Key`; it does not implement mouse activation. Crossterm supports mouse events/capture, but drawing `[Start]` alone does not make it clickable.

For real buttons, keyboard and mouse must produce the **same typed action** after the same eligibility checks. Hit regions must come from the current rendered layout, with stale regions discarded after resize/modal changes. A click begun on one request must not activate a different request that appeared before release. Non-destructive focused buttons can use Enter. Approval, grant, revert and restart confirmation retain explicit decision activation; **do not make Enter authorize them as an incidental visual change**.

**Settled:** the redesigned UI enables mouse clicks and scrolling by default, retains complete keyboard operation, and provides a discoverable way to disable mouse capture. Mouse capture can affect ordinary terminal selection/copy behavior; expose the toggle through Help/More rather than requiring a settings editor. Hit-testing and keyboard actions share the same eligibility checks. This is a design decision, not behavior already present in the current key-only event loop; do not claim a button is clickable until that path exists.

Relative effort is not uniform: painting a popup or styled action is small; reliable focus restoration, modal precedence, resizing and stale-action prevention are the substantial work. Mouse interaction adds event/hit-region testing. Images and structured dashboards add terminal/data contracts. These are relative design costs, not delivery-time estimates.

### 3.4 Dependencies

No dependency is required just to add an overlay, drawer or action row. A small local component layer fits the current code. The official ecosystem also documents `ratcn` (themeable buttons/dialogs/focus), `ratatui-textarea`, `throbber-widgets-tui`, tree widgets and `ratatui-image`. These are **candidates, not chosen or version-validated dependencies**. Adoption would need a compatibility, maintenance, input-routing and accessibility check against our pinned stack; this review installs none.

## 4. A smaller information hierarchy

### 4.1 Proposed workspace structure

Keep one session workspace and the three familiar sections: **Activity, Artifacts, Edits**. Let Activity be a state-aware overview rather than a raw-event-first page. A completed result can be its landing content without adding another permanent tab.

- **Compact identity/header — settled:** project/session title, one status badge and current step. Full goal, workflow roadmap, model identifiers, attempt UUIDs and branch IDs move under Run details.
- **Session navigation — settled:** a collapsible sidebar is hidden by default. A visible Sessions control opens it for switching/browsing. Do not automatically allocate a permanent sidebar just because multiple sessions exist; pinning/persistence can be reviewed separately.
- **Main content:** one primary task. Running overview, blocker resolution, report reading, or source-edit review—not all four simultaneously.
- **Inspector opening — settled:** Enter or click explicitly opens the selected activity/file details. Use an inspector beside the list when width permits, or a full-width reader on a small terminal. Preserve the exact Back target; selecting rows while details are closed does not open a preview.
- **Composer visibility — settled:** open on demand through a visible Compose action. Closing it preserves the memory-only draft and returns its space to the content. Recipient/mode remains explicit; a decision/form owns its own inputs, with no second apparently active editor underneath.
- **Contextual action dock:** a short row of actions for the current focus/state. Help is available without copying its contents into every border/title.
- **Temporary feedback:** a small receipt for minor success; persistent notices for stops, errors and incomplete operations.

### 4.1.1 First layout pass after review

The agreed **visibility/opening behavior** is illustrated below. Labels, three-tab navigation and exact sizing are still proposals, not additional accepted decisions.

```text
 Greenlight / project          Working · baseline       [Sessions] [Help h]
 Activity   Artifacts   Edits

 Main task content
   Current state, meaningful activity or selected report/file list

 [Open selected]   [Compose]   [More actions]
```

At rest, the sidebar, details and composer do not occupy separate permanent panes. Sessions reveals navigation; Open/Enter/click opens details; Compose opens the editor and restores its draft. Closing one returns the space to the main task. This layout does not change what a message can actually do during a live workflow, and it does not allow a composer behind a pending decision to receive input.

### 4.2 Disclosure levels

| Level | Visible by default | Opens deliberately |
| --- | --- | --- |
| Overview | Human state, current task, last meaningful outcome, primary action | Full goal, workflow route and selected-item details |
| Selected item | Name, concise summary, outcome/provenance label | Full command, tool output, document, diff or verification evidence |
| Inspector | Relevant evidence and scoped metadata | Exact record/attempt/branch IDs, original payload and audit links |
| Audit | Exact historical records and machine details | CLI/export or external review snapshot |

Do not move **critical decision information** down this ladder: the operation, target, permission scope, affected file count, destructive consequence and stale/conflict warnings stay visible before a decision. Hide verbosity, not consequences.

**Settled:** navigation changes the selected row while details remain closed. **Open/Enter/click** explicitly opens the item; simple browsing does not trigger an automatic preview. Focusing a pane makes its available actions clear. **Also settled:** once explicitly opened, the inspector follows deliberate list selection changes. Reader scrolling and background updates do not count as a new selection; retain the person's reading context.

### 4.3 Surface choices

- **Small popup:** Help, New session, session switcher, More menu.
- **Decision overlay:** tool approval and persistent grants. If evidence does not fit, use a larger review surface/full reader while keeping the pending request and its owner clear.
- **Inspector drawer:** selected activity evidence, metadata, provenance, exact step context.
- **Full reader:** artifact, log, long diff and long restore/restart review.
- **Inline banner/card:** running/waiting/retry/blocked summary and important action feedback.
- **Report page:** usage and completed-run evidence. Long documents should not be squeezed into small popups just to demonstrate popup support.

### 4.4 Layer and focus contract

A painted overlay and its input owner must always agree. One top interactive surface owns keys/clicks; background activity can update without receiving those events. Closing restores the previous selection, draft and sensible scroll position.

Proposed priority is an active decision/recovery surface above ordinary inspection, then contextual help for that surface. New minor notices cannot cover a command or confirmation button. No click-through, background button activation or approval based on a stale selected session is acceptable.

**Settled:** `h` opens read-only contextual Help above a pending approval. Decision keys are disabled while Help owns input. Escape closes Help first, **not silently denying the underlying request**; the still-valid pending approval then owns input again. If the request expired or its worker disappeared while Help was open, show that fact instead of restoring actionable stale choices. Current behavior disables help during approvals, so this agreed design still requires implementation and dedicated tests. Avoid unbounded nested windows; one decision plus one read-only help/reader layer is enough initially.

### 4.5 Width and height, not just “desktop versus mobile”

Proposed layout targets to validate, not new hardcoded thresholds:

| Space | Recommended layout |
| --- | --- |
| Around 120×35 and larger | Optional narrow session sidebar, main content, inspector only if both content widths remain useful |
| 80×24 | Compact header, one content area; list/reader split only when readable; inspector becomes a full reader |
| 60×20 | One task/page at a time, compact action row; session switching in a popup |
| Current minimum / smaller | Preserve the current safe minimum behavior until a tested alternative exists; never clip a critical decision and still leave approval enabled |

Allocate by minimum readable content widths and evidence height. A fixed percentage can turn a file path, command and report into three illegible columns. Long goals/paths may be shortened in navigation **with full text available**, but not shortened away in an approval's authoritative command/target.

## 5. Color, typography and status rules

### 5.1 Proposed semantic theme

**The semantic role mapping is settled; exact RGB shades below remain examples, not an implemented/final palette.** Use named roles across the app so a later light/high-contrast theme changes the palette without changing semantics. User input changes from the current green to blue, leaving green for recorded positive outcomes. Neutral model body, cyan focus, amber attention and red errors/destructive emphasis complete the agreed mapping; optional attribution accents remain proposed.

Example dark theme background: `#111827`; raised/selected surface: `#1F2937`.

| Role | Example foreground | Use | Do not use it for |
| --- | --- | --- | --- |
| Primary content | `#E5E7EB` | Ordinary UI text, document/model body, command text | Turning every line into a colored alert |
| Secondary metadata | `#94A3B8` | Timestamps, sizes, secondary labels, inactive separators | Critical permission scope, errors or the only actionable instruction |
| Focus / UI accent | `#67E8F9` | Focus ring, active tab, primary navigation/action affordance | Claiming that a command or result succeeded |
| User contribution | `#93C5FD` | Editable user text and user-message attribution | Completed-state success or generic headers |
| Agent attribution | `#C4B5FD` | Small “Agent report” / “Manager” label | Coloring whole long documents purple or making claims authoritative |
| Recorded positive outcome | `#4ADE80` | A named check passed, file write recorded, receipt of a completed operation | Input text, a merely finished workflow, unvalidated model intent |
| Attention / uncertainty | `#FBBF24` | Needs a decision, unverified result, recovery attention, bounded retry state | Treating every active model request as a warning |
| Error / destructive action | `#F87171` | Failed operation, unresolved error, explicit destructive-control emphasis | Ordinary Deny/Cancel; declining a request is not itself an error |
| Command/code surface | Primary content on raised background | Exact request/code; limited syntax accents | Rendering the entire shell command like a success message |
| Diff additions / deletions | Green `+` / red `-` | Source/document comparison only, with signs and legend | Overall application state; deleted lines are not execution failures |

Calculated foreground/background contrast ratios for these example colors against `#111827` are 14.33 (content), 6.92 (metadata), 12.24 (focus), 9.84 (user), 9.61 (agent label), 10.18 (positive), 10.63 (attention), 6.41 (error). These are arithmetic checks for the example RGB palette, **not terminal/accessibility test results**. Terminal themes, 16-color mappings and selected backgrounds still require testing.

Rules:

1. **Color supplements words and shape.** Always retain a state label/icon; do not require red/green discrimination.
2. **Focus is different from selection.** Selection persists; focus indicates where keys go. Highlight the selected row, but distinguish an inactive selection from the active pane.
3. **One accent, a few semantic exceptions.** Most text stays neutral; color the important label/action rather than every frame.
4. **Success has a named scope.** “Check passed” and “file stored” may be green; neither proves that startup became faster.
5. **Warnings remain readable.** Use muted text for optional metadata, not vital scope or failure reasons.
6. **Code controls are visible.** Preserve the existing escaped-control/bidi protections and distinguish model text from application controls.
7. **Fallbacks are deliberate.** Provide named-color and monochrome presentations, using words/signs when RGB, special glyphs or font support are unavailable. Do not depend on Nerd Fonts.
8. **Animation is restrained.** Keep the input caret clear; offer reduced motion for activity indicators. Do not blink whole warnings, buttons or status paragraphs.

### 5.2 Greenlight status motif

Recommend a **small colored lamp with a text label** (), not a large three-row traffic-light drawing in every pane. A traffic-light motif is useful branding, but three colors cannot replace the state model.

| User-facing state | Visual recommendation | Source / interpretation |
| --- | --- | --- |
| Ready | Neutral hollow lamp + Ready | Idle eligible state; an action can start work |
| Starting / working / waiting for model | Cyan active mark, optional restrained motion + exact phrase | Local runner/provider facts where available; externally observed runs must be labeled as such |
| Needs approval | Amber lamp + Your decision | Typed pending approval, not a guess from model prose |
| Blocked / interrupted | Amber attention or red failure mark + reason | Use the recorded failure/ownership state; interruption is not a successful stop |
| Workflow finished | Neutral check + Finished · review result | Control flow ended; do not infer goal success |
| Recorded checks passed | Green check + named check scope | Persisted verdict with the actual scope visible; no-check policies remain “No checks required” |
| Result unverified / measurement unavailable | Amber question mark + exact evidence limitation | Show recorded facts and labeled agent assessment separately; never classify success by keyword guessing |
| Stop requested | Amber/neutral stop mark + Stopping future requests | Current in-flight response/tool may finish; do not show Stopped prematurely |
| Durable free-form question | Amber + Waiting for your reply | **Future backend state**, not shipped by this UI review |

ASCII equivalents such as `[RUN]`, `[YOU]`, `[!]`, `[DONE]` must retain the same words. A literal red/amber/green cluster can be explored in a prototype, but it must not make “running” and “verified success” look identical.

### 5.3 Progress, graphs and icons

The current stepper is valuable, but should be compact by default. Show the current step and observed route, with full attempt/loop history in Details. A label such as “3 recorded steps finished” is more honest than a time-to-completion percentage; retries/goto can revisit a step.

Use a spinner for unknown duration, a count/gauge for a genuinely bounded quantity, and a measured chart only for structured data with units and provenance. Two bars drawn from a model's untested “before/after” narrative would recreate the same false-confidence problem as green COMPLETED. A usage chart also needs explicit estimated/unknown values and scope; it is not a spend budget or rate limit feature.

Icons are supplementary: one for working, attention, document, source edit, and verified check is enough initially. Avoid emoji whose width/rendering varies or multiple animated indicators competing on screen.

## 6. Contextual actions, buttons and help

### 6.1 Replace duplicated hints with an action dock

Show **one primary action and up to two relevant secondary actions** near the current task. **Approval exception:** show every valid grant choice immediately; do not hide them under More. Put additional actions under More and the full shortcut reference under Help. A short key label may appear on a button; do not duplicate the whole keymap in every pane title.

The dock must derive from **state + focus + selection + ownership + active overlay**, not only the tab. Labels, enabled state, keyboard activation and eventual mouse activation should refer to the same action definition. A disabled action must explain why when selected/inspected; it must not appear to do nothing.

| Context / focus | Primary action | Secondary / More | Required explanation |
| --- | --- | --- | --- |
| Empty home | New session | Help | Which project is selected |
| Workflow choice | Continue | Cancel, Workflow details | Invalid choices are not startable |
| Goal editor | Start workflow | Cancel | Starts execution, not just a ready record |
| Ready overview | Start step | Add instructions, Details | What will run next |
| Running activity list | Inspect selected event | Follow latest, Stop requests | Reading older evidence pauses following |
| Running agent composer | Ask manager, if supported/chosen | Cancel composition | Direct live guidance is not implemented; never imply that typed text was delivered to the running step |
| Tool approval | All valid choices visible: once / current worker / project / global / deny, as typed mode permits | Help and full evidence reader | Exact request and scope; Enter never implicitly decides |
| Human checkpoint | Approve / Reject | Review evidence | Exact evidence version being approved |
| Persistent grant confirmation | Explicit confirm grant | Cancel | Exact command/all-writes scope and project/global reach |
| Blocked overview | Inspect cause, then supported retry/recovery | Restart from step, Ask manager | Retry/resume semantics and prerequisites |
| Interrupted owner | Recover owner | Inspect evidence | Does not resume the workflow or grant pending requests |
| Pending native-write recovery | Open recovery guidance | Inspect record | Current path may require CLI reconciliation; do not advertise a non-existent one-click repair |
| Completed result | Review report | Review edits, Restart from step | Finished does not mean independently validated |
| Artifact list / reader | Open / Read | Versions, Details or Copy | Artifact is a document, not a source write |
| Edits file list | Open selected diff | Revert file, More → Revert all | Recorded native coverage, not Git/whole-machine undo |
| Revert/restart preview | Explicit confirm operation | Cancel, Expand diff | Affected paths/scope and what history/execution will change |
| Usage/context reader | Close / Back | Copy, Details | Report scope and estimates/unknowns |
| Preview loading/error | Keep reading / retry supported preparation | Export source | UI preparation is not a new model request |

More should hold useful but less frequent actions, not become a dumping ground. Revert-all can remain visible beside Revert-file when reviewing edits and space allows; it must remain distinct from Restart, and should never silently activate based on whichever file happens to be selected.

### 6.2 Button contract

Each button needs normal, focused, pressed/busy and disabled states. Use a short verb and object: **Start workflow**, **View result**, **Revert file**, **Cancel**, **Ask manager**. Avoid ambiguous **Go**, **Continue** for destructive transitions, or **Send** with an invisible recipient.

A primary button is visually stronger; secondary buttons are quiet. Cancel/Deny are neutral protective choices. Destructive confirmation may use red emphasis but keeps its exact action text. Broader grants must not be styled as the easiest default route just because they reduce future prompts.

Immediate direct-key decisions remain compatible with button presentation. Selecting/focusing a button does not authorize anything. Mouse/keyboard must pass current request ID, mode, session/branch scope and freshness checks to the existing authority. No change to exact-command permission matching, retry limits or restore coverage is implied.

### 6.3 Help and keybinding proposal

**User-selected direction:** lowercase **`h`** for Help in navigation, retaining `?` as an alias initially. In the current application, `h` still means activity/history; that action must be remapped before the new Help binding is implemented. Uppercase `H` is the user-suggested replacement, pending the keybinding review. During text entry letters remain text; provide a visible Help action accessible through navigation. Help over approvals follows the agreed read-only overlay/input-isolation contract in section 4.4.

Help should start with **This screen**: focus, eligible actions, what Enter/Escape do, how to return. A Navigation section explains movement. All actions is searchable or grouped by task, with descriptions rather than just letters. Advanced key combinations and CLI alternatives belong here, not on the always-visible footer.

Recommended defaults to discuss:

- Tab/Shift+Tab move between interactive regions in a predictable order. Arrow keys inside text edit text; they do not switch panes.
- Enter opens/activates an ordinary focused control or submits an explicitly focused composer. Escape closes the top surface or leaves composition with the draft intact.
- Approvals, persistent grants, revert and restart confirmation keep explicit decision activation; Enter stays inert there.
- Back always names its destination in a full reader. Do not make users infer whether Left will close a window, move a caret or change sessions.
- Preserve existing aliases where they do not conflict; document migration in Help rather than adding permanent shortcut clutter.

### 6.4 Composer modes

Show the actual recipient and effect before submission: **Instructions for next run** versus **Ask manager · read-only**. These map to existing direct resume and manager behaviors. While execution is live, a future “Send to running step” must remain unavailable until targeted delivery exists; do not quietly route it elsewhere or promise a queue.

During a decision or new-session goal form, only that task's inputs own focus. During completed-state review, show a compact Ask manager/Restart action rather than an apparently normal resume box. Keep memory-only draft behavior explicit in Help; appearance alone must not suggest drafts are durably saved.

## 7. Element-by-element review

This is the component review checklist for our discussion. “Recommended” means proposed, not implemented or approved. Surface IDs refer to section 2.

| Element | Current issue | Recommended default | What moves to detail / what must remain visible |
| --- | --- | --- | --- |
| E01 — App/project identity | Goal dominates; project identity is less clear | Small Greenlight + project/session identity | Full project path under Details; selected project must remain unambiguous before writes |
| E02 — Long session goal | One truncated line loses the user's actual objective | Readable short title with an explicit Goal expansion | Full original goal in a drawer; do not replace the stored goal with a generated title |
| E03 — Workflow/model labels | Technical identifiers compete with the task | Human workflow title and current step | Full workflow ID, provider/model string and configuration source under Details |
| E04 — State badge | Large uppercase status is easy to overinterpret | Lamp + plain state + one short reason | Separate workflow state from verification/outcome; never hide a needed decision |
| E05 — Attempt and branch IDs | UUID/event noise in ordinary header | Hide identifiers from default chrome | Keep attempt number where useful; exact IDs copyable in inspector/audit; branch changes remain legible |
| E06 — Workflow stepper | Long route plus header metadata consumes height | Short observed route/current step | Expanded attempts/loops in workflow details; no guessed percentage |
| E07 — Next-action sentence | Competes with footer and repeated shortcut hints | Put the supported action beside the state | Detailed prerequisites/help on demand; reason for disabled recovery is not optional |
| E08 — Primary tabs | Three familiar sections are useful; manual labels contain extra help | Keep Activity / Artifacts / Edits, clear active/focused treatment | All keyboard aliases in Help; no proliferation of tabs for every dialog |
| E09 — Session sidebar | Four-row entries repeat the entire header | **Settled:** collapsible and hidden by default; visible Sessions control; compact rows and attention counts remain proposed | Full timestamps/workflow/step in selected-session detail; session switcher remains visible when collapsed |
| E10 — Session selector/search | Replaces content and creates no-results confusion | Search inside switcher/sidebar, clear filter chip | Distinguish no sessions from no matches; offer Clear filter or New session appropriately |
| E11 — Empty state | “No events/start elsewhere” or empty boxes are not useful direction | One sentence explaining emptiness and one working action | Preserve exact underlying error if empty is really unreadable/corrupt data |
| E12 — Workflow picker | Minimal context for what a workflow will do | Title, short purpose and supported start action | Read full workflow, required outputs or validation failure on demand |
| E13 — New-session goal editor | Main composer is still drawn below the form | Single focused form/editor and visible Start/Cancel | Workflow summary can expand; other-session draft stays untouched |
| E14 — User input | Green text conflicts with success meaning | Blue user role, real caret, clear input boundary | Keep grapheme-aware edits/wrapping; do not make navigation keys consume text |
| E15 — Composer | Recipient/effect can be ambiguous or unusable in current state | **Settled:** open on demand and retain drafts when closed; visible recipient/mode and submit label remain proposed | No phantom running-step delivery; disabled reason and draft-retained receipt |
| E16 — Step context box | Always present and repeats objective/outputs/verdict | One short current-purpose line where it helps | Output names, pinned inputs and full definition under Step details |
| E17 — Activity card title | Record number precedes human meaning | Human action/outcome first; small source/role attribution | Event ID and raw kind under Details; preserve exact tool correlations |
| E18 — Activity summary | Gray body can make useful content hard to read | Normal content contrast; mute only metadata | Longer output expands rather than being squeezed into a clipped sentence |
| E19 — Agent intent | Can visually resemble proof of execution | Distinct “Agent intent” label, neutral/information tone | Named recorded tool result/verification provides evidence separately |
| E20 — Tool result/error | Often a raw exit code or protocol wording | Short factual outcome and scoped reason | Full stdout/stderr, command, request/output IDs in inspector |
| E21 — Live stream | A permanent half-pane exposes raw YAML and competes with activity | Small live preview during generation, user-collapsible and expandable | Full stream on request; keep “drafting / not executed” label. Do not regress to unexplained silence after Start |
| E22 — Follow/paused state | Hidden in long pane titles | Small visible Paused/Following control beside activity | Keep selected item and scroll until explicit Follow latest |
| E23 — Activity filter/count | Repeated footer instructions and record counts | Filter chip + compact search; count only when useful | Metadata-only search scope disclosed in Help; no claim of full payload search |
| E24 — Inspector/evidence view | Enter swaps the whole list away | **Settled:** explicit Enter/click opening; inspector when width permits and full reader on small terminals | Exact evidence stays accessible without changing workflow state |
| E25 — Artifact list | Many draft/availability notes compete with final required outputs | Mark required/final/current documents; readable filename and source step | Bytes, immutable record IDs and ancestry detail on demand; grouping must not discard extra artifacts |
| E26 — Document reader | Permanent split and border text reduce reading width | Expandable/full-width reader with scrollbar and simple title | Raw toggle/versions/provenance in clear controls; retain sanitization and preview limits |
| E27 — Version/provenance controls | `v`, `p`, `d`, `m`, `o` depend on context without strong affordances | Labeled Versions / Provenance / Compare controls | Exact approval/consumer version references remain available; source-edit diff stays a different concept |
| E28 — Tool approval choices | Decision-looking text but no button/focus grammar | Distinct command/diff block, typed scope, every valid grant choice immediately visible | Broader scope review explicit; process/project/global matching semantics unchanged |
| E29 — Human checkpoint | Generic approval frame can hide that actual evidence is being reviewed | Name checkpoint and exact documents being approved | Full artifacts must be reviewable without accidentally deciding or approving a newer version |
| E30 — Persistent grant confirmation | Dense prose and scope warnings can blend together | Scope/location summary plus separate deliberate confirm/cancel controls | Global reach and all-native-writes coverage never relegated to optional details |
| E31 — Diagnostics | Different severities share a large ATTENTION pane | Cause-first banner/card; expand evidence; actionable supported recovery | Data loss/partial restore/accounting failures persist, not dismissing themselves as a toast |
| E32 — Usage footer | Always visible despite being irrelevant to many tasks | Optional Usage badge opens detailed report | Retain estimated/unknown counts, cached-input semantics and pricing freshness; never imply unknown is zero |
| E33 — Help | Long manual replaces the main pane | Focused popup; This screen first, then navigation/all actions | Advanced shortcuts and CLI fallbacks remain discoverable and searchable |
| E34 — Footer/action dock | Repeated static hints, truncated notices, not truly focus-aware | Few relevant actions with an overflow menu | No action that silently does nothing; important long notices get a persistent readable location |
| E35 — Completion result | Improved factual content, still long report-first scrolling | Short conclusion + named verification scope + actual edit count; expand report | Agent-authored claims and authoritative checks remain visibly distinct; historical report version pinned |
| E36 — Edits file list | Paths and statuses compete in a narrow third column | File-focused layout with selected path readable | Net recorded state, reverted state and history gaps remain explicit |
| E37 — Source diff reader | Recorded diff may look like general repository/Git changes | Standard `+`/`-` colors, clear recorded-native coverage and scope | Exact before/after source accessible; no hidden hunk-only undo feature implied |
| E38 — Revert controls | One-letter commands are hard to discover | Revert file and separately scoped Revert all actions | Preview, manual drift protection, live-owner checks and partial-failure reporting remain mandatory |
| E39 — Restart selection/preview | Checkpoint IDs and restore/fork semantics are dense | Human step/attempt description then explicit Restore and restart branch review | Never confuse workspace-only Revert with branch-creating Restart; expose restoration limits |
| E40 — Copy/editor controls | Can imply clipboard/editor acts on the user's local machine/source | Copy exact content / Review snapshot in editor, with receipt | Clipboard is on the running machine; edited snapshots are not imported automatically |
| E41 — Loading/truncation | A reader can look empty or stuck | Local loading indicator, clear limit/error note and source-export route | Existing bounded background preparation must stay responsive; no paid regeneration just to show UI |
| E42 — Borders and titles | Every box/title behaves like another toolbar | Quiet separators, one focus accent; borders mainly for groups and overlays | Critical modal identity stays visible; avoid decorative nested frames |
| E43 — Notices and receipts | A single notice can be overwritten or carry UUID-heavy messages | Human session title + operation state; minor receipt expires, important outcome persists | Trace ID and original error retained in Details; expiry cannot erase unresolved failure |
| E44 — Resize/compact mode | Percentage splits and tiny footers can hide next actions | One task at a time, clamped overlays and minimum evidence sizes | Draft, selection and pending request retained; no unsafe approval at insufficient size |
| E45 — Multiline paste | The event loop has no explicit `Event::Paste` handling; terminal paste behavior must not be assumed | Plan protected multiline paste/review in text fields | Paste must not activate global shortcuts, approvals or submission; bracketed-paste support and unsupported-terminal fallback need explicit testing |

### Order for the joint component review

First review **E01–E09 and E42** (overall structure). Then **E04, E14, E17–E23 and E43** (meaning/focus/feedback), then **E28–E31 and E38–E39** (decisions and recovery), then **E25–E27 and E35–E37** (reading/review). Finally refine search/help/forms and evaluate optional graphics. This avoids polishing isolated buttons while the workspace remains overloaded.

## 8. Feedback and missing components

### 8.1 Action lifecycle

Every meaningful action should visibly move through **accepted → working/waiting → outcome**, with a precise failed/refused state. “Clicked” is not “executed”, “approved” is not “tool succeeded”, and “request sent” is not “reply received”.

| Operation | Immediate acknowledgment | While waiting | Outcome feedback |
| --- | --- | --- | --- |
| Start / resume | Starting the named workflow/step; draft handled explicitly | Preparing request / waiting / receiving where facts support it | Running activity, decision needed, blocked cause or finished result |
| Manager question | Original message saved/submitted to Manager | Manager answering; coding may continue independently | Answer ready in the named session, or failure with original message retained; no claim a route executed |
| Tool approval | Decision sent to engine | Awaiting recorded outcome | Approved/denied recorded, grant save failed, tool result or expired request—separate states |
| Stop requests | Stop requested | In-flight response/tool may finish | No further provider request starts; eventual worker state reported |
| Restore/revert | Confirmed request for the visible scope | Restoring recorded files; keep controls from repeated activation | Files restored and history recorded, stale/conflict refusal, or explicit partial failure |
| Restart | Selected checkpoint then confirmation accepted | Guarded restoration followed by fork | New branch ready, or exact partial-success/failure distinction; do not claim execution resumed automatically |
| Copy | Copying named content | Brief busy indication | Copied on this machine / backend unavailable; preserve source and selection |
| Editor review | Opening review snapshot | Terminal suspended for editor | Returned; snapshot edits were not imported / editor failed |
| Document preparation | Opening selected document/version | Preparing preview locally | Display ready / truncated / missing / failed, with source access route |
| Invalid or unavailable action | No operation accepted | No fake busy state | Reason plus supported alternative; retain user draft |

Existing starting/live-stream, preview-preparation, copy and diagnostic feedback are useful foundations, not reasons to assume all paths are solved. Manager completion currently uses long ID-heavy notices; direct resume uses a generic running notice; an unusable composer can still look normal. These are specific review items.

Use an elapsed duration only when a real start time is available. An animation proves that the UI is alive, not that the remote provider is progressing. For external owners, show persisted observed state rather than a local-only LLM stream or invented per-tool phase.

### 8.2 Components missing from the present UI

| Proposed component | Why it is missing/useful | Backend dependency / boundary |
| --- | --- | --- |
| Semantic theme and status badge | Current colors are distributed and meanings collide | UI-only mapping; actual status still comes from existing authoritative state |
| Modal host with focus restoration | Present dialogs replace panels and focus rules are scattered | UI state/input routing; preserve owner, expiry and confirmation semantics |
| Action dock + shared button states | Current hints do not reliably answer what is usable now | Map to existing actions; mouse needs event routing/hit-testing |
| Contextual Help / More menu | Shortcut wall is noisy but discoverability is weak | UI-only for existing actions; do not list future actions as working |
| Optional inspector/full-reader navigation | Important material competes with long documents | UI-only navigation; preserve exact record/version and reading scope |
| Readable notice/receipt area | Single transient strings are easy to miss/overwrite | UI state; important persisted failures remain inspectable |
| Scroll affordance | Long content can look like it ends at the viewport | UI-only with current wrapped offsets/preparation state |
| Explicit composer recipient/mode | Manager and direct resume have different effects | Existing behaviors can be surfaced; live routing/delivery remains a separate feature |
| Dedicated completed-result overview | Current result is a long replacement panel | Existing pinned evidence, no summarizer call required |
| Human-reply request dialog | Needed for manual logs/systemctl/journalctl collaboration | **Separate durable `ask_user`/conversation-continuation backend work**, not part of cosmetic redesign |
| Typed measurement/result visualization | Would make benchmark graphs and goal-level result badges trustworthy | Requires agreed structured outcome/metric evidence; do not parse model prose into authoritative success |
| File/branch tree or settings editor | Could help larger projects or configuration | Needs demonstrated use and action/data contracts; not required for the first visual revision |

### 8.3 Gaps worth keeping visible in the discussion

- A better Stop button is not process interruption or a durable mid-step pause.
- A new Reply field cannot preserve a conversation across restart until the human-question/checkpoint feature exists.
- A green finished badge cannot prove performance improvement when verification requires no checks.
- An Edits page is not arbitrary shell/Git/service rollback, and multi-file restore is not an atomic transaction.
- Clipboard support on the cloud/SSH machine is not automatically clipboard synchronization with the user's laptop.
- A new library's screenshots do not prove keyboard, terminal, version or focus compatibility with Greenlight.
- Existing docs contain dated descriptions: for example, [terminal UX v0.1](05-terminal-ux.md) still describes some now-shipped features as future work, and parts of [current TUI controls](11-tui.md) retain older key summaries alongside later additions. Source and dated decision records were used to resolve the audit, not those stale summaries alone.

## 9. Conceptual layouts

**These are design wireframes, not screenshots, runnable prototypes or claims about current controls.** Button labels illustrate proposed affordances. Exact sizes/keys need the decisions in section 10. No fake benchmark percentage is used.

### 9.1 Running: one main task, optional detail

The [focused running-screen review](24-running-screen-design.md) now records the agreed compact header, small expandable/hideable live preview and follow-selection-after-opening behavior, with [static visual studies](mockups/README.md). The text below remains an illustrative sketch; exact action placement is still under review.

```text
 Greenlight / my-project   Bar startup investigation       ◌ Working · baseline
 Activity   Artifacts   Edits                         [Sessions] [Details] [Help]

 Baseline investigation
 Current operation: waiting for model output
 Last recorded result: read src/bar.py

 Recent activity
   Read project instructions
   Read src/bar.py                       [Open result]

 Live draft · not executed yet                    [Expand] [Collapse]
   action: tool
   tool: shell
   ...

 [Inspect selected]   [Compose]   [Stop requests]   [More]
```

If an inspector is opened on a wide screen, it replaces some main-content width, not the optional session switcher plus an already narrow third reader. If the person is reading older evidence, new activity does not pull them back to this view.

### 9.2 Approval: separate decision layer

```text
 ┌ Tool permission · Bar startup investigation ────────────────────────┐
 │ Command to run                                                     │
 │ systemctl --user status pure-black --no-pager -l | head -50         │
 │                                                                    │
 │ Scope: this exact command, including arguments and operators        │
 │ Environment: isolated shell; host user-service access may be absent │
 │                                                                    │
 │ [Allow once · Y]   [Deny · N]   [This run · A]   [Help · h]         │
 │ [Review project grant · P]   [Review global grant · G]              │
 │ Enter does not decide.                                             │
 └────────────────────────────────────────────────────────────────────┘
```

The [focused approval review and four static studies](25-approval-dialog-design.md) detail scope, confirmation and Help behavior. The user selected every valid grant choice immediately visible.

The workspace remains recognizable behind the layer. Long commands/diffs get a real review reader; scope and decision ownership do not scroll away into ambiguity. A systemctl request is not granted host service access merely because the shell command is approved.

### 9.3 Completed: outcome first

The [focused completed-screen design](26-completed-screen-design.md) expands this proposal with three static studies, evidence attribution, scoped checks and historical results after restore. This remains a review proposal, not an accepted goal-outcome classifier.

```text
 Greenlight / my-project   Bar startup investigation      Finished · review result
 Activity   Artifacts   Edits                                      [Help]

 Agent-reported outcome: measurement unavailable
 No measured improvement was established.
 Verification policy: no checks required
 Recorded source edits: 2 files

 [Read final report]   [Review edits]   [Restart from step]   [More]

 Details
   Report and conclusion are agent-authored; exact saved versions are available.
   The workflow reached its end; this is not proof the bar became faster.
```

The outcome text above is illustrative of the user's reported case, not a new deterministic classifier of arbitrary artifact prose. The implementation can show existing recorded summaries and verification scope now; a goal-level validated outcome needs an explicit evidence contract.

### 9.4 Edits and a separate revert preview

```text
 Greenlight / my-project    Edits · recorded native changes          [Help]

 Files                         src/bar.py · Modified
 > src/bar.py                  - old implementation
   src/startup.py              + candidate implementation

 [Open diff]   [Revert file]   [More: Revert all]
```

```text
 ┌ Revert recorded edits ──────────────────────────────────────────────┐
 │ Target: src/bar.py                                                  │
 │ Restore its first recorded pre-edit contents.                       │
 │ Current manual changes are checked before applying.                │
 │                                                                    │
 │ [Review exact reverse diff]                                        │
 │ [Confirm revert · Y]   [Cancel · N / Esc]                            │
 │ Workspace files change; workflow history remains. Enter is inert.   │
 └────────────────────────────────────────────────────────────────────┘
```

Revert-all must replace the target with an explicit file count/list and remain separate from restart/fork. Conflict or stale preview has no confirmable “force” shortcut in this design.

### 9.5 Help: retain location and return path

```text
 ┌ Help · Artifact reader ─────────────────────────────────────────────┐
 │ [This screen]   [Navigation]   [All actions]                         │
 │                                                                    │
 │ Read the selected saved document.                                   │
 │ Open Versions to compare saved copies; Details shows provenance.    │
 │                                                                    │
 │ Enter: return to document list    m: Markdown/raw                    │
 │ C: copy full source               E: review snapshot in editor      │
 │                                                                    │
 │ [Close help]                                                       │
 └────────────────────────────────────────────────────────────────────┘
```

This is where full shortcut explanations belong. Brief task-relevant button labels remain in the workspace so the person need not open Help just to discover the next action.

## 10. Decisions to discuss before implementation

Rows marked **Settled** reflect explicit user choices recorded at the top of this document. The remaining defaults are recommendations for review. These design decisions do not authorize UI implementation yet; we can continue using the screen/component IDs above.

| Decision | Recommended starting point | Alternative / tradeoff to discuss |
| --- | --- | --- |
| D01 — Main frame | **Settled:** collapsible sidebar, hidden by default; composer opens on demand. Three sections and remaining frame details are proposed | Open/closed preference persistence, exact compact-header height and action placement still need review |
| D02 — Default content | State-aware Activity overview; preserve explicit artifact/edit readers | Rename Activity to Overview, or add a permanent Result tab; both add migration/navigation questions |
| D03 — Details | **Settled:** Enter/click opens details; inspector when wide, full-width reader when narrow; once open, follows deliberate selection | Focus transfer, return controls and useful reading-position preservation still require validation |
| D04 — Help key and presentation | **Settled:** lowercase h for Help, ? alias; read-only Help can open over approvals as D05 specifies | Uppercase H is the suggested Activity/history replacement; exact contextual Help layout and final remapping remain to review |
| D05 — Help during decisions | **Settled:** read-only Help above the pending approval; decision keys disabled; Esc returns to the approval without denying it | Validate focus restoration, held keys and expired requests; Help must not reactivate stale decisions |
| D06 — Input/color roles | **Settled:** user blue, focus cyan, neutral model body, green recorded positive outcomes, amber attention, red errors/destructive emphasis | Exact RGB shades, attribution accents, selected backgrounds and terminal fallbacks still require validation |
| D07 — Mouse | **Settled:** mouse enabled by default; complete keyboard controls; discoverable mouse-capture toggle | Implement/test shared action dispatch, current hit regions, resize/modal transitions and text-selection fallback |
| D08 — Live model draft | **Settled:** small preview during generation, expandable and hideable | Exact height, reader behavior and truncation indication remain to validate; do not invent progress or execute draft content |
| D09 — Finished-run behavior | Open result when following live; show a quiet View result notice if already reading something else | Always auto-open result is simpler but interrupts review |
| D10 — Visual motif | One compact lamp + word; optional traffic-light prototype for comparison | Literal three-lamp graphic may be memorable but consumes space and has ambiguous running/success semantics |
| D11 — Component approach | Small local theme/overlay/button/action layer; reuse existing engine and reader | Evaluate a component library if it removes complexity without competing for focus/approval ownership |
| D12 — Scope of the first visual revision | Information hierarchy, theme, Help/decisions, action dock and reading surfaces | Charts, file trees, images, settings editor and animations remain later options, not prerequisites |

**Review progress:** sidebar visibility, explicit detail opening, on-demand composition, Help key, semantic color roles, mouse default and Help-over-approval behavior are settled above. The [running-screen pass](24-running-screen-design.md) additionally settles the compact header, small expandable/hideable live preview and inspector following deliberate selection after opening. Continue reviewing its concrete action placement, reader return behavior and composer modes, then the approval, finished-result and edit-review layouts. The remaining component proposals and implementation sequence are still open. Do not begin by rewriting all screens or adopting a whole new UI framework.

## 11. Acceptance criteria and possible delivery order

This is a proposed validation plan for future implementation. It is **not evidence that the new design has been implemented or tested**.

### 11.1 Quality bar

- On empty, ready, working, approval, blocked and finished screens, a person can identify the state and next supported action without knowing a shortcut. A five-second recognition exercise is a target to validate, not a measured result.
- Default chrome has no UUIDs, raw event kinds, full model ID or repeated shortcut wall. Full goal, exact IDs, original payloads, usage and evidence remain deliberately accessible.
- Every visible button/action is valid for the current selection, owner and overlay, or has a readable disabled explanation. No button looks busy if the operation was refused before dispatch.
- The first render after submission shows acknowledgment even if provider preparation or inference takes time. Keep feedback responsive without inventing completion percentages, success or cancellation.
- Help, inspector and full readers preserve the previous session/branch/item/scroll and drafts. Browsing an older item never gets dragged to the live tail by a background update.
- Commands, scope and file counts remain readable before decisions. Enter, repeated/held keys, stale clicks, resize and expired requests cannot grant permissions or apply restores.
- Mouse and keyboard use the same eligible action path. Mouse-off interaction remains complete; no core task depends on hover.
- At 80×24, the main task is readable without three competing columns. Validate wider and compact layouts, long Unicode goals/paths, narrow decisions and the current safe minimum/approval thresholds.
- Verify protected multiline paste/review where the terminal supports it, and document/offer a deliberate fallback where it does not. Pasted text must not masquerade as navigation or an approval.
- Test normal, dark/light-theme equivalents, named-color and monochrome presentations; color-blind users can still distinguish states through labels/signs. Test actual terminal behavior; computed RGB contrast alone is insufficient.
- Native-write/restore/approval/ownership protections, read-only manager limits, usage estimates/unknowns and historical evidence semantics remain unchanged.
- A model-generated availability note, intent title or reported benchmark result cannot be painted as authoritative tool state or a verified improvement.

### 11.2 Testing tools appropriate to this UI

Use Ratatui `TestBackend` for layout/content and **explicit cell-style assertions** for the palette; plain-text snapshots cannot catch a warning rendered in the wrong color. Keep key/focus tests tied to actual action eligibility. Existing reader, navigation, picker, approval, result and edits tests provide a base.

Complement snapshots with a small number of real PTY scenarios: goal submission, reading while output arrives, an approval interrupting a reader, read-only Help above an approval, editor return, finished measurement-unavailable report, selected-file/all-file revert with a manual conflict, and recovery after worker loss. Add mouse tests for the agreed default-on interaction and mouse-off fallback. Use a local scripted provider—no paid inference is necessary to verify a visual redesign.

Check overlay geometry, cursor visibility, disabled reasons, click-through prevention, current request/session binding and selection restoration after resize. A screenshot that looks good once is not sufficient evidence for input routing or safe action activation.

### 11.3 Possible delivery sequence, only after design agreement

1. **Visual baseline:** agree the theme, compact header, state labels and what moves into Details. Prototype running, approval, completed-unverified and edit review side by side.
2. **Interaction foundation:** one action model and overlay/focus contract; contextual Help, state-aware dock and shared button states. Make keyboard behavior coherent before adding more places to click.
3. **Reader hierarchy:** optional sidebar, inspector/full reader, scroll affordances and state preservation. Keep existing background document preparation and exact version references.
4. **Decision/feedback surfaces:** apply the hierarchy to approvals, persistent grants, restart/revert and diagnostics. Verify action lifecycle and ownership alongside the agreed default-on mouse interaction and its keyboard/mouse-off paths.
5. **Remaining refinements:** search/empty/form states, usage/manager presentation and small status/progress graphics. Evaluate charts or extra component dependencies only against observed need and available data.

No phase here authorizes durable ask-user questions, executing manager routes, wider command grants, broader filesystem rollback, new model roles or automatic model-generated summaries. Those remain separate product/backend work. UI actions must expose actual capabilities, not imply them into existence.

## 12. Research sources and audit references

### 12.1 Official online documentation and source

Read on 2026-11-17 through the official GitHub repositories. The documentation-site links are included for normal browsing; direct access to `ratatui.rs`/`docs.rs` from this environment was blocked. For reproducibility, the website source links below use the audited documentation commit. The pinned core list and popup source use the actual 0.30.2 release tag.

| Source | What it substantiates |
| --- | --- |
| [Ratatui 0.30.2 built-in widget list](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.2/ratatui-widgets/src/lib.rs) · [API reference](https://docs.rs/ratatui/0.30.2/ratatui/widgets/index.html) | Core drawing primitives, lists/tables/tabs, Clear, gauges, Canvas and charts; no built-in Button in this list |
| [Versioned popup example](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.2/examples/apps/popup/src/main.rs) · [Documentation source](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/examples/Apps/popup.md) | Background → Clear → popup rendering is already possible on our version |
| [Centering a widget/popup](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/recipes/layout/center-a-widget.md) · [Dynamic layouts](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/recipes/layout/dynamic.md) | Rect-based positioning and layouts can adapt to content/state; check exact helper availability before coding |
| [Custom widgets, including Button example](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/recipes/widgets/custom.md) | A button can be a small custom widget; rendering does not supply full event behavior |
| [Styling text](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/recipes/render/style-text.md) | Foreground/background colors and modifiers on spans/lines/text/widgets |
| [Event handling](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/concepts/event-handling.md) · [Mouse capture](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/concepts/backends/mouse-capture.md) | Event ownership is application/backend work; mouse capture has terminal-dependent behavior |
| [Third-party widget showcase](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/showcase/third-party-widgets.mdx) | Image, textarea, spinner and tree options exist; this is not a compatibility endorsement |
| [ratatui-image README](https://github.com/ratatui/ratatui-image/blob/master/README.md) | Kitty/Sixel/iTerm2 rendering, protocol detection and character-cell fallback; images are possible but not universally portable |
| [Component-library documentation](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/ecosystem/ratcn.md) | Optional themed buttons/dialogs/focus routing are available in the ecosystem; not installed or selected here |
| [Application architecture guidance](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/concepts/application-patterns/the-elm-architecture.md) | State → action/update → view is an applicable model; no wholesale framework rewrite is necessary |
| [Snapshot and PTY testing guidance](https://github.com/ratatui/ratatui-website/blob/cceec13d0152da3a66305cf9230a124ae010bd74/src/content/docs/recipes/testing/snapshots.md) | TestBackend snapshots, limits of text/color assertions and complementary real-terminal interaction checks |

The local cached sources for Ratatui 0.30.2 / ratatui-widgets 0.3.2 and Crossterm 0.29.0 were also inspected to confirm the widget list and mouse/paste event support. No library upgrade is proposed as a prerequisite.

### 12.2 Greenlight audit map

| Source | Inventory/design relevance |
| --- | --- |
| [Workspace rendering](../src/tui/view.rs) | Header, tabs, sidebar, current overlay-like branches, artifact/list splits, live draft, composer and footer |
| [App state, refresh and worker polling](../src/tui.rs) | Actual state flags, ownership/status projections, notices, automatic result opening, key-only event loop |
| [Routing](../src/tui/routing.rs) · [Focus](../src/tui/focus.rs) · [Reading state](../src/tui/reading.rs) | Real key precedence, modal behavior, composer mode, navigation, selection preservation and eligibility |
| [Approvals](../src/tui/approval.rs) · [Restart](../src/tui/recovery.rs) | Typed decisions, exact command/scope rendering, persistent confirmation and guarded historical restart |
| [Activity](../src/tui/activity.rs) · [Card projection](../src/activity_cards.rs) | Human labels versus raw events, model intent, exact tool evidence, follow/filter and no-check verification labels |
| [Artifact history](../src/artifact_history.rs) · [Reader](../src/tui/reader.rs) | Saved versions/provenance, bounded background preparation, raw/Markdown and missing/truncated states |
| [Result](../src/tui/result.rs) · [Edits](../src/tui/edits.rs) · [Restore](../src/restore.rs) | Completed-run evidence, historical report pinning, branch-visible recorded diffs and guarded selected/all-file revert |
| [Manager UI](../src/tui/manager_ui.rs) · [Exports](../src/tui/exports.rs) · [Editor](../src/tui/editor.rs) | Read-only Q&A, separate manager job, clipboard/editor receipts and actual input behavior |
| [Status projection](../src/workspace_view.rs) · [Provider stream](../src/provider_stream.rs) | Current state/stepper/attempt labels, local preview phases and authoritative versus unvalidated output |
| [Current help](../src/tui/help.rs) · [Current contract](11-tui.md) · [Earlier UX research](12-tui-ux-analysis.md) | Existing key meanings, historical proposals and migration caveats |

**Review deliverable:** a current inventory, researched toolkit and draft visual/interaction specification. No application code, dependency, keybinding, permission policy or execution behavior is changed by this document.

## Focused attention/recovery follow-up

The [attention and recovery proposal](27-attention-and-recovery-design.md) separates blocked-step continuation, provider failures, owner recovery and unfinished native writes. Its four static studies preserve existing recovery contracts; this remains design-only. The user has approved the overall completed-screen visual direction in [the previous pass](26-completed-screen-design.md).

The [session creation and composer proposal](28-session-creation-and-composer-design.md) adds focused workflow/goal selection, explicit manager-versus-step targets, editing rules and submission feedback. These remain design proposals rather than runtime capabilities.

The [Edits/revert proposal](29-edits-and-revert-design.md) and [cross-screen consistency review](30-ui-consistency-review.md) cover restore scope, explicit confirmation, failure states and remaining design/validation work.

The user-selected [Edits review amendment](29-edits-and-revert-design.md) adds All files, Tab cycling and per-file/all-file acceptance of applied edits. [Artifact reading, versions and provenance](31-artifact-reader-and-history-design.md) remain a separate document-evidence flow.
