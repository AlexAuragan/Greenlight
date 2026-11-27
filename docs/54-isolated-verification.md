# Offline checks in isolated verification workspaces

GL-027 implements named checks on a writable disposable source copy. The original project stays read-only to shell commands. No check can access original Git metadata, host user services, credentials or the network. This is separate authority from an ordinary shell grant.

## Configure a named recipe

Add checks to the project's host-owned `.greenlight/config.yaml` (or its supported legacy configuration), alongside the existing provider/settings:

```yaml
checks:
  tests:
    command: cargo test --locked --offline
    requires: [cargo, rustc, cc]
    git: false
  precommit:
    command: pre-commit run --all-files
    requires: [pre-commit, git]
    git: true
  python-tests:
    command: |
      python3 -m venv --without-pip /cache/venv &&
      /cache/venv/bin/python -m unittest discover
    requires: [python3]
```

Recipes allow `command`, `requires`, `git`, and optional [`preparation`](55-pinned-check-dependencies.md). Pinned artifact fetching is a separate explicit operation; installation remains offline inside each check. Commands are arbitrary project-code execution **inside** this profile, not a safe-command whitelist. Requirements are executable basenames, checked with the sandbox's own PATH before check approval. This verifies binary presence and sandbox startup, not that every interpreter, dependency or service required by the project is installed. Errors after launch retain their exact output; a nonzero status alone does not establish a source regression. Preflight reports all missing declared tools together, before command approval. Its diagnostic-only host lookup checks up to 32 absolute PATH directories: a discovered host executable is reported as a candidate, never run or mounted. A candidate is not proof that its interpreter/libraries work. Relative PATH entries are ignored.

Declare directly needed interpreters and linkers explicitly (`cc` for the Rust test recipe above); the harness does not infer arbitrary shell-command dependencies or execute unapproved runtime version probes. Unknown transitive requirements retain their actual failure output after launch. The sandbox provides fixed `cc`/`c++` aliases only when their canonical system targets are executable regular files under `/usr/bin`. These aliases point to already-visible executables; they do not expose `/etc/alternatives`. Other tools, including Node under `/opt`, host virtualenvs and arbitrary PATH entries, remain unprovisioned. Install a supported system runtime or explicitly prepare pinned offline dependencies; there is no automatic host-runtime import.

At most 32 recipes are supported; names are ASCII letters/digits/hyphen/underscore, up to 64 bytes. Each command is nonempty and at most 16 KiB, with up to 32 required executable names of at most 128 bytes each. Agent file tools cannot read or modify these settings. Recipe names and command excerpts are advertised to the model; full commands appear in approvals/history, so credentials do not belong in recipe commands. A model can request a recipe, not replace its command or broaden its profile.

`git: true` creates a **fresh synthetic all-files Git index** inside the copy. It does not copy the original `.git`, branch/history, staged index, remotes, config, hooks, templates, alternates or credentials. System/global Git configuration is disabled. Checks that depend on original staged changes/history, submodules, LFS or worktree state need another design or manual execution. `pre-commit run --all-files` can use this index if the framework and offline hook environments are available; declaring the recipe does not install them. Local/system hooks can use installed tools; remote environments need a complete pinned offline preparation recipe. `pre-commit install` is not needed to run a check and never installs hooks into the live project.

## Request and inspect a check

The user can explicitly run a configured recipe without a model/API request:

```sh
greenlight check SESSION_ID tests
```

This is a separate user-requested operation, not workflow advancement or completion. Its exit status is zero only for a passing unchanged live input snapshot; unavailable/failed/unknown/cancelled checks and generated edits return an error while retaining their evidence. It uses project/session execution ownership. `permissions.shell: deny` disables checks as well. Normal shell grants, including `shell: allow`, do not authorize the agent's new writable-copy profile.

To let a workflow agent request checks, explicitly include `check` in that step's `tools`. It is not added to the default tool list. The agent receives recipe excerpts and a capability summary, and uses:

```yaml
action: tool
tool: check
input: tests
```

The existing TUI permission popup describes the actual command, profile, synthetic Git choice, network restrictions, required runtimes and recipe fingerprint. Allow once or allow that exact recipe for the current process; changing the recipe invalidates the process grant. Esc still hides the popup without deciding. Unsupported namespace/runtime preflight returns **unavailable**, without asking for a meaningless command approval or running the configured check. This preflight uses an empty private workspace; source capture begins after authorization.

A workflow can also require a named check as its verification policy:

```yaml
verification:
  type: workspace
  recipe: tests
```

Policy acceptance requires a passing check, **no permitted source modifications**, confirmed cleanup and live inputs matching the captured manifest at completion. Passing on a formatter-modified copy is evidence about that copy, not verification of unchanged live files. Composite `any` policies cannot suppress cleanup quarantine or cache it as an ordinary rejected check.

The activity list records preparation/running phases, a check result, and **Check raw diagnostic** for the full bounded captured output. Technical Details explains the current sandbox boundary. Small generated diffs appear as `verification-RUN_ID.patch` in Artifacts; they are excluded from automatic final-report selection. Large diffs and exact manifests remain accessible through the run reader:

```sh
greenlight check-show SESSION_ID RUN_ID
```

This prints the check outcome, launch certainty, cleanup, input hashes/executable flags, exclusions, full retained raw diagnostic, proposed edits and imported-file count. Check output is captured with the common runner's 128 KiB stdout/stderr caps. The model gets an excerpt rather than an unbounded tool response. Overflow/cancellation/execution errors are unknown/cancelled, never a passing check. Runtime/dependency errors may occur after launch; no automatic dependency installation, tool retry or host fallback is performed.

Historical check reruns use the original result's recipe fingerprint and bind it into the new request. Changed/missing/unknown original authority is rejected: explicitly select the current recipe with a fresh check instead. The execution rechecks that fingerprint against the recipe it actually loads.

## Review and import edits

Checks never modify live source automatically. After reviewing the full diff, explicitly select files:

```sh
greenlight check-import SESSION_ID RUN_ID --file src/example.rs --yes
# Repeat --file for multiple selected files.
```

Import is currently a CLI user action; there is no new TUI import button or model import tool. Each write goes through the existing native journal, so the resulting edits/diffs are visible and reversible under normal Edits controls. Shell/write process or persistent grants cannot trigger automatic import.

Import is bound to the exact session/branch and published run. It requires unchanged live input hashes and executable flags across the permitted snapshot, then rechecks each selected file's preimage during its native write. Manual drift, duplicate paths, unknown cleanup and hidden/unpublished runs refuse import. Multiple imports from one run account for already imported files. Deletions, binary edits, executable-mode changes and newly executable files require manual review and cannot be imported through this path.

Multi-file import is not transactional rollback. Each applied file is journaled and recorded separately; a later error can leave earlier selected files imported. A crash between a native write and receipt update makes a later import refuse drift rather than overwrite it. Inspect Edits and run a fresh check after any import; importing a formatter result is not fresh validation.

## Inputs, runtimes and retention

A check captures the permitted working tree, not Git's tracked-file set. Its own snapshot budget permits 20,000 visited entries, depth 32, 4 MiB per file, 64 MiB of bodies and 4 MiB of path names. Exceeding a limit refuses preparation; no needed body is silently dropped to fit. The source is rechecked after capture. The manifest records hashes, executable flags and exclusions.

Private/sensitive/generated paths follow existing project-discovery and sandbox rules: `.git`, Greenlight settings/state, `.env` files, credential paths, `target`, `node_modules`, virtualenvs and similar caches are excluded and listed. Allowed symlinks/special files and non-UTF-8 paths refuse capture rather than being followed. These exclusions are not a universal secret scanner; secrets embedded in otherwise permitted source remain visible to executed code.

