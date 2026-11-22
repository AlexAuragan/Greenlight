# Workspace recording and journal correlation

**Scope:** a read-only, bounded audit of project-file state around each workflow step. It does **not** change the existing native-write rollback mechanism, and it does **not** snapshot file contents.

## What is recorded

For a step execution (including retries), Greenlight attempts two manifest events, with an additional delta event **only when there is a detected difference**:

- `workspace.manifest.before` — scanned just before executing the step, after the step has been selected.
- `workspace.manifest.after` — scanned after execution returns, even if the step failed or blocked.
- `workspace.delta` — emitted when the scans differ, listing paths that were created, modified or removed. It records an observation, **not** an assertion about who made the edit.
- `workspace.correlation` — records the before/after manifest IDs, journal references, path classifications and scan/correlation metrics when there are journal/delta paths or coverage/recovery warnings. The engine records this after the second scan, before publishing the step outcome. The default TUI shows a **Workspace audit** card; Enter opens the report and raw evidence.

Each manifest is a JSON object stored in the append-only, branch-aware SQLite event journal:

```json
{
  "files": {
    "src/main.rs": {
      "size": 215,
      "sha256": "64-character hex SHA-256 digest"
    }
  },
  "skipped": [],
  "scan_stats": {"files": 1, "bytes_hashed": 215, "elapsed_us": 180}
}
```

These are illustrative values. No source content is stored in workspace manifests; native-write journaling continues to record before/after content independently.

Run:

```sh
greenlight --root /path/to/project workspace-status SESSION_ID
greenlight --root /path/to/project workspace-status SESSION_ID --json
greenlight --root /path/to/project workspace-benchmark --runs 7 --json
```

`workspace-status` classifies differences between the most recent **complete pair of step recordings** (which may represent a failed/blocked step), then compares a fresh scan with the **latest recorded** manifest on the active branch. It does **not** edit files, restore anything, append events or make model requests. It can read saved state without loading the current workflow/provider configuration. If there is no recording yet, the command reports that fact. The JSON result has `latest_step` and `current` reports. Benchmark mode needs no session and saves no manifest/history.

## File scan policy

- Recursively scan project files; hash contents using SHA-256 in small chunks.
- Exclude symlinks, `.git`, `.greenlight/state`, legacy `.stepwright/state`, `.env*`, `.ssh`, `.aws`, known credential filenames/extensions, `target`, `node_modules`, Python virtualenvs/caches, `dist`, `build`, and similar generated directories. The source code contains the exact list.
- Bound the scan to **10,000 included regular files** and **20 MiB per file**. Oversized and non-UTF8-named paths are listed as skipped. The scanner refuses an unstable file or a failed read rather than silently recording a stale hash.
- These exclusions are heuristic; they are not a comprehensive secret detector. The manifest stores **path names and hashes**, which should still be treated as private project metadata. It is not sent to the model by this feature.
- The scanner does not implement Git ignore semantics, snapshot binary contents, or attempt to change permissions.

## What this proves — and what it doesn't

| Question | Can a manifest answer it? |
| --- | --- |
| Did a scanned project file change between two scans? | Yes, if its final bytes differ |
| Was a file created or removed? | Yes, within the scanned scope |
| Does the observed state agree with recorded native writes/restores? | Yes, when a complete hash chain connects both scans; this is **agreement, not proof of who performed an edit** |
| Was a change specifically made by the user? | No; external processes and Git operations look the same |
| Can we reconstruct old bytes from the SHA-256 digest? | **No** |
| Did a file change and then change back between two scans? | Not necessarily detectable |
| Are build caches, secrets, symlinks and excluded files restored? | No |
| Can this guarantee an atomic rollback of multiple files? | No |

The existing guarded native restore remains the way to revert recorded `write` actions. It already refuses conflicting tracked-file contents and uses recorded before/after blobs.

## Failure behavior

- If scanning fails **before** a step starts, the engine blocks that step and surfaces the error. It does not run it without the requested baseline.
- If scanning or recording fails **after** execution, the error is recorded as a step failure. An underlying execution error is preserved in the error context.
- Native writes now have durable write intents and explicit `recover-writes` reconciliation after interruption. The audit warns about pending intents and never treats them as completed journal evidence. Partial/interruptible multi-file restores remain a separate limit; scans do not make restoration atomic.
- An interrupted process may leave a `before` event without a corresponding `after` event. That is not a completed step snapshot.

