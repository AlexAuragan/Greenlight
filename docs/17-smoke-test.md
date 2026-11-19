# Manual smoke tests

Use a disposable project. Automated commands and current verification evidence are in [validation](06-validation.md). Real-provider runs can incur charges; fixture tests need no paid key. Record actual outcomes rather than treating a model-authored conclusion as proof.

## 1. Install and select a project

Follow [installation](22-installation-and-projects.md), initialize an empty directory with `greenlight init`, configure a supported model/key and run `greenlight validate fix-bug`. Start Greenlight from a subdirectory and confirm it selects the containing project. An explicit `--root` selects that project's boundary. Existing project settings/workflows must survive reinitialization and executable upgrades.

## 2. Create, type and navigate

Press `n`, choose a valid workflow, enter a short goal and start with Enter. Back/Cancel retain the unsent goal; Clear discards it. Editors must support blinking Unicode-aware carets and arrow movement. Letters such as h/j/k, slash-prefixed messages and bracketed paste remain literal while editing; paste never sends. Closing/reopening a composer retains the draft, scoped by session, branch and recipient.

The header should show Starting, Running, waiting or a recorded stop—not silently sit Ready. Live model text is an explicitly unvalidated preview (`P` hides; `Z` expands). Browse with arrows, open details explicitly, and use Tab/Shift+Tab for actions. The session picker selects a candidate without switching until Open/Enter; cancelled/no-match browsing retains the opened workspace and its drafts. Help is `h`; no F-keys are required.

## 3. Tool permissions and human checkpoints

Request a native write or shell operation requiring approval. Review the exact command/path/diff and offered scopes. Enter and repeated key events must not decide. Esc hides the tool/grant popup **without approving or denying**; the worker waits and `[a] Review permission` reopens it. Background clicks/shortcuts must not act through the popup. Keys/paste outside the hidden popup must not answer it; text in an editor remains literal. Dismiss the tool popup before opening Help.

Project/global grants require a fresh scope confirmation. `n` explicitly cancels/denies; Esc only hides the confirmation and saves nothing. Process shell grants match the exact command, not an executable prefix. Saved native-write grants retain their stated broader scope.

Human checkpoints use separate review controls: Tab cycles selected documents, Enter/e opens exact-version evidence, and y/n/Enter in the evidence reader must not decide. Esc returns to the review; at the review itself, n/Esc declines. Help can sit above a human review without answering. Change an input version while review is pending and confirm the old approval cannot authorize the new selection.

## 4. Project-only native discovery

Ask for a short source investigation with shell denied and read enabled. Activity should show native `read` on `.` or a subdirectory, then a returned source file—not guessed paths or shell-list approvals. A path substring filter should narrow names, not search contents. Truncation/unreadable counts must remain explicit.

In a controlled fixture, request an absolute outside file, a `..` traversal and a project symlink pointing outside. Each read must fail without returning outside contents, even if read is allowed. Private configuration/dotenv/state remain inaccessible. Compare with a valid project file and root listing. See [discovery scope](36-native-file-discovery.md).

## 5. Actions and full evidence

Using a scripted/local provider, return commentary plus one valid YAML fence: the intended action should run and original raw text remain recorded. Return two action blocks or a missing required field: a protocol correction should occur before any tool execution. Put nested Markdown fences in literal artifact content and verify exact preservation. Unsupported/disabled/denied tools remain distinct from formatting errors.

Use More → Conversation dump (or `/dump` in Commands). Refresh captures new evidence; Save full dump/Copy retain bodies beyond preview clipping. CLI `greenlight dump SESSION_ID --json --output conversation.json` must not overwrite an existing file or change history. New request message references must reconstruct prepared inputs; old missing records remain explicit. See [dumps](35-conversation-dump.md).

## 6. Provider errors and continuation

