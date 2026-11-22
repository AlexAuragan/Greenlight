# Project execution coordination (GL-007)

Status: implemented by GL-028 on `feat/project-execution-coordination`, awaiting merge (2026-11-22). GL-007 supplied the design. This is exclusive project execution, not GL-021 parallel orchestration.

## Problem and first policy

Before GL-028, Greenlight owned execution per session and serialized individual native writes across the project. Two different sessions could still run simultaneously. For example, A reads a source file and prepares a change; B changes the same file; A subsequently acts on its earlier interpretation. A successful per-write lock does not make A's conversation or verification current. This is possible across CLI processes, separate TUIs, or workers in the same process.

The implemented policy is **one executing workflow or workspace control operation per project**. Hold project ownership across model calls, tools, verification, step transitions and interactive waits. Reject competing execution immediately with the owning session and available actions. Browsing, exports and read-only manager questions remain available.

This deliberately serializes even workflows that currently request only read tools. Their transitions and later steps may execute work; inferring independence from today's tool choice would need a separate concurrency design. Different projects can run concurrently.

No automatic queue, lock stealing, timeout takeover, per-file scheduling, Git worktree provisioning or parallel execution is introduced. A competing Start/Resume/Retry must not silently begin later.

## Operations and boundaries

| Operation | Project ownership |
| --- | --- |
| Start/resume an executing workflow | Exclusive for the entire worker lifetime |
| Explicit historical tool rerun, including a read/shell rerun | Exclusive for the entire rerun |
| Retry a paused provider request in its live worker | Retain the existing ownership; no reacquisition |
| Resume a saved coding/verification/detector request | Acquire first; then validate checkpoint and workspace |
| Native writes within an owned workflow | Reuse project ownership; retain the short native-write lock |
| Apply a restore/revert or restart with restore | Exclusive from final validation through mutation, journal and branch publication |
| Recover a session or reconcile native write intents | Exclusive maintenance operation; see recovery below |
| Switch an execution branch, fork without restore, or otherwise change runnable workflow control | Exclusive short control operation, plus the target session lock |
| Explicit CLI artifact registration (`put-artifact`) | Exclusive short control operation; cannot replace inputs during live execution |
| Create an idle session / retain a new goal | Allowed; creation does not authorize execution |
| List/read history, artifacts, usage, diffs or prepare a restore preview | Allowed; preparation cannot authorize subsequent mutation |
| Read-only manager question or saved manager retry | Allowed under its existing manager lock; never receives execution authority |
| Approve a request from the owning live worker | Existing request identity and permission rules; project ownership is already held |
| Edit configuration or source in an external editor | Outside advisory coordination; existing drift and execution-boundary checks still apply |

A restart must not implement restore and branch publication as independently guarded public calls with an ownership gap. Use one outer control owner through both operations. Inner helpers receive/reuse that owner rather than reacquiring the same OS lock.

Session creation remains separate from automatic Start. If a new-session form creates a session but cannot claim project execution, show **Created; not started — project busy**, keep the entered goal in that session, and offer Open owning session / Start when available. Do not show Running or discard the user's text. If creation itself fails, retain the draft and show that error.

## Ownership mechanism and identity

Use a stable, descriptor-anchored `project-execution.lock` in the selected project's state directory. Reuse the GL-003 anchored filesystem and explicit-unlock guard. Never delete or replace a lock file to release it: two inode identities would permit two owners.

The scope is the selected project's existing state store, shared by its CLI/TUI clients. Canonical project selection and legacy state migration must resolve to that same store before ownership is attempted. Separate copies/checkouts with separate state stores are separate projects for this policy; shared files reached through other filesystem aliases and external tools are not made mutually exclusive by this lock. Replacing/moving state while clients are running is unsupported.

The OS lock proves live exclusion. A durable singleton project-owner row identifies interruption and supplies UI context. Persisted fields:

- Owner token, session ID, captured branch, operation kind (`workflow`, `tool-rerun`, `restore`, `branch-control`, `recover-session`, `recover-writes`) and PID for diagnostics.
- A singleton constraint and foreign key to the owning session. Neither PID nor a timestamp proves liveness.

Workflow/tool execution publishes the project-owner row, existing session execution-owner row, Running status and `runner.started` event in one transaction, after both OS locks and local preconditions succeed. Both owner records share a token. A losing claimant publishes no instruction, runner event, usage dispatch or changed status. A busy explanation is UI/CLI feedback, not fabricated agent history.

Maintenance claims use an operation-specific project record and appropriate audit evidence. A maintenance record left by a crash requires explicit recovery even if no runnable session became Running. Its recovery is not permission to finish a partially applied restore automatically.

Finish commits token-checked owner removal and existing runner outcome/state changes together before releasing guards. If cleanup cannot commit, report recovery required and leave durable evidence. If the lock is held but the row is temporarily absent during acquisition/cleanup, show **Project control operation in progress** and refresh; do not infer that execution is available.

