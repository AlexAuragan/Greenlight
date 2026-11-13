# Workspace recording — first implementation

**Scope:** a read-only, bounded audit of project-file state around each workflow step. It does **not** change the existing native-write rollback mechanism, and it does **not** snapshot file contents.

## What is recorded

For every step execution (including retries), Greenlight records two events:

- `workspace.manifest.before` — scanned just before executing the step, after the step has been selected.
- `workspace.manifest.after` — scanned after execution returns, even if the step failed or blocked.
- `workspace.delta` — emitted when the scans differ, listing paths that were created, modified or removed. It records an observation, **not** an assertion about who made the edit.

Each manifest is a JSON object stored in the append-only, branch-aware SQLite event journal:

```json
{
  "files": {
    "src/main.rs": {
      "size": 215,
      "sha256": "64-character hex SHA-256 digest"
    }
  },
  "skipped": []
}
```

These are illustrative values. No source content is stored in workspace manifests; native-write journaling continues to record before/after content independently.

Run:

```sh
stepwright --root /path/to/project workspace-status SESSION_ID
```

The command prints changes between the most recent **completed step's** before/after manifests, then compares a fresh scan with the **latest recorded** manifest on the active branch. It does **not** edit any file or restore anything. If there is no recording yet (older sessions, no step execution), the command reports that fact.

## File scan policy

- Recursively scan project files; hash contents using SHA-256 in small chunks.
- Exclude symlinks, `.git`, `.stepwright/state`, `.env*`, `.ssh`, `.aws`, known credential filenames/extensions, `target`, `node_modules`, Python virtualenvs/caches, `dist`, `build`, and similar generated directories. The source code contains the exact list.
- Bound the scan to **10,000 included regular files** and **20 MiB per file**. Oversized and non-UTF8-named paths are listed as skipped. The scanner refuses an unstable file or a failed read rather than silently recording a stale hash.
- These exclusions are heuristic; they are not a comprehensive secret detector. The manifest stores **path names and hashes**, which should still be treated as private project metadata. It is not sent to the model by this feature.
- The scanner does not implement Git ignore semantics, snapshot binary contents, or attempt to change permissions.

## What this proves — and what it doesn't

| Question | Can a manifest answer it? |
| --- | --- |
| Did a scanned project file change between two scans? | Yes, if its final bytes differ |
| Was a file created or removed? | Yes, within the scanned scope |
| Did Greenlight's native writer cause a change? | **Not by itself.** Compare observed hashes with the native edit journal in a follow-up |
| Was a change specifically made by the user? | No; external processes and Git operations look the same |
| Can we reconstruct old bytes from the SHA-256 digest? | **No** |
| Did a file change and then change back between two scans? | Not necessarily detectable |
| Are build caches, secrets, symlinks and excluded files restored? | No |
| Can this guarantee an atomic rollback of multiple files? | No |

The existing guarded native restore remains the way to revert recorded `write` actions. It already refuses conflicting tracked-file contents and uses recorded before/after blobs.

## Failure behavior

- If scanning fails **before** a step starts, the engine blocks that step and surfaces the error. It does not run it without the requested baseline.
- If scanning or recording fails **after** execution, the error is recorded as a step failure. An underlying execution error is preserved in the error context.
- A crash between a native `write` and its journal commit remains a separate TODO; these boundary scans improve diagnostics but do not make that write atomic.
- An interrupted process may leave a `before` event without a corresponding `after` event. That is not a completed step snapshot.

## Why this is intentionally small

We are collecting evidence before deciding whether broader workspace rollback is useful. The next step is to **correlate manifest differences with native journal entries** and measure untracked drift in realistic sessions. Only then would we investigate storing selected changed-file bytes for actual recovery of external edits.

This feature is designed to stand alone: no new provider calls, no changes to approval semantics, no automatic user-file restore, and no dependency on the planned manager/summarizer roles.