Only changed UTF-8 source bodies needed for reviewed imports are retained, with a 4 MiB before/after budget; exceeding it yields unknown modifications and no import. A check can modify excluded/generated paths in its disposable copy, but those are outside source review/import and do not become native edits. This is not a full filesystem audit or resource quota.

System Python and job-local virtualenvs can use `/cache`; host virtualenvs with absolute shebangs are not copied or exposed. `PYTHONPYCACHEPREFIX`, `PRE_COMMIT_HOME`, `XDG_CACHE_HOME` and Cargo's writable paths point at the check's own cache. Existing narrow read-only Rust toolchain/offline-registry mounts remain available. No host HOME, credential-bearing Cargo config, writable host virtualenv or shared model-writable executable cache is introduced.

Copies/cache live under private `.greenlight/state/verification-runs/workspace-RUN_ID-*`; successful completed workspaces are removed. Receipts and generated edit bodies remain private state. If cleanup is unconfirmed—or an execution future is dropped while its invocation remains active—the workspace and cache are retained together. GL-033 quarantines further execution/recovery until cleanup is confirmed. Retained abandoned workspace directories need manual cleanup after safe recovery; there is no blind background garbage collection of potentially active jobs.

Separately approved dependency provisioning, downloaded artifact provenance, CPU/PID/disk quotas, original Git integration, and successful live-isolation/performance measurements on a supported host remain follow-up work. This cloud environment rejects UID-map setup, so it validates explicit unavailability/cleanup and trusted snapshot/import fixtures; successful isolation tests remain ignored here.

## Phase timing evidence

New receipts carry `phase_ms`, a map of monotonic elapsed milliseconds. `check-show` prints it separately from raw diagnostics. Old receipts decode to an empty map: absent timings mean unmeasured/not reached, never zero or backfilled estimates. Timing does not change outcome, cleanup, approval, import or policy acceptance.

| Phase | Scope |
| --- | --- |
| `workspace_setup` | Private run directories, source/state handles, initial preparation event and sandbox profile construction |
| `preflight` | Runtime/namespace probe, including launch, capture and owned process cleanup |
| `dependency_staging` | Verify prepared artifacts and copy them into private read-only dependency staging |
| `approval_wait` | The authorization callback, recorded separately from execution work |
| `source_capture_and_recheck` | Copy permitted inputs and rehash live inputs to detect capture-time drift |
| `synthetic_git` | Optional fresh Git initialization/index, including process lifecycle |
| `offline_setup` | Optional configured offline installation, including process lifecycle |
| `check_execution` | Configured command, including namespace launch, bounded capture and process cleanup |
| `change_inspection` | Inspect copy/live input state and assemble proposed edits/diffs |

These are coarse wall-clock phases, not CPU time, pure command runtime, first-output latency or a complete decomposition of the whole operation. Inter-phase event publication, receipt persistence, final temporary-copy removal and caller ownership release are outside these spans. Required-runtime preflight failure has no check-execution span; failed bootstrap retains its setup and change-inspection spans without inventing a check span. Fatal errors before a receipt can return, including unconfirmed cleanup, still propagate normally and do not fabricate timing evidence.

## Output latency and Linux child accounting

New receipts also carry `executions`, keyed by subprocess phase (`preflight`, optional `synthetic_git`/`offline_setup`, and `check_execution`). `check-show` prints these measurements. Old receipts have no execution observations; nothing is backfilled. Metrics do not affect verification policy or authorize execution/imports.

`first_stdout_ms` and `first_stderr_ms` record the first nonempty read observed by the executor, measured from immediately before stage launch after command construction. Stdout means the final pipeline stage; stderr covers captured stderr from any stage, including startup diagnostics. Intermediate pipe bytes and discarded upstream data are not visible stdout. Silent streams remain null, not zero. This is executor-observed latency, not model-token latency or time from a user's action to a TUI repaint. `elapsed_ms` covers launch through capture and confirmed owned-child cleanup, plus final diagnostic assembly; source capture and command construction are outside this clock.

