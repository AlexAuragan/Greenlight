# Native file edit history

Greenlight records each successful, changed **native write tool** operation as immutable before/after UTF-8 snapshots in `.greenlight/state/blobs` and indexes them in SQLite `file_changes`. Each entry is tied to the session, branch, step and event ID.

```bash
greenlight changes SESSION_ID
greenlight diff SESSION_ID --edit EVENT_ID
```

The active branch includes inherited edits only up to its fork point. The diff renderer uses a line-based shortest edit script to produce **multiple unified hunks** with three lines of context. Adjacent or overlapping context regions are merged. It also marks lines missing their final newline. Both the approval preview and `diff` command share this renderer. For extremely divergent files (more than 2,048 line edits), a memory-bounded fallback renders the differing middle as one replacement.

Before a write, the proposed diff is printed. Approval is requested as usual, and the original file is re-read afterward to reject concurrent modifications. Denied writes are not recorded. An identical write is skipped.

**Limitations:** Only native `write` calls (and separately journaled explicit restores) have historical before/after contents. Sandbox shell commands run under **mandatory Bubblewrap isolation** with read-only project source, but temp/cache writes and external/manual edits are not intercepted. A guarded opt-in `restore` can revert tracked native edits; it is not automatic and does not rebuild arbitrary workspace state. A write can succeed on disk before journal persistence fails; in that case execution stops with an explicit error and the file may need manual reconciliation. Future work includes stronger crash consistency and durable attempt IDs.
