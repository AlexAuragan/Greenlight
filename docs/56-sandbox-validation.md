# Synthetic sandbox validation (GL-035)

**Current status (2026-11-28):** functional GL-035 validation is complete after the user-host direct-fetch v4 smoke; UI profiling and aggregate accounting remain explicit GL-025/GL-023 work. Historical reports and pending-status notes below are not backfilled.

For current acceptance status and the distinction between completed subsets, host blockers and remaining measurements, see the [GL-035 review](57-sandbox-validation-review.md). Historical reports below retain their original scope and fields.

Implemented on `docs/sandbox-verification-plan`, pending merge. This runner tests production sandbox/verification/import paths against disposable trusted fixtures. It does not use an LLM or an API key, load project `.env`, edit the current project or execute fixture programs on the host as a fallback. The default mode does not download dependencies; optional public provisioning is described below.

## Run

On Linux with Bubblewrap and user namespaces enabled:

```sh
cargo build --release --locked
target/release/greenlight sandbox-benchmark --warm 30 --cold 5 > sandbox-report.json
```

For a quick compatibility check use `--warm 1 --cold 1`. Without `--report`, final JSON goes to stdout; progress and errors go to stderr. Use `--report PATH` to atomically save startup and sample checkpoints as well as the final report. The parent directory must already exist; symlink report targets are refused. Exit zero means every synthetic gate passed; a blocked, failed or incomplete gate returns nonzero after writing the report. An infrastructure error before report construction can return nonzero without JSON. A passed synthetic report does not complete the broader GL-035 matrix.

The suite uses its own random directories under `/tmp`, irrespective of `--root`. It explicitly authorizes only its fixed recipes. Runtime cases need Rust/Cargo plus the default C linker, Python 3 with venv, Node, Git and pre-commit available **inside the existing sandbox profile**. Installing a tool somewhere on host PATH does not automatically expose it to checks. No runtime installation is performed.

`--keep-failures` retains failed/blocked synthetic source and sentinel directories, reported on stderr and in JSON. Otherwise cleanly owned fixtures are removed. Unconfirmed cleanup always retains both directories and stops replacement execution. Do not delete an in-use retained fixture. Only benign generated sentinels are used; there are no real credentials in the corpus. Diagnostic previews are bounded and control-escaped; their hashes cover the complete diagnostic. Retained verification receipts provide fuller evidence.

## Cases and interpretation

The 14 cases cover namespace startup; bounded inspection with `head`; private project paths/outside symlinks; TCP and Unix-socket isolation with host positive controls; offline Rust, Python, Node and local pre-commit; immutable pinned fixture staging and a fresh venv; formatter copy/import/recheck; deterministic external drift and import refusal; an intentional failed check; output overflow; and cancellation of a TERM-ignoring session-escaped descendant.

A gate passes when its expectation is met. Thus an intentional failed check must remain failed, output overflow must remain incomplete, and a manual edit must refuse import. None becomes a successful product check merely because the harness passed. Missing runtimes remain blocked; other unexpected results remain failed. If isolation startup fails, downstream cases are explicitly not run, with no host fallback.

Dependency bytes in this suite are seeded locally with a synthetic manifest and verified through the real staging/bootstrap path. This is **not** evidence of a real public HTTPS/DNS download. The local pre-commit hook uses `language: system`; remote hook environments and registry installs are not covered. Original Git history, staged state and hooks remain unsupported, rather than approximated as equivalent by synthetic Git.

## Timing and reproducibility

Each case gets five independent cold projects, a separately recorded warmup and 30 warm samples on one synthetic project. Fixture reset and owner acquisition precede timing; operation checks and owner cleanup are included. Cold means fresh Greenlight state, not flushed OS/page/toolchain caches. Warm checks still receive fresh disposable verification caches and venvs. No public artifact cache is downloaded.

Reports record fixture revision/hash/size, executable SHA-256, package/build profile, kernel, Bubblewrap version, raw outcome/diagnostic hash, cleanup and retained paths. p50/p95 use nearest rank and separate cold/warm/warmup. Both all-attempt and met-expectation-only distributions are supplied; absent successful samples have null latency, not zero. Stop-request-to-cleanup is recorded separately. First-output timing, per-phase timing, peak RSS/CPU/disk measurements and TUI acknowledgement remain unmeasured. Debug timings below are diagnostic baselines, not release performance claims.

## Cloud results — 2026-11-26

The execution context matters: the default Codex tool filesystem wrapper denies Bubblewrap UID-map writes (`Read-only file system`). An approved execution outside that wrapper successfully used the **same production Bubblewrap path**, without host settings changes or a host execution fallback. Earlier documents' unsupported-host observations refer to the wrapped invocation, not a demonstrated kernel-wide limitation.

