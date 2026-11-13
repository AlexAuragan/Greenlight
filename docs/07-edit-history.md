# Native file edit history

Greenlight records each successful, changed **native write tool** operation as immutable before/after UTF-8 snapshots in `.greenlight/state/blobs` and indexes them in SQLite `file_changes`. Each entry is tied to the session, branch, step and event ID.

```bash
greenlight changes SESSION_ID
greenlight diff SESSION_ID --edit EVENT_ID
```

The active branch includes inherited edits only up to its fork point. The diff renderer uses a line-based shortest edit script to produce **multiple unified hunks** with three lines of context. Adjacent or overlapping context regions are merged. It also marks lines missing their final newline. Both the approval preview and `diff` command share this renderer. For extremely divergent files (more than 2,048 line edits), a memory-bounded fallback renders the differing middle as one replacement.

Before a write, the proposed diff is printed. Approval is requested as usual, and the original file is re-read afterward to reject concurrent modifications. Denied writes are not recorded. An identical write is skipped.

Before mutation, SQLite durably records a `file.write-intent` event and the original/intended contents in `write_intents`. The tool writes and syncs a temporary sibling, rechecks the source, atomically replaces it and syncs its directory. Journal completion publishes `file.changed`, its snapshots and the completed intent status in one SQLite transaction. A project mutation lock prevents overlapping native writes, recovery and explicit restores; this is not a session execution lease or protection against external editors.

After an interruption, inspect history and run:

```bash
greenlight --root /path/to/project recover-writes
```

Recovery does not change source files. If a target matches the intended contents, it completes the journal; if it matches the original contents (including an absent new file), it cancels the intent. Any other contents, non-UTF-8 data or symlink replacement remain unresolved and are reported. Preserve conflicting manual edits before reconciling the file with either saved state and retrying recovery. Pending snapshots are retained in the local SQLite `write_intents` table; do not delete history to bypass a conflict. Recovery can be repeated safely and leaves affected sessions blocked for an explicit resume.

If the write's execution owner died, first run `greenlight recover-session SESSION_ID` (or `/recover` in the TUI), then `recover-writes`. Write reconciliation refuses sessions with live or unrecovered execution owners.

Unresolved intents block execution, further native writes, restoration and branch changes across the project. Read-only inspection remains available, and the TUI shows a recovery notice on opening. A crash may leave `.greenlight-write-<intent-id>.tmp` beside the target or unreferenced journal blobs; they are not treated as applied edits or automatically deleted.

**Limitations:** Only native `write` calls (and separately journaled explicit restores) have historical before/after contents. Sandbox shell commands run under **mandatory Bubblewrap isolation** with read-only project source, but temp/cache writes and external/manual edits are not intercepted. A guarded opt-in `restore` can revert tracked native edits; it is not automatic and does not rebuild arbitrary workspace state. Explicit restores still have their own apply-before-journal window and are not atomic across files. Concurrent external edits remain a race between the last check and replacement. Durable attempt IDs and broader recovery remain future work; this change cannot reconstruct pre-intent historical writes.
