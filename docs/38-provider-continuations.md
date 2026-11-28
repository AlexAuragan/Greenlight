# Saved provider requests

A provider pause no longer depends on the worker or TUI remaining alive. Exact request messages and an execution frame are saved locally before inference; failures and outstanding cooldowns remain available after restarting.

## Retry or start fresh

- **[t] Continue saved work** restores a workflow's exact message list, model, step and attempt. Earlier tool calls and completed verification checks do not run again.
- **More → [T] Retry saved manager** restores the original side question, message/snapshot IDs and evidence snapshot. This is read-only; a proposed route is never executed.
- Ordinary resume instructions or `/resume` deliberately start a fresh attempt and replace the old workflow continuation. Earlier work may run again. They do not replace a pending manager request.

Nothing resumes automatically on launch. If an execution owner was abandoned, recover it first. Existing write reconciliation and session ownership guards still apply. Help and permission dialogs own input; retry shortcuts remain text in editors. The same controls can be clicked.

A successful provider response closes its checkpoint. A later provider call creates another one. A failure, Stop or process death leaves the outstanding request available. This is recovery of suspended provider work, not an arbitrary instruction-level snapshot of a process, running shell command, unfinished permission prompt or filesystem. If no checkpoint exists, the existing Continue step action starts fresh.

## Execution frames

Coding checkpoints retain the full prepared conversation. A verifier or detector called from a finish/deviation action also retains that final action and its original model-event identity. Restoring reprocesses only that control action; it does not replay historical tool actions or register their artifacts again. Human/command steps retain the completed step result before verification. Outer deviation detection retains its original outcome without counting the deviation again.

Verification checkpoints include the ordered results of completed leaf checks. These are reused only within the unchanged original attempt/workspace; an unfinished check resumes its pending model call. No unanswered human/tool decision is saved or manufactured. Existing successful approvals remain audited records; normal permissions apply to any new tool work. Process-scoped grants are not reconstructed after a restart.

Manager checkpoints preserve the original evidence snapshot rather than silently building a new question against new state. Independent per-session manager locks prevent simultaneous recovery/new inference from competing for the same manager continuation. Manager questions remain independent of the coding execution owner.

## Credentials, settings and drift

A newly opened worker loads credentials using the normal environment/dotenv precedence. API-key values are not part of the checkpoint. Updating `.env` and restarting can therefore repair credentials while retaining the original messages. An already-running worker continues using its captured credentials.

Before retrying, Greenlight verifies checkpoint identity, session/branch/current step, the latest recorded attempt, configuration/workflow definitions, compiled prompts, input/approval/edit evidence and project file hashes. It checks again after claiming ownership, after restored cooldowns, and before manual or automatic retries. It also re-reads the current session so an old in-memory session snapshot cannot authorize a different step or branch. Changed project files, changed configuration, superseded attempts/checkpoints, missing/corrupt blobs and unfinished writes refuse exact retry. The saved request is retained; review the change and start a fresh attempt when appropriate. Provider/model/max-token settings are not silently replaced. Pricing and usage remain snapshots of each physical dispatch.

Workspace validation uses the existing bounded manifest scope: state, Git internals, credentials and generated/cache directories are excluded; symlinks are not traversed. Scans are not atomic against concurrent filesystem mutation. If the manifest reports omitted files, exact recovery refuses rather than assuming unchanged contents. Descriptor anchoring is covered by GL-003. Project-wide execution is serialized by the [GL-028 project lease](41-project-execution-coordination.md). A saved request acquires project/session ownership before its execution-time workspace validation; an idle saved checkpoint does not reserve the project.

## Persistence, cooldown and accounting

Version-1 and version-2 checkpoint blobs use the private content-addressed trace store with checksum validation. Branch-visible `continuation.state` events reference the newest saved blob or explicitly close that slot. Workflow and manager slots are independent. The payload includes exact messages, model/purpose, frame, completed checks, attempt and interaction identity, a validation fingerprint and optional cooldown deadline. Keys and unanswered approvals are excluded. Routine checkpoint metadata is hidden from default Activity cards. Active saved cooldowns have a visible waiting card. Malformed metadata disables the affected retry and reports an error while preserving the TUI and raw history; a readable independent slot remains usable.

Every physical retry uses a new usage record and exact conversation trace. A saved checkpoint is not a billed dispatch. Remaining Retry-After is persisted at each physical rejection, including before automatic backoff, and is not shortened by restarting. Cancellation of a local HTTP request cannot prove that remote processing/billing stopped; an explicit retry can therefore incur additional billing. Known returned usage and earlier tool results remain in history.

Successful recovery records `attempt.continued` and reopens the same attempt. Completed checks and tool effects remain historical evidence, and newly executed work still goes through ordinary verification. No full conversation compaction or general tool replay is introduced.

Regression coverage includes an actual child-process restart with updated dotenv credentials, exact request comparison and proof that earlier tools and the original attempt are retained. Files changed during restored cooldowns, retry prompts or automatic backoff prevent dispatch and retain the checkpoint.

## Stop at coding boundaries (GL-019)

`k` stops active local inference, tool execution/approval waits and retry waits; it does not undo applied edits or establish that remote billing stopped. After a tool returns with confirmed cleanup, stopping saves its recorded outcome and the next coding conversation. Continuing uses that outcome without repeating the tool or reopening its unanswered approval. A new model-requested tool call still requires the normal permissions. An uncertain native write or unconfirmed sandbox cleanup cannot become a resumable boundary.

If a response completed just before Stop but its action was not executed, its exact response and model-event identity are retained. Explicit continuation processes that action once under normal input/permission guards, then sends the updated conversation on the next request. Retained unexecuted coding responses use checkpoint version 2 so older binaries refuse them rather than apply obsolete replay semantics. Ordinary request checkpoints retain version 1. The saved response slot is closed before processing its action; a crash inside an action does not offer blind automatic replay. A checkpoint is not an instruction-level snapshot of an in-progress tool.

The visible `[t] Continue saved work` control handles both cases. Workspace/config/attempt validation, explicit owner recovery and refusal on drift still apply. A cancelled approval remains unanswered; none is saved as an affirmative decision. Full process restarts never automatically resume execution.

To queue instructions during execution, use the existing Conversation routing/accepted guidance inbox described in [manager delivery](42-idle-manager-delivery.md). FIFO guidance is consumed only at a new coding-request boundary. An in-flight or saved exact request remains frozen; pending live-owner guidance is not silently rebound to another owner/attempt after a stop or restart. Inspect retained delivery state and explicitly submit/accept a fresh route when needed. Direct Step sends while running still retain the draft and refuse a competing worker. This work adds no concurrent executor, automatic tool retry or arbitrary command replay.
