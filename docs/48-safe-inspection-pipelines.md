# Safe read-only inspection pipelines — GL-032

Implemented on `feat/optional-summaries`, pending merge. This extends [project inspection](46-project-inspection-permissions.md), without introducing wildcard grants.

## Eligible requests

Under the existing inspection policy (shell asks, native read allows, profile enabled), a complete validated 2–4-stage pipeline can run without a prompt. Existing exact grants, explicit allow/deny, profile opt-out and user-requested reruns retain their existing precedence and ordinary shell behavior.

| Role | Allowed commands |
| --- | --- |
| First stage | Existing supported project inspection sources: pwd, ls, cat, head, tail, wc, grep, rg, sort, uniq, cut, diff, cmp |
| Subsequent stdin filters | grep, rg, head, tail, wc, sort, uniq, cut, tr |

All commands retain their conservative flag whitelist. Downstream file operands, recursive discovery and filename-listing flags are excluded. Downstream `rg` receives explicit stdin. `tr` supports literal character sets and validated delete/squeeze/complement combinations, without file access. Standalone `tr` is excluded.

Examples: `cat notes | grep TODO | head -n 20`, `cat notes | sort | uniq -c`, `ls -a src | head -n 20`.

The host validates every stage before capturing files or launching anything. Only plain unquoted `|` connects stages. Unsupported syntax, quoted pipe literals, comments in pipelines, empty stages, redirects, logical operators, substitutions, wrappers and downstream file operands return the **entire original command** to exact approval. No automatically approved prefix runs first. Capture/resource failures after classification return an inspection error, without widening access.

## Scope and execution

One private filtered project snapshot serves the whole request. Sensitive/outside paths and symlinks retain the existing exclusions. Names-only sources do not copy file bodies. Every stage uses a fixed trusted executable in separate Bubblewrap isolation, with no shell interpreter and no original-project descriptor. Intermediate data flows through OS pipes.

Budgets are shared: 16 KiB/128 tokens for the command, at most four stages, 1 MiB per captured file, 8 MiB total captured bodies and 2,000 entries. Launch/execution has one 120-second deadline. Final stdout and combined stderr each have a 128 KiB capture cap; the normal tool presentation limit still applies. Stage summaries and scope notes remain after presentation truncation. These limits do not implement CPU or address-space quotas.

Cancellation, timeout, output overflow, namespace failure and partial launch failure kill and reap started stages. A startup error can mean earlier read-only stages started; it never triggers host execution or automatic replay.

## Results and short reads

One shell request/result reports final stdout, labelled stderr and every stage outcome. The rightmost nonzero stage determines aggregate failure; a successful final filter does not hide an earlier error or grep no-match.

Bubblewrap converts signals to numeric exit codes. The runner therefore never guesses that code 141 was harmless SIGPIPE. It holds a read-end guard on each pipe and drains remaining bytes only after its consumer exits. Successful `head` may intentionally stop early; remaining data is discarded with a bounded buffer, and the byte count is reported. All real failures remain visible. Other otherwise-successful early closes with discarded bytes are conservatively reported as incomplete. Draining shares the deadline and cancellation controls.

## Validation boundary

Parser, policy, snapshot and lifecycle tests run without provider requests. Test-only trusted local fixtures verify pipes, stdin argv, shared-view stability, bounds, early reads, status propagation and process cleanup; production has no such host fallback.

The cloud host rejects Bubblewrap UID-map setup with `Read-only file system`. Live integration is included but ignored by default and must pass on a suitable Linux host:

```sh
cargo test --locked sandbox::descriptor_tests::integration_project_pipeline_isolated_data_and_stage_outcomes -- --ignored
```

This host cannot establish successful end-to-end namespace isolation. See [validation](06-validation.md).

GL-033 records an observed startup-cleanup limitation: on this host, failing UID-map setup can leave a pre-namespace Bubblewrap helper retaining stdio after the direct stage process exits. Capture draining can then stall beyond the execution deadline. Full descendant containment and bounded cleanup remain deferred sandbox work; the current tests do not establish that guarantee for every real Bubblewrap startup failure.
