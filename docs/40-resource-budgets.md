# Resource budgets and refresh work (GL-005)

No database schema or recorded evidence is removed. Limits apply to materializing data, not to workflow turn counts or retained history. Readers reject oversized full records with actionable errors; previews are explicitly labelled. Settings live in an optional root `limits` mapping. Restart Greenlight after changing them.

Defaults:

| Setting | Default | Applies to |
| --- | ---: | --- |
| source_bytes | 1 MiB | A project/workflow/include source read |
| expanded_prompt_bytes | 1 MiB | Fully expanded prompt, checked before replacement allocation |
| blob_bytes | 32 MiB | One artifact, trace, checkpoint or event body |
| materialized_bytes | 64 MiB | An aggregate history, artifact/edit or usage read |
| history_events | 100,000 | Full history, artifact metadata and usage record counts |
| export_bytes | 128 MiB | Materialized conversation dump and serialized output |
| export_events | 100,000 | Events in a conversation dump |

For example, add this to `.greenlight/config.yaml` (or the selected legacy project config):

```yaml
limits:
  source_bytes: 1048576
  expanded_prompt_bytes: 1048576
  blob_bytes: 33554432
  materialized_bytes: 67108864
  history_events: 100000
  export_bytes: 134217728
  export_events: 100000
```

Config, dotenv and approval files have a fixed 1 MiB bootstrap cap. The blob default accommodates the existing 20 MiB native snapshot limit; that mutation limit does not increase. Read budgets do not permit edits to previously oversized files.

All values must be positive. Metadata remains browsable when full history exceeds its budget: a separate diagnostic window retains at most 2,000 recent records, with up to 512 characters per preview. These fixed diagnostic bounds apply independently of the full-history limits. Previews are labelled and only one exact selected event body is retained. A partial window never supplies a workflow result or verification projection. A body over its own limit remains on disk and cannot masquerade as a complete preview. Raise the appropriate limit explicitly to inspect/export larger legacy records. Serialization/parsing and UI structures have overhead beyond these UTF-8 byte budgets; they are not process RSS limits.

Unchanged committed histories may share immutable cached rows. Cache keys include session, branch and local/external database revisions; caches are not used or published inside transactions. Rollbacks and other processes' updates must not produce stale evidence. Artifacts are listed by metadata and only the selected immutable body is loaded. Missing/oversized selected bodies show an error without hiding the list or blocking other artifacts. Bodies and failures stay cached for that immutable version until selection/version changes or restart; out-of-band edits to immutable blobs are outside the cache contract. Activity rows, result reports, artifact metadata and usage accounting are reused while their database revision is unchanged. Ownership/liveness is still checked during refresh; cache reuse must not authorize a stale execution decision.

Full history readers preflight counts, UTF-8 bytes and maximum body size in one SQLite read snapshot before materializing payloads. SQLite `octet_length(column)` obtains byte sizes from record metadata; diagnostic previews omit bodies above the per-blob cap. Dump reads separately bound event count, event/message bytes and rendered Markdown/JSON. Missing/corrupt messages remain explicit forensic diagnostics; an oversized trace refuses a full export rather than masquerading as an unavailable message. Serialization refuses excess output before publishing any export. Terminal Markdown escapes are streamed to avoid allocating an additional expanded copy. Files and records are never deleted to satisfy a read budget.

The existing line-diff implementation already bounds edit-distance trace growth. Artifact comparisons retain the 256 KiB per-version guard and never diff clipped text. Journal diff/body readers now inherit the blob and aggregate budgets. No compaction, automatic pruning, schema migration or new model turn cap is introduced.

Validation covers over-budget sources/includes, sparse oversized legacy files, history/export aggregate limits, branch and transaction cache isolation, external commits, selected-body loading and idle refresh read counts/timing. GL-003 descriptor boundaries remain in force.

## Refresh/diff profile

Run `cargo test --locked --offline profile_large_history_refresh_and_diff -- --ignored --nocapture` to repeat the local fixture. On 2026-11-21, a debug build with 5,000 512-byte events and twenty 16 KiB artifacts took about 60 ms for first refresh and 2.1 ms per idle refresh (100 iterations), with zero artifact-body reads during those idle refreshes. A 10,000-line one-edit unified diff took about 1.7 ms. These are local fixture measurements, not release-build or live-provider guarantees. Normal tests assert shared row identity and zero repeated body reads without timing thresholds.
