# File-body snapshot evaluation (GL-011)

Decision: retain hash manifests and guarded native-write snapshots. Do not enable automatic retention or restoration of external file bodies. GL-011 is complete as an evaluation; it does not implement a new snapshot store or expand restart coverage.

## The concrete gap

Native `write` actions already preserve before/after bodies and support guarded restore. Boundary hashes add broader observation, including changes outside the native journal. The existing audit tests distinguish a continuous native chain from manual modification/creation and reject unrelated branch/attempt explanations. The restore tests refuse conflicting manual contents without altering the file.

An external edit can therefore be detected without retaining its old contents. Deletion is the clearest case: an after-step capture has nothing left to read. Capturing only changed files after execution cannot recover their pre-step contents. Likewise, a file changed and reverted entirely between scans is not observable from its endpoint hashes alone unless other journal evidence exposes it. More boundary bodies do not provide a complete event history or establish authorship.

To guarantee a preimage for an arbitrary future change, Greenlight must retain the eligible baseline **before** the step, or introduce a different filesystem/Git integration. That is a broader storage/privacy policy than saving a few changed files after a step. Git cannot be assumed to cover untracked files or work inside the current masked-metadata sandbox.

## Evidence and costs

Reproduction on the development workspace at `65770fc`, before adding this evaluation:

```bash
greenlight workspace-benchmark --runs 3 --json
cargo test --locked --offline workspace_audit
cargo test --locked --offline restore::tests
```

All eight audit tests and six restore tests passed. The [raw scan results](benchmarks/gl011-workspace-scan.json) show 443 included files, 26,608,001 bytes hashed and a 57,075-byte manifest per run. Scan times were 640,665, 509,379 and 511,585 microseconds (median about 0.512 seconds). These are local scan timings, not measurements of a proposed body-capture implementation or other projects.

Retaining that complete raw baseline would represent about 25.4 MiB rather than a 55.7 KiB manifest—about 466 times the manifest bytes before deduplication/compression. The scan includes binary assets; this is the scanned candidate set, not a measured text-only snapshot size. Deduplication can reduce unchanged-body storage but does not remove the need for baseline selection, reference retention, privacy checks and capture-race handling.

Current scan bounds are 10,000 files and 20 MiB per file, with no small aggregate body-retention quota. Those limits alone would permit a theoretical baseline near 195.3 GiB. They must not become an implicit snapshot-storage allowance. State history also has no general reference-aware body-retention/pruning policy that would make repeated baselines automatically safe to collect or discard.

## Current policy

| Area | Decision now |
| --- | --- |
| Source content | Retain existing native-write/restore bodies; no new external-body collection |
| External edits | Detect with hashes, label uncertainty and protect from overwrite |
| Restart | Restore only supported journaled preimages; keep manual/untracked files outside that guarantee |
| Privacy | Keep current scan exclusions; paths/hashes remain private project data |
| Caches/build output | Continue excluding generated/cache/virtualenv/state directories |
| Retention | Add no new body cache, quotas, migration or pruning behavior |
| Provider usage | No additional model calls |

Filename exclusions are heuristic, not a secret detector. Broad body capture would retain credentials embedded in otherwise ordinary files even when their names pass the scanner. Storing a body locally, exporting it and sending it to a model are separate disclosures; capture must not implicitly authorize the latter two.

## Conditions for a future opt-in implementation

The following is a concrete candidate policy for future design, **not supported configuration or a promise of new restoration coverage**:

1. Require explicit project opt-in and a narrow path allowlist. Initially capture only regular UTF-8 text, with no symlinks, private/provider configuration, environment/credential files, generated output, virtualenvs or application state. Never widen current sensitive-path exclusions to satisfy the allowlist.
2. Start with ceilings of 1 MiB per file, 100 files / 8 MiB per boundary, and 64 MiB of retained unique bodies per project. A ceiling is a hard capture limit, not permission to prune referenced history. Record incomplete capture and its reason; do not treat skipped/error paths as absence or silently claim they can be restored.
3. Capture preimages for eligible files before executing the step. Store postimages only when useful for a concrete diff/recovery case. Record absence separately from empty content, and verify the body hash matches the manifest through descriptor-anchored reads and stability checks. A changed file between scan and capture makes that capture uncertain.
4. Keep content-addressed bodies in private project state. Define branch/event references, crash-orphan cleanup, export defaults and reference-aware retention before enabling collection. Do not delete bodies required by a surviving checkpoint merely to stay under quota. Optional capture failure must be reported without manufacturing a verification failure or silently broadening recoverability.
5. Keep external edits **protected by default**. A later external-body restore feature would need its own explicit per-file preview/confirmation, matching capture/hash evidence, the project execution lease, drift checks and journaled restoration. A newer manual edit must not be overwritten just because an old body exists. Do not promise atomic multi-file rollback.

Reopen implementation when a real workflow requires recovery of changes outside native writes and these limits/privacy choices are accepted. Until then, GL-010's historical checkpoints, native diffs and recorded branch comparisons provide inspection without claiming unsupported recovery.
