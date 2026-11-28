# GL-018 — Restore and rollback assessment

Assessed 2026-11-28 against the restore implementation and existing regression suite. **Decision: retain guarded per-file restoration; do not introduce full-workspace capture or multi-file atomic rollback.** This completes GL-018's conditional assessment, not an implementation of workspace-wide recovery. No user-run benchmark or provider request is needed for this assessment.

## Evidence and failure boundaries

`RestorePlan::prepare_paths` derives desired/expected bodies from branch-visible native edits and prior restorations. It rejects journal gaps, unsupported/private paths, symlinks, non-UTF-8 or oversized preimages, and conflicting manual drift before mutation. Per-file selection excludes unrelated paths; all-file restore checks every tracked selected path. Files already at the desired content can be skipped; that is not proof that an earlier failed operation was fully journaled.

After confirmation, application rechecks the active branch, history cutoff and selected preimages under project ownership and the native-write lock. Each file is published through anchored filesystem operations, then its restoration is recorded in a SQLite transaction. These checks serialize cooperating Greenlight operations; they do not prevent external programs from editing the project.

| Failure point | Result and recovery boundary |
| --- | --- |
| Preview/preflight finds drift, a history gap, unsafe path or unsupported preimage | Refuse before mutation; preserve the conflicting file and obtain a new preview after inspection. |
| History/branch/preimage changes after preview | Refuse the stale plan; no automatic refresh-and-apply. |
| A later file operation or per-file journal publication fails | Stop immediately. Earlier applied files stay applied. The failed file can already have changed; later targets are not processed. Return the error and attempt `workspace.restore-failed` with changed/journaled counts. A failed filesystem call can have published before reporting an error, so counts describe confirmed mutations, not a complete forensic inventory. |
| Failure-event recording also fails | Include both errors in the returned diagnostic. Do not claim a durable failure event exists. |
| Final completion-event recording fails | All completed per-file restorations remain journaled and applied. Return an error stating completion publication failed; do not fork or claim successful restart. |
| Fork publication fails after a completed restore | Files remain restored. Report restore-applied/fork-failed separately; do not imply the former branch's workspace was put back. |
| Process exits or crashes between mutation and journal publication | Project maintenance ownership can require explicit recovery. A durable per-file restoration/event is not guaranteed in this window. Inspect live files, saved native bodies and history; owner recovery is not permission to replay the restore. |

For a partial failure, stop further workspace edits first, preserve any manual changes, and inspect the affected files plus native/restoration history and the failure diagnostic. Treat a changed-versus-journaled mismatch as incomplete evidence. Create a fresh preview only after understanding that mismatch. Do not blindly retry or use `git reset --hard` as an assumed equivalent; Git checkpoints/export remain GL-020 and shell/manual changes are outside the native journal's recovery scope.

## Regression evidence

- Existing restore tests cover native history, deletion of a created file, manual drift, symlink/traversal/private-path refusal and the 20 MiB preimage bound.
- Existing Edits tests cover per-file selection and stale content/history previews; existing project/manager tests cover owned restart and failed fork publication.
- The strengthened TUI fault injection rejects the second file's restoration journal in a three-file restore: the first two filesystem mutations remain visible, only the first restore is journaled, the third file is untouched, the diagnostic opens, and no completion or fork is published.
- A new fault injection rejects the final completion event after two successful restorations: both files and per-file records remain restored; the operation returns an error and the session remains on its original branch.

These are reproducible synthetic failure cases, not measured user incident frequencies. The supplied user workflow reports do not establish a requirement for atomic rollback or recovery of unjournaled external bodies. The snapshot/privacy evaluation in [GL-011](45-file-body-snapshot-evaluation.md) remains applicable.

## Why broader rollback is not justified now

A second pass that writes old contents back is another fallible multi-file operation. It can overwrite a new external edit, run out of disk, fail to journal, or crash partway through compensation. Renaming individual files atomically does not make the whole workspace and SQLite one transaction. Full-workspace capture also creates new retention/privacy obligations and does not automatically preserve Git state, binary content, permissions, symlinks, open-file semantics or host side effects.

The proportionate change is clearer preview/error guidance and explicit failure-path coverage, keeping the existing refusal and native-journal boundaries. There is no new automatic rollback, body collection, execution permission or blind retry.

## Reopen criteria

Revisit the decision when a recorded real recovery case cannot be handled with existing native per-file restore and manual conflict preservation, or repeated measured partial-restoration failures justify the complexity. Record affected paths/body sizes, failure stage, changed-versus-journaled evidence, external edits and what the user needed recovered. A future proposal must define capture/retention/privacy limits, crash-safe intent/reconciliation, external-drift protection during compensation, and accurate partial/failure UX before promising stronger atomicity. Git-aware checkpoint design remains GL-020; full-workspace/body capture is not silently added to that item's supported behavior.

Validation: 598 ordinary tests pass; twelve opt-in tests remain ignored. The restore-filtered suite passes all 22 selected tests. Strict all-target Clippy and formatting pass. No provider calls, networked benchmarks or host configuration changes were required.