Use a local fake provider for 401, billing/quota failures, ambiguous/explicit 429, connection errors, incomplete streams and server failures. Error messages must give safe category-specific guidance without echoing a fake secret from the provider body. Usage remains reported/estimated/unknown; each actual network retry has a distinct call ID.

After automatic retries stop, a live workflow shows Retry request / Stop for now. Esc hides and retains the same pending request; a reopens it. Fix/wait, then explicitly retry: prepared messages, attempt and branch must match, and the earlier tool must not replay. Respect remaining Retry-After; k stops execution during backoff or an active response, retaining billing uncertainty. Timeouts/server errors/usage-bearing failures retain billing uncertainty. Partial stream output must never execute.

Stop for now leaves a blocked run with `[t] Continue step`. This is a fresh step attempt and may repeat tools; the screen must state that difference. A worker/process restart does not restore a suspended stack. See [provider behavior](19-provider-failures.md).

## 7. Results, artifacts and native edits

Completed Activity opens the outcome overview; navigate away and use `[s] Overview` to return. Agent conclusions remain attributed. Unavailable benchmarks and `verification: none` must not appear as measured improvement. Read the pinned final report, browse artifact versions/provenance, compare adjacent versions and verify older missing bodies remain explicit.

Open `[3] Edits`: Tab cycles All files/files, arrows select/scroll, Enter focuses ordinary actions. Keep file/all records review of an already-applied native revision; it does not stage Git changes or grant tool approval. Later writes invalidate that revision's mark.

Preview a single-file/all-recorded revert. Enter does nothing in its confirmation; Esc/n cancels and a fresh y applies. Only journal-covered eligible files may revert. Prior manual edits must survive; drift, changed history/branch, active/interrupted ownership and unresolved writes must block stale application. After revert, the old report/checks are historical evidence.

## 8. Worker loss, restoration and isolation

Interrupt a disposable worker while awaiting a decision. Reopening/recovery must expire unanswered requests without consuming them or deleting saved grants. `/recover` checks that the execution lock is free and does not resume. Run `recover-writes` separately for unresolved native writes; manual conflicts are preserved. An observing second process cannot approve, resume or take over a live owner.

For restore/fork, inspect the native-edit preview and scope before confirming. A declined restore may fork without changing files; failures can leave partial changes and must not silently fork or claim full rollback. Successful restoration does not establish complete repository equivalence. Verify shell isolation separately using sandbox-check on a suitable Bubblewrap host.

## 9. Read-only manager questions and exports

Preview `manager-context`, ask a scoped question and inspect citations/saved exchanges/usage. Manager answers must not mutate coding context, execute tools, grant approval or perform a forward. A valid proposed route stays visibly unavailable/unexecuted; stale scope requires review. Missing/oversized sources remain labelled, and unsafe/symlinked documents are excluded. See [current manager Q&A](21-manager-qa.md).

Copy selected evidence with C and verify full text beyond clipping. E uses a private temporary review snapshot while idle, restores the terminal on success/failure and never imports edits automatically. Browsing, Help, versions, usage and previews must add no model requests or execution events.

## 10. Resize and mouse

Exercise 120×36, 80×24 and compact layouts. Selection/drafts and pending requests should survive resizing. Under 40×20, permission decisions are disabled; Esc can still hide a tool/provider popup without answering it. Restore a supported size and reopen the same pending request. Very small terminals show a resize notice, not active hidden controls.

Mouse presses started on one request must not activate a replacement after resize/state change. No click-through when a layer closes. Disable mouse capture in Help for terminal selection, then re-enable it; keyboard operation must remain complete. [Actual captures](screenshots/README.md) are useful references, not substitutes for real-terminal checks.

Stop check: while streaming or running a long shell command, press k or click [k] Stop. The worker must return promptly and start no further tool/request. q must stay Quit. Composer/search k remains text. Help and permission popups retain input ownership; after Esc hides a permission popup, k stops the wait without a denial/grant. Applied writes remain visible in Edits.