After the supervisor has reaped its invocation and received ECHILD, it optionally records Linux `getrusage(RUSAGE_CHILDREN)` counters in its private lease. The application reads them only after the existing cleanup confirmation. No process-wide accounting from the multithreaded Greenlight process is used, no unrelated process is sampled, and no new host-command authority is introduced. Metrics cannot replace cleanup evidence; unavailable/malformed resource records remain null. Failure to collect telemetry never weakens cleanup or changes a command result. A cleanup-incomplete error still propagates without claiming complete measurements.

Each `stage_resources` entry corresponds to a launched stage's supervisor:

| Field | Interpretation |
| --- | --- |
| `user_cpu_us`, `system_cpu_us` | Kernel CPU accounting for reaped children, in microseconds; excludes Greenlight and the supervisor itself |
| `largest_child_maxrss_kib` | Linux's largest-child RSS high-water value in KiB; **not** peak concurrent memory of the whole tree, nor a sum across stages |
| `input_block_operations`, `output_block_operations` | Kernel `ru_inblock`/`ru_oublock` counters; not byte totals, cache footprint, or a count guaranteed to match physical device requests |

Descendant accounting follows Linux's child-reaping rules, including descendants accounted to intermediate children when those children waited for them; it is not independent exhaustive accounting of adversarial process trees. Resource data is returned per stage, without inventing an aggregate memory peak or combining unrelated invocations. Zero is a legitimate measured counter; null means unavailable. Retained output-limit/cancellation errors can carry measurements after confirmed cleanup while keeping their original failure classification and diagnostic.

These observations impose no memory/CPU/PID/disk quotas. Resource enforcement remains GL-023. Aggregate process-tree peak memory and bytes-written accounting still need separate measurement work. Retained private-cache observations are described below.

## Retained private-cache footprint

Returned check receipts now include optional `cache_storage`. This is a metadata-only observation of the job's `/cache` after its child processes are confirmed clean, before the disposable cache is normally removed. It measures files remaining at the end of the command, not storage retained after the receipt returns; observation does not preserve the cache. For launched checks, observation precedes the final live-input/change inspection. It cannot replace cleanup evidence, and its failure never changes a check's result. `check-show` prints the complete observation; `phase_ms.cache_inspection` records its cost.

A complete `footprint` reports non-root entries, unique regular files, their logical lengths, their allocated block totals, and counts of skipped symlinks/special files. Regular files are deduplicated by device/inode, so hard-linked paths do not multiply storage. Allocation is `st_blocks * 512` per unique regular inode. Logical size can exceed allocation for sparse files; small files can allocate more than their logical length. Reflinks/shared extents can still be counted in multiple inodes. Directory/symlink allocation and filesystem metadata are excluded. This is not total physical device occupancy or bytes written during execution.

The scan uses descriptor-relative `O_PATH`/`O_NOFOLLOW` metadata: no contents are read, no symlink target is followed, and FIFOs/devices/sockets are not activated. Budgets are 20,000 entries, 64 directory levels, 8 MiB of queued pathname bytes, and a best-effort two-second elapsed budget checked between filesystem operations and before reporting totals. This is not a hard deadline on a blocked kernel filesystem call or a memory quota; container/allocator overhead is separate from queued pathname bytes.

If a budget or metadata operation fails, `footprint` is null and `unavailable_reason` explains why. Partial sums are discarded. A complete empty cache is measured zero, while old receipts without an observation remain unknown. Failed setup/check receipts can still have complete retained-cache observations; that does not turn their check outcome into a pass.

The snapshot excludes the disposable source copy, staged `/deps`, shared immutable artifact objects, host caches mounted only inside the namespace, and namespace-private `/tmp` after it is destroyed. Deleted temporary data and repeated overwrites cannot be reconstructed from retained files. It assumes completed job processes and is not an atomic filesystem snapshot against independent host edits. No limits or cleanup behavior are relaxed.
