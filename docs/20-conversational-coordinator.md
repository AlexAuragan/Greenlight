# Conversational coordinator — routing contract v1

**Status: implemented on `feat/project-execution-coordination`, pending merge.** [Manager Q&A](21-manager-qa.md) supplies grounded replies. [Guidance delivery](42-idle-manager-delivery.md) covers default Conversation routing, explicit read-only questions, linked clarifications, attempt-scoped live delivery and guarded restart confirmation. `manager.enabled: true` selects Conversation for fresh TUI drafts; Ask manager and CLI `ask` remain read-only. Coding-agent YAML, permissions and execution ownership remain enforced by Rust.

## Product behavior

Ordinary session messages go through a manager by default when this feature is enabled. The manager answers project/pipeline questions, selects a workflow step to receive an instruction, or asks a focused clarification. The UI shows the decision and recipient. Existing deterministic commands, permission decisions, recovery and restart controls keep their existing routes; an approval answer is never sent to the manager for interpretation. Users can still operate the harness without manager inference when the feature is disabled or unavailable.

Examples:

| User message | Manager decision | Harness behavior |
| --- | --- | --- |
| “Where are we on the TODO?” | Answer using the selected TODO and actual pipeline evidence | Display an evidence-linked answer without resuming or steering the coding agent |
| “Can you retry the optimization but with multithreading instead?” | Forward to the workflow's actual optimization step ID | Deliver the exact original message at a valid boundary, or propose the existing guarded retry/restart flow |
| “Why did verification fail?” | Answer from the recorded check/error and aggregate verdict | Distinguish failed checks, model reports and policy verdicts; no automatic retry |
| “Do it differently” with several plausible targets | Clarify | Ask which step/change is intended; keep the original message pending |
| “Why did the benchmark regress, and retry optimization with threads” | Forward if the explicit instruction has one clear target | Preserve the whole message, including its question; show a short routing explanation. A separate grounded answer can be requested afterward |

Question grammar alone is insufficient: “Can you retry…” requests action. A question about a failure or suggestion alone does not authorize retry. A clear step name/description wins over an assumption that every instruction belongs to the current step. Match the supplied workflow, not a hard-coded list of names. Unknown or ambiguous targets require clarification.

The default manager shares the existing provider account. It may share the coding provider/model. Explicit Step and Commands recipients remain available; Conversation never silently bypasses the manager based on a heuristic.

## Original message and scope

The host saves the original UTF-8 message **before** inference, assigning an immutable `message_id`, session and branch. Preserve spelling, punctuation, Unicode and newlines. A clarification is a new linked message; it does not edit the original. Define the message at the submit boundary before any current composer `.trim()` transformation.

The manager returns a reference and `step_id`, **not a rewritten instruction**. The host loads the saved original text for delivery. The manager's display text never substitutes for the user's text. Transport encoding can escape JSON, but the decoded coding-message content must equal the saved text byte for byte. Metadata such as message ID and destination is supplied separately.

A route is scoped to one session, branch, target step and chosen attempt. A running target binds to that exact attempt/owner. A guarded restart binds to the newly created attempt only after the user accepts its existing preview/confirmation. A not-yet-started target gets one explicit “next visit on this branch” reservation, bound to the first matching attempt when it starts. Changing branch/workflow definition invalidates the reservation. Completed/blocked targets are never mistaken for an unvisited step merely because no worker is currently executing them.

Delivery does not become the existing run-wide `extra` argument: today that argument is reused across step runs in `Engine::run_owned`. Manager-scoped instructions must not leak to later steps, branches or retries. Registered artifacts remain the normal way for a step's results to inform downstream steps. A further retry/loop does not silently replay an already-delivered user message; reuse requires a new explicit routing decision.

## Evidence supplied to the manager

