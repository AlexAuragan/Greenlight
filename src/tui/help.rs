//! Shortcut reference generated from command metadata, plus contextual safety rules.
const GUIDE: &str = r"Navigation
Arrows select or scroll the current region.
Tab / Shift+Tab focus and cycle action buttons; Enter activates the focused action.
Esc leaves actions, closes a reader/layer, or toggles the sidebar.
Help also accepts ?. H returns to recorded activity.
Sessions opens search; arrows select candidates, Enter opens the selected session.
Back/Esc cancels browsing; Clear search resets the filter.
Search letters and paste stay literal. Compose closes without discarding its draft.

Diff review
Edits: [ and ] open the previous/next recorded hunk; D returns to the full file diff.
Tab still cycles files. Hunk navigation changes no files or review decisions.
Edits g opens optional grouped review; [ / ] select groups, t toggles step/attempt.
Grouped review is a frozen read-only snapshot; reopen it to refresh. C/E exports its full text.
V exports historical verification evidence (also More / Details). No checks are rerun.

Artifacts
e edits the selected artifact into a new version; N adds a version-specific comment.
b reads comments. These actions also appear in More. Stop workers before editing.
Save and close the external editor to import; unchanged drafts/empty comments save nothing.
E remains a read-only export; comments do not approve work or message the agent.

Decisions
Use the displayed decision key or click its row; Enter never decides.
Human checkpoint: Tab/Shift+Tab cycles documents; Enter/e opens evidence.
Evidence readers disable y/n; Esc returns to the review.
Tool permissions own input; unrelated shortcuts are ignored.
Esc hides the popup without deciding; the agent keeps waiting.
Review permission / Review retry reopens the pending request.
Dismiss tool popups before opening Help; human checkpoints retain h Help.
Help owns input above the pending decision; Esc closes Help only.
Provider pause: y retries the exact request, n stops, Esc hides.
[t] Retry saved request restores exact messages/attempt after reopening.
[T] Retry saved manager restores the original question and evidence snapshot.
Changed workspace/settings block saved retry; fresh instructions start a new attempt.
Recover an interrupted owner first. Credentials reload; earlier tools are not replayed.
Revert/restart: y confirms, n/Esc cancels; Enter is inert.

Evidence and actions
Copy exports full selected text. Editor opens a private snapshot while idle.
[R] Run tool again reruns the selected historical call while idle.
Exact arguments; no extra approval; side effects may repeat. Results are linked in history.
More > Conversation dump or /dump opens the recorded transcript.
More > Usage or /usage opens session token/cost accounting.
More > Provider and Request details include recorded API errors and support IDs.
Copy exports these diagnostics; known credentials are redacted.
Request details: arrows select, Enter/click opens the exact dispatch.
Back/Esc returns within details before returning to the workspace.
v shows raw events in Activity, or artifact versions in Artifacts.
o opens the exact artifact; p shows provenance; d opens previous/journal diff.
m switches artifact Markdown/raw; u/U previews file/all recorded reverts.
Edits: Tab cycles All files/files; Enter focuses actions; Esc returns.
Keep saves review intent for already-applied files, not tool grants or Git staging.
e opens blocker evidence; b opens its card; r previews restart.
End/G follows latest activity; Home/g pauses at the first record.
Running Activity uses a compact list beside the larger model/tool output pane.
Enter/click opens an event there; Live output returns to the stream.
Narrow screens stack activity above output. P hides output; Z toggles full width.
[k] Stop cancels active execution and retry waits; applied edits remain.
[q] Quit exits while idle. Stopping may leave uncertain billing/partial command effects.

Text editing
Arrows move the caret; Home/End and Delete/Backspace edit.
Enter sends/starts; Ctrl+J or Alt+Enter inserts a newline.
Letters h/j/k and pasted text stay literal while typing. Paste never sends.
Blue denotes user input; focused carets blink.
Drafts are memory-only, scoped by session, branch and recipient.
Conversation answers or routes originals; restarts still require confirmation.
Change recipient selects Conversation, read-only manager or step.
Navigation / opens Commands.
Slash-prefixed text in message modes stays literal. Closing never sends.

Mouse and reading
Click buttons/rows to open; scroll with the wheel.
M toggles mouse capture here for terminal text selection.
No function keys are required.
PgUp/PgDn pages; Home/End moves to the top/bottom of a reader.
Live output and model claims are not executed/verified evidence.
Artifact revisions are documents; Edits covers native recorded writes.
/resume continues ready/blocked work; /recover never resumes it.
/ask QUESTION asks the read-only manager; routes do not execute.
More → Generate summary makes an optional background model request.
Latest summary reopens captured evidence without inference; Stop summary cancels it.
Restart review lists historical runs; arrows select, Enter previews, y confirms.
More → Compare branches reads captured file hashes without changing the workspace.
q quits only when there is no local worker or manager job.
";

pub(super) fn text() -> &'static str {
    static HELP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HELP.get_or_init(|| {
        let mut text = String::from("Action shortcuts · available in their displayed context\n");
        for binding in super::actions::BINDINGS {
            text.push_str(binding.label);
            text.push('\n');
        }
        text.push_str("\nContext and safety rules\n\n");
        text.push_str(GUIDE);
        text
    })
}
