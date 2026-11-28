# GL-035 validation review — 2026-11-28

**Current disposition: functional validation complete; branch ready for final checks and merge.** The user-host [v4 smoke summary](benchmarks/sandbox-user-v4-smoke-summary-2026-11-28.json) records fresh direct HTTPS provisioning and offline execution of both dependency projects: **3/3 cases, nine samples, all cleanup confirmed**, no retained fixtures. Reviewed pins match the repository. Python 3.14.10 was reported separately by the user; the independent exact-version regression remains separate evidence. The successful run does not explain the preceding interrupted startup stall.

This review reconciles the accepted [benchmark matrix](53-sandbox-and-verification-design.md#proposed-benchmark-matrix), implementation and recorded evidence. It is a scoped review of the sandbox stack, not an exhaustive project security audit. Earlier sections/reports retain their original failure gates and collection contexts. The scope revision below makes deferred measurements explicit rather than calling them complete.

## Acceptance and evidence

| Accepted area | Evidence / current result | Remaining boundary |
| --- | --- | --- |
| Namespace startup and unsupported setup | Successful live runs outside the tool filesystem wrapper; wrapped startup denial remains explicit | Wrapped denial is not proof the whole cloud host lacks namespaces |
| Read-only shell, finite pipelines and `head` | Startup/privacy/pipeline gates, preserved stage statuses and short-read drain behavior; shell measurements now retained | Executor output timestamps are not human-action/TUI repaint latency |
| Offline checks and prepared dependencies | Rust/Python plus pinned Node/pre-commit provisioning; [14-case provisioned report](benchmarks/sandbox-cache-footprint-2026-11-26.json.gz) passes 504 samples | Versions/corpus/build hashes are recorded; shared/debug timings are not controlled release guarantees |
| Representative package/hook graph | [Focused dependency report](benchmarks/sandbox-dependency-projects-2026-11-28.json.gz): 3 cases, 108 samples; npm transitive miss and actual Python hook rewrite covered | The hook uses a verified archive and reconstructed local Git mirror; no live GitHub clone or general online resolver is claimed |
| Explicit public artifact fetching | Digest/URL/address/size/redirect/HTTP/cancellation regressions; real verified-cache staging | User-host fresh direct broker fetch/provisioning passed in the v4 smoke; the cloud tiny probe still fails at direct DNS. User evidence is supplied, not a cloud reproduction |
| Formatter, failing checks, drift and guarded import | Copy-only edits, explicit native imports, fresh recheck, intentionally failed tests and manual-drift refusal | Native import is not a transaction covering arbitrary shell/manual edits |
| Git state | Synthetic all-files index and private mirror tested | Original staged/history/worktree/submodule semantics remain deliberately unsupported (GL-020), not implicitly equivalent |
| Output flood and blocked pipes | Bounded capture, explicit incomplete/unknown outcomes, cleanup regressions and recorded lifecycle/resource observations | No output-limit result is recategorized as an ordinary successful check |
| Descendants, `setsid`, cancellation and helper failure | Owned supervisor/ECHILD confirmation, deterministic child readiness, retained uncertainty/quarantine; original lifecycle regressions remain green | Kernel-uninterruptible cleanup can remain incomplete; exact per-PID inventories are not persisted in successful benchmark reports |
| Stop/UI feedback | Stop-request-to-owned-cleanup measured; silent streams stay null | Human input-to-render acknowledgement, full app shutdown timing and non-verification phases remain unmeasured; tracked under GL-025 |
| Secrets/network/IPC | Benign sentinels, positive host TCP/Unix controls, denied sandbox connections and private/outside-path checks | These tests are scoped evidence, not a universal secret scanner or permission to expose host services |
| Resources and cache | Phase clocks, observed streams, Linux reaped-child CPU/largest-child RSS/block counters, bounded end-of-command regular-file footprints | Concurrent whole-tree peak memory and bytes written are not established; supported-host accounting and enforcement design remain GL-023. Cache size/block counters are narrower observations |

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

## Closure review and scope revision — 2026-11-28

1. **Functional containment and result integrity:** the existing matrices cover namespace startup/denial, privacy/network/IPC, bounded output/pipelines, runtime availability, offline preparation, failed checks, copied formatter changes, source drift/import guards, escaped descendants, cancellation and cleanup uncertainty. Success and unsupported cases remain separate. No host fallback or broader shell approval was introduced.
2. **Direct transport and Python portability:** the small direct user probe, user-host v3 bundle preparation, exact Python 3.14.10 regression, and final v4 direct-fetch smoke close the previous compatible-host transport/fixture gates. The final smoke is nine observations, not a statistically representative performance matrix. Historical 30/5 matrices identify their older fixtures and remain unchanged.
3. **UI performance moved to GL-025:** executor timestamps establish when bytes reached Greenlight and when owned cleanup completed. They cannot measure user input, event-loop scheduling, terminal repaint or full shutdown. Those require a separate synthetic interactive profiling harness and representative workload/terminal sizes. Keep the provisional 100/250 ms goals as goals, not achieved guarantees. Non-verification preparation/inspection phases belong in that same end-to-end harness. Existing correctness tests and bounded cleanup still apply.
4. **Aggregate accounting moved to GL-023:** a valid concurrent tree peak/write metric needs exclusive ownership and supported delegated accounting across descendant lifetimes. The current cloud has no task-owned exclusive delegated cgroup. Polling `/proc`, summing child high-water marks, or reading environment-wide counters would supply a misleading total. Design accounting and resource limits together; do not change host delegation just to fill a benchmark field. Current CPU/largest-child RSS/block counters and final cache footprints remain accurately labelled.
5. **Known unsupported behavior:** original staged/index/history/worktree semantics remain GL-020; live GitHub clone/general registry resolution remain outside the offline pinned recipe contract. The pinned hook mirror is explicitly reconstructed. These are capability boundaries, not failed claims of supported execution.

This narrows completion of GL-035 to the reviewed functional validation and existing measured observations. It does **not** delete the original requested metrics: their acceptance details now live in GL-023 and GL-025 in the canonical TODO. No execution guard, timeout, byte bound or quarantine condition is relaxed by this planning change.

## Final code review

Reviewed supervisor ownership/control-FD lifetime, detached process groups, cancellation/reaping and quarantine; bounded pipeline capture/status handling; HTTPS URL/address pinning and digest/budget validation; offline staging and recipe fingerprint binding; copied-source capture, retained cleanup uncertainty and guarded native import. Existing regression coverage exercises these boundaries. Removed a duplicated assignment in the edit-retention overflow path. The final parallel suite exposed a lock-release fixture race: dropping a descriptor need not release a flock until a concurrently forked pre-exec copy closes too. The fixture now explicitly unlocks before asserting recovery; production fail-closed recovery is unchanged, and separate application-exit/escaped-descendant tests retain real cleanup coverage. Benchmark non-passing output now refers to the actual report rather than permanently declaring a TODO item open.

No additional merge-blocking defect was found in those reviewed paths. This is a scoped review, not a claim that every line of the application was audited. Runtime namespace tests must still be selected explicitly on a capable host; ordinary tests do not silently acquire network access or paid model usage.

The validation counts below are historical to the earlier review, not the final suite result: 594 ordinary tests passed, eleven live/network/profiling tests were ignored; strict Clippy/formatting/offline build passed. Final checks are recorded in [validation](06-validation.md).
