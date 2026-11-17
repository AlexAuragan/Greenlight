# UI design — cross-screen consistency review

**Status:** design-only review, 2026-11-17, Europe/Paris. This consolidates the [audit](23-ui-audit-and-redesign.md) and focused studies; it is not authorization to implement them. The user approved the overall creation/composer direction in the previous pass; detailed proposals and runtime limitations remain explicit.

## 1. Shared rules checked across the studies

| Concern | Consistent rule | Relevant focused review |
| --- | --- | --- |
| Default layout | Compact header; session sidebar hidden, composer closed, inspector closed until deliberately opened | [Running](24-running-screen-design.md) |
| Hierarchy | One current task, evidence on demand, IDs/provenance under Details | [Running](24-running-screen-design.md), [results](26-completed-screen-design.md) |
| Color | Blue user text, cyan focus/active controls, neutral model body, green scoped recorded positive outcomes, amber attention, red error/destructive | All studies; diff colors are contextual addition/deletion notation |
| State lamp | Label required; finished is neutral, not proof of goal success | [Results](26-completed-screen-design.md), [attention](27-attention-and-recovery-design.md) |
| Edits review navigation | User-selected Tab/Shift+Tab cycles All files and file views while review owns focus; modal/editor/control focus retains its own routing | [Edits amendment](29-edits-and-revert-design.md) |
| Ordinary actions | One primary plus few secondary actions, More for extras; no permanent global shortcut legend | All workspace studies |
| Approval exception | Every valid grant choice visible immediately; typed mode determines validity; project/global still confirmed separately | [Approvals](25-approval-dialog-design.md) |
| Enter | Open/select in navigation, submit in editor; never authorize approval/grant/revert/restart confirmation | [Approvals](25-approval-dialog-design.md), [input](28-session-creation-and-composer-design.md), [revert](29-edits-and-revert-design.md) |
| Help | `h`, with `?` alias in navigation; typed letters remain text; topmost read-only layer captures input | [Approvals](25-approval-dialog-design.md), [input](28-session-creation-and-composer-design.md) |
| Esc | Close topmost Help/reader first; main approval denies; grant confirmation cancels and denies; revert confirmation cancels without applying | [Approvals](25-approval-dialog-design.md), [revert](29-edits-and-revert-design.md) |
| Details | Enter/click opens; once open follows deliberate selection; background events do not count as selection | [Running](24-running-screen-design.md), [revert](29-edits-and-revert-design.md) |
| Feedback | Submitted, accepted, running and outcome/error are distinct; no optimistic success or blank Ready after Start | [Input](28-session-creation-and-composer-design.md), [attention](27-attention-and-recovery-design.md) |
| Mouse | Default on; full keyboard operation, capture-off option; current-layout hit regions and freshness checks | [Audit](23-ui-audit-and-redesign.md) |
| Narrow layout | Stack content/full-width reader; never leave critical clipped decisions actionable | All studies; actual terminal-cell rendering still to test |
| Scope | Agent claims, configured checks, recorded native edits and current workspace truth are separate concepts | [Results](26-completed-screen-design.md), [revert](29-edits-and-revert-design.md) |

## 2. Tensions resolved in the written design

- The ordinary three-action preference does not hide approval scopes: approvals deliberately show every valid grant choice.
- Enter may start a workflow or submit instructions, but cannot authorize a tool or destructive confirmation as an incidental navigation action.
- Help does not steal `h` from typing. Visible focusable Help remains available in editor mode; navigation `h` requires remapping current history access first.
- “This run” on approval studies means the current worker's in-memory grant, not durable authorization for every later resumed worker.
- A green check is scoped recorded evidence; workflow completion stays neutral and agent conclusions stay attributed.
- Revert is workspace-only restoration of recorded native edits. Restart includes a reviewed restore/fork path and still needs explicit continuation.
- Closed inspectors stay closed during browsing. Follow-selection after opening never means jumping to new background events.
- Explicit composer purpose must align with the backend: read-only manager routing cannot silently become live workflow instruction delivery.

These are documentation-level consistency checks, not proof that runtime behavior implements the proposals. No existing screenshot or current-help document should be mistaken for the future design.

## 3. Remaining design work and validation

The audit inventories more surfaces than these focused studies render. Before calling the design complete, cover the artifact reader/version/provenance comparison, session navigation/filter/empty home, human verification checkpoint, run details/usage, and Help itself at sufficient detail for implementation. They should reuse the reviewed patterns rather than add permanent panes.

The user subsequently selected All files plus Tab cycling and per-file/all-file acceptance for Edits; proposed Keep markers must stay separate from tool and human-checkpoint approvals. The [artifact reader/history follow-up](31-artifact-reader-and-history-design.md) now covers the document-reader surfaces listed above; navigation/checkpoint/details/Help still need focused review.

Remaining choices include final Activity/history binding (`H` suggested), precise focus traversal/return rules, inspector positions per item, theme fallback and contrast, compact breakpoints, exact RGB/glyphs, mouse-selection toggle placement, and full editor draft/literal-command behavior. Some can be validated during a small implementation slice, but none is settled solely because it appears in a static drawing.

Exact sizes, keyboard escape sequences, blinking carets, bracketed paste, mouse hit testing and assistive/monochrome behavior require actual Ratatui/TestBackend and real-terminal checks. PNG/SVG checks establish readable study assets, not those runtime properties. No library upgrade or new widget dependency has been selected as a prerequisite.

## 4. Suggested implementation order, after design approval

1. Shared semantic theme, layout/focus/action definitions, modal/Help stack and mouse/paste boundaries.
2. Running workspace and on-demand inspector/sidebar/composer, retaining existing authority checks.
3. Session creation and explicit-purpose message actions with truthful submission receipts.
4. Results and attention/recovery surfaces using recorded evidence.
5. Approval and Edits/revert presentation with dedicated input/freshness/ownership tests.
6. Remaining readers/navigation/settings surfaces and a complete keyboard/mouse/resize regression pass.

This is a proposal for later planning. No code changes, new model requests, merges or environment publication are part of this review.
