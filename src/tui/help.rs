//! Current discoverable keyboard reference.
pub(super) const HELP: &str = "Navigation
←/→ or Tab: move between sessions, activity/artifacts, evidence and composer
↑/↓ or j/k: select items or scroll evidence
Enter: open selected evidence; from evidence, focus composer
1/2: activity/artifacts · m: toggle artifact Markdown/raw
v in artifacts: saved versions · p provenance · d previous-version diff
Version browser: Enter read, Left list, o content, Esc close
f: search sessions · n: create a ready session
c or /context: saved attempt and input versions
e: inspect blocker · h: activity · b: blocker card
r or /restart: choose earlier step; preview before confirming
v or /events: include protocol events · d: recorded file diff

Activity following
Opening evidence or navigating activity pauses following.
End/G in activity: select latest event and follow new events.
Home/g in activity: select first event and pause.

Reading
↑/↓ or j/k: scroll · PgUp/PgDn: page · Home/End: top/bottom
Esc: close help, diagnostic or context

Messages and drafts
i: compose · Enter: send · Ctrl+J: newline · Backspace: delete
Esc/Tab: leave composer while keeping draft
Drafts belong to their session and remain in memory until exit.
New sessions have their own empty draft. Nothing is saved to disk.
/resume: run a ready/blocked session without extra instructions
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
