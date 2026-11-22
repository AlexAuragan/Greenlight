# Manager guidance delivery (GL-008)

Merged into `master` on 2026-11-22, together with GL-028/GL-029/GL-030. Conversation automatically accepts validated forwards through the host. Explicit Ask manager and CLI ask remain read-only; their proposals require acceptance.

## Controls

In Activity, select a manager forward proposal and use **[g] Deliver original**. The host revalidates its saved snapshot, original message, branch, workflow definition, target kind/state, project ownership and native recovery prerequisites.

- Current Ready/Blocked agent step: queue the original and start a fresh attempt through the normal engine. A second click never starts another attempt. Startup rechecks that this specific queued message still targets the current step; another worker consuming it or changing branches cannot cause an unrelated run.
- Unvisited agent step: queue for its next matching attempt on this branch. Do not start another step or skip an edge. Unreachable targets are not forced to run.
- Previously attempted target or completed session: open the guarded restart/restore preview. Explicit confirmation restores the reviewed checkpoint and atomically binds the original to the new branch. Revalidation rejects changed proposals, workflows or checkpoints. A failed branch publication starts no worker; any already-applied filesystem restoration is reported explicitly.
- Same-session live workflow owner advertising the inbox protocol: queue to its exact current attempt for the next coding request. An in-flight request, provider retry or tool action retains its prepared context. Other owners, interrupted ownership, unsupported workers and unresolved recovery remain unavailable for executing delivery.

**[G] Instruction delivery**, also in More, opens a current-branch status snapshot with originals, targets, attempt and first prepared-call references. Reopen to refresh it. Select a pending delivery event and use **[w] Withdraw instruction** before request inclusion, including while the owner runs. Inclusion cannot be retracted; stopping and explicitly starting a fresh attempt does not replay included guidance.

CLI equivalents:

```bash
greenlight ask SESSION --message 'Can you retry optimization with threads?'
greenlight manager-history SESSION
greenlight deliver-route SESSION MANAGER_RESULT_EVENT_ID
greenlight deliver-route SESSION MANAGER_RESULT_EVENT_ID --queue-only
greenlight routes SESSION
greenlight withdraw-route SESSION MESSAGE_ID
greenlight restart-route SESSION MESSAGE_ID --dry-run
greenlight restart-route SESSION MESSAGE_ID
greenlight continue-guidance SESSION
```

`--queue-only` prevents automatic startup of a current idle target. `routes` is inspection only. IDs come from recorded manager results/delivery evidence; neither command accepts replacement instruction text. Delivering a stale/invalid result fails visibly rather than guessing a destination.

Help and permission menus retain keyboard priority; `g` in a permission dialog keeps that dialog's existing meaning. It is never interpreted as a manager route there. Direct Step messages and deterministic commands remain available independently of manager inference.

## Scope, bytes and lifecycle

The host copies the saved `manager.message` original, not manager display prose. Guidance is inserted as a separate original user message, with a separate host metadata message identifying its message ID, target and attempt. Whitespace, Unicode and newlines survive JSON encoding byte for byte after decoding. It never becomes the run-wide `extra` prompt.

A `manager_routes` table projects immutable `manager.delivery` events. It records one row per message ID, session/branch, target, original, workflow digest, source decision, state, chosen attempt and first prepared call. Pending messages use FIFO acceptance/insertion order; equal text with distinct IDs is not deduplicated. The read path enforces original-message, aggregate byte and record limits.

`queued` means not yet included. The first matching attempt binds the reservation in the same transaction as attempt creation, before input/permission gates; a failed binding publishes neither. Outgoing coding-envelope persistence atomically records central usage registration, exact message references and `included_in_request` before contacting the provider. This means inclusion in a durable prepared request, not provider acceptance, tool execution, verification or task success. Each physical retry still has its own accounted call and exact trace; it does not append the instruction again.

If envelope persistence fails, roll back inclusion and retain a not-sent ledger record. An attempt ending before inclusion marks its bound pending guidance **Needs review**. A completed workflow also marks any unreachable remaining reservations for review. Fresh attempts require explicit withdrawal/review of unresolved guidance. Explicit saved-request retry may reuse the original attempt's exact prepared context and restore its inclusion, but cannot send withdrawn instructions. No automatic recovery/replay is added.