## Why this is intentionally small

Journal correlation and measured scanning are implemented. [GL-011 evaluated the external-preimage gap](45-file-body-snapshot-evaluation.md) and retained hash auditing plus overwrite protection. Broader body collection/recovery needs a concrete opt-in recovery case and the documented size/privacy/retention policy; it is not current restore coverage.

This feature is designed to stand alone: no new provider calls, no changes to approval semantics, no automatic user-file restore, and no dependency on the planned manager/summarizer roles.

## How journal correlation works

- **Journal explained:** every ordered write/restore before-hash agrees with the previous expected state, starting at the baseline; the final after-hash agrees with the observed scan. Creation distinguishes absence from an empty file, and a restore can remove a file. The report lists exact native event IDs.
- **Unexplained:** a scanned path changed without an in-interval journal entry, a journal before-state has a gap, or its final state disagrees with the observed hash. This does not identify a human, Git operation or external process as the actor.
- **Coverage unknown:** the path was skipped/excluded, its scan coverage changed, a native event lacks journal snapshots, or a live comparison crosses an attempt/fork boundary. An oversized file is not confidently labelled deleted just because its hash disappeared. Excluded paths are not newly enumerated; a previously recorded journal path can still appear with an explicit coverage qualification.

Paths with journal records are examined even when their net scan delta is zero. Thus A→B→A can be explained, while A→external B followed by a native B→A exposes a gap despite the same final hash. A change and reversal that leave **no journal record** between scans remain undetectable.

Only branch-visible journal rows strictly after the baseline and through the observed-event cutoff participate. Completed pairs must bound one uninterrupted attempt with no intervening manifest/fork/attempt boundary; retries cannot borrow an older baseline. A live status captures its journal cutoff **before scanning**, so later journal rows cannot explain an earlier observation. Scans are not atomic filesystem snapshots: check idle projects for the clearest result. A file replaced by a symlink disappears from the scanned regular-file set; the report does not assert the filesystem object was deleted.

Correlation reads/hashes only journal blobs in the requested interval, not every old edit body. Missing evidence in that interval is an explicit error; a missing unrelated old blob does not prevent a new interval's audit. The existing engine records an audit failure as a step error if post-step recording/correlation cannot complete. Drift classifications themselves do not grant permissions, route a deviation, block a valid action or automatically restore anything.

## Measured scan overhead

Measurements on **2026-11-15**, one cloud Linux workspace, unoptimized debug build, seven consecutive scans per project. “First” is the first sample in this run, **not a controlled cold-cache measurement**; development/tests may already have warmed filesystem caches. The next six samples supply the repeat median. No contents or manifests were saved by the benchmark command.

| Source tree | Included files | Bytes hashed per scan | First scan | Repeat median | Manifest JSON per scan |
| --- | ---: | ---: | ---: | ---: | ---: |
| Greenlight working tree | 90 | 1,069,283 | 19.160 ms | 18.199 ms | 10,452 bytes |
| Tokio 1.54.1 cached source distribution | 582 | 5,026,731 | 122.684 ms | 91.213 ms | 68,450–68,451 bytes |

Both scans skipped zero oversized/non-UTF8 paths under the standard exclusions. Observed ranges were 17.908–19.833 ms and 88.231–122.684 ms respectively. This is a small/medium source-tree sample, not a monorepo, network filesystem or production performance guarantee. Two scans per step suggest roughly 36 ms / 182 ms of warm scan work here, before SQLite, serialization and journal-correlation costs. Journal-heavy restoration costs were not benchmarked.

Each new manifest records `scan_stats` (included files, bytes hashed, elapsed microseconds); the audit records serialized manifest bytes and correlation microseconds separately. Old manifests retain **unknown** metrics rather than fabricated zeros. Scanning does not store file contents. The 10,000-file and 20 MiB/file bounds remain unchanged. Reproduce on a representative project with `workspace-benchmark --runs 7 --json`; run it while files are stable, and assess local cache/storage behavior before expanding recording/restoration.

GL-011’s [snapshot evaluation](45-file-body-snapshot-evaluation.md) documents the observed recovery gap and the opt-in size/privacy/retention requirements for any future external-body store. Current runtime behavior remains hash-only outside native journaling.
