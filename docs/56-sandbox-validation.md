# Synthetic sandbox validation (GL-035)

Implemented on `docs/sandbox-verification-plan`, pending merge. This runner tests production sandbox/verification/import paths against disposable trusted fixtures. It does not use an LLM or an API key, load project `.env`, edit the current project or execute fixture programs on the host as a fallback. The default mode does not download dependencies; optional public provisioning is described below.

## Run

On Linux with Bubblewrap and user namespaces enabled:

```sh
cargo build --release --locked
target/release/greenlight sandbox-benchmark --warm 30 --cold 5 > sandbox-report.json
```

For a quick compatibility check use `--warm 1 --cold 1`. The JSON report goes to stdout; progress and errors go to stderr. Exit zero means every synthetic gate passed; a blocked, failed or incomplete gate returns nonzero after writing the report. An infrastructure error before report construction can return nonzero without JSON. A passed synthetic report does not complete the broader GL-035 matrix.

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

The fixtures target Linux x86_64, Node 22.15.0 and CPython 3.13 (the PyYAML wheel is ABI-specific), with pre-commit 4.2.0. These are reproducible test versions, not application deployment recommendations. Node needs `tar`/`xz`; pre-commit needs Python's `venv` and system Git. The pre-commit hook is local with `language: system`; remote hook provisioning is not covered. Node tests syntax/execution of a zero-dependency fixture, not an npm package graph. The full pin metadata includes URLs and SHA-256; wheel digests came from the corresponding PyPI version JSON, and Node's digest from `https://nodejs.org/dist/v22.15.0/SHASUMS256.txt`.

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
