# UI redesign — consolidated decisions and implementation plan

**Status:** planning only, 2026-11-17, Europe/Paris. The user approved the compact visual direction and requested continuation. This document makes the proposed implementation concrete; it does not implement the app or treat every prior sketch as an approved interaction contract.

## 1. Current design reference

Read [compact layout/navigation](33-compact-layout-and-navigation.md) first, then the focused screen document. The compact revision supersedes older loose spacing and F6 hints. The [audit](23-ui-audit-and-redesign.md) remains the inventory, not the final pixel specification. [All static studies](mockups/README.md) use fictional data and do not prove terminal-cell fit.

| Surface | Reference |
| --- | --- |
| Running workspace, preview and inspector | [24 — Running](24-running-screen-design.md) |
| Typed tool grants, scope and Help layering | [25 — Approvals](25-approval-dialog-design.md) |
| Outcome attribution, checks and historical results | [26 — Results](26-completed-screen-design.md) |
| Provider errors, blocked steps and distinct recovery paths | [27 — Attention/recovery](27-attention-and-recovery-design.md) |
| Workflow/goal entry and explicit-purpose composer | [28 — Creation/composition](28-session-creation-and-composer-design.md) |
| Recorded edits, reverse-diff confirmation, All files/Keep/Tab amendment | [29 — Edits/revert](29-edits-and-revert-design.md) |
| Shared rules and review caveats | [30 — Consistency](30-ui-consistency-review.md) |
| Exact saved documents, versions, comparison and provenance | [31 — Artifacts](31-artifact-reader-and-history-design.md) |
| Empty home, sessions, checkpoints, details, usage and Help | [32 — Supporting surfaces](32-navigation-checkpoints-details-and-help-design.md) |
| Denser layout and no-function-key navigation | [33 — Compact revision](33-compact-layout-and-navigation.md) |

## 2. Settled direction versus implementation choices

**User-selected design:** sidebar hidden by default; inspector opens explicitly and then follows deliberate selection; composer opens on demand and retains its memory-only draft; compact header; small expandable/hideable live preview; semantic colors; default-on mouse with keyboard operation and capture-off control; Help above a pending approval without deciding it; all valid grant choices immediately visible; All files plus per-file review/Keep and Tab cycling; denser screens; no required function keys; `h` Help with arrows/`j/k`/Tab navigation.

**Detailed proposals to validate:** exact shades/row counts/breakpoints, Enter-to-focus Edits actions, candidate-versus-open session selection, separate drafts by target/purpose, creation-draft cancellation behavior, Keep marker persistence and data contract, version-notice wording, per-mode scroll positions, and final Activity/history remapping. Implementing a proposal should be explicit in its change description and tested against the accepted direction.

Keep acceptance means review of already-applied recorded edits, not a staged-before-write system, tool grant, human checkpoint, Git stage or commit. Native edit coverage excludes unrecorded shell/manual work. The proposed review feature needs a revision-scoped data design before its code slice.

## 3. First implementation slice: compact workspace and interaction foundations

**User-visible result:** a running session opens into a compact workspace with meaningful content, not permanent sidebar/composer/metadata panes. Sessions, Compose, Details and Help are visible reachable actions. Focus is clear; navigation needs no function keys. Existing evidence and permission semantics remain intact.

### 3.1 Scope and concrete deliverables