Full machine reports are gzip-compressed JSON:

- [Live run outside the tool wrapper](benchmarks/sandbox-validation-cloud-2026-11-26.json.gz): default 30 warm / 5 cold / 1 warmup per case; 504 samples, all cleanup confirmed, no retained fixtures. Debug binary; exact SHA and versions are in the report.
- [Default outer-wrapper probe](benchmarks/sandbox-validation-outer-wrapper-2026-11-26.json.gz): 1 warm / 1 cold / 1 warmup, isolation blocked, remaining cases not run. Same executable as the live run.

Extract with `gzip -dc REPORT.json.gz > report.json`. Reports retain failed/blocked attempts. The live report's overall gate is **failed**, despite its completed sample count.

| Case | Gate | Warm p50 / p95, ms (all attempts) |
| --- | --- | --- |
| Isolation startup | Passed | 9.7 / 23.9 |
| Inspection head pipeline | Passed | 16.0 / 21.3 |
| Private path boundaries | Passed | 9.1 / 10.1 |
| Network/IPC sentinels | Passed | 93.4 / 140.9 |
| Rust offline tests | Failed: linker `cc` unavailable | 188.9 / 293.6 |
| Python stdlib tests | Passed | 218.4 / 279.9 |
| Node | Blocked: runtime unavailable | 11.6 / 27.4 |
| Local pre-commit | Blocked: runtime unavailable | 11.9 / 16.7 |
| Pinned fixture / fresh venv | Passed | 282.4 / 338.5 |
| Formatter/import/recheck | Passed | 89.8 / 125.1 |
| External drift refusal | Passed | 49.2 / 60.1 |
| Intentional failed check | Passed expectation | 37.3 / 45.4 |
| Output limit cleanup | Passed expectation | 61.3 / 80.4 |
| Session-escaped child stop | Passed expectation | 62.4 / 67.6 |

Host `/usr/bin/cc` goes through `/etc/alternatives/cc`, hidden by the existing sandbox's selected `/etc` mounts. Host Node is under `/opt/codex/...`, outside its selected runtime mounts/PATH. Pre-commit is not installed. These are concrete compatibility/diagnostic findings tracked as GL-036; this implementation does not broaden mounts to make the benchmark pass.

GL-035 stays open for that runtime coverage, real pinned public fetch/provenance and representative dependency recipes, missing measurements and review of successful results. Resource enforcement remains GL-023.

## GL-036 follow-up — 2026-11-26

Fixed compiler aliases now use private `cc`/`c++` symlinks to canonical executable targets already under `/usr/bin`. A preliminary bind-at-new-path approach was rejected after a real run showed it broke GCC's relative plugin lookup. The final symlinks preserve that installation layout without exposing `/etc/alternatives`, arbitrary host PATH directories or new runtime trees. Targets outside `/usr/bin` are not aliased.

Preflight reports all missing declared runtimes, with diagnostic-only host PATH candidates and actionable scope explanations. It never executes or imports a candidate. The [recipe guide](54-isolated-verification.md) now declares Rust's linker explicitly and explains presence-check limits. Node under `/opt` is deliberately not automatically mounted; pre-commit still needs installation/provisioning. These remaining runtime coverage requirements stay in GL-035.

The [post-fix full report](benchmarks/sandbox-validation-runtime-fix-2026-11-26.json.gz) contains another 504 samples using the same namespace-capable execution context outside the outer tool wrapper. **12/14 cases passed**; Node/pre-commit are blocked, so the overall result is **incomplete**, not passed. Every sample confirmed cleanup; no fixtures were retained. The earlier failed report remains historical evidence. This run used a debug executable; exact hash and diagnostics are included.

| Case | Gate | Warm p50 / p95, ms (all attempts) |
| --- | --- | --- |
| isolation_startup | passed | 14.7 / 80.3 |
| inspection_head_pipeline | passed | 17.9 / 26.7 |
| readonly_secret_path_boundaries | passed | 10.0 / 11.0 |
| network_and_ipc_sentinels | passed | 108.6 / 157.5 |
| rust_offline_tests | passed | 237.5 / 448.4 |
| python_stdlib_tests | passed | 221.8 / 440.7 |
| node_syntax_and_execution | blocked | 14.8 / 22.5 |
| local_precommit_system_hook | blocked | 13.0 / 13.7 |
| pinned_fixture_and_fresh_venv | passed | 273.6 / 500.0 |
| formatter_import_recheck | passed | 83.9 / 96.9 |
| external_drift_import_refusal | passed | 53.0 / 63.1 |
| failed_check_is_not_pass | passed | 37.6 / 56.4 |
| output_limit_cleanup | passed | 55.6 / 73.1 |
| setsid_stop_cleanup | passed | 61.7 / 69.6 |

