# Optional grouped native edit review — GL-016

Merged into `master` on 2026-11-23.

## Open and navigate

In **Edits**, choose **More → Grouped review** or press `g`. This explicitly opens a read-only historical reader; the normal file overview, hunk controls and Keep/revert behavior remain the default.

The footer shows Previous group (`[`), Next group (`]`), Group by step/attempt (`t`) and Back. Groups open in attempt mode. Switching mode starts at the first group; Previous/Next stop at the boundaries. Groups are ordered by their first visible journal event, with files ordered by path within each group. Arrows/Page Up/Down scroll the selected combined diff. Tab cycles action buttons in this reader, while ordinary Edits retains its file cycling.

Copy (`C`) and read-only editor export (`E`) use the selected group's complete text, including scope and warnings. The existing large-document renderer may limit its viewport preview; that does not clip the exported source. Export limits still apply to constructing the full group. Escape/Back returns to the underlying workspace without keeping, reverting or approving anything.

The reader captures one database snapshot, labelled with session, selected branch and history cutoff. Incoming activity does not silently change the group while it is being read. Close and reopen grouped review to refresh. Switching session or branch closes the reader. The diff is cached for the current selection rather than regenerated during idle refreshes.

## Group identity and scope

Only native write/restoration records already present in the journal are included. Ordinary source files are not reread for this view.

- **By attempt:** native writes are assigned using durable attempt IDs, source branch, step and event bounds. Retries of the same step remain separate groups.
- **By step:** native writes from the same step/source branch are combined across attempts. This includes older writes lacking a durable attempt ID; step grouping does not invent an attempt identity for them.
- **Unattributed writes:** attempt mode gives these an explicit “no durable attempt ID” group. Such a group may contain legacy retries and is not presented as a single model run.
- **User-requested reruns:** recorded rerun markers provide separate groups, including when a stale/open attempt interval overlaps the rerun. Completed reruns and runner stop/recovery/start boundaries end that marker scope.
- **Restorations:** each explicit restore event remains a separate group, labelled as a workspace restore, rather than being attributed to an agent attempt. Existing restore records do not provide a shared multi-file operation ID, so this view does not invent one.

Branch ancestry/fork cutoffs apply to every journal row. Inherited groups retain their source branch; child writes are not merged into an ancestor's step group. Other sessions and excluded future history do not contribute rows.

## Combined diff semantics

For each path, a continuous group compares its first recorded pre-write body with its last recorded post-write body. Intermediate revisions are collapsed in that combined net diff, but all included journal event IDs and the write count remain listed. A continuous group with no net content change is labelled explicitly. Added/deleted files retain `/dev/null` headers.

If consecutive records for a path do not connect (`previous.after != next.before`), the reader flags a history gap or intervening changes outside the group. It then shows each exact recorded write separately instead of attributing a combined diff to the selected group. This matters when another step, a rerun or a manual change intervened between two writes from the selected step. Later visible journal changes are also labelled: an older group is not the latest recorded file state.

These diffs are historical journal evidence. Shell/manual changes outside the journal, current filesystem drift and changes after the captured cutoff are not covered. The reader performs no live disk validation and does not establish authorship beyond recorded attempt/rerun metadata.

## Authority and limits

Opening, navigating, toggling and exporting this view adds no workflow events, provider calls, tool permissions, review decisions or source writes. Already-applied native edits remain applied. There is no new staging engine, pre-apply group approval, group Keep, partial undo or atomic multi-file restore. Existing file/all-file Keep and guarded revert controls remain outside this reader with their original scope.

The implementation reuses journal blobs and branch history, not additional persistent file-body snapshots or a new journal schema. A lookup index on existing attempt columns supports event attribution. Existing history/blob/materialization limits bound capture; the smaller of materialization/export byte limits bounds a rendered group. Missing blobs, missing visible history or exhausted budgets produce explicit errors rather than an empty-success or partial combined diff.

## Validation

Tests cover multi-file attempt aggregation, distinct retries and step-wide grouping, intervening changes, user-rerun/restore/unattributed separation, inherited source branches and fork cutoffs, unrelated sessions, frozen snapshots, read-only history/filesystem behavior, empty/missing/over-budget evidence, real TUI actions and narrow/normal rendering. They also check full selected-group exports, idle-render caching, reopening to refresh, branch-change closure and permission-layer precedence. See [validation](06-validation.md).
