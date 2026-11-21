# User-requested tool reruns

Tool failures return to the model normally. Greenlight does not pause each failure or automatically replay operations.

## Use

1. Stop active execution with **[k] Stop** and wait for it to finish. Recover an interrupted owner or unfinished write if needed.
2. In Activity, select a tool result/error and optionally press Enter to inspect its exact recorded arguments and output. Raw request events also work.
3. Click **[R] Run tool again** or press uppercase **R**. This explicit action authorizes that exact call without another approval prompt. Side effects can repeat, including for a successful or interrupted original call.
4. The new result opens automatically in Activity when you remain in the same session/branch. Inspect it. The workflow remains stopped. Continue when ready; continuing starts a fresh attempt, rather than restoring the original model conversation position.

No permissions are saved by this action. Agent-issued calls retain normal permissions. Project/private-path protections, current workflow tool availability, shell isolation, size limits and native-write journaling still apply. A call outside the project's access boundary fails even when requested by the user. All supported tools can be selected; credentials and current environment are loaded when the rerun worker starts. Arguments come from the immutable recorded request, not from model reconstruction or editable UI text.

A rerun in a completed session leaves it blocked/awaiting review; the previous completion does not verify newly executed work. The current workflow step is unchanged, even if the selected call came from an earlier step. Continuing uses the current step.

## History and interruption

A new `tool.request` and `tool.rerun` link are committed together before execution. The link contains the original request ID, new request ID and `initiator: user`. The original failure/output remains unchanged. Results use normal `tool.result`/`tool.error` and `tool.finished` correlations; cards label the output **User rerun**. Native writes also appear in Edits. `tool.rerun-completed` means the execution's outcome was recorded, including when it failed; it is not a verification or workflow-completion event.

The rerun uses the normal execution-owner lock and persistent owner record. A live worker prevents it; a process crash requires recovery. A request without a correlated result has an unknown outcome, not a successful or safely repeatable one. Never automatically replay it. Cancelling a shell/network operation cannot establish that it had no partial effects. A synchronous native-write journal operation finishes safely before stopping; unfinished writes require reconciliation.

Reruns add no model request or token usage. The next attempt receives bounded evidence from up to eight recent branch-visible reruns: targets, request/outcome references and output excerpts, within a 16,000-byte budget. Full arguments/results remain in history and conversation dumps. This evidence does not satisfy artifact-output requirements, carry human approvals, establish current file contents or instruct the model to repeat the operation. Durable exact-message continuation is covered by [GL-002 provider recovery](38-provider-continuations.md).