The ordinary suite passes 585 tests with eight live/profiling tests ignored. The new missing-runtime preflight integration was also run explicitly with namespaces enabled and passed: both names were reported, the check stayed not-started, and no approval was requested. Strict all-target Clippy and formatting pass. No provider calls, public downloads, runtime installations or host configuration changes were used.

## Opt-in provisioned runtime fixtures

The default benchmark still performs no public downloads. To test the artifact broker and fresh offline runtime setup explicitly:

```sh
greenlight sandbox-benchmark --public-dependencies --warm 1 --cold 1 > public-smoke.json
```

This replaces the installed-Node and installed-pre-commit cases with pinned provisioning cases. It uses the production broker to fetch the [compiled fixture pins](../src/sandbox_validation_dependencies.json), then extracts Node or installs pre-commit and its complete pinned wheel set **inside** each fresh network-isolated check. It never installs host packages or imports arbitrary host runtimes. `requires` lists bootstrap tools, not tools that setup has yet to install. Pin selection is fixed, not registry discovery during a benchmark.

The original v1–v3 fixtures targeted Linux x86_64, Node 22.15.0 and CPython 3.13 (their PyYAML wheel was ABI-specific), with pre-commit 4.2.0. Current v4 uses a pure-Python source build; see the portability fix below. These are reproducible test versions, not application deployment recommendations. Node needs `tar`/`xz`; pre-commit needs Python's `venv` and system Git. The pre-commit hook is local with `language: system`; remote hook provisioning is not covered. Node tests syntax/execution of a zero-dependency fixture, not an npm package graph. The full pin metadata includes URLs and SHA-256; wheel digests came from the corresponding PyPI version JSON, and Node's digest from `https://nodejs.org/dist/v22.15.0/SHASUMS256.txt`.

In this cloud environment the direct broker attempt fails: its supervised DNS resolver returns exit 2 for the public endpoints. The configured cloud proxy can download them using ordinary HTTPS clients, but the broker's direct destination validation cannot succeed through that route. Proxy-only networks remain unsupported; the benchmark does not weaken address pinning, TLS checks or redirect rules to make a test pass.

For a separate offline provisioning test, obtain the exact manifest files externally and verify their sizes/digests, then explicitly supply that directory:

```sh
greenlight sandbox-benchmark --public-dependencies \
  --artifact-directory /absolute/path/to/reviewed-artifacts \
  --warm 30 --cold 5 > provisioned-report.json
```

Each file must have the exact declared basename. The runner rejects symlinks, oversized files and digest mismatches before fetching or running setup. It copies verified bytes into private fixture caches; the real broker then verifies cache reuse and the check stages private read-only `/deps`. Missing or invalid supplied files do not fall back to fetching. Artifacts are never extracted or executed on the host. `--artifact-directory` requires `--public-dependencies`.

Reports include the pin manifest, explicit `artifact_origin`, and mode-specific cache definitions. External-artifact mode measures copying/hash verification, broker cache verification, fresh offline setup/check/cleanup. It does **not** measure or establish a successful broker DNS/TLS/public download. Warm samples currently reverify and copy supplied files too; their cost is included. Direct-fetch cold projects have empty artifact caches, while direct-fetch warm samples reuse verified objects and still reinstall each runtime afresh.

### Recorded provisioned-runtime results — 2026-11-26

- [Full externally provisioned run](benchmarks/sandbox-validation-provisioned-public-2026-11-26.json.gz): **14/14 cases passed**, 504 samples (5 cold, 1 warmup, 30 warm per case). All cleanup confirmed; no job fixtures retained. External artifact source files remain caller-owned and are not deleted.
- [Direct broker smoke run](benchmarks/sandbox-validation-direct-public-2026-11-26.json.gz): 12/14 cases passed; both public provisioning cases blocked at the supervised DNS resolver, overall **incomplete**. This remains evidence of failed direct fetching, not an artifact integrity or offline-runtime failure.

Both ran outside the outer tool filesystem wrapper so production Bubblewrap could create namespaces. Public files were obtained by task tooling through the configured cloud HTTPS proxy, bounded and checked against the committed manifest, then supplied explicitly. No downloaded package code ran on the host and no system runtime was installed. Reports retain the executable SHA, pins, artifact origin, exact failures and all-attempt distributions. The public-case preparation expectation includes the broker's cache-verification path; `artifact_origin` identifies whether downloading was actually exercised.