Build a bounded, immutable evidence bundle with a `snapshot_id`, capture time, session/branch, visible-event cutoff and a control fingerprint. Capture database evidence in one read transaction. Record exactly which sources were included, missing or truncated. The manager sees that bundle, never an unbounded live history or unrestricted filesystem tools.

Include:

- Goal and selected workflow; valid step IDs, kinds, objectives, success/deviation edges and required inputs/outputs. Include the saved active-attempt definition and identify any newer on-disk definition separately.
- Session/owner state, advertised steering capability, current step/attempt and per-step recorded outcomes, including retries and verification verdicts. Distinguish “not run”, interrupted and failed; do not infer success from prose.
- Recent relevant errors, safe provider-failure categories, linked tool outcomes, artifact version/provenance references, pending human gates, usage summaries and workspace-audit qualifications. Do not include API keys or permission-store contents. Approval status is evidence, not a decision the manager may answer.
- Selected project overview/instructions and an identified project TODO or planning artifact. “TODO progress” and “workflow progress” are separate: if no reliable mapping exists, say so. Do not mark checklist work done solely because a pipeline stage completed.
- A small, explicit window of prior manager exchanges for references such as “retry that”. Manager answers and activity titles are interpretations, not new execution evidence.

Initial implementation bounds: 12 KiB per original message, 64 KiB total manager input bundle including message and history, at most 20 evidence entries, and at most four previous exchanges within 16 KiB of that total. Trim at UTF-8 boundaries and disclose omitted sources; reject an oversized original message visibly instead of silently truncating it. Use the existing supported tokenizer estimates for accounting, not to invent missing context. These are proposed defaults, not new accepted config keys.

Project files are captured through an allowlist of selected project documents, selected workflow definitions and explicitly selected artifacts; code can be supplied as a bounded read-only excerpt when needed. Apply existing checked-root/sensitive-path restrictions. Do not ingest `.env*`, credential files, `.git`, runtime state, approval stores or provider/security configuration. A root `greenlight.yaml` is not manager context. Secret filtering is heuristic; show the actual source list and the selected provider before enabling the feature. Files and database state are not one atomic snapshot: file evidence includes captured bytes/digest/time and is labelled separately. Artifact references pin creation event IDs, not latest-by-name lookups.

Every evidence entry receives a host-generated `source_id` and provenance. Responses cite only IDs in that bundle. Citation membership and visibility are machine-checked; that cannot prove the model's interpretation is correct. Display the source list and snapshot time. A status answer stays qualified “as of” its snapshot if execution progressed during inference.

## Manager output contract

This is a **new JSON interface**, separate from the coding agent's YAML protocol. Require one object, no fence/preamble, no unknown/duplicate keys and no coercion of field types. Use a tagged action union in Rust. Common required keys:

| Key | Type | Validation |
| --- | --- | --- |
| `schema_version` | integer | Exactly `1` |
| `message_id` | string | Exact ID of this saved user message |
| `snapshot_id` | string | Exact ID of the supplied evidence bundle |
| `action` | string | `answer`, `forward` or `clarify` |
| `content` | string | Nonempty display content; at most 4,000 Unicode scalar values for answer, 500 otherwise |
| `citations` | array of strings | At most ten unique IDs, all in the supplied evidence bundle |

`forward` additionally requires `step_id`, exactly one supplied valid agent-step ID. `step_id` is forbidden on `answer` and `clarify`. Command/human steps are valid evidence/context but are not destinations for coding guidance; clarify or explain their deterministic controls. No output variant accepts an instruction body, arbitrary tool call, approval response, checkpoint, branch switch or automatic transition command.

Escape terminal/control characters through the existing safe reader; reject control characters outside permitted text whitespace during schema validation. Apply the existing bounded Markdown/literal rendering policy. An answer's factual claims need citations; if the snapshot lacks evidence, acknowledge that gap and ask for the missing context rather than fabricate progress. A clarification need not cite a missing source.

Example route for a hypothetical snapshot whose actual step list contains `optimize`:

