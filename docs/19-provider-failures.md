# LLM provider failures

Provider failures are harness facts, not model-authored diagnoses. CLI stderr and TUI diagnostics give safe, actionable guidance. Structured `provider.failure`, `provider.retry`, `provider.retry-stopped` and `provider.retry-cancelled` events retain failure categories, statuses, delays and usage-call references without copying error bodies, messages, URLs or credential values.

| Category | Evidence and action |
| --- | --- |
| Credentials | Missing/empty/invalid bearer-key environment value, HTTP 401 or recognized auth code. Check `provider.api_key_env` in `greenlight.yaml` (default `DEEPINFRA_API_KEY`), reload or replace the key, then resume. |
| Billing/quota | HTTP 402, recognized billing codes/types (including `insufficient_quota` on 429), or known credit/balance message phrases. Check account balance/plan/billing limits before resuming. |
| Rate limit/capacity | HTTP 429. Recognized response hints distinguish concurrency, token throughput, request frequency and model/server capacity; unknown reasons stay explicit. A 429 alone does not establish that the account concurrency ceiling was reached. Only explicit recognized rate-limit codes without a usage report qualify for automatic retry. |
| Permission | HTTP 403 without a more specific auth/billing code. Check key permissions and organization/project/model access. |
| Request | Other 4xx/request-builder failures. Check model, context length, endpoint and request configuration. |
| Service/timeout | 5xx, 408 or timeout. Check provider status/connectivity. Inference may have run; billing is uncertain and automatic retries are disabled. |
| Connection | Failure establishing a connection before a response. Check network/endpoint configuration; bounded retries are eligible. |
| Protocol/unknown | Invalid success JSON, missing content, incomplete/oversized/invalid UTF-8 response, or an unrecognized failure. Check endpoint/model compatibility or service/account status. No raw response is displayed. |

Known `error.code`, `error.type` and top-level code/type fields are checked together; billing evidence takes precedence over rate-limit evidence. Unrecognized formats retain a generic safe fallback. Non-JSON HTTP errors still use their status. `models` uses the same categories and guidance, without automatic retries or inference-usage records.

## Retry policy

Coding, verification and detector calls use one central request loop. Each automatic cycle sends at most three physical requests, with at most two waits. A manual Retry request starts another explicitly requested cycle. Only explicit HTTP 429 rate-limit codes without usage and connection-establishment failures qualify. A usage report accompanying a rejection disables automatic retry because processing/billing may be uncertain. Credentials, billing/quota, permission errors, bad requests, unknown 429s, timeouts, protocol errors and 5xx responses require an explicit user decision; they are not automatically repeated. These failures during verification bypass ordinary verification-failed workflow deviations, so they cannot trigger another detector or workflow retry.

That stop rule also applies inside `any`/`all` composites and to ledger/audit failures at the central model-call boundary, including failures after inference returned. They cannot become an ordinary rejected check, trigger another alternative/reviewer/detector or pass the step. A normal model review verdict of `passed: false` remains a failed check, so an `any` policy can still accept a later genuinely passing alternative. The original safe failure/accounting detail remains in the error chain and the initial usage record is retained when its completion could not be saved.

`Retry-After` supports integer seconds and HTTP dates. A server wait over 15 seconds stops automatic retry and offers a manual decision in a live workflow TUI; the delay is never shortened to fit the budget. A past date means zero delay. Without a valid header, waits are approximately one and two seconds, each with 0–250 ms jitter. An automatic cycle’s scheduled backoff cannot exceed 30 seconds; an explicitly requested manual retry can wait longer for the provider’s remaining cooldown. Connections time out after 10 seconds and each HTTP request after 120 seconds; response bodies are capped at 8 MiB. Redirects are not followed automatically.

TUI **[k] Stop** cancels a pending backoff or active provider request and prevents further requests from that worker. Cancelling local HTTP work cannot guarantee remote cancellation or known billing. Active sandboxed shell commands and web requests are also cancelled; synchronous journaled writes finish safely and applied edits remain. Approvals retain input ownership; hide the popup with Esc before stopping. A hidden unanswered prompt is interrupted without recording a denial. Text-entry fields keep literal k. A new resume uses a new cancellation flag and starts a fresh attempt. CLI interruption retains existing process/owner recovery semantics.

## Explicit retry in the workflow TUI

After automatic retries stop, a **Provider request paused** popup shows the provider category and corrective guidance. **[y] Retry request** resends the exact prepared messages with the same model/provider configuration. The current Rust workflow frame and conversation remain alive: no new branch or step attempt is created, and earlier tools, writes and human approvals are not replayed. This applies to coding, verification and deviation-detector requests from the live workflow worker. The provider failure is not fed back to the model as a tool failure.

**[n] Stop for now** ends this continuation and leaves recorded work available. **Esc** merely hides the popup; the worker keeps waiting. **[a] Review retry** reopens it, and keys outside the popup cannot answer it. A manual retry waits out any remaining `Retry-After` time, counting time already spent reviewing the popup; `k` cancels that wait and prevents another request. Each actual request retains its own usage UUID. `provider.retry-pending`, `provider.retry-decision` and a `provider.retry` record with `manual: true` link the decision to its failed call.

