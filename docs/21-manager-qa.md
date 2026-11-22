# Manager Q&A and routing proposals

**Implemented:** bounded context, strict manager decisions, cited answers, linked clarification replies, saved originals, independent TUI inference and central accounting. Asking alone executes no route. A forward is `forward_proposed` (or `needs_review` if state changed); the separate [idle delivery action](42-idle-manager-delivery.md) can accept it explicitly. Owner-scoped live queues and explicitly confirmed restart delivery are implemented; ordinary messages still require explicit route acceptance.

This implements the Q&A stage of the [coordinator contract](20-conversational-coordinator.md). It loads the text block in the [manager system prompt](prompts/manager-v1.md); the coding-agent YAML contract stays unchanged.

## CLI

```bash
greenlight manager-context SESSION_ID                    # preview sources/model; no inference
greenlight ask SESSION_ID --message "Where are we on the TODO?"
greenlight ask SESSION_ID --file question.txt              # original UTF-8, including whitespace
greenlight ask SESSION_ID --message "Explain this plan" --artifact plan.md
greenlight ask SESSION_ID --reply-to RESULT_EVENT --message "Use threads"
greenlight manager-history SESSION_ID                    # branch-visible exchanges, JSON lines
greenlight usage SESSION_ID --json                       # includes purpose: manager
```

`ask` is an explicit request to send context to the configured model, even without enabling the default TUI manager route. `--message` and `--file` are mutually exclusive; messages are nonempty and at most 12 KiB, without silent truncation. At most four `--artifact NAME` selections add their latest branch-visible version **at snapshot time**; each captured body pins its creation ID. They do not track later same-name updates. Use `manager-context` with the same selections to inspect what is shared. Manager history uses the active branch's visible ancestry and includes private original/source/output records, one JSON envelope per line; it is not public telemetry.

The manager reuses the existing provider. Optionally select a separate model ID on that same provider:

```yaml
manager:
  enabled: true         # suggest the manager recipient for a fresh TUI draft
  model: example-model # optional; defaults to provider.model
```

Default `enabled: false` suggests the step recipient when it can resume; explicit TUI recipient selection takes precedence. It does not prohibit explicit `ask` or `/ask`. Rates/tokenizers for the manager model use the same per-model metadata/discovery rules as coding calls. No second account is required. Configuration remains strict; a supplied manager model cannot be empty. Do not add this example model name unless it exists on your provider.

## TUI

- Ask manager explicitly sends a literal read-only question. Send & resume explicitly delivers literal step instructions, bypassing the manager route. In the separate Commands editor, `/ask QUESTION` also asks the manager; the prefix is removed and remaining text preserved. `/resume`, `/restart`, `/recover`, `/usage`, existing navigation and approvals keep their deterministic behavior.
- `/manager-context` opens a scrollable source/model preview and makes no inference request or history entry. This initial TUI does not implicitly send a selected artifact body; explicit artifact selection is available in the CLI.
- More → Manager replies (or `/manager` in Commands mode) filters the activity list to manager exchanges. Enter expands the answer and cited captured source text; Escape/Left returns as in normal readers. `F` then Escape clears the filter. Raw mode retains snapshot and model-output records.
- Stop manager retries in the active manager composer (or `/cancel-manager` in Commands mode) stops future manager requests/retries; an in-flight request may finish. Navigation `x` cancels the manager when no coding cancel handle takes precedence; when both are running, use `/cancel-manager` for the manager. Quitting waits for an active manager job to finish so completion/uncertain usage can be inspected.

Manager inference runs in a separate worker from coding execution, so a question can be answered while a coding owner is active. Only one manager request per TUI instance runs at once; a second draft stays intact. Approval menus retain priority, and their decision keys never become manager messages. Replies stay with the session/branch that received the question; inspecting another session does not retarget them. A bootstrap/configuration failure retains the foreground-saved original message. A failure during inference/validation records an unresolved result without running a fallback action.

## Context, evidence and privacy

