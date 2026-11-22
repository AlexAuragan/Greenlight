# Optional evidence-linked summaries (GL-009)

Implemented on `feat/optional-summaries`, pending merge. This is an on-demand background reader for long-running steps and completed workflows. Nothing schedules summaries automatically, and workflow execution never waits for one.

## Use

In the TUI, **More → Generate summary (uses model)** starts a read-only request. The result opens when the current interface is free. Another approval, reader or new draft stays open; **More → Latest summary** opens the saved result later without inference. **Stop summary** cancels the background request; an already dispatched request can have uncertain billing. The workflow's execution/approval controls remain independent.

The manager and summarizer share the TUI's single background-request slot. A second background request is refused visibly; wait or stop the first. The coding worker runs independently. Quitting waits for the background slot to finish, preserving existing completion/accounting behavior.

CLI:

```bash
greenlight summarize SESSION_ID  # generates a summary; provider usage is recorded
greenlight summary SESSION_ID    # reads the latest branch-visible saved summary
```

CLI generation waits for its own summary request, never starts/resumes the workflow, and needs no project execution lease. Concurrent summary generation for the same session is excluded across processes by a descriptor-anchored advisory lock.

Optional model selection uses the existing provider/account:

```yaml
summarizer:
  model: your-inexpensive-model
```

When omitted, the coding provider model is used, independently of `manager.model`. No model is assumed to be inexpensive: the normal model metadata, historical pricing snapshots and unknown-cost rules apply. The central ledger records every physical dispatch/retry with purpose `summarizer`, including failed provider responses and rejected summary output. Input, cached input and output retain their existing count-source rules. Viewing a saved summary incurs no request.

## Evidence and authority

The request reuses the manager's bounded, allowlisted evidence capture: project documents, workflow definition, recorded pipeline/verification/artifact information and recent relevant events. It does not load the full conversation, read arbitrary source files, grant tool access or automatically include every artifact body. Missing/truncated evidence stays labelled. The summary input is capped at 64 KiB including its dedicated system prompt; current manager snapshot/source limits also apply. Credential/private path exclusions remain those documented in [Manager Q&A](21-manager-qa.md).

Saved records identify the exact source bodies, hashes, snapshot time, branch, step and event cutoff. The model returns only nonempty summary text (at most 2,400 Unicode characters) and 1–10 distinct supplied source IDs. Extra fields, invalid JSON, invalid text controls and missing/duplicate/unknown citations are rejected and recorded. Citation validation establishes reference identity; it does not prove the model's interpretation correct.

The reader labels the result **Model-authored summary**, with its captured time and event cutoff, and includes cited captured source excerpts. It is a snapshot rather than a live progress indicator. It cannot establish success, edits, measurements or verification independently of its cited records. It never replaces the workflow's recorded result or becomes an instruction, artifact, permission decision or verification verdict.

A result stays on the branch captured at startup. Switching session/branch leaves it in that original history; it does not retarget the request. Fork ancestry controls whether an old summary is visible. Generation failures leave execution state intact. Summaries have no executable continuation; their retries must not consume, replace or validate a paused coding/manager request.

## Comparison with activity titles

| | Coding-agent activity titles | Optional summary |
| --- | --- | --- |
| Purpose | Short intent label for one action | Explain recorded progress, blockers and limitations |
| Additional requests | None | One logical request, plus any provider retries |
| Availability | With the coding response | Background network latency |
| Evidence | Model's action description | Captured source references and excerpts |
| Authority | Model-authored annotation | Model-authored interpretation |

Titles remain the immediate, low-cost feedback. Request a summary when the accumulated history is harder to follow; there is no automatic replacement or mandatory extra model. Quality/price comparisons against real providers require separately requested provider evaluation; deterministic tests do not claim a live-model quality improvement.

Validation uses scripted/local providers: live-owner coexistence, unchanged approvals/state, separate accounting/model choice, malformed output/citations, branch visibility, stale startup and preservation of a paused workflow checkpoint. No paid provider calls are needed.