Timeouts, server errors, incomplete responses and failures carrying a usage report warn that the provider may already have processed/billed the request. They can be repeated **only by an explicit user retry**, with the uncertainty visible. No ledger or trace-persistence failure is offered as a retryable provider error. Authentication/billing errors remain visible until fixed or stopped; retrying does not make an invalid key or empty balance valid. Changing exported credentials, model or endpoint settings may require restarting Greenlight; a paused request retains its original settings.

For sessions already stopped/blocked, **[t] Continue step** offers the normal resume operation without requiring a typed instruction. Its screen explicitly states that this starts a fresh step attempt and earlier tools may run again. It retains the existing branch and recorded work and rechecks execution ownership, pending writes and the selected session before starting. This is different from exact-request Retry. Suspended continuations are currently memory-only; process restarts and independent manager side questions remain on their existing recovery path. Durable request checkpoints and manager continuations are tracked in the TODO.

## Usage and recovery

Every inference request attempt gets its own durable call UUID before sending, correlated to the workflow attempt and purpose. Local credential/request preflight failures are marked `not-sent`; their input estimate describes prepared text, not billed usage. Failed responses retain reported usage when available; fallback estimates and unknown output/cache/billing values stay explicit. Processing-uncertain failures suppress cost estimates. Cancelling a wait does not create a fictional second request. After final failure, a live workflow TUI offers an exact-request retry as described above. Stopping that retry, CLI failure or ending the worker leaves the session blocked; ordinary Resume/Continue step starts a fresh step attempt. No failed tool operation or model-approved action is replayed by the provider retry loop.

Tests use scripted completions and local HTTP servers, covering classification/redaction, Retry-After, budgets, cancellation, usage durability and failure during verification. They make no paid provider calls. Provider-specific taxonomy will need extension as services change; arbitrary prose is never displayed as a trusted diagnosis.

For initialized projects, edit provider settings in `.greenlight/config.yaml`; legacy `greenlight.yaml` remains supported. Credentials may be exported or stored in the selected project’s `.greenlight/.env`. See [installation and projects](22-installation-and-projects.md).

## DeepInfra transport audit — 2026-11-19

A single workflow awaits one model response before executing its action or asking for another response. Each tool turn therefore makes a new model request, sequentially; it does not fan out into concurrent model calls. One separately requested manager question can overlap with the coding worker. Other processes/accounts and server-side work surviving a timeout are outside this local bound. Public pricing discovery is an unauthenticated catalog GET, normally cached for 24 hours (failed refreshes for 15 minutes), not another inference call.

The default transport sends JSON to `https://api.deepinfra.com/v1/openai/chat/completions` with Bearer authentication, `model`, `messages`, configured `max_tokens` (default 4096) and a stream flag. TUI requests add `stream_options.include_usage`; they do not request multiple completions with `n`. Local HTTP tests check the exact path, headers and body in both modes. DeepInfra's [official SDK README](https://github.com/deepinfra/deepinfra-python/blob/3a6c0b93182b2861dde043025db87180ea273562/README.md#L124-L126) confirms the compatible base URL. Its [error decoder](https://github.com/deepinfra/deepinfra-python/blob/3a6c0b93182b2861dde043025db87180ea273562/deepinfra/_exceptions.py#L124-L140) documents `error` and `detail` wrappers. The hosted [rate-limit page](https://deepinfra.com/docs/advanced/rate_limits) could not be retrieved from this environment; this audit does not independently establish current account/model limits or the cause of a user's live rejection.

The classifier now reads those `detail` wrappers, including nested billing codes. On HTTP 429, a fixed `limit_reason` enum preserves recognized concurrency, token-throughput, request-frequency or model/server-capacity hints. Unknown reasons remain explicitly unknown, rather than asserting the account concurrency ceiling was reached. These are response hints, not independently verified account settings. Raw prose remains excluded from saved/displayed diagnostics, and legacy failure records remain readable. Billing evidence still takes precedence; new prose hints alone do not authorize automatic retry.

Reqwest's internal protocol-NACK retries are explicitly disabled. They do not retry HTTP 429 by default and were not established as the cause here; disabling them keeps retry policy and request accounting in the engine.

A local HTTP/CLI fixture exercised the actual engine with no paid provider calls:

| Scenario | Physical inference POSTs | Peak in-flight inference | Usage records |
| --- | ---: | ---: | ---: |
| Eight read-tool turns, then finish | 9 | 1 | 9 |
| Unknown 429 | 1 | 1 | 1 |
| Explicit retryable 429, exhausted budget | 3 | 1 | 3 |
| Model-overloaded detail response, no retryable code | 1 | 1 | 1 |

A separate native PTY fixture confirms that a manually retried streaming request preserves exact messages and does not repeat the earlier tool, branch or attempt. These fixtures rule out request fan-out in the exercised paths; they cannot identify an already-discarded live response or reproduce provider account policy. Input context grows as tool results accumulate, so a token-throughput rejection, if reported, needs separate investigation from concurrent-request counts.

## Verify the credential used by a paused request

The [Provider / API key page](11-tui.md#inspecting-the-provider-and-api-key) exposes first/last-five masking and source precedence, with the active worker's captured snapshot. Changing `.env` does not override a preexisting exported variable and does not reload a running process. Normal provider construction now captures credentials once; manual retry cannot silently switch keys if the process environment changes later. No full key or credential snapshot is persisted in accounting or audit events.

A retry succeeding after a brief pause is consistent with temporary throttling or model capacity, but does not identify which limit was enforced. Checking the masked key rules out a credential-selection mistake; it does not independently resolve an HTTP 429's cause.
