//! Current discoverable keyboard reference.
pub(super) const HELP: &str = "Navigation
←/→ or Tab: move between sessions, activity/artifacts, evidence and composer
↑/↓ or j/k: select items or scroll evidence
Enter: open selected activity/artifact; from evidence, return to its list
o: open an artifact card’s exact version · d: journal diff for a file card
v or /events: raw audit history; Enter/Left/Esc returns from expanded evidence
1/2/3: activity/artifacts/recorded edits · m: toggle artifact Markdown/raw
v in artifacts: saved versions · p provenance · d previous-version diff
Edits: u previews selected-file revert · U previews all recorded edits
Revert: y confirms · n/Esc cancels · Enter never confirms
s: completed-run result · h: activity
Version browser: Enter toggles read/list, Left list, o content, Esc close
f: search sessions · n: choose workflow, enter goal, start session
F: filter activity metadata/summaries · Enter applies, Esc clears
C: copy full selected text to this machine’s clipboard
E: review a temporary snapshot in VISUAL/EDITOR (idle worker only)
c or /context: saved attempt and input versions
e: inspect blocker · h: activity · b: blocker card
r or /restart: choose earlier step; preview before confirming

Activity following
Live model output is an unvalidated preview, not evidence of executed work.
Agent intent cards are model-authored; outcomes and verification stay separate.
Opening evidence or navigating activity pauses following.
End/G in activity: select latest event and follow new events.
Home/g in activity: select first event and pause.

Reading
↑/↓ or j/k: scroll · PgUp/PgDn: page · Home/End: top/bottom
Esc: close help, diagnostic or context

Messages and drafts
i: compose · Enter: send/start · Ctrl+J or Shift+Enter: newline
Input: ←/→ move · ↑/↓ move between wrapped rows · Home/End · Delete/Backspace
Green text is your input; focused fields show a blinking caret.
Esc/Tab: leave composer while keeping draft
Drafts belong to their session and remain in memory until exit.
Activity/artifact tabs retain selection, scroll and journal diff per session/branch.
Help/context/diagnostic scroll stays separate from the underlying tab.
New sessions have their own empty draft. Nothing is saved to disk.
/resume: run a ready/blocked session without extra instructions
/ask QUESTION: read-only manager · /manager: its saved exchanges
/manager-context: preview shared sources without inference
/cancel-manager: stop future manager retries (in-flight request may finish)
With manager.enabled, ordinary messages ask the manager; /resume still runs the agent.
Manager forwarding is a proposal only; live/idle delivery is not implemented yet.
/usage: token and cost report · /recover: recover interrupted owner
/help or ?: this panel (? is literal text inside composer/search)

Approvals
Only explicit decision keys approve. Enter never decides.
y: once · a: process · p: project · g: global (when offered)
Persistent grants require a second confirmation. n/Esc: deny.
Navigation/help shortcuts stay inactive during approvals.

Small terminals
Below 72 columns or 22 rows: one pane at a time; arrows switch panes.
Below 40×14: resize notice; approvals require 40×20. Esc denies.
Resizing preserves drafts and selections; scroll clamps to content.
x: stop future provider requests/cancel retry wait; in-flight request may finish
q: quit when this TUI has no active worker
";
