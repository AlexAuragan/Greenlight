# GL-035 validation review — 2026-11-28

GL-035 remains open. **Portability follow-up:** user-provided direct-fetch evidence subsequently passed transport/integrity but exposed the v3 CPython-only wheel defect; the [v4 fix and exact Python 3.14.10 regression](56-sandbox-validation.md#python-minor-portability-fix--fixture-v4-2026-11-28) supersede that fixture limitation. The cloud host observations below remain historical and do not negate the user-host transport result. This review reconciles the accepted [benchmark matrix](53-sandbox-and-verification-design.md#proposed-benchmark-matrix), implementation and recorded evidence. It is not an approval to merge, a complete security audit of the project, or a claim that every proposed measurement exists.

## Acceptance and evidence

| Accepted area | Evidence / current result | Remaining boundary |
| --- | --- | --- |
| Namespace startup and unsupported setup | Successful live runs outside the tool filesystem wrapper; wrapped startup denial remains explicit | Wrapped denial is not proof the whole cloud host lacks namespaces |
| Read-only shell, finite pipelines and `head` | Startup/privacy/pipeline gates, preserved stage statuses and short-read drain behavior; shell measurements now retained | Executor output timestamps are not human-action/TUI repaint latency |
| Offline checks and prepared dependencies | Rust/Python plus pinned Node/pre-commit provisioning; [14-case provisioned report](benchmarks/sandbox-cache-footprint-2026-11-26.json.gz) passes 504 samples | Versions/corpus/build hashes are recorded; shared/debug timings are not controlled release guarantees |
| Representative package/hook graph | [Focused dependency report](benchmarks/sandbox-dependency-projects-2026-11-28.json.gz): 3 cases, 108 samples; npm transitive miss and actual Python hook rewrite covered | The hook uses a verified archive and reconstructed local Git mirror; no live GitHub clone or general online resolver is claimed |
| Explicit public artifact fetching | Digest/URL/address/size/redirect/HTTP/cancellation regressions; real verified-cache staging | Fresh direct-broker public DNS/TLS success is still missing; current tiny public probe fails at DNS |
| Formatter, failing checks, drift and guarded import | Copy-only edits, explicit native imports, fresh recheck, intentionally failed tests and manual-drift refusal | Native import is not a transaction covering arbitrary shell/manual edits |
| Git state | Synthetic all-files index and private mirror tested | Original staged/history/worktree/submodule semantics remain deliberately unsupported (GL-020), not implicitly equivalent |
| Output flood and blocked pipes | Bounded capture, explicit incomplete/unknown outcomes, cleanup regressions and recorded lifecycle/resource observations | No output-limit result is recategorized as an ordinary successful check |
| Descendants, `setsid`, cancellation and helper failure | Owned supervisor/ECHILD confirmation, deterministic child readiness, retained uncertainty/quarantine; original lifecycle regressions remain green | Kernel-uninterruptible cleanup can remain incomplete; exact per-PID inventories are not persisted in successful benchmark reports |
| Stop/UI feedback | Stop-request-to-owned-cleanup measured; silent streams stay null | Human input-to-render acknowledgement, full app shutdown timing and other UI phases remain unmeasured |
| Secrets/network/IPC | Benign sentinels, positive host TCP/Unix controls, denied sandbox connections and private/outside-path checks | These tests are scoped evidence, not a universal secret scanner or permission to expose host services |
| Resources and cache | Phase clocks, observed streams, Linux reaped-child CPU/largest-child RSS/block counters, bounded end-of-command regular-file footprints | Concurrent whole-tree peak memory and bytes written are not established; cache size and block counters must not be relabelled as those metrics |

## Review fixes

The isolation, private-path and stopped-descendant fixtures used an executor wrapper that returned raw output while discarding measurements already collected by the pipeline. They now retain the structured observations, including cleanup-confirmed stop errors. Generic fixture errors preserve attached observations too, without changing their failure gate.

The private-path fixture previously removed its test symlink even when execution returned cleanup-incomplete. It now keeps that fixture intact while a child may still own it; ordinary completed cleanup still permits removal. This changes fixture handling, not shell authority or production cleanup classification. Touched compact control flow was expanded for readability.

A new [native runtime report](benchmarks/sandbox-shell-observation-review-2026-11-28.json.gz) contains 504 samples. **12/14 gates passed, two were blocked** (`node_syntax_and_execution`, `local_precommit_system_hook`), with cleanup confirmed throughout. The overall gate is **incomplete**. It fills shell-observation coverage without silently substituting externally provisioned runtimes for unavailable system tools. The separate provisioned and focused reports above provide those runtime results. All 36 observations of each repaired shell case retained its execution/resource record; silent privacy/stop streams remain null. Historical reports are unchanged.

## Actual host observations

The [machine-readable host review](benchmarks/sandbox-validation-host-review-2026-11-28.json) was collected outside the outer tool filesystem wrapper, matching the execution context of successful sandbox runs:

- The ordinary HTTPS client, using the inherited cloud proxy, returned HTTP 200 for the pinned cfgv wheel; all 7,249 bytes matched the declared SHA-256.
- The direct broker integration failed before HTTP: `Dependency DNS resolver failed for files.pythonhosted.org: exit status: 2`. The diagnostic now names the requested hostname and explains that environment proxies are not used. No new proxy/DNS fallback, redirect policy, TLS exception or network grant was added.
- Outside the wrapper, cgroup2 is mounted read-write, but neither its root nor `cgroup.procs` is task-user-writable. No exclusive job cgroup is configured. The wrapper's read-only view must not be generalized to the actual benchmark path. This review does not read a whole-environment `memory.peak` and report it as a job peak, or change delegation/host security settings merely to obtain a number.

## Minimal compatible-host fetch check

The new test downloads one fixed 7 KiB public artifact using the broker, requires a fresh download event, verifies cached bytes and confirms no check/package execution event. It is ignored by default so ordinary tests do not acquire external network dependencies:

```sh
cargo test --locked --offline integration_public_fetch_verifies_pinned_bytes -- --ignored --nocapture
```

`--offline` applies to Cargo dependency resolution, not the explicitly selected test's HTTPS operation. This cloud's explicit execution of the test **failed at DNS**, and is recorded as an unmet environment-dependent gate. On a compatible direct-DNS/HTTPS host, a pass establishes this small fresh public transfer; it does not by itself prove large-file throughput or close every GL-035 item. After that, run the focused project smoke command from the [validation guide](56-sandbox-validation.md#focused-dependency-projects) without `--artifact-directory` to exercise real project-bundle fetches.

## Closure conditions

1. Obtain successful direct broker DNS/TLS/digest/provenance evidence on a compatible host; retain failures as well.
2. Complete the remaining requested UI/non-verification timings and aggregate memory/bytes-written observations, or explicitly revise their accepted scope with recorded rationale. Existing child RSS/cache-size/block counters cannot be substituted silently. Exclusive cgroup/accounting infrastructure needs its own supported-host/delegation design; enforcement remains GL-023.
3. Review the final evidence and remaining unsupported behaviors before marking GL-035 done or merging. The existing branch is still unmerged; prior successful subsets are not a blanket pass.

Validation for this review: 594 ordinary tests pass, eleven live/network/profiling tests are ignored by default, strict all-target Clippy/formatting/offline build pass. The explicitly selected public-fetch integration fails at the recorded DNS gate. No paid provider requests or host security configuration changes were made.
