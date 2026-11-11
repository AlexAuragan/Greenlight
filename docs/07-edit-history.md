# Native file edit history (v0.2 preview)

Greenlight records each successful, changed **native write tool** operation as immutable before/after UTF-8 snapshots in `.stepwright/state/blobs` and indexes them in SQLite `file_changes`. Each entry is tied to the session, branch, step and event ID.

```bash
stepwright changes SESSION_ID
stepwright diff SESSION_ID --edit EVENT_ID
```

The active branch includes inherited edits only up to its fork point. The display uses a unified one-hunk diff showing the changed section with up to three lines of surrounding context. For complex changes this can yield a large hunk; per-hunk review remains future work.

Before a write, the proposed diff is printed. Approval is requested as usual, and the original file is re-read afterward to reject concurrent modifications. Denied writes are not recorded. An identical write is skipped.

**Limitations:** Only native `write` calls are recorded. Shell commands and external edits are not intercepted. There is no automatic rollback and no sandbox. A write can succeed before journal persistence fails; in that case execution stops with an explicit error. Future work can introduce stronger transaction guarantees and attempt IDs.
