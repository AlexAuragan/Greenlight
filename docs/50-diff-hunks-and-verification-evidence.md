# Diff hunks and verification evidence — GL-015

Merged into `master` on 2026-11-23.

## Native diff review

In **Edits**, open a file and use `]` for Next hunk or `[` for Previous hunk. The same actions are in **More**. Opening hunk mode shows one exact unified-diff block, its original file headers, and a **Hunk N of M** label. Arrows and Page Up/Down scroll that block; wrapping and terminal resizing do not change the selected block. The footer exposes Previous hunk, Next hunk, Full file diff and More.

`D` returns to the complete recorded file diff. At either boundary, navigation stays on the first/last hunk. Tab/Shift+Tab retain file cycling; changing files, branch/session, or recorded diff content clears hunk selection. All files returns to the overview. A file with no diff hunks reports that explicitly. History-gap warnings remain visible in hunk mode.

Hunk navigation is inspection only. It neither keeps nor reverts a hunk, stages Git changes, writes files nor records approval. Existing Keep/revert actions still apply to their full recorded file/group scope. The underlying net diff is journal evidence, not a fresh disk comparison; shell/manual edits remain outside its coverage. Artifact-version comparisons and proposed revert previews retain their existing readers.

Copy (`C`) and read-only editor export (`E`) export the complete **selected hunk** while in hunk mode, including its scope label. Return to Full file diff first to copy/export the full file diff. Preview truncation does not trim the exported source.

## Export recorded verification evidence

Use **More → Export verification evidence**, `V` in the workspace, or the export action in **Details** / **Technical details**. From the CLI:

```sh
greenlight --root /path/to/project verification-evidence SESSION_ID
```

The action prints/shows a newly created file under `.greenlight/exports/verification-<UUID>.json`, with Unix `0600` permissions. It reads one database snapshot and publishes a complete file without overwriting an existing export. It starts no worker, sends no provider request, runs no command and changes no history or verdict.

The versioned JSON format contains:

- Session identity, active branch, visible history cutoff and explicit interpretation limits.
- Recorded step definitions/starts/outcomes, all `verification.*` records, input manifests, human input approvals/declines, native-change/restoration and user-revision metadata. All visible attempts are retained, including earlier failed checks.
- Exact artifact bodies referenced by recorded input manifests, human input approvals or verification evidence. An older reviewed artifact is exported by its event ID even if a newer version exists.
- Recorded tool/check observations referenced by verification evidence, plus exact tool requests where durable `tool.finished` correlation exists. Requests are never guessed from adjacent events.
- Diagnostics for missing/hidden references, malformed/legacy input references and absent verification records.

Branch ancestry/fork cutoffs apply to every body reference. References into another session or excluded future history produce diagnostics and never export that body's contents. A referenced visible artifact with an unreadable/missing blob fails the export; it is not substituted with an empty body or a newer version.

The export uses the existing history/blob/materialization budgets, `limits.export_events` for records/references and `limits.export_bytes` for bounded JSON serialization. Exceeding a budget publishes no partial report. A symlink replacing the export directory is refused. Reference expansion is bounded and deduplicated; there is no recursive inference of missing evidence.

## Interpretation

This is a historical evidence bundle, not a new pass/fail verdict or an attestation of the current workspace. In particular, `verification.none` followed by `verification.passed` means the configured policy required no substantive checks. Model assessments and human decisions retain their recorded meaning. Existing tool-output truncation is retained verbatim. Restorations or later edits do not retroactively revalidate an older result.

Full provider requests, environment credentials, unreferenced artifact bodies and unrecorded filesystem state are not collected. Raw recorded policies and tool/project output can themselves contain sensitive text; inspect an export before sharing it. Legacy checks without exact references cannot supply evidence that was never recorded.

## Validation

Tests cover UTF-8 hunk boundaries, header-like body text, first/last navigation, real TUI actions, narrow/full-size rendering, full-vs-selected export scope and file-cycle reset without writes/review decisions. Evidence tests cover exact historical bodies and tool correlation, retained failures, branch/session isolation, stale selection, no-check/legacy cases, missing blobs, event/byte/escaping limits, private file permissions and symlink refusal. See [validation](06-validation.md).