| Provisioned case | Cold p50 / p95, ms | Warm p50 / p95, ms |
| --- | --- | --- |
| public_node_offline | 4329.8 / 4714.9 | 4095.3 / 4329.5 |
| public_precommit_offline | 4314.0 / 4441.6 | 4133.4 / 4662.2 |

These are debug/shared-host measurements, including copying/hashing and fresh installation. Builds/tests ran during part of collection; use them as functional/setup-cost evidence, not a controlled release performance comparison. First-output/per-phase timings, peak resource accounting, representative remote-hook/npm dependency graphs, and a successful direct broker public fetch still remain GL-035. Resource enforcement remains GL-023.

Validation passes 587 ordinary tests (eight live/profiling tests ignored), strict all-target Clippy, formatting and an offline build. Added regressions validate the fixed artifact manifests and reject corrupt/symlink-supplied artifacts before any preparation/check event; CLI parsing refuses an artifact directory without explicit public-dependency mode. No paid provider calls were used.

## Verification phase measurements

Benchmark verification samples now copy the primary receipt's [`phase_ms`](54-isolated-verification.md#phase-timing-evidence), keeping absent phases absent. Each cold/warm/warmup summary reports `measured_samples`, p50 and p95 per phase across **all measured attempts**, including failed/blocked attempts. A phase that only some attempts reached has a smaller count, rather than zero-filled observations. These distributions do not filter to passing checks.

Public provisioning cases additionally measure `artifact_preparation` (the broker call, including verified cache reuse or an actual attempted download) and, when explicitly supplied, `external_artifact_verification_and_copy`. Their origin remains labelled; measuring a DNS failure does not establish download success. Hard external-input validation errors may return before a phase observation is available.

Sample phase maps are partial evidence, not a sum guaranteed to equal sample elapsed time. In multi-operation cases such as formatter/import/recheck, the primary check's receipt supplies the sample map; subsequent checks have their own recorded receipts. Non-verification shell/inspection phases, first output, peak resources and UI stop acknowledgement remain unmeasured. Historical compressed reports retain their original fields and are not backfilled.

### Phase measurement results — 2026-11-26

The [full provisioned phase report](benchmarks/sandbox-phase-provisioned-2026-11-26.json.gz) passes **14/14 cases, 504 samples**, with all cleanup confirmed and no retained job fixtures. It uses the same externally supplied pinned files and namespace-capable execution context as the preceding run. It is not evidence of successful direct fetching. Each provisioning phase below has 30 measured warm samples; timings include process launch/capture/cleanup where applicable.

| Provisioned case | Phase | Warm p50 / p95, ms |
| --- | --- | --- |
| public_node_offline | artifact_preparation | 511.2 / 640.1 |
| public_node_offline | dependency_staging | 500.9 / 685.2 |
| public_node_offline | offline_setup | 2179.0 / 2414.1 |
| public_node_offline | check_execution | 54.9 / 74.6 |
| public_precommit_offline | artifact_preparation | 132.5 / 171.9 |
| public_precommit_offline | dependency_staging | 128.6 / 165.6 |
| public_precommit_offline | offline_setup | 2568.1 / 2779.2 |
| public_precommit_offline | check_execution | 880.0 / 962.1 |

The [direct broker failure probe](benchmarks/sandbox-phase-direct-failure-2026-11-26.json.gz) remains incomplete: both public cases record `artifact_preparation` duration and omit `check_execution`, because DNS failed before checks began. No zero-filled or invented command timings are supplied.

Validation: 587 ordinary tests pass (nine live/profiling tests ignored), strict all-target Clippy, formatting and offline build pass. The new live setup/check failure regression was run explicitly and passed, including persisted receipt phase keys. Existing regressions now verify legacy timing absence, numeric round-trip, reached/absent phase boundaries, and summary coverage without zero-filling. These remain debug/shared-host functional measurements; compilation/tests overlapped part of collection. First-output latency, non-verification phases, peak resource accounting and UI acknowledgement are still unmeasured.

## Output/resource observations

Verification samples now retain the primary receipt's [`executions`](54-isolated-verification.md#output-latency-and-linux-child-accounting); the finite inspection pipeline retains its execution observation too. Other shell cases can still have no retained observation. Earlier reports are unchanged. All values describe subprocess execution, not application/TUI responsiveness, and are separate from the coarse `phase_ms` clocks.