Branch selection/fork invalidates pending reservations rather than copying them. Workflow changes invalidate queued guidance before execution. Relevant input/artifact/approval evidence changing after manager inference requires a fresh proposal. Existing tool permissions, input pins, human gates and verification remain authoritative; a routed message cannot grant or answer approvals.

## Live boundaries and completion review

Live delivery binds the owner token, branch, workflow and attempt. Producers append inbox metadata without acquiring execution authority. The owner consumes queued originals FIFO only at a new coding-request boundary. Withdrawal and request registration serialize in immediate transactions: withdrawal first rebuilds an unsent fresh request; registration first makes withdrawal too late. Exact saved-request retries are never silently edited.

Closing a step atomically closes its inbox boundary. If an accepted instruction has not reached a coding request, it becomes **Needs review**. Successful completion remains recorded and automatic progression pauses in **Instruction review**. Select its delivery event for **[g] Review restart** or **[w] Withdraw instruction**. After withdrawing unresolved guidance, **[v] Continue after review** applies the recorded success transition without rerunning the completed step; starting the next step remains explicit. Deviations/errors retain their actual evidence and block progression for review. Late arrivals after closure require restart review rather than pretending another request will happen.

The inbox capability is removed with its execution owner. Recovery never silently replays accepted instructions. A prepared request remains evidence of inclusion, not proof of provider acceptance or exactly-once execution.

## Linked clarifications

Select a `manager.result` clarification in Activity and choose **Reply to clarification**. The composer identifies the reply; closing it preserves the draft/link for that session, branch and recipient. **Ask manager** starts an independent question. CLI: `greenlight ask SESSION --reply-to MANAGER_RESULT_EVENT --message 'Use threads'` (or `--file answer.txt`).

Each answer is a new immutable message. The saved thread retains earlier originals and manager questions; it never edits the initial request. Only visible clarification results are valid parents, and chains are capped at eight turns / 24 KiB in addition to the 12 KiB latest-message limit and total inference budget. A fresh snapshot re-evaluates current state; a clarification question never grants approval or resumes execution.

If the manager then proposes a forward, acceptance delivers each original and answer as a separate verbatim user message with host-labelled question context. The entire chain shares the route's target, owner/attempt, withdrawal, retry and restart lifecycle. It does not become a run-wide instruction or leak to later steps.

## Default Conversation routing

With `manager.enabled: true`, a fresh TUI composer defaults to **Conversation**. The visible recipient explains that the manager can answer, clarify or deliver the saved original to a workflow step. **Change recipient** cycles Conversation, read-only Manager and available direct Step delivery; Commands remains separate. More also exposes Conversation and Ask manager. An explicit recipient and existing drafts take precedence over defaults. With the setting disabled, fresh drafts retain the existing direct-step default when available.

A Conversation submission records `manager.routing-requested` before inference. A valid forward is accepted through the same branch/workflow/ownership checks as `[g] Deliver original`: current idle targets can start, future targets reserve their next visit, and supported live owners receive next-boundary guidance. Restart targets open the existing preview when the UI is available, and always require confirmation. A result arriving behind another popup/draft leaves a restart proposal for explicit review. Activity filters do not prevent routing.

Answers/clarifications open their recorded result when the UI is available. They do not dismiss another popup or replace a new draft. Clarification replies inherit Conversation only when their parent came from Conversation; replies to read-only questions remain read-only. All linked originals retain their own bytes.

Cancellation disables automatic delivery even if an in-flight manager response completes. A stale/invalid result, interrupted/unsupported owner or provider failure retains evidence and never falls back to direct execution. Switching session/branch before the reply arrives leaves a proposal on its original branch for explicit acceptance rather than starting background work. Closing Greenlight does not add automatic replay. Retrying an exact saved manager request is conservative/read-only: inspect and explicitly accept its forward.

`greenlight ask`, `/ask` in Commands, and **Ask manager** remain read-only. `/resume`, recovery, restart, permission decisions and human/input gates retain deterministic controls.
