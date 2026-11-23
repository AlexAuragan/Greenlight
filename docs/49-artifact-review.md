# Artifact review comments and human revisions — GL-014

Merged into `master` on 2026-11-23.

## TUI actions

Select a document in **Artifacts**, or select an exact historical version in **Versions**. The same actions appear in **More** and Help:

| Action | Key | Result |
| --- | --- | --- |
| Edit artifact (new version) | `e` | Open a writable private draft of the selected version |
| Add review comment | `N` | Open a blank private draft for a comment on that version |
| Review comments | `b` | Read branch-visible comments on the selected exact version |
| Read-only editor export | `E` | Existing snapshot viewer; changes are never imported |

The editor comes from `VISUAL`, then `EDITOR`, then `vi`. Settings are parsed into argv without a shell; supported GUI editors get their existing wait flag. Greenlight suspends and restores terminal/mouse/paste capture while the editor runs. Stop this client's workflow, manager and summary workers first. A project busy in another client cannot publish a revision or comment.

Save and close the editor with a successful exit to publish. No separate approval is needed for these explicitly requested human edits. Closing without changing the original, or leaving a comment blank, saves nothing. An intentionally empty replacement artifact is allowed. Edits are limited to 1 MiB (or the smaller configured blob limit); comments to 64 KiB. Non-UTF-8, oversized, missing or symlink-replaced drafts are rejected.

On editor failure, save conflict or publication failure, nothing is imported. An ownership-cleanup failure can occur after a version or comment was saved; the diagnostic directs you to inspect history and recover ownership before retrying, rather than claiming that nothing was saved. The diagnostic gives the path to the retained private draft; recover its contents manually, then remove the private directory when finished. Successful/unchanged drafts are deleted automatically. Greenlight does not hold an execution lease while a person uses the editor.

## History and authority

Original artifact blobs remain immutable. A successful edit creates a new latest version on the active branch, with the same document name and recorded step, explicit user authorship and a link to the source version. Editing a historical version is allowed: it creates a new version based on that historical body. The latest version must still match the one captured before opening the editor. A concurrent revision or branch change rejects publication and retains the draft.

New body publication and its `artifact.user-edit` attribution are one SQLite transaction, after durable blob creation. A failed transaction removes the unpublished blob. The existing project/session control lease serializes final validation and publication; interrupted/busy ownership follows the normal recovery rules. No new database schema is required.

Comments append `artifact.comment` records naming the exact source event. They create no artifact body, do not migrate to a later version, and follow existing branch inheritance/cutoffs. Comments on an inherited artifact are visible on the commenting branch, without modifying the ancestor. Comments can be added even if a newer version exists. The Comments reader refreshes when new comments arrive.

The Versions list identifies user revisions; Version details shows their source link and authorship instead of claiming a producing model attempt. Activity distinguishes user revisions and review comments. Ordinary copy/export remains available from the Comments reader.

These actions do not approve a checkpoint, resume a workflow, alter source files, erase history, rerun verification or send feedback to the agent. Recorded checks and pinned historical reports continue to describe their original versions. A future workflow input selection can use the new latest artifact under the existing input/approval policies. To ask the agent to act on a comment, send an explicit message using the normal manager/workflow controls.

## Validation

Tests cover real TUI actions and an external-editor fixture, exact-version comment reading, original preservation, user/source attribution, latest-version and branch conflicts, cross-client owner exclusion, no-op/size limits, failed-editor and symlink replacement rejection, recoverable drafts, and atomic rollback of a failed attribution publication. No provider requests are needed. See [validation](06-validation.md).