Each cold/warm/warmup summary adds `execution_statistics`. Keys identify the execution phase, stream/counter, and zero-based stage where applicable; every statistic has `measured_samples`, `unit`, `p50`, and `p95`. Null stream times and missing resource records are omitted from distributions, rather than filled with zero. Genuine zero counters are retained. These are all-measured-attempt distributions, including failed checks and cleanup-confirmed output-limit errors. The raw per-stage resource records remain available; no aggregate peak RSS is inferred.

### Recorded output/resource results — 2026-11-26

The [full output/resource report](benchmarks/sandbox-output-resource-2026-11-26.json.gz) passes **14/14 cases, 504 samples**, all cleanup confirmed, no retained job fixtures. Every retained launched-stage observation in this run has a valid resource record. It uses externally supplied pinned artifacts and real Bubblewrap outside the outer tool filesystem wrapper; direct broker download success remains unvalidated. Exact executable/fixture hashes and all raw observations are retained.

| Provisioned case | Execution phase | First stdout warm p50 / p95, ms | User CPU warm p50, ms | Largest-child RSS warm p95, KiB |
| --- | --- | --- | --- | --- |
| public_node_offline | offline_setup | Not observed | 1226.8 | 66432 |
| public_node_offline | check_execution | 9.9 / 11.6 | 28.4 | 41908 |
| public_precommit_offline | offline_setup | 1527.8 / 1907.0 | 2354.0 | 68836 |
| public_precommit_offline | check_execution | 393.3 / 786.7 | 819.4 | 29836 |

The table uses stage 0, the sole stage of each listed invocation, with 30 warm resource observations per row. Silent Node extraction has no stdout observation. The finite inspection pipeline retains two separate stage records, not an invented combined memory peak. Output-limit fixtures retain measured output/usage after cleanup while preserving unknown/incomplete check outcomes as their expected gate behavior.

Validation passes 589 ordinary tests (nine live/profiling tests ignored), strict all-target Clippy, formatting and offline build. The live failed-setup/failed-check persistence test passes explicitly. Added tests cover silent/stdout/stderr-only and nonzero commands, output-flood error metadata, pending/malformed/truncated lease records and summary coverage without zero-filling. Cleanup quarantine and existing cancellation/lifecycle regressions remain intact. This is shared-cloud/debug evidence; builds and tests overlapped part of collection, so it is not a controlled release performance comparison.

Remaining measurement work includes shell cases without retained observations, user-action/TUI feedback latency, non-verification phases, aggregate concurrent tree memory, cache footprint and byte-level disk accounting. These are separate from enforced quotas (GL-023). GL-035 also retains direct-fetch, broader dependency-fixture and evidence-review requirements.

## Retained-cache measurements

Verification samples now copy the primary receipt's [`cache_storage`](54-isolated-verification.md#retained-private-cache-footprint). Summaries add `cache.unique_regular_files`, `cache.logical_regular_file_bytes` and `cache.allocated_regular_file_bytes` under `execution_statistics`, with units, measured sample counts and p50/p95. Only complete footprints enter these distributions. Unavailable scans are omitted, not filled with zero; measured empty/sparse allocation values remain genuine zeros. `phase_ms.cache_inspection` includes the attempted scan's elapsed time even when no footprint is available.

This is retained regular-file metadata, not cache writes, total filesystem occupancy or resource enforcement. Symlinks and special files are skipped, hard links are deduplicated, and budgets can leave an observation unavailable without failing the underlying check. Original compressed reports remain unchanged.

### Recorded cache-footprint results — 2026-11-26

The [full cache-footprint report](benchmarks/sandbox-cache-footprint-2026-11-26.json.gz) passes **14/14 cases, 504 samples**, all cleanup confirmed. Every attempted footprint observation completed within its budget in this run; no job fixtures were retained. Externally supplied pinned artifacts and real namespace-capable Bubblewrap are used as before. Direct broker download success remains unvalidated.

| Provisioned case | Warm complete footprints | Logical regular-file size p50, MiB | Allocated regular-file size p50 / p95, MiB | Inspection p50 / p95, ms |
| --- | --- | --- | --- | --- |
| public_node_offline | 30 | 177.07 | 187.83 / 187.83 | 44.71 / 54.61 |
| public_precommit_offline | 30 | 21.57 | 24.64 / 24.64 | 11.63 / 26.65 |

These values describe files remaining in `/cache` after command completion, before normal disposable-cache removal. They are not permanent project-cache growth, bytes written, peak storage, total device occupancy or resource limits. The report keeps original integer byte counters, counts and scan-unavailability fields. This remains shared-cloud/debug evidence with builds/tests overlapping part of collection.