1. Central semantic theme roles replacing scattered presentation colors in the touched views: blue user text, cyan focus, neutral model content, green scoped recorded positive outcomes, amber attention, red error/destructive. Retain text labels and diff prefixes; handle low-color/default terminal backgrounds.
2. A compact workspace layout with optional sidebar, on-demand composer, explicitly opened inspector and small live-output preview. Preserve drafts, selected records and reading/follow state. Default sizing uses actual terminal cells, not PNG coordinates.
3. Shared action descriptions for label, availability, keyboard/mouse activation and Help. Move full static shortcut legends into Help; use a short contextual action row. Do not delete an old hint until its action is discoverable elsewhere.
4. Topmost-layer routing and focus restoration for Help/reader/decision surfaces. `h`/`?` Help works in navigation without stealing editor letters. Current `h` history access moves to a visible Recorded activity action; a new letter alias remains optional until agreed.
5. Mouse and paste event handling through the existing event loop, with default capture, a keyboard-reachable toggle, resize-safe hit regions and terminal cleanup on normal/error exit. Paste inserts text literally; no hidden shortcut execution.
6. Read-only Help above typed pending approvals, with all decision shortcuts disabled while it owns input, Esc returning only to a valid pending request, and expiry leaving no stale actionable controls. Existing approval choices stay fully visible and retain explicit decisions.

No new Keep persistence, manager route execution, command-name permission grants, provider retry policy, outcome classifier or recovery mechanism belongs in this slice. No broad framework/dependency migration is required; start from pinned Ratatui/Crossterm.

### 3.2 Implementation boundaries

| Existing module | Intended work |
| --- | --- |
| [tui.rs](../src/tui.rs) | UI-only visibility/focus/layer state, mouse/paste event dispatch and terminal lifecycle |
| [view.rs](../src/tui/view.rs) | Cell-based compact layout, readable fallback, shared theme/action rendering |
| [focus.rs](../src/tui/focus.rs), [reading.rs](../src/tui/reading.rs) | Valid focus transitions, preserved selections/drafts/reader positions |
| [routing.rs](../src/tui/routing.rs), [help.rs](../src/tui/help.rs) | Topmost ownership, editor literal input, current action/key reference |
| [approval.rs](../src/tui/approval.rs) | Presentation and Help layering only; typed decision semantics unchanged |
| [editor.rs](../src/tui/editor.rs), [reader.rs](../src/tui/reader.rs) | Retain grapheme editing, blinking caret and bounded async preparation; integrate new geometry/paste |

A small internal theme/action/layout module may reduce duplication; choose its structure during implementation. Do not build a generic widget framework beyond what the first slice needs.

### 3.3 Delivery sequence within the slice

First add shared theme/action/focus structures with behavior still equivalent. Next wire compact workspace visibility and action controls, preserving legacy surfaces behind reachable actions. Then integrate mouse/paste and safe Help-over-approval routing. Finish with representative rendering and PTY interaction checks, updating runtime documentation only for behavior that actually landed.

Treat these as reviewable commits in one coherent slice. Do not ship a hidden composer/sidebar without a working keyboard path to reopen it, or replace the bottom legend with unbound buttons. Record any deferred subpart explicitly rather than claiming the slice complete.

## 4. First-slice acceptance matrix

| Scenario | Required evidence |
| --- | --- |
| Normal running view | Compact header, default-hidden auxiliary panes, relevant actions, truthful waiting/streaming preview |
| Inspector closed/open | Browsing stays quiet; explicit open follows deliberate selection; new events do not steal reading |
| Composer/editor | Draft/caret retained on close/Help/resize; arrows edit; h/j/k and pasted newlines remain literal |
| Focus/action navigation | Every visible action keyboard-reachable; Tab reverse cycle; no function keys; no Enter fall-through |
| Pending approval + Help | Help consumes y/a/p/g/n, Esc closes Help only; returning restores only a current request |
| Expired/stale owner or request | No actionable stale approval or mouse target; report the actual expired/unavailable condition |
| Mouse and resize | Keyboard and mouse hit same typed action; no click-through or stale hit regions after layer/geometry changes |
| Mouse disabled | Terminal selection possible; toggle and every action still reachable by keyboard |
| Small terminal | Single readable surface where needed; critical clipped decisions disabled; drafts/selections preserved |
| Existing result/artifact/recovery/Edits paths | Remain reachable; exact references, restore guards and typed choices unaffected |
| Terminal exit/error | Restore terminal mode, cursor and mouse/paste capture settings reliably |

