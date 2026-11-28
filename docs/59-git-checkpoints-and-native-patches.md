# Git-aware observations and native patch export (GL-020)

A Greenlight checkpoint records **identity and hashes**, not a Git commit, index backup, full-workspace snapshot or rollback transaction. The native journal supplies text bodies for patch export; arbitrary shell/manual changes cannot be reconstructed. Existing guarded restore/restart semantics remain unchanged.

## Use from the TUI or CLI

More exposes `[K] Record checkpoint` and `[J] Export native patch`; Help lists both. Checkpoint recording requires an idle project with no unresolved ownership/write recovery. Export is a read-only historical operation and does not start a worker, run verification, stage files or apply a patch. The TUI exports the net continuous native journal history on the active branch, not automatically the interval after the most recent checkpoint. Its notice shows the saved path and scope.

```sh
greenlight checkpoint SESSION --label 'Before refactor'
greenlight checkpoints SESSION
greenlight checkpoint-show SESSION CHECKPOINT_EVENT

greenlight patch-export SESSION --checkpoint CHECKPOINT_EVENT
# Or explicitly export the net native journal history:
greenlight patch-export SESSION
```

`patch-export` prints a new private `.greenlight/exports/native-UUID.patch` path (the existing legacy layout is respected). It never overwrites a chosen source file or an existing export. The patch preamble records session, branch, event cutoff, selected checkpoint and its recorded Git identity. It identifies the export as native-only evidence; its preimage is not necessarily HEAD, the index, or the current live file.

To validate a patch, use a separate disposable checkout/copy whose text preimages match the exported range, review the patch, then run `git apply --check /path/to/native-UUID.patch` there. Greenlight does not apply it automatically. Original staged content can differ from both the native preimage and live worktree. Do not use `git apply --index`, reset or checkout as an assumed recovery operation.

## Recorded checkpoint scope

- Project-root `.git` directories: bounded, descriptor-anchored reads of HEAD, its loose or packed reference, and the index digest. Symbolic, detached, unborn and SHA-1/SHA-256-shaped identities are supported as observations. Object existence/type/ancestry are not validated. Configuration, hooks, filters, pagers, credential helpers and Git executables are never read/run by this operation.
- A root without `.git` is explicitly recorded as such; Greenlight does not search parent directories for another repository.
- `.git` files used by linked worktrees/submodules are marked unsupported. Their external paths are not followed. Symlink metadata paths are refused. This does not provide staged/history/submodule equivalence.
- Workspace content uses the existing bounded regular-file hash manifest and exclusions: state, Git internals, credentials and generated/cache paths are outside that manifest. Bodies are not retained. Oversized/non-UTF8 omissions remain explicit; an incomplete checkpoint cannot establish a checkpoint-relative patch baseline.

Project/session ownership and the native-write lock serialize cooperating Greenlight operations. Git identity is read before and after the workspace scan; a detected HEAD/index change refuses publication. External programs are not locked out, and neither equal endpoint reads nor the workspace scan constitute an atomic filesystem/Git snapshot. No automatic step checkpoints or additional model disclosure are introduced.

Checkpoint records are checksum-validated trace blobs referenced by branch-visible events. A checkpoint-relative export requires the checkpoint's original session and branch; merely inheriting an ancestor's event does not rebind its identity. These observations do not become extra restore/restart points. Existing historical restart checkpoints remain available separately.

## Patch safety and supported representation

Exports merge only native writes and explicit native restores visible in the chosen branch/range. Each recorded postimage must match the next preimage for the same path. A gap refuses the entire export, even if later changes would cancel the net diff. With a checkpoint baseline, the first native preimage must also match its recorded hash/absence. This protects against folding intervening external content into a checkpoint-relative patch.

Text modifications, creation, deletion, empty files, quoted filenames and missing final newlines are represented as Git-compatible diffs. NUL-containing text and unsafe/private paths are refused. Net-zero changes produce no patch. File-mode changes, rename identity, binary content, LFS/submodule data and arbitrary worktree/index changes are outside native journal coverage. New/deleted text files use the conventional non-executable Git mode; original executable metadata is not reconstructed. Review `git apply --check` results against the actual target.

Materialized native transitions and generated patch output are capped at the smaller of 16 MiB and `limits.export_bytes`; the existing history/blob/materialization limits still apply. Budget or continuity failures publish no complete export. A single SQLite read transaction fixes the exported journal cutoff, and exclusive anchored publication prevents replacing an existing export. This does not freeze the live filesystem or mean the historical patch describes its current state.

## Validation boundaries

Tests apply exported patches with real Git in a disposable directory, including empty-file addition/deletion, quoted names and EOF-only differences. They verify an existing index is byte-for-byte unchanged while staged and unstaged contents differ, even with invalid repository config; HEAD/packed-ref and symlink handling; omission/budget/continuity/foreign-checkpoint refusal; and the TUI controls' preservation of manual contents. No production Git command, provider request, host security change or automatic patch application is introduced.