Validation passes 593 ordinary tests (ten live/profiling tests ignored), strict all-target Clippy, formatting and offline build. Both live regressions were also run explicitly: persisted failed setup/check receipts keep their observations, and an over-depth cache returns an unavailable footprint while its underlying check remains clean/passed with live inputs matched. Unit tests cover sparse files, hard links, outside symlinks, sockets, replaced-root anchors, measured empty caches, entry/depth/time budgets and summary coverage without zero-filling. No model calls, public downloads, host installations or security-configuration changes were required for this validation.

## Focused dependency projects

`--dependency-projects` selects isolation preflight plus two richer dependency cases. It requires `--public-dependencies` and replaces the normal 14-case selection for this invocation, rather than repeating the completed baseline:

```sh
greenlight sandbox-benchmark --public-dependencies --dependency-projects \
  --artifact-directory /absolute/path/to/reviewed-artifacts \
  --warm 1 --cold 1 > dependency-project-smoke.json
```

Omit `--artifact-directory` to exercise actual broker fetching on a compatible direct-DNS/HTTPS network. The cloud network limitation still applies. Default counts (30 warm / 5 cold, plus one warmup) produce 108 observations across the three selected cases. Public artifact verification/copying and fresh installation are included in timings. No host runtime installation or package execution is performed.

The original focused source corpus/artifact bundle was fixture revision v3; current [source corpus](../src/sandbox_validation_projects.json) and [pins](../src/sandbox_validation_dependencies.json) are v4 after the portability fix. Reports identify focused mode and include only the selected artifact groups. Older compressed reports retain their original corpus revisions and are not backfilled. Node remains pinned at 22.15.0; the v3 Python bundle targeted CPython 3.13/Linux x86_64; v4 removes that ABI-specific wheel. These are reproducible validation versions, not deployment recommendations.

### npm graph and offline miss

The project locks `is-odd` 3.0.1 with transitive `is-number` 6.0.0. Their exact registry tarball URLs and published SHA-512 integrity values are in `package-lock.json`; preparation additionally pins each tarball's SHA-256. Task-tool downloads were checked against both hashes. The sandbox seeds a fresh npm content cache from read-only `/deps`, then runs `npm ci --offline --ignore-scripts --no-audit --no-fund` against the lock.

`npm test` actually exercises both modules and writes a private success marker only after assertions pass. The fixture's preinstall marker must stay absent. This validates the fixture's disabled lifecycle script; it is not a new global package-manager policy.

A second private project receives a cache containing only `is-odd`. Its offline install must fail specifically with `ENOTCACHED` for `is-number`. Any unexpected success or unrelated error fails the benchmark. This tests missing-transitive-dependency feedback without permitting network access or claiming the project's source is defective. Generated `node_modules` remains inside the disposable copy and is excluded from live-source imports by the existing scope policy.

### Python hook environment from pinned upstream source

The Python case uses upstream pyupgrade 3.19.1, revision `ce40a160603ab0e7d9c627ae33d7ef3906e2d2b2`. Its codeload archive SHA-256 is `d6fce02b9ceb5ca96f269089b3beb36447e84802472ae1d41a7f2206ebc28cca`. The bundle includes pre-commit's complete pinned wheel set plus setuptools 80.9.0, wheel 0.45.1, packaging 25.0 and tokenize-rt 6.1.0. Exact wheel URLs/digests come from their versioned PyPI metadata.

Inside the sandbox, setup extracts that archive and creates a fresh Git repository with deterministic fixture author/date and tag `fixture-source-v3.19.1`. It is a **reconstructed local mirror**, not the upstream repository's original Git history/commit identity. The source identity comes from the declared upstream revision and verified archive digest. Git templates/hooks/global configuration and original project metadata stay excluded.

The source configuration points pre-commit at `file:///cache/hook-repo` with a real `language: python` hook. Only file-protocol Git is allowed; pip uses `PIP_NO_INDEX` and `/deps` as its offline wheel source. Pre-commit clones the local mirror, builds the pinned source and creates its own Python virtualenv. The fixture locates that environment and checks the installed pyupgrade/tokenize-rt versions, so a system executable cannot silently substitute for hook installation.

The positive check passes against the original Python fixture. A negative fixture must then be rewritten and rejected by the real hook; the benchmark checks both the framework's modification diagnostic and changed bytes before restoring the original disposable source. The final receipt must still report no source changes and matched live inputs. No live project file is written.

Hook environment installation happens on the first pre-commit invocation inside `check_execution`; `offline_setup` covers the framework and reconstructed mirror. Each sample starts fresh. This validates the framework's repository-install/build/environment path using an offline mirror, not a live GitHub clone or arbitrary online dependency resolution. Original staged/history/worktree semantics remain unsupported, and other package graphs/platforms still need their own complete recipes.

