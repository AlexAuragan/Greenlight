# Native file edit history (v0.2 preview)

Greenlight records each successful, changed **native write tool** operation as immutable before/after UTF-8 snapshots in `.stepwright/state/blobs` and indexes them in SQLite `file_changes`. Each entry is tied to the session, branch, step and event ID.

```bash
stepwright changes SESSION_ID
stepwright diff SESSION_ID --edit EVENT_ID
```

The active branch includes inherited edits only up to its fork point. The diff renderer uses a line-based shortest edit script to produce **multiple unified hunks** with three lines of context. Adjacent or overlapping context regions are merged. It also marks lines missing their final newline. Both the approval preview and `diff` command share this renderer. For extremely divergent files (more than 2,048 line edits), a memory-bounded fallback renders the differing middle as one replacement.

Before a write, the proposed diff is printed. Approval is requested as usual, and the original file is re-read afterward to reject concurrent modifications. Denied writes are not recorded. An identical write is skipped.

**Limitations:** Only native `write` calls are recorded. Shell commands and external edits are not intercepted. There is no automatic rollback and no sandbox. A write can succeed before journal persistence fails; in that case execution stops with an explicit error. Future work can introduce stronger transaction guarantees and attempt IDs.
