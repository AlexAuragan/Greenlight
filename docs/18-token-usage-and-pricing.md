# Token usage and model pricing

Status: accounting, configured flat/context-tier rates, cache-creation billing, DeepInfra public catalog discovery and automatic freshness flags are implemented on the P1 branch. Discovery adapters for other providers, invoice reconciliation and budget enforcement remain separate work. P0 stabilization is merged into `master`.

## Usage commands

```bash
greenlight usage SESSION_ID                         # recorded spending across all branches
greenlight usage SESSION_ID --scope branch          # new requests on the active branch only
greenlight usage SESSION_ID --scope visible         # ancestry-visible requests; includes inherited history
greenlight usage SESSION_ID --json                  # per-request counts, sources, rate snapshots and IDs
greenlight model-info                              # selected model metadata; cached/discovered public rates
greenlight model-info provider/model
greenlight model-info --refresh-pricing            # force public catalog refresh when no override applies
greenlight refresh-pricing                         # refresh supported provider catalog, no inference
greenlight models --pricing                        # provider model list with resolved metadata
```

The TUI footer shows token totals (`~` estimated; `?` incomplete). Use More → Usage, Run details → Usage, or `/usage` in Commands mode for a session summary with count sources and pricing coverage. Request details opens the recorded dispatch list; Enter/click opens exact request evidence. Full accounting retains model/attempt subtotals and captured pricing-source/date notes. Back/Esc returns through the nested views. Accounting records remain in full history but are hidden from the default conversation. Missing historical data is not backfilled.

## Account for every model request

- Record provider/model, request identity, purpose, session, branch and attempt where applicable. Include coding calls, independent verification, deviation detection, rejected responses, retries. Future manager/summarizer calls must use the same central accounting path.
- Persist input, cached-input and output tokens separately. Document whether the provider's input total includes cached tokens; normalize cached input as a subset so it is never counted twice. Preserve additional usage categories such as reasoning tokens when supplied, without counting them twice if already included in output.
- Prefer provider-reported usage, which is the best available source for billing and cache hits. Store the original usage fields and the source/status of normalized counts.
- When usage is missing, use the model's compatible tokenizer when available. Use tiktoken for supported encodings, or as an explicitly approximate fallback for other models. Account for conversation/message framing where possible. Tokenizer counts are estimates of usage, not proof of billable tokens.
- Cached-input counts cannot be inferred from tokenization or repeated prompts. Keep them unknown unless reported by the provider. Failed, cancelled or interrupted requests may still incur charges; record uncertainty instead of treating missing usage as zero.
- Save usage even when a response is malformed, a detector rejects a report, or parsing/verification fails. Deduplicate repeated reporting of the same request while counting genuinely repeated paid requests separately.

## Price each configured provider/model

`model-info` and `models --pricing` surface configured metadata. Adding/selecting a model should surface its input, cached-input and output prices, currency, rate unit (for example, per million tokens), source and effective/retrieval date. Discover authoritative provider pricing where supported; otherwise accept explicit configured rates and overrides. Missing rates must remain visibly unknown, never default to free. Track relevant pricing tiers if a provider varies rates by context size or request type.

Snapshot the rate information used for each cost calculation so a later pricing change does not rewrite historical spending. Distinguish calculated estimates, stale pricing and invoices; provider charges are not inferred from prices.

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

All three rates are optional: omitted/null means unknown, never free. A pricing block requires currency, source and a valid effective date. Negative/nonfinite rates and unsupported tokenizer names are rejected. Metadata is scoped to this provider configuration and model ID, including overrides. A returned model ID that differs from the requested model leaves cost unknown rather than applying a potentially incorrect rate. Explicit pricing overrides discovered pricing as a whole, including null/unknown rates. It is never partially filled from a different source. Configured prices use their effective date for age checks; provider snapshots use retrieved_at (RFC3339) and do not invent an effective date. Both are labelled stale after 30 days; future-dated prices cannot produce a cost estimate.

## Current implementation boundaries

- Requests are durably registered before dispatch. A crash leaves a started/interrupted record with unknown output/billing; a successful response updates that same request ID. Accounting failures stop execution and retain the initial record when possible.
- OpenAI-style usage, DeepSeek cache-hit fields, Responses-style usage fields and native-shaped Anthropic cache read/creation totals are normalized. This does not add new provider transports; completion requests still use the existing OpenAI-compatible endpoint.
- Missing counts use tiktoken on serialized input messages and returned text. This approximates message framing, does not implement every model's actual tokenizer, and cannot count unreturned reasoning tokens. Native cache-creation counts are also exposed separately as a subset of total input, and can be priced with explicit creation/TTL rates as described below.
- Unknown cache usage prevents a point cost estimate when cached and uncached rates differ. If the configured rates are identical, an estimate can use total input without pretending the cache split is known. Known-cost subtotals and incomplete-cost request counts are shown separately, with currencies never combined.
- The ledger groups actual dispatched request records, not provider invoices. It deduplicates repeated recording of a request ID; separate network attempts are separate records even if responses resemble one another. No past usage is inferred from old event history.
- Discovery currently supports the exact DeepInfra default HTTPS base URL, with default on-demand token billing. Other transports and billing modes require explicit rates. Actual invoice reconciliation and budgets remain separate work. Provider-reported raw usage is retained; calculated costs are explicitly estimates.


