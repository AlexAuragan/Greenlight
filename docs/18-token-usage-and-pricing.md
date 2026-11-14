# Token usage and model pricing

Status: core accounting and configured flat-rate pricing are implemented on `feat/token-usage-and-pricing`. Automatic pricing discovery, freshness checks, tiered billing, cache-creation pricing and budget enforcement remain TODOs. P0 stabilization is merged into `master`.

## Usage commands

```bash
greenlight usage SESSION_ID                         # recorded spending across all branches
greenlight usage SESSION_ID --scope branch          # new requests on the active branch only
greenlight usage SESSION_ID --scope visible         # ancestry-visible requests; includes inherited history
greenlight usage SESSION_ID --json                  # per-request counts, sources, rate snapshots and IDs
greenlight model-info                              # selected model metadata; no network request
greenlight model-info provider/model
greenlight models --pricing                        # provider model list with configured metadata
```

The TUI footer shows token totals (`~` estimated; `?` incomplete). Use `/usage` for a scrollable session spending report; arrows scroll and Escape closes it. Accounting records remain in full history but are hidden from the default conversation. Missing historical data is not backfilled.

## Account for every model request

- Record provider/model, request identity, purpose, session, branch and attempt where applicable. Include coding calls, independent verification, deviation detection, rejected responses, retries. Future manager/summarizer calls must use the same central accounting path.
- Persist input, cached-input and output tokens separately. Document whether the provider's input total includes cached tokens; normalize cached input as a subset so it is never counted twice. Preserve additional usage categories such as reasoning tokens when supplied, without counting them twice if already included in output.
- Prefer provider-reported usage, which is the best available source for billing and cache hits. Store the original usage fields and the source/status of normalized counts.
- When usage is missing, use the model's compatible tokenizer when available. Use tiktoken for supported encodings, or as an explicitly approximate fallback for other models. Account for conversation/message framing where possible. Tokenizer counts are estimates of usage, not proof of billable tokens.
- Cached-input counts cannot be inferred from tokenization or repeated prompts. Keep them unknown unless reported by the provider. Failed, cancelled or interrupted requests may still incur charges; record uncertainty instead of treating missing usage as zero.
- Save usage even when a response is malformed, a detector rejects a report, or parsing/verification fails. Deduplicate repeated reporting of the same request while counting genuinely repeated paid requests separately.

## Price each configured provider/model

`model-info` and `models --pricing` surface configured metadata. Adding/selecting a model should surface its input, cached-input and output prices, currency, rate unit (for example, per million tokens), source and effective/retrieval date. Discover authoritative provider pricing where supported; otherwise accept explicit configured rates and overrides. Missing rates must remain visibly unknown, never default to free. Track relevant pricing tiers if a provider varies rates by context size or request type.

Snapshot the rate information used for each cost calculation so a later pricing change does not rewrite historical spending. Distinguish provider-reported charges from calculated estimates and stale pricing.

For providers where cached tokens are included in total input:

`estimated cost = (input - cached input) × input rate + cached input × cached-input rate + output × output rate`

Normalize rates to the same billing unit. If cache usage or rates are unknown, show a qualified estimate, range or unavailable total; do not silently substitute zero. Provider-specific billing rules take precedence over this illustrative formula.

## Reporting and acceptance checks

- Expose request details and totals by attempt, session, branch and model in CLI/TUI, with counts and costs labelled reported, estimated or unknown.
- Separate actual incurred charges from the ancestry-visible history of a fork. Inheriting an earlier call's evidence does not create a new charge; a repeated call does.
- Test provider field variations, cache-inclusive input totals, absent cache/usage/prices, tokenizer compatibility, malformed responses, failed/cancelled calls, retries, deduplication, fork inheritance, rate changes and restart persistence.
- Token accounting is always active; detailed views may be optional. Budget enforcement, new model roles and telemetry export are separate work. Do not include credentials in usage or pricing records.


## Configure model metadata

The example rates below are illustrative only, not actual provider prices. Replace them with rates verified for the provider/model and context tier you use.

```yaml
provider:
  model: provider/example-model
  model_metadata:
    provider/example-model:
      tokenizer: cl100k_base  # optional: cl100k_base or o200k_base; always an estimate
      pricing:
        input_per_million: 2.0
        cached_input_per_million: 0.5
        output_per_million: 10.0
        currency: USD
        source: "Example only — replace with a verified pricing URL or citation"
        effective_date: "2026-11-14"
```

All three rates are optional: omitted/null means unknown, never free. A pricing block requires currency, source and a valid effective date. Negative/nonfinite rates and unsupported tokenizer names are rejected. Metadata is scoped to this provider configuration and model ID, including overrides. A returned model ID that differs from the requested model leaves cost unknown rather than applying a potentially incorrect rate. Rates are configured snapshots, not live-verified prices or invoices; no automatic expiration or refresh is implied.

## Current implementation boundaries

- Requests are durably registered before dispatch. A crash leaves a started/interrupted record with unknown output/billing; a successful response updates that same request ID. Accounting failures stop execution and retain the initial record when possible.
- OpenAI-style usage, DeepSeek cache-hit fields, Responses-style usage fields and native-shaped Anthropic cache read/creation totals are normalized. This does not add new provider transports; completion requests still use the existing OpenAI-compatible endpoint.
- Missing counts use tiktoken on serialized input messages and returned text. This approximates message framing, does not implement every model's actual tokenizer, and cannot count unreturned reasoning tokens. Native cache-creation premiums are retained as raw usage but do not receive a flat-rate cost estimate.
- Unknown cache usage prevents a point cost estimate when cached and uncached rates differ. If the configured rates are identical, an estimate can use total input without pretending the cache split is known. Known-cost subtotals and incomplete-cost request counts are shown separately, with currencies never combined.
- The ledger groups actual dispatched request records, not provider invoices. It deduplicates repeated recording of a request ID; separate network attempts are separate records even if responses resemble one another. No past usage is inferred from old event history.
- Pricing discovery/freshness, tier selection, actual invoice reconciliation and budgets remain separate work. Provider-reported raw usage is retained; calculated costs are explicitly estimates.
