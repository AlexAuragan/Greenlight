# Synthetic sandbox validation (GL-035)

Implemented on `docs/sandbox-verification-plan`, pending merge. This runner tests production sandbox/verification/import paths against disposable trusted fixtures. It does not use an LLM or an API key, load project `.env`, edit the current project, download dependencies or execute fixture programs on the host as a fallback.

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
