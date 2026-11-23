# Historical restarts and branch comparison (GL-010)

Merged into `master` on 2026-11-23 alongside GL-009.

## Choose a recorded run

The TUI's Restart review lists **every visible run** of each workflow step, including retries, rather than only its first execution. Each row identifies the step, its run number within visible history, and the exact checkpoint event before that execution. The latest visible run of the current step is selected by default. Arrows select; Enter opens the existing guarded restore preview; Esc cancels.

The checkpoint is the preceding visible event, not an event ID obtained by subtracting one: global IDs can include other sessions. Branch ancestry and its cutoffs determine visibility. Inherited history beyond a fork point never supplies restart candidates. Legacy `step.started` records remain usable even without a durable attempt row. The ordinary picker does not retarget manager-proposed restarts; those retain their reviewed canonical target/binding policy.

The preview includes the selected run/checkpoint, per-file native restore diffs, and live hash differences since the most recent visible workspace scan. These live differences flag files created, removed or modified outside that scan, including possible shell/manual changes; they are not claims about who changed a file or Git's tracking status. Excluded and omitted paths stay outside comparison coverage. If a scan is unavailable, the preview states that the changes cannot be assessed.

Confirmation uses the existing project/session ownership, history-cutoff and native-preimage checks. It restores only supported journaled files, forks at the chosen checkpoint and leaves the session Ready. It does not start a model request; resume explicitly after reviewing the result. Unjournaled/manual files are not reconstructed or silently deleted. Drift on a journaled target can still refuse restoration, protecting later manual work. A partial restore/fork failure remains explicit.

## Compare recorded branches

**More → Compare branches** opens a read-only comparison of the selected branch with the session's other branches. Each pair identifies both branch IDs, parent/fork checkpoints, visible history cutoffs and workspace scan events. It lists created/removed/modified paths using saved hashes, or labels coverage changes. Missing scans are unavailable evidence, never proof that two branches match.

This compares recorded snapshots rather than changing branches, reading old file bodies or promising that disk currently matches either branch. Full native content diffs remain in the restart preview and Edits reader; hash-only workspace history cannot recreate shell/manual files. Other sessions' branches are rejected. Comparison output obeys the configured materialization budget.

CLI equivalents:

```bash
greenlight restart-points SESSION_ID
greenlight compare-branches SESSION_ID --left BRANCH_ID --right BRANCH_ID
```

`restart-points` lists the exact `--event` event and target step for each visible run. The existing `fork` command accepts an arbitrary visible checkpoint and provides its normal restore/confirmation controls; use `greenlight fork --help` for syntax. Neither inspection command switches branches, restores files or executes tools. Stop a running session before opening its restart choices.

Validation covers retries and canonical manager defaults, inherited history cutoffs, selected-run restore/fork, preservation of a manual file, captured branch hash differences, missing scans and cross-session rejection. No provider requests are required.