Use targeted existing TUI/navigation/reading/approval tests plus meaningful new state/action tests. TestBackend renders at representative 80×24, 120×36 and smaller boundary sizes should verify geometry, clipping and labels; choose thresholds from actual evidence. PTY checks cover key sequences, terminal modes, paste and cancellation/Help layering. Static screenshots alone cannot establish these behaviors.

Use scripted/local fixtures with no paid provider calls. Run relevant tests and the repository's required checks; broaden only for failures or changed scope. Documentation-only planning does not run Rust tests or claim these future checks already passed.

## 5. Later slices

| Order | Deliverable | Additional prerequisite |
| --- | --- | --- |
| 2 | Creation, explicit manager/step composer targets and startup/send receipts | Resolve richer draft/literal-command routing without claiming live guidance |
| 3 | Results, attention/recovery, run details/usage | Bind view data to exact session/branch/attempt; preserve unknown counts and evidence scope |
| 4 | Full approval/checkpoint and artifact/history reader presentation | Typed presentation data where needed; exact versions and bounded display/diff |
| 5 | Edits file cycle, Keep tracking and guarded revert presentation | Decide revision-scoped review storage/coverage; no reuse as tool/human approval |
| 6 | Navigation polish, remaining error/minor states and complete regression review | Whole-screen action/keymap consistency and real terminal checks |

First-slice foundations may touch these surfaces enough to keep them accessible, without falsely claiming their focused redesigns are complete. Default mouse and Help safety cannot be postponed after presenting their controls as active.

## 6. Handoff and completion reporting

Before each code slice, restate the concrete scope and any changed design choices, then implement and verify that slice. Report actual behavior, tests and known limits; distinguish completed UI work from pending proposals. Update [current TUI docs](11-tui.md) and runtime Help when their behavior changes, leaving research/decision history dated.

No implementation, merge or environment publication occurs in this planning document. The next work item is implementing the first slice above after this design handoff.

## 7. First-slice implementation — 2026-11-17

The compact workspace and interaction foundations are now implemented: shared semantic roles, hidden sidebar/on-demand composer, explicit activity/artifact/native-edit inspection, small hideable/expandable live preview, keyboard/mouse actions and More menu, Help above typed approvals, literal bracketed paste, caret clicks and terminal capture cleanup. Current keys and scope are in [the runtime TUI guide](11-tui.md). Help retains a static full reference alongside the shared action definitions; generation of every reference entry from one registry remains a refinement.

This does not implement later-slice Keep tracking/file cycling, richer draft/recipient routing or the full screen studies. TestBackend/state tests cover geometry, navigation, drafts, topmost Help, stale click rejection and paste; a local scripted-provider PTY smoke checks actual rendered streaming, Unicode editing, cursor/capture sequences and Help/mouse toggling. These are implementation checks, not user acceptance of every remaining proposal.

Verification for this slice: 287 tests passed, one existing test ignored; offline build and all-target Clippy completed (five pre-existing test-code warnings). A local fixture-provider PTY smoke passed without paid calls. Follow-up TUI runs covered the final focus/layer refinements.

## 8. Visual-fidelity correction

User terminal screenshots showed that the first slice changed the shell while preserving old result/list presentation. The follow-up replaces the result dump with evidence-scoped fields, preserves full evidence in a separate reader, pins the report action to its original version, removes the full goal from the header, adds the shared dark palette/framed controls, and gives artifact lists compact rows and contextual actions. Actual [Ratatui cell captures](screenshots/README.md), rather than new proportional mockups, are the review reference for these screens. This does not claim all planned UI surfaces or Keep-review semantics are finished.

Visual-fidelity verification: 291 full-suite tests passed with one existing ignored test; the final 92 TUI tests passed after layout polish. Offline build, all-target Clippy (existing test-code warnings only), documentation/capture validation and the local-provider PTY smoke completed without paid model calls.