### Recorded dependency-project results — 2026-11-28

The [focused full report](benchmarks/sandbox-dependency-projects-2026-11-28.json.gz) passes **3/3 cases, 108 samples**: isolation plus the two new dependency projects, each with five cold samples, one separately recorded warmup and 30 warm samples. This supplements the historical 14-case baseline instead of claiming it was rerun. Every sample confirmed cleanup, every attempted footprint was complete, and no job fixtures were retained.

| Dependency project | Cold p50 / p95, ms | Warm p50 / p95, ms | Warm check-execution p50 / p95, ms |
| --- | --- | --- | --- |
| npm_dependency_project | 5683.0 / 5742.3 | 5813.2 / 6351.3 | 848.4 / 916.8 |
| python_hook_project | 10237.2 / 10365.8 | 9764.7 / 10314.4 | 6471.1 / 7049.5 |

Every npm sample exercised the positive graph and the specific missing-transitive `ENOTCACHED` failure, with the lifecycle marker absent. Every Python sample verified the hook environment's actual versions, observed the real rewrite rejection, restored the disposable fixture and returned a clean aggregate receipt. Successful aggregate benchmark commands include assertions about these expected internal failures; they do not recategorize a failed application check as passed.

The cloud run used reviewed externally obtained artifacts, the production broker's verified cache path, fresh offline setup and real Bubblewrap outside the outer tool wrapper. Source bytes were downloaded/verified by task tooling; package/hook code executed only inside the sandbox. This is **not** a successful direct broker fetch, live GitHub clone, original Git-history check, or general package resolver. Reports retain selected pins, source revision/hash, executable hash, phase/output/resource/footprint observations and all-attempt distributions. Timings are debug/shared-host evidence, not a controlled release performance comparison.

Validation passes 594 ordinary tests (ten live/profiling tests ignored), strict all-target Clippy, formatting and offline build. The existing recipe regression now covers all four provisioning profiles, and a new lock/manifest test checks the two-module npm graph's URLs/versions and integrity metadata. CLI parsing rejects focused dependency mode without explicit public-dependency consent. The normal 14-case selection remains available without this flag.

## Shell-observation review follow-up — 2026-11-28

Isolation, private-path and stopped-descendant samples now retain the executor measurements they previously discarded. [The native-runtime run](benchmarks/sandbox-shell-observation-review-2026-11-28.json.gz) collected 504 samples: 12/14 gates passed, two system-runtime cases blocked, all cleanup confirmed. Overall status is incomplete; it is not substituted for the separate provisioned runtime reports. Missing output from silent commands remains null. The [acceptance review and host observations](57-sandbox-validation-review.md) reconcile this evidence with the public DNS failure and lack of task-user cgroup delegation.

## Python-minor portability fix — fixture v4, 2026-11-28

A user-run direct test exposed a fixture defect: artifact fetching and npm passed on Python 3.14.10, but the Python hook bundle contained a CPython 3.13-only PyYAML wheel. pip correctly refused that wheel. This was an incomplete fixture, not a reason to change the user's Python, enable network inside checks, or weaken wheel compatibility. The [sanitized user evidence summary](benchmarks/sandbox-user-direct-fetch-summary-2026-11-28.json) records reported results without publishing their raw host report. It establishes user-reported fresh broker transfer/integrity success for the small probe and both bundle preparations; the v3 hook run itself remained blocked.

Fixture v4 replaces that wheel with pinned PyYAML 6.0.3 source. Its build tools (setuptools/wheel/packaging) are separately pinned and installed offline in the disposable venv. Setup explicitly disables LibYAML (`PYYAML_FORCE_LIBYAML=0` and `--without-libyaml`), builds a `py3-none-any` wheel, installs it locally, and asserts both the version and that the C extension is absent. No compiler/Cython, host installation, ABI guessing, unpinned registry resolution or networked check is needed. The source archive/build tools remain verified `/deps` inputs; the locally built wheel is a job-local build output, not claimed to have a published wheel digest.

The ordinary framework wheel lock remains hash-required and excludes source archives. All remaining wheel pins in the Python profiles are interpreter-independent `none-any` wheels; a regression rejects ABI-specific wheel pins. The basic pre-commit profile has 13 artifacts and the Python-hook profile 15, within the unchanged limit of 16. Node's Linux x86_64 restriction remains. Python 3.13.5 and 3.14.10 are tested; these results do not claim every interpreter/platform combination.

Evidence:

- [v4 focused smoke on system Python 3.13.5](benchmarks/sandbox-pureyaml-smoke-2026-11-28.json.gz): all three cases passed, nine cold/warmup/warm observations, cleanup confirmed.
- [Exact Python 3.14.10 hook regression](benchmarks/sandbox-python314-portability-2026-11-28.json): source build, framework, hook environment/version checks, positive check and actual rewrite rejection/restoration all passed; no source modifications, matched live inputs and confirmed cleanup. This is one functional sample, not a new 30/5 timing matrix.

The exact-version regression stages a pinned standalone Python archive only inside `/cache`. Its [test-only runtime record](../src/sandbox_validation_python314.json) identifies the upstream URL/hash and a gzip-to-xz recompression of the identical tar stream. Task tooling did not extract or execute it on the host. The recompressed 26.9 MB artifact fits the existing 32 MiB limit and is the 16th test artifact, so no limit was raised. This runtime is not part of normal benchmark downloads and does not replace host Python.

Developers can reproduce the opt-in exact-version test with a reviewed artifact directory containing the normal Python-hook pins plus `python314.tar.xz` matching the test record:

```sh
GREENLIGHT_VALIDATION_ARTIFACT_DIRECTORY=/absolute/path/to/reviewed-artifacts \
  cargo test --locked --offline integration_python314_hook_with_external_runtime -- --ignored --nocapture
```

Ordinary users should instead pull/build the fix and rerun their normal focused smoke command, without an external artifact directory, against their existing Python. Fresh end-to-end v4 fetching on that user's host remains to be confirmed. The existing 108-sample v3 report remains historical evidence and is not backfilled. Ordinary validation passes 595 tests, with twelve live/network/profiling tests ignored; strict Clippy, formatting and offline build pass.

## Interrupted-run diagnostics — 2026-11-28

A user-host v4 run produced no case banner and was interrupted before final JSON was emitted. The cause of that startup stall is not yet established. The benchmark now skips project-root/config discovery and prints startup stages before executable hashing, fixture/SQLite creation, Bubblewrap probing, and each sample. Executable hashing has a five-second best-effort budget checked between reads; a blocked filesystem syscall cannot be interrupted by this budget. Missing executable identity is recorded explicitly, never as a partial digest.

For the next diagnostic run, keep stderr visible and use checkpoint output:

```sh
./target/debug/greenlight sandbox-benchmark \
  --public-dependencies --dependency-projects \
  --warm 1 --cold 1 --report dependency-diagnostic.json
```

A checkpoint with `gate_status: "running"` is unfinished evidence. `current_phase` identifies the last entered stage; completed samples remain available. An interruption inside a sample does not establish that sample's outcome or cleanup. Setup errors leave the last checkpoint rather than inventing a completed report. Checkpoint writes use the existing anchored atomic-file mechanism, and a write error stops the run.

The invocation supervisor now enters a separate process group, so terminal Ctrl+C reaches Greenlight without also killing its cleanup owner. Closing Greenlight's control pipe then triggers the existing owned-child teardown and reaping. This improves interruption cleanup; it does not make a partial report proof of confirmed cleanup.

Validation for this diagnostic change: 597 ordinary tests pass (12 ignored); strict Clippy, formatting and offline build pass. A live focused 1/1 run with externally supplied verified artifacts passed all three cases/nine samples with confirmed cleanup. A separate controlled SIGINT check preserved valid startup JSON, and a symlink report target was rejected without changing its destination. This validates reporting/cleanup behavior, not the unresolved user-host stall or fresh direct downloads.

## User-host direct v4 confirmation — 2026-11-28

The final user-supplied report has `current_phase: Finished` and `gate_status: passed`. All three focused cases passed cold/warmup/warm, with cleanup confirmed for all nine samples and no retained fixtures. npm project totals were 3.33–3.86 seconds per sample; Python hook totals were 11.28–12.60 seconds. These are debug-build smoke observations, not controlled release latency guarantees. The report uses direct production-broker public preparation, and its public pins match the current recipes. The Python version (3.14.10) was supplied separately by the user rather than inferred from a benchmark field.

The [sanitized summary](benchmarks/sandbox-user-v4-smoke-summary-2026-11-28.json) retains case outcomes, timings, identity hashes and scope. Its source-report hash identifies the original attachment, which is not published with personal host paths/diagnostics. This result closes the pending user-host v4 smoke; it does not establish the cause of the preceding interrupted startup run. The [closure review](57-sandbox-validation-review.md#closure-review-and-scope-revision--2026-11-28) records the explicit GL-023/GL-025 measurement follow-ups.