A Rust owner object must retain the project and session guards for the complete operation. Use owned guards and a borrowed capability for inner mutation paths; do not depend on a boolean that any caller can set. Public mutation entrypoints must acquire ownership or require that capability. Do not add a second independent ownership mechanism to tools.

## Lock order and revalidation

Use this order whenever multiple resources are needed:

1. Project execution/control lock.
2. Target session lock(s), in stable session-ID order if several are needed.
3. Native-write lock, only around filesystem mutation/reconciliation.
4. Short SQLite transaction for publication/validation that needs it.

Never hold a SQLite transaction or native-write lock while trying to obtain a project/session owner. Never hold a SQLite transaction through a model request, user wait or subprocess execution. Long project ownership does not mean a long database transaction.

Acquisition is nonblocking for interactive execution/control actions. Failure returns a typed busy or interrupted-owner result; it is not a generic provider/tool failure and does not consume a model retry. Release any earlier guards when a later precondition fails.

Write recovery now uses the common order. It discovers pending sessions, claims project ownership and locks affected sessions in stable ID order before taking native-write, then reloads pending intents. Initial discovery is not authority; revalidation happens inside the protected scope. The project guard excludes cooperating changes during this preparation. Empty recovery also checks project and native-write availability.

After acquiring all required locks, reload session/branch/status, interrupted-owner records and pending writes before publishing execution. Final restore validation must also recheck history cutoff and current file snapshots. A preview is always provisional. Preserve existing exact-command approvals, project read boundaries, attempt/input pins, verification policies and native-journal uncertainty handling.

## Waiting, stopping and saved requests

An active approval, `wait_user`, human gate, provider backoff or Retry/Stop decision keeps project ownership. The model's retained conversation can still be based on the current workspace; another workflow must not change that workspace merely because the first is waiting.

The busy surface explains this explicitly and lets the user navigate to the owner. To work on another session, stop the owning worker using the existing stop behavior and wait for it to actually finish. `k` requests stop; it does not instantly free ownership, terminate every subprocess, undo edits or establish that no more in-flight output can arrive. Only worker cleanup/drop releases the live guard.

When a worker has stopped and retained a durable provider checkpoint, it releases ownership normally. Another session may then execute. Later saved-request retry must acquire project/session ownership **before** running existing workspace/config/input checks. If another run changed the workspace, the old prepared request remains unavailable for exact continuation and receives the existing explicit drift explanation; do not silently rebuild it, replay earlier tools or discard its history.

A blocked or completed idle session owns no live project execution lease. An interrupted durable owner is different from an ordinary blocked session.

## Crash, recovery and upgrade behavior

Process death releases OS locks, but does not erase durable ownership. A new claimant finding an interrupted project record or legacy interrupted execution must stop before provider dispatch or mutation, even when its own session is idle.

- If the OS project lock is unavailable, recovery refuses. Never use PID lookup, elapsed time or a user-visible stale status to force release.
- If it is available, recovery checks the stored operation/token and affected session under the common lock order, records explicit reconciliation and removes matching stale owner records atomically.
- Recovering an execution does not resume it, repeat a tool, replay an unanswered approval or change a completed session back into runnable work. Saved grants and checkpoints remain intact.
- Pending native write intents continue to block execution until the existing explicit write-recovery path reconciles them. Recovery preserves manual/conflicting files and partial-restore diagnostics.
- An interrupted maintenance record must identify the affected session(s) and operation. The recovery surface must handle records without a normal `execution_owners` row; it must not depend only on session Running status.
- Before first acquisition after upgrade, inspect existing session-owner records and legacy Running flags while holding the project lock. Refuse takeover of live legacy owners and require explicit recovery of interrupted ones. Inconsistent/multiple stale rows are a diagnostic, not a reason to pick one silently.

All simultaneously active clients must implement this protocol. An older binary never acquiring the new project lock cannot be made cooperative by a new binary's lock. Upgrade/stop older clients before relying on project exclusion. Schema initialization must be transactional and preserve old session/event data; no history rewrite or owner-row pruning on ordinary startup.

## User feedback

Show **Project busy** separately from the selected session's own status. Include the owning session goal, current step and whether it is running, awaiting approval/input, retrying or stopping when that evidence is available. If an observer cannot see the worker's UI-local prompt, say **Active in another worker; exact wait state unavailable** rather than inventing its state.

The workspace header shows project contention in amber, independently of the selected session status. **[O] Open owner** navigates to the current owning session and **[R] Refresh project** rechecks availability. Unknown/transient control owners offer Refresh without inventing an owner. Normal navigation closes the context; no modal decision is required. Stop is available only through an existing authenticated/local worker control handle; browsing another process's session does not create a remote kill mechanism. Do not offer a retry countdown, automatic start, Deny or a permission grant for a project-busy result. Preserve drafts and distinguish a saved idle goal from a successfully started run.

