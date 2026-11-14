# LLM provider failures

Provider failures are harness facts, not model-authored diagnoses. CLI stderr and TUI diagnostics give safe, actionable guidance. Structured `provider.failure`, `provider.retry`, `provider.retry-stopped` and `provider.retry-cancelled` events retain failure categories, statuses, delays and usage-call references without copying error bodies, messages, URLs or credential values.

| Category | Evidence and action |
| --- | --- |
| Credentials | Missing/empty/invalid bearer-key environment value, HTTP 401 or recognized auth code. Check `provider.api_key_env` in `greenlight.yaml` (default `DEEPINFRA_API_KEY`), reload or replace the key, then resume. |
| Billing/quota | HTTP 402, recognized billing codes/types (including `insufficient_quota` on 429), or known credit/balance message phrases. Check account balance/plan/billing limits before resuming. |
| Rate limit | HTTP 429. Wait or reduce frequency; ambiguous codes also advise checking account quota. Only explicit recognized rate-limit codes without a usage report qualify for automatic retry. |
| Permission | HTTP 403 without a more specific auth/billing code. Check key permissions and organization/project/model access. |
| Request | Other 4xx/request-builder failures. Check model, context length, endpoint and request configuration. |
| Service/timeout | 5xx, 408 or timeout. Check provider status/connectivity. Inference may have run; billing is uncertain and automatic retries are disabled. |
| Connection | Failure establishing a connection before a response. Check network/endpoint configuration; bounded retries are eligible. |
| Protocol/unknown | Invalid success JSON, missing content, incomplete/oversized/invalid UTF-8 response, or an unrecognized failure. Check endpoint/model compatibility or service/account status. No raw response is displayed. |

Known `error.code`, `error.type` and top-level code/type fields are checked together; billing evidence takes precedence over rate-limit evidence. Unrecognized formats retain a generic safe fallback. Non-JSON HTTP errors still use their status. `models` uses the same categories and guidance, without automatic retries or inference-usage records.

## Retry policy

Coding, verification and detector calls use one central request loop. At most three physical requests are sent for one logical call, with at most two waits. Only explicit HTTP 429 rate-limit codes without usage and connection-establishment failures qualify. A usage report accompanying a rejection disables automatic retry because processing/billing may be uncertain. Credentials, billing/quota, permission errors, bad requests, unknown 429s, timeouts, protocol errors and 5xx responses require explicit resume. These failures during verification bypass ordinary verification-failed workflow deviations, so they cannot trigger another detector or workflow retry.

`Retry-After` supports integer seconds and HTTP dates. A server wait over 15 seconds stops automatic retry and tells the user to resume later; the delay is never shortened to fit the budget. A past date means zero delay. Without a valid header, waits are approximately one and two seconds, each with 0–250 ms jitter. Total scheduled backoff cannot exceed 30 seconds. Connections time out after 10 seconds and each HTTP request after 120 seconds; response bodies are capped at 8 MiB. Redirects are not followed automatically.

TUI `x` cancels a pending backoff and prevents further provider requests from that worker. It does not cancel an in-flight request or already-running tool, and a returned action may still finish before the next provider-call boundary. Approvals keep their decision keys; text-entry fields keep literal x. A new resume uses a new cancellation flag. CLI interruption retains existing process/owner recovery semantics; this is not a general workflow/tool cancellation system.

## Usage and recovery

Every inference request attempt gets its own durable call UUID before sending, correlated to the workflow attempt and purpose. Local credential/request preflight failures are marked `not-sent`; their input estimate describes prepared text, not billed usage. Failed responses retain reported usage when available; fallback estimates and unknown output/cache/billing values stay explicit. Processing-uncertain failures suppress cost estimates. Cancelling a wait does not create a fictional second request. After final failure the session blocks with actionable guidance; correcting credentials/credit/network settings and explicitly resuming starts a fresh request sequence. No failed tool operation or model-approved action is replayed by the provider retry loop.

Tests use scripted completions and local HTTP servers, covering classification/redaction, Retry-After, budgets, cancellation, usage durability and failure during verification. They make no paid provider calls. Provider-specific taxonomy will need extension as services change; arbitrary prose is never displayed as a trusted diagnosis.