The source preview identifies the actual engine transport origin and selected model, message/snapshot IDs, event cutoff, capture time, control fingerprint, prompt hash, valid agent destinations and source IDs. It omits URL userinfo/query strings and provider/security configuration. The same captured engine config is used for the real request even if the config file changes during inference.

Database evidence is read in one transaction; project/workflow files are independently captured observations, not an atomic filesystem snapshot. Evidence includes the workflow, recent saved attempt definition/input references, recent branch-visible outcomes/errors/verdicts, a native-recovery check and explicitly selected artifact versions. Unanswered approval prompts are UI-local and **unknown** to this snapshot; the manager must not invent their status. At most four prior manager exchanges supply context, labelled interpretations rather than execution facts.

The project-document allowlist is `README.md`, `TODO.md` and `.greenlight/instructions.md`. When a README roadmap/TODO heading lies beyond its initial preview, a separate `file:README.md#todo` source captures that section with its byte offset and captured-text digest; the search reads at most 128 KiB. Symlinked/unsafe/unreadable documents are omitted. The manager has no filesystem/tool API and cannot expand its own allowlist. `.env*`, provider/security files, credential files, runtime state and permission stores are not supplied by this collector. Selected workflow/artifact/document text is still sent to the chosen provider; it can itself contain sensitive material. Inspect the preview and select sources appropriately—this is not a comprehensive secret detector.

Sources and total input are bounded: at most 20 source entries, a 40 KiB snapshot including history, 64 KiB total input (system message and serialized user bundle), and UTF-8-safe source prefixes. Documents/artifact bodies normally cap at 6 KiB, a separate README roadmap/TODO excerpt at 12 KiB, and workflow text at 8 KiB. The last eight relevant recorded outcomes provide recent evidence, not a claim of complete historical coverage. Truncation/omissions are explicit. `sha256_captured_text` hashes the supplied prefix, **not** an unread full document. Display uses the existing terminal-safe bounded reader; raw manager history exports retain the captured payloads.

The runtime validates JSON types, action variants, version/message/snapshot IDs, content/citation bounds, supplied citations and agent-step destinations. No fences/prose/extra actions, unknown or duplicate fields, replacement instruction bodies or human/command-step targets are accepted. An `answer` requires at least one supplied citation; missing evidence can produce `clarify` instead. Valid citation membership does not prove the model's interpretation; verify important claims against sources.

Fresh session/branch/attempt/owner/workflow/recovery state is checked after inference. A changed fingerprint qualifies an answer as an older snapshot and marks a forward `needs_review`; unrelated manager/usage events do not invalidate every answer. All forwards remain unexecuted until the separate explicit idle-delivery action is accepted. A pure question never joins coding context, creates an attempt, grants approval, restores files or advances a step.

## Persistence, failures and costs

Append-only `manager.message`, `manager.snapshot`, `manager.output`, `manager.result` and `manager.failed` records are separate from executing `user.instruction` records. Manager originals/sources/results persist privately with session state until that state is deleted; they are not ephemeral side questions. Reopening a session never resends a saved question. A crash can leave a received message or started usage record without a result: inspect it before explicitly asking again.

Every actual provider dispatch uses the existing central ledger with `purpose: manager`, its selected model, reported/estimated input/cached-input/output and pricing snapshot. `manager_message_id` and `manager_snapshot_id` link each request/retry to the exact original question and evidence bundle, including overlapping calls from other interfaces. A manager question observes execution but is not assigned to a coding step attempt; reports group it under unassigned rather than implying the step executed it. Provider retries get separate call IDs and retain the conservative retry policy. Invalid JSON still costs a recorded call; there is no model repair loop or automatic coding fallback. Context preview, invalid local input and cancellation before dispatch do not invent billed requests. Costs remain estimates with existing unknown/stale qualifications.

Read-only manager records and their model usage may be appended while another worker owns execution. This exception does not give the manager the session execution lock or permission to publish an executing instruction. Future guarded delivery and durable owner-bound live queues must be implemented and tested separately.