## Provider failures and retries

Each inference request attempt, including an automatic retry, has its own durable call UUID and usage record under the same workflow attempt/purpose. Local preflight failures are marked `not-sent`; their input estimate is prepared text, not billed usage. Rejected requests retain any reported usage, fallback input estimates and unknown output/cache values; rejection is not a fabricated zero-cost response. Error bodies/messages are not retained as diagnostics. Safe failure details and retry scheduling events refer to call IDs. If processing/billing is uncertain, the call's notes say so and no per-call cost estimate is supplied. Cancelling a backoff adds an event without inventing another sent request. A process exit during a wait retains the already-finished failed-request record. See [provider failure policy](19-provider-failures.md).

## Public pricing discovery and freshness

When no explicit pricing override exists, `model-info`, `models --pricing` and the central inference accounting path resolve the exact requested model ID against DeepInfra's public [model catalog](https://api.deepinfra.com/models/list). Discovery is supported only for `https://api.deepinfra.com/v1/openai` (optional trailing slash). A proxy, different provider, query string or credential-bearing URL does not inherit DeepInfra rates. Set `provider.pricing_discovery: false` for offline/explicit-only operation. Explicit `refresh-pricing` still permits a requested catalog fetch; `model-info --refresh-pricing` preserves configured overrides.

The GET sends no API credentials, prompts, goal or project data. It has a 10-second timeout, an 8 MiB response bound and no redirects. HTTPS verification stays enabled; an explicit SSL_CERT_FILE CA bundle is honored (bounded to 2 MiB) alongside built-in roots. The local private atomic cache is `.greenlight/state/deepinfra-pricing.json`; successful checks last 24 hours. Failed checks retain older rate snapshots with a warning and back off 15 minutes; `refresh-pricing` retries immediately and exits with an error if refresh/persistence failed. An unavailable catalog does not block inference. Unsupported or missing prices remain unknown. `model-info`/metadata and `usage` show source, retrieval/effective dates and warnings; request snapshots preserve the exact rates and freshness qualification used at dispatch. Later refreshes never reprice previous ledger records.

The adapter converts catalog cents/token to USD/million tokens (`×10,000`) and applies the catalog's cached-input/cache-write multipliers to its base input rate. Missing multipliers remain unknown. It accepts only ordinary text-generation token prices; catalog tables, discounts and explicit-cache storage/granularity billing are deliberately unsupported rather than flattened. Priority/flex prices do not apply: the current transport does not request those service tiers. There is no built-in price list and no HTML scraping. Other providers can use explicit rates until a tested authoritative adapter is added.

## Context tiers and cache creation

The following **illustrative, not actual provider rates** extend an existing pricing block:

```yaml
cache_creation:
  default_per_million: 2.5
  five_minute_per_million: 2.5
  one_hour_per_million: 4.0
tiers:
  - above_input_tokens: 200000
    input_per_million: 4.0
    cached_input_per_million: 1.0
    output_per_million: 20.0
    cache_creation:
      default_per_million: 5.0
      five_minute_per_million: 5.0
      one_hour_per_million: 8.0
```

Tier boundaries must be strictly increasing. Above a boundary (not at equality), the highest applicable tier replaces **all rates for the entire request**, including output and creation rates. Omitted tier rates stay unknown; they do not inherit potentially cheaper base prices. Tier selection requires provider-reported total input, including cached/created input. Estimated input cannot safely select a tier. This supports context tiers, not marginal tax-style bands, batch pricing, storage rent or account-specific volume discounts.

Native-shaped usage separates uncached `input_tokens`, `cache_read_input_tokens` and `cache_creation_input_tokens`. The normalized total includes all three. Cost subtracts cached and created tokens from that total before applying the ordinary input rate, then adds cache reads, cache creation and output separately. A `cache_creation` usage breakdown with `ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` uses the matching TTL rates and must sum exactly to the reported creation total. Without a breakdown, a positive creation total requires `default_per_million`; the harness does not guess a TTL. Missing rates, invalid counts and unsupported categories leave cost unavailable. This normalization does not add an Anthropic-native request transport.
