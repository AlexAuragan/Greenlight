# Conversation dumps

Use **More → Conversation dump** on a selected run, or enter `/dump` in the Commands composer. The reader shows a fixed snapshot of the selected branch and its ancestry. **[R] Refresh** takes a new snapshot; **[w] Save full dump** writes Markdown under `.greenlight/exports/`. **Copy** exports the complete snapshot, beyond the reader's per-body previews. Help and approval overlays retain priority; viewing/exporting does not resume work, execute tools or grant permissions.

For a complete file from the command line:

```sh
greenlight dump SESSION_ID --output conversation.md
greenlight dump SESSION_ID --json --output conversation.json
greenlight dump SESSION_ID --branch BRANCH_ID --output branch.md
greenlight dump SESSION_ID --all-branches --json --output all-branches.json
```

Without `--output`, output goes to stdout. Existing destination files are never overwritten. Plain terminal output escapes control bytes; saved files and JSON preserve original bodies. Dumps contain project instructions, user messages and tool output, so review the file before sharing it.

## What is included

- Every recorded event, in chronological order, with step, branch, timestamp and original body. This includes step definitions/boundaries, raw model output, protocol corrections, tool arguments/results/errors, permission decisions, verification, manager activity, retries, usage and workflow conclusions.
- Diagnostics distinguish parser rejection, tool-operation failure and provider failure. Validated shell result links flag nonzero exits. A request without a validated outcome is explicitly unresolved; it is not proof that the tool never ran. Legacy unlinked outputs remain visible.
- New model dispatches record the exact prepared **message values and their order**, for coding, verification, deviation detection and manager requests. `model.request` lists message hashes; `messages` in JSON / the Markdown message dictionary contains each distinct value once. Follow those hashes to reconstruct any request, including harness-generated feedback to the model. `model.response` records visible returned content and elapsed request time; usage records contain returned model identity and aggregate reasoning-token counts when reported.
- Long bodies have 800-character previews. Markdown includes the complete body under an expandable section; JSON contains complete originals. No additional inference generates summaries. Reader-wide display limits still apply; Save/Copy preserve the full snapshot.
- If this TUI owns the selected run, refreshing captures its current live output buffer and state, when available. This is ephemeral, unvalidated text, not execution evidence or a guaranteed association with one durable call. The stream buffer can discard earlier text after 64 KiB. CLI exports cannot recover another process's live buffer.

Private reasoning is not available. The dump includes visible model text and supplied activity titles, not a fabricated internal thought process. It records Greenlight's messages, not provider-added chat templates, headers or authentication credentials.

## Existing runs and incomplete records

Older runs still export all their saved raw outputs, context and tool evidence. Their exact prepared message arrays were not recorded; the dump labels this gap instead of rebuilding a conversation using today's prompts. A mixed old/new run can contain both kinds of records. Missing or corrupt message blobs retain their references and produce diagnostics without discarding the remaining transcript.

A request without a response trace may still be running, have been interrupted or have lost its final trace publication. Inspect its usage/provider events before retrying; a missing response does not establish that no request was billed. A completed workflow alone does not establish that its goal was achieved.

## Storage and behavior

New traces use immutable SHA-256-addressed message blobs in `.greenlight/state/blobs/trace-*.json`; repeated context is stored once. Trace events link messages to the existing request/usage IDs. Messages are saved before dispatch. If that persistence fails, the request is marked not sent and stops before contacting the provider. If response persistence fails after dispatch, execution stops with usage retained; it does not automatically repeat an uncertain request. Routine trace metadata stays hidden in the default Activity projection and remains available in raw history and the dump.

The database snapshot respects branch fork cutoffs. The JSON `session` field describes current session metadata; `scope` identifies the exported branch(es), which can differ from the current active branch. Dumping/exporting never changes the session state, branch, approvals, event history, source files or Git state. Trace capture makes local storage writes during ordinary model dispatches; exports are explicit local user actions.

Full reads and exports obey the configurable [resource budgets](40-resource-budgets.md). An oversized dump fails before publication; it is not silently truncated. Oversized history remains browsable through explicitly labelled recent previews, with exact selected bodies opened separately. Raise the applicable limit and restart for larger complete exports.
