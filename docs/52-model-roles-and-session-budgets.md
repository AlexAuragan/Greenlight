# Model roles and session budgets — GL-017

Implemented on `feat/optional-summaries`, pending merge. Both features are optional; existing configurations keep their model choices and unlimited dispatch policy.

## Role defaults

Add model IDs available through the project's existing provider endpoint/account:

```yaml
model_roles:
  coding: your-coding-model-id
  verification: your-review-model-id
  manager: your-manager-model-id
  summarizer: your-summary-model-id
```

Replace these example IDs with real supported model IDs. Omitting a role keeps its fallback:

| Request | Model selection order |
| --- | --- |
| Workflow coding | CLI `--model`, step model (including workflow defaults), `model_roles.coding`, `provider.model` |
| Agent verification and deviation detectors | `model_roles.verification`, otherwise the resolved workflow coding model |
| Manager | Existing `manager.model`, `model_roles.manager`, `provider.model` |
| Optional summaries | Existing `summarizer.model`, `model_roles.summarizer`, `provider.model` |

An explicitly configured verification role is independent of the coding override. Leaving it unset preserves the previous behavior, including using a CLI-selected workflow model for verification. Command and human verification keep their existing behavior; a role does not turn them into model calls.

Summaries do not inherit the manager model or the coding role. Manager enablement, optional summary generation, tools, approval/verification policies and workflows remain unchanged. These are model defaults, not additional provider accounts: every role uses the existing provider URL, credential and request settings. Usage/pricing records retain the actual selected model and purpose.

## Session-wide dispatch budgets

Optional configuration:

```yaml
budgets:
  session:
    requests: 100
    input_tokens: 1000000
    output_tokens: 100000
    estimated_cost_usd: 2.00
```

Each field is independent. Omit a field to disable that limit; omit `budgets` to disable every limit. `requests: 0` pauses all new model dispatches. Negative counts, nonfinite/negative costs, empty role IDs and unknown keys are rejected. `provider.max_tokens` must be positive.

The scope is one Greenlight session across **all branches and purposes**, including workflow coding, verification/detectors, manager questions, summaries and retries. Forking does not reset spending. Different sessions have independent budgets; this is not an account-wide, project-wide or monthly provider cap. Calls predating the usage ledger and usage outside Greenlight cannot be backfilled.

Before publishing each dispatch, the central dispatcher checks the ledger and persists a reservation in the same SQLite immediate transaction as its request record. Independent connections cannot both consume the last request allowance. Denied dispatches append a `budget.blocked` diagnostic but create no model-call/usage charge and send no model completion request. Pricing metadata lookup may occur before the guard.

Reservations use:

- Estimated serialized-message input tokens, using the existing tokenizer fallback.
- The actual provider request's configured maximum output (`provider.max_tokens`), rather than predicting a short answer.
- For estimated USD budgets, the highest configured input/cache/context-tier and output rates, without assuming a cache hit. Cache creation rates, when configured, are included in that maximum input rate.

Completed successful calls use recorded reported/fallback counts and their captured cost estimate. Pending, interrupted and failed requests retain reservations, including failed retries; any larger recorded partial amount is retained. This can deliberately reserve more than the eventual bill. Cached input is part of total input and is not added again. `not-sent` records do not consume the budget.

Unknown historical output/cost blocks an affected configured limit, rather than counting as zero. It does not block unrelated request-only limits. Historical pricing is not replaced with today's rates. Raising a limit does not erase usage or rewrite reservations.

## Cost and token limits are estimates

These guards do **not** guarantee a maximum provider invoice. Fallback tokenization excludes unknown chat-template/billing differences, failed-request billing can be uncertain, and provider prices or billing rules may differ. An in-flight response can report more than the reserved estimate; later dispatches then stop. The guard does not cancel a request already sent.

An estimated cost limit requires complete recent USD pricing for the new model, including input, cached-input and output rates. Catalog discovery or explicit `provider.model_metadata` can supply it. Missing rates/dates, stale/future-dated pricing, unsupported currencies and priority-tier pricing are not treated as free. Current cost-budget reservations do not support `service_tier: priority`; request/token budgets still work with that tier. A completed historical request whose cost cannot be estimated prevents a cost-limited continuation until that uncertainty is addressed by the chosen policy. There is no exchange-rate conversion.

Output-token budgets reserve the full request cap. For example, a 1,000-token output allowance cannot dispatch a request with `provider.max_tokens: 4096`, even if the expected answer is short. Configure suitable request caps before starting, or raise the session allowance.

Workers retain their startup configuration. Stop/restart active workers after changing limits or role defaults; editing the file does not retroactively change an in-flight worker's policy or request. The atomic shared-budget check assumes workers use the same configured policy. Viewer/CLI settings describe the configuration they loaded, which may differ from an older active worker.

## Inspect and continue

**Usage** and **Technical details** display configured limits, accounted/reserved totals and unknown values. Request details retain the dispatch reservation separately from billed usage. Technical details also list role defaults. The activity list and blocker distinguish a session budget stop from an earlier provider rate-limit error.

From the CLI:

```sh
greenlight --root /path/to/project model-budget SESSION_ID
```

This is read-only and sends no provider request. It shows current configuration, not a promise that an older running worker reloaded it.

When a workflow hits its budget, recorded edits and earlier tool results remain. If an exact request checkpoint exists, changing **only budgets** and retrying the saved request validates and reuses its original messages/attempt. Budget policy is excluded from that checkpoint's request fingerprint; model, provider/request settings, workspace and other existing checks still apply. The next worker reloads configuration. Changing `provider.max_tokens` or a model changes the prepared request and can require a fresh attempt; it is not equivalent to raising a budget. Manager saved requests follow their existing continuation checks; a summary is regenerated explicitly. No budget setting grants tool permission or replays an unanswered approval.

## Compatibility and validation

Reservations are optional fields in existing per-dispatch ledger payloads; older records still load. Empty role/budget defaults are omitted when serializing configuration so they do not change legacy default checkpoint fingerprints. No new ledger table, account credentials or provider integration is added.

Tests cover role precedence and same-provider separation, request/token/unpriced cost stops before any dispatch, bounded retries, cross-branch accounting, cached-input subset semantics, maximum rate reservations, old ledger JSON, independent-connection contention, unknown/foreign-currency/tier costs, exact saved continuation after budget-only changes, invalid/default configuration and TUI blocker/status text. Scripted providers make no paid calls. See [validation](06-validation.md).