```json
{"schema_version":1,"message_id":"msg-17","snapshot_id":"snap-42","action":"forward","step_id":"optimize","content":"Route your original instruction to optimize.","citations":["workflow:optimize-performance"]}
```

Output validation includes the IDs/branch/step allowlist and limits, not just syntactic JSON parsing. Invalid output or provider failure is visible and leaves the original message unresolved. Do not guess a route or fall back to automatic execution. Keep inference retries within the existing conservative provider policy; do not add an unbounded “repair the JSON” loop. Deterministic navigation/recovery and the original message remain available.

## Delivery and control matrix

The manager selects a destination; Rust derives the delivery mode from fresh execution state. Recheck the branch, workflow-definition digest, target kind/state, active attempt, owner token/capability and pending recovery before enqueueing or applying a route. A changed control fingerprint means re-evaluate or clarify visibly, not silently retarget. Unrelated events such as the manager's own usage record must not make every route stale.

| Fresh target state | Delivery behavior |
| --- | --- |
| Same running agent attempt, supported live owner | Queue for its next **coding** model call. An in-flight request/tool action may complete first; it is not modified or automatically cancelled |
| Running attempt awaiting a tool/human approval | Queue separately; the original approval menu retains priority and its normal decision keys. Guidance never grants or answers it |
| Current ready agent step, or blocked current step with no recovery prerequisite | Show destination and use the existing engine resume path; bind the message to the new attempt. Preserve normal tool/input/verification checks |
| Completed/previously attempted non-current target | Propose existing retry/restart with exact target and normal checkpoint/restore preview. Apply no transition/restore until its existing confirmation succeeds |
| Valid agent target not yet visited | Show “Queued for next visit to STEP on BRANCH”; deliver only on its first matching attempt. Do not skip graph edges or force reachability |
| Active owner without the steering protocol | Explain that live delivery is unavailable and retain the unresolved instruction; do not claim it was queued to a worker that cannot consume it |
| Interrupted owner or pending native-write recovery | Explain required recovery and retain message; never replay expired approvals or automatically recover/resume |
| Target missing, branch changed, incompatible workflow change or ambiguous target | Mark unresolved/needs review; no default destination or cross-branch replay |

For the last in-flight response, a queued message may arrive before a finish, deviation, turn exhaustion, cancellation or error ends the attempt without another coding call. Do not discard it or append it to the following step. Finish recording the actual outcome, then hold automatic progression with a separate **pending-instruction review** state. The UI says “Step finished/stopped; instruction was not delivered” and offers the existing guarded retry flow or explicit dismissal, subject to recovery prerequisites. Keep a completed step/verified result intact; this hold is not a fabricated verification failure. Configured retry/goto also respects the hold rather than silently carrying guidance to another attempt.

A queued instruction does not itself trigger an extra model call, change execution policy or require a new summarizer. Multiple messages for the same bound attempt are delivered FIFO as separate original user messages. Do not rewrite, concatenate, deduplicate by text or silently drop contradictory instructions. Before inclusion, a user can withdraw a queued message. Once a request has started, show withdrawal as too late to retract that request; any correction is a new message.

## Ownership, persistence and honest acknowledgement

This needs a durable control inbox, not only TUI-local channels. CLI and TUI workers advertise a steering-protocol version and poll the same session/branch/attempt inbox at coding-call boundaries. A legacy/external owner without that capability remains observation-only for forwarding, while manager Q&A still works.

Observers may append immutable manager exchanges and pending routing proposals to separate control records; that does not grant the session execution lock or publish an executing `user.instruction`. Only the live lock owner may claim delivery and change its active conversation or workflow state. Implement atomic FIFO claiming keyed by owner token, attempt and message ID, with explicit withdrawal/stale state handling. The current execution-lock rule must be narrowed/documented for these non-executing control records; do not weaken it for actual execution instructions.

