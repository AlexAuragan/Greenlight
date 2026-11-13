# Token usage and model pricing — planned

Status: accepted TODO, not implemented. Priority: P1, before additional model roles or cost budgets. The existing P0 stabilization checklist remains complete on the review branch.

## Account for every model request

- Record provider/model, request identity, purpose, session, branch and attempt where applicable. Include coding calls, independent verification, deviation detection, rejected responses, retries and future manager/summarizer calls.
- Persist input, cached-input and output tokens separately. Document whether the provider's input total includes cached tokens; normalize cached input as a subset so it is never counted twice. Preserve additional usage categories such as reasoning tokens when supplied, without counting them twice if already included in output.
- Prefer provider-reported usage, which is the best available source for billing and cache hits. Store the original usage fields and the source/status of normalized counts.
- When usage is missing, use the model's compatible tokenizer when available. Use tiktoken for supported encodings, or as an explicitly approximate fallback for other models. Account for conversation/message framing where possible. Tokenizer counts are estimates of usage, not proof of billable tokens.
- Cached-input counts cannot be inferred from tokenization or repeated prompts. Keep them unknown unless reported by the provider. Failed, cancelled or interrupted requests may still incur charges; record uncertainty instead of treating missing usage as zero.
- Save usage even when a response is malformed, a detector rejects a report, or parsing/verification fails. Deduplicate repeated reporting of the same request while counting genuinely repeated paid requests separately.

## Price each configured provider/model

Adding/selecting a model should surface its input, cached-input and output prices, currency, rate unit (for example, per million tokens), source and effective/retrieval date. Discover authoritative provider pricing where supported; otherwise accept explicit configured rates and overrides. Missing rates must remain visibly unknown, never default to free. Track relevant pricing tiers if a provider varies rates by context size or request type.

Snapshot the rate information used for each cost calculation so a later pricing change does not rewrite historical spending. Distinguish provider-reported charges from calculated estimates and stale pricing.

For providers where cached tokens are included in total input:

`estimated cost = (input - cached input) × input rate + cached input × cached-input rate + output × output rate`

Normalize rates to the same billing unit. If cache usage or rates are unknown, show a qualified estimate, range or unavailable total; do not silently substitute zero. Provider-specific billing rules take precedence over this illustrative formula.

## Reporting and acceptance checks

- Expose request details and totals by attempt, session, branch and model in CLI/TUI, with counts and costs labelled reported, estimated or unknown.
- Separate actual incurred charges from the ancestry-visible history of a fork. Inheriting an earlier call's evidence does not create a new charge; a repeated call does.
- Test provider field variations, cache-inclusive input totals, absent cache/usage/prices, tokenizer compatibility, malformed responses, failed/cancelled calls, retries, deduplication, fork inheritance, rate changes and restart persistence.
- Token accounting is always active; detailed views may be optional. Budget enforcement, new model roles and telemetry export are separate work. Do not include credentials in usage or pricing records.
