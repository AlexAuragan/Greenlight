# Offline checks in isolated verification workspaces

GL-027 implements named checks on a writable disposable source copy. The original project stays read-only to shell commands. No check can access original Git metadata, host user services, credentials or the network. This is separate authority from an ordinary shell grant.

## Configure a named recipe

Add checks to the project's host-owned `.greenlight/config.yaml` (or its supported legacy configuration), alongside the existing provider/settings:

```yaml
checks:
  tests:
    command: cargo test --locked --offline
    requires: [cargo]
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

Recipes allow `command`, `requires`, `git`, and optional [`preparation`](55-pinned-check-dependencies.md). Pinned artifact fetching is a separate explicit operation; installation remains offline inside each check. Commands are arbitrary project-code execution **inside** this profile, not a safe-command whitelist. Requirements are executable basenames, checked with the sandbox's own PATH before check approval. This verifies binary presence and sandbox startup, not that every interpreter, dependency or service required by the project is installed. Errors after launch retain their exact output; a nonzero status alone does not establish a source regression.

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