Persist the prepared coding-request envelope's guidance references together with its durable model-call registration before network dispatch. Deliver the saved text once into that conversation, and record every dispatched retry's call ID separately through central usage accounting. Provider retries reuse the prepared context instead of appending the same message again. “Included in request” means inclusion in a persisted outgoing envelope, **not** proof the provider accepted it or the requested work was done. A completed response/usage record can qualify acceptance; only execution/verification records establish work outcomes.

Suggested lifecycle: `received` → `routing` → `answered` / `clarification_needed` / `queued` / `restart_proposed` / `unavailable`; queued messages can become `included_in_request`, `withdrawn`, `needs_review` or `delivery_uncertain`. Store state changes as append-only references or transactional projections backed by immutable records. Preserve message/snapshot/decision/source IDs, route target, owner/attempt binding, delivery/request IDs and timestamp; do not persist credentials or permission answers in manager exchanges.

A process exit between registration and response leaves delivery/billing uncertain. Recovery must not automatically replay guidance against a reconstructed conversation, repeat tools, grant approval or resume work. Keep the original message and request evidence available, mark uncertainty, and use normal explicit recovery/retry. Idempotency prevents duplicate context insertion; it cannot guarantee exactly-once provider execution across a crash. No promise of fully resumable in-flight conversations is added here.

## Manager history and usage

Manager questions and answers are a separate history lane with visible routing labels; a pure Q&A never joins the coding conversation. A snapshot includes only a bounded window of manager history, while the full original exchanges are retained privately with session state until the user deletes that state. This matches existing local session retention; do not advertise ephemeral/private side questions without a separate deletion policy. Reopening a session does not rerun unanswered manager calls automatically.

Use the central per-dispatch ledger with purpose `manager`, including failures, invalid routing output, retries and cancelled/uncertain responses. Capture selected provider/model, count input/cached-input/output, apply the existing pricing snapshot/unknown-rate rules and expose the calls in `/usage`/CLI. Reading a question to route it is a paid request when the configured provider charges for it; do not call the manager gratuitously for deterministic commands/approval keys. No second provider subscription, mandatory summarizer or new cost-budget system is needed.

## Runtime acceptance and implementation order

Q&A, idle delivery, restart binding and live inbox boundaries have deterministic coverage; the following remains the full acceptance target. This is not a claim that the whole contract exists today:

1. Store/deserialize the tagged response union and validate limits, unknown/duplicate fields, IDs, target/citation membership, Unicode and hostile source contents. The manager cannot emit agent YAML, grants, restore commands or fabricated destinations.
2. Capture a consistent database snapshot plus qualified immutable file/artifact evidence. Test TODO-vs-pipeline distinctions, missing sources, mixed intents, stale control state and completed-verification/model-claim distinctions.
3. Add manager Q&A and visible routing as a separate history lane; scripted-provider tests assert no coding context/tool/transition mutation and complete usage accounting on every response/failure.
4. Add targeted idle-step delivery and guarded retry/restart proposals. Assert byte-for-byte original message equality and no run-wide `extra` leakage, automatic restore or approval bypass.
5. Add the durable live-owner inbox, transactional request-envelope references and FIFO delivery at next coding calls. Test same text submitted twice as distinct IDs; retries without duplicate insertion; withdrawal races; unsupported owners; turn exhaustion/finish/deviation holds; branch/workflow changes; crash windows and explicit recovery.
6. Exercise TUI input during routing and running execution, approval priority, visible destinations/acknowledgements, inspection/resize and manager-disabled operation. All current sandbox, restoration, input-approval, ownership and usage regressions must continue to pass.

Treat this as incremental implementation: ship grounded Q&A with no executing route first, then idle guarded routing, then live delivery. Until a stage exists, the interface must label it unavailable; it must not imply the full specification is already supported. Evaluate the [candidate prompt](prompts/manager-v1.md) against the examples above using deterministic fixtures before any optional live provider evaluation.
