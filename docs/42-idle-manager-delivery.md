# Explicit idle manager delivery (GL-008, first delivery slice)

Implemented on `feat/project-execution-coordination`, together with GL-028/GL-029, pending merge. This extends manager Q&A with explicit acceptance of a validated forward. Asking a question or receiving a forward alone still executes nothing.

## Controls

In Activity, select a manager forward proposal and use **[g] Deliver original**. The host revalidates its saved snapshot, original message, branch, workflow definition, target kind/state, project ownership and native recovery prerequisites.

- Current Ready/Blocked agent step: queue the original and start a fresh attempt through the normal engine. A second click never starts another attempt. Startup rechecks that this specific queued message still targets the current step; another worker consuming it or changing branches cannot cause an unrelated run.
- Unvisited agent step: queue for its next matching attempt on this branch. Do not start another step or skip an edge. Unreachable targets are not forced to run.
- Previously attempted non-current target or completed session: record **Restart proposed**. Nothing is restored, jumped to or resumed. The existing restart preview/confirmation remains separate; after changing branch, obtain a fresh routing decision. Binding a proposal through confirmed restart is a later GL-008 slice.
- Running/interrupted project owner or unresolved native writes: refuse idle delivery and retain the original proposal. Live delivery is not implemented.

**[G] Instruction delivery**, also in More, opens a current-branch status snapshot with originals, targets, attempt and first prepared-call references. Reopen to refresh it. Select a pending delivery event and use **[w] Withdraw instruction** while idle. Inclusion cannot be retracted; stopping and explicitly starting a fresh attempt does not replay included guidance.

CLI equivalents:

```bash
greenlight ask SESSION --message 'Can you retry optimization with threads?'
greenlight manager-history SESSION
greenlight deliver-route SESSION MANAGER_RESULT_EVENT_ID
greenlight deliver-route SESSION MANAGER_RESULT_EVENT_ID --queue-only
greenlight routes SESSION
greenlight withdraw-route SESSION MESSAGE_ID
```

`--queue-only` prevents automatic startup of a current idle target. `routes` is inspection only. IDs come from recorded manager results/delivery evidence; neither command accepts replacement instruction text. Delivering a stale/invalid result fails visibly rather than guessing a destination.

Help and permission menus retain keyboard priority; `g` in a permission dialog keeps that dialog's existing meaning. It is never interpreted as a manager route there. Direct Step messages and deterministic commands remain available independently of manager inference.

## Scope, bytes and lifecycle

The host copies the saved `manager.message` original, not manager display prose. Guidance is inserted as a separate original user message, with a separate host metadata message identifying its message ID, target and attempt. Whitespace, Unicode and newlines survive JSON encoding byte for byte after decoding. It never becomes the run-wide `extra` prompt.

A `manager_routes` table projects immutable `manager.delivery` events. It records one row per message ID, session/branch, target, original, workflow digest, source decision, state, chosen attempt and first prepared call. Pending messages use FIFO acceptance/insertion order; equal text with distinct IDs is not deduplicated. The read path enforces original-message, aggregate byte and record limits.

`queued` means not yet included. The first matching attempt binds the reservation in the same transaction as attempt creation, before input/permission gates; a failed binding publishes neither. Outgoing coding-envelope persistence atomically records central usage registration, exact message references and `included_in_request` before contacting the provider. This means inclusion in a durable prepared request, not provider acceptance, tool execution, verification or task success. Each physical retry still has its own accounted call and exact trace; it does not append the instruction again.

If envelope persistence fails, roll back inclusion and retain a not-sent ledger record. An attempt ending before inclusion marks its bound pending guidance **Needs review**. A completed workflow also marks any unreachable remaining reservations for review. Fresh attempts require explicit withdrawal/review of unresolved guidance. Explicit saved-request retry may reuse the original attempt's exact prepared context and restore its inclusion, but cannot send withdrawn instructions. No automatic recovery/replay is added.

Branch selection/fork invalidates pending reservations rather than copying them. Workflow changes invalidate queued guidance before execution. Relevant input/artifact/approval evidence changing after manager inference requires a fresh proposal. Existing tool permissions, input pins, human gates and verification remain authoritative; a routed message cannot grant or answer approvals.

## Remaining GL-008 work

- Bind an original proposal through explicitly confirmed retry/goto/restart and the resulting branch/attempt, including partial restore/fork failure handling.
- Durable live-owner inbox, advertised steering capability, withdrawal/inclusion races, FIFO live coding-boundary consumption and pending-instruction review holds when an attempt ends before another request.
- Linked clarification delivery and the proposed default ordinary-message coordinator flow. This increment retains explicit Manager/Step recipients and explicit route acceptance.

The complete [coordinator contract](20-conversational-coordinator.md) remains the target. A durable idle reservation is not a live inbox, and a Restart proposed record does not implement executing restart guidance.