For interrupted ownership show Recover owner / Inspect history, followed by Recover writes when needed. Keep diagnostics accessible in the CLI and TUI. Normal readers and manager questions remain usable; manager answers retain their captured-snapshot qualifications and cannot claim atomic filesystem consistency while the worker writes.

## Implementation map (GL-028)

- `project_execution.rs`: typed busy/interrupted diagnostics, descriptor-relative project lock, singleton owner publication and token-checked cleanup, control scopes and native-write capabilities. The owning Store holds only a weak capability; an observer cannot reuse a live worker's lease. Legacy owner counts remain explicit and each interrupted session requires individual recovery.
- `session_execution.rs`: acquire project then session ownership; atomically publish/clear both owner records with execution state, instructions and runner evidence. Recovery handles interrupted maintenance even when no session is Running.
- `restore.rs`, `store.rs`, `write_journal.rs`: guarded native writes, stable recovery lock order, branch control and one shared lease through restore/fork publication. Failure still preserves partial-restore/native-intent evidence.
- `continuation.rs`: the saved coding request is validated inside owned execution, before resuming its attempt or dispatching. Live retries retain the original worker's guards.
- `tui_actions.rs` and TUI routing: preserve typed project failures across worker channels, retain unsent instruction bytes after a claim race, show idle created goals truthfully, navigate to the owner and refresh without queuing work. Help/approval layers retain input priority.

Maintenance scopes clear their owner row on an ordinary returned validation/action error, while preserving existing audit/intents. An unwind or process death leaves the durable row; a cleanup failure reports explicit recovery required. No source rollback is inferred from cleanup.

## Required validation

- Two sessions in one project, both in-process and separate CLI/TUI processes: exactly one claim, no loser instruction/status/branch/usage changes and no loser provider dispatch.
- Separate project roots: simultaneous execution succeeds. Separate stores for one selected root: exclusion still applies.
- Approval/input waits, provider retry/backoff and step transitions retain exclusion. Stop does not free it until the worker really exits.
- Restore/revert/branch/recovery cannot mutate under another session's live owner. A prepared preview is rechecked after ownership acquisition.
- Session/native-write/project locking cannot deadlock through reverse acquisition. Error/unwind and copied-descriptor cases release guards correctly.
- Kill the owner process before/after owner publication and during a journaled mutation. Recovery preserves completed sessions, original checkpoints and unanswered-approval expiration. Failed publication/cleanup leaves no false idle/success state.
- Another run after a saved request stops invalidates exact retry when workspace facts change. Earlier tools never rerun implicitly.
- Read-only browsers, exports and manager questions continue while a worker owns the project; they cannot acquire mutation authority through that exception.
- Legacy live/interrupted owners and inconsistent rows have explicit migration/recovery behavior. No implementation claims coordination with older uncooperative clients or external editors.

Automated coverage includes cross-session/process exclusion, retained waits, independent projects, atomic claim/cleanup failure injection, maintenance unwind, legacy/multiple-owner recovery, combined restore/fork, zero-dispatch rejection, retained goals/drafts and owner navigation. Test and native terminal evidence is recorded in [validation](06-validation.md). Advisory locks still do not coordinate old binaries or external editors.

## Refresh correctness (GL-029)

While validating ownership updates, the intermittent missing diagnostic was traced to tagging old activity rows with a newer database revision observed later in the same refresh. Refresh now captures its generation before history/derived reads and also includes the visible history cutoff in activity-cache identity. A concurrent commit forces a subsequent rebuild rather than permanently hiding the new blocker. A deterministic regression simulates the old interleaving; the original engine-backed test retains its diagnostic assertion.

## Observation/startup coordination (GL-030)

A status probe must briefly acquire `project-execution.lock` to distinguish an idle project from an interrupted durable owner. Previously, a worker racing that probe could fail with “project control operation in progress” even though no workflow was running.

`project-observation.lock` now serializes only probes and execution-lock acquisition. A probe holds this descriptor-anchored guard until its execution-lock inspection ends; a claimant holds it only while trying to acquire the existing execution lease. Claimants can wait for a short inspection, while a real execution lease still rejects competing work. Neither workflow execution nor model/tool/input/approval waits hold the observation guard. No timeout, PID heuristic, automatic retry of work or durable owner/schema change is introduced.

The order is observation guard → execution lock → release observation guard → session/native-write locks and execution transactions. A probe releases its execution lock before releasing its observation guard. Attempting observation or acquisition inside a SQLite transaction is rejected before waiting, preserving the existing lock-order rule. Both locks use stable anchored files and explicit unlocks, including inherited-descriptor handling; neither lock file is deleted. Older binaries still share the execution lease, but their probes do not participate in the new observation guard.

A cross-process regression freezes an idle probe, starts a separate claimant, verifies it remains unclaimed until inspection ends, then verifies that a second session still cannot claim against the actual worker. The existing TUI startup/approval test remains unchanged.
