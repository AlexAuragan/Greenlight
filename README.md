# Greenlight (v0.1 / experimental)

A **human-supervised, step-centric AI harness in Rust**. Each workflow is a directory containing a `workflow.yaml` and one Markdown-with-YAML-frontmatter file per step. Every step starts with fresh model context, explicitly selects artifacts, and cannot advance until its verification policy has passed.

This is an **experimental harness**, not a production-ready agent. DeepInfra is the first provider. The CLI remains available, alongside a Ratatui conversation TUI that can resume blocked work, present approvals, and restart from earlier recorded steps. Linux with Bubblewrap is now required for shell execution. See `docs/10-shell-sandbox.md`.

## Requirements

- Rust toolchain (Cargo, Rust 2021 edition)
- Linux, Bubblewrap at `/usr/bin/bwrap`, and enabled unprivileged user namespaces
- DeepInfra API key
- Optionally, a Brave Search API key for web searches
- Git is recommended for checkpoints. Greenlight can **explicitly restore recorded native writes**, but it does not automatically restore arbitrary project files, Git state, or external effects

## Quick start

```bash
export DEEPINFRA_API_KEY='...'
# Optional web search:
export BRAVE_SEARCH_API_KEY='...'
# Optional outgoing Discord notifications:
export DISCORD_WEBHOOK='https://discord.com/api/webhooks/...'

cargo build
cargo test
cargo run -- sandbox-check
cargo run -- validate fix-bug
cargo run -- start fix-bug --goal 'Find and fix the intermittent test failure'
```

Instead of exporting these, you can put them in a `.env` file (in the current directory or the project root). It is loaded automatically at startup and never overrides variables that already exist in the environment.

Run `greenlight restore SESSION_ID --event EVENT_ID --dry-run` to preview restoring tracked files to a historical event. `fork` now offers workspace restoration before creating a new execution branch; see `docs/09-workspace-restore.md` for limitations.

`greenlight.yaml` specifies the provider model and default host tool approval policies. When asked to approve shell commands or native writes, use `[p]` to save a project whitelist entry or `[g]` to save a global entry. Shell grants match the exact command string; write grants cover native writes to permitted project files. `greenlight approvals` shows stored grants and their paths (edit the YAML files to revoke grants). See `docs/08-persistent-approvals.md` for details. Replace the default model with one available to your account if needed, using `cargo run -- models` to discover models.

**Important:** Agent shell commands, workflow command steps and command verification now run **inside Bubblewrap**, with a read-only `/workspace`, private `HOME`, no inherited API keys and no network. Shell writes are limited to an isolated `/tmp` and a project-local build cache under `.greenlight/state/sandbox-cache`. Source edits must use native `write` to keep diffs and history. There is **no unsandboxed fallback** if Bubblewrap is unavailable. This is not complete protection against untrusted programs or resource exhaustion; see `docs/10-shell-sandbox.md`.

## Commands

```text
greenlight sandbox-check
greenlight recover-writes       # reconcile interrupted writes; source files stay unchanged
greenlight recover-session SESSION_ID  # recover a dead owner; does not resume work
greenlight validate fix-bug
greenlight start fix-bug --goal "..."
greenlight start add-feature --goal "..." --model "provider/model"
greenlight resume SESSION_ID --instruction "Do not change dependencies"
greenlight sessions
greenlight attempts SESSION_ID  # durable step attempts on the active branch
greenlight tui                  # interactive session conversation
greenlight tui SESSION_ID       # focus on a session, resume or restart
greenlight history SESSION_ID
greenlight workspace-status SESSION_ID  # compare step-boundary file hashes
greenlight branches SESSION_ID
greenlight switch SESSION_ID --branch BRANCH_ID --step implement
greenlight artifacts SESSION_ID
greenlight artifact SESSION_ID plan.md > plan.md
greenlight put-artifact SESSION_ID plan.md --file plan.md
greenlight fork SESSION_ID --event 42 --step plan
greenlight models
greenlight usage SESSION_ID
greenlight model-info
```

For a globally installed binary, run `greenlight ...` instead of `cargo run -- ...`.

`workspace-status SESSION_ID` compares the current project files with the most recent recorded step-boundary manifest and shows changes during the latest completed step. It records **paths, sizes and SHA-256 hashes, not file contents**. The recording omits sensitive paths, symlinks, caches and files above its size limit. Hashes detect drift; they do not restore files or identify whether a human or another process made a change. See [workspace recording](docs/14-workspace-recording.md).

Run with `--root /path/to/project` to select the project directory containing `greenlight.yaml`, `workflows/` and `.greenlight/instructions.md`. Workflows may use includes in `shared/`. Config and workflow reload for each *new step run*.

**Renamed from Stepwright:** The Cargo package and installed command are now `greenlight`. Existing `stepwright.yaml` files remain readable as a fallback, but `greenlight.yaml` takes precedence. On first use, the local `.stepwright/state` directory is moved to `.greenlight/state` (including session history, blobs, approvals and cache). If both exist, Greenlight stops instead of choosing one. See [migration notes](docs/15-name-migration.md).

Use `greenlight tui` to open a session conversation, send instructions to resume blocked work, approve tools and human gates, inspect edit diffs, browse workflow artifacts in the **[2] Artifacts** tab, and restart an earlier workflow step. The TUI polls the existing event database, so it can also observe a workflow running in another terminal. To restart, type `/restart`, choose a recorded step and confirm the guarded workspace restore before forking; see `docs/11-tui.md`.

When execution blocks, use `history`, inspect artifacts and then `resume --instruction`. If an attempt should be superseded without losing history, use `fork` from the desired event. Forking offers to restore **tracked native edits** first. Shell-only or manual changes cannot be reconstructed; preview with `restore --dry-run`.

## Design summary

- YAML frontmatter, YAML agent action protocol, Markdown prompt bodies. **Agent output fields and semantics:** [docs/13-agent-action-protocol.md](docs/13-agent-action-protocol.md) (current parser is the enforcement authority).
- Fresh model context for each step, populated with selected registered artifacts.
- Required artifacts checked per attempt; additional artifacts are always allowed.
- Explicit verification, including `type: none`; composite `all` and `any` supported with the nesting limits described in [the execution contract](docs/01-execution-contract.md).
- Deviations named by step, actions `pause`, `goto`, or bounded `retry`; optional independent model-based detector prompt.
- Human approval for risky tools, with once, process, project or global trust scopes; no automatic hidden error recovery.
- Append-only SQLite event stream with fork ancestry, artifact snapshots and immutable historical branches.
- DeepInfra/OpenAI-compatible chat-completions transport and model listing.
- Read-only, network-disabled sandboxed shell; project read/journaled native write tools; optional Brave web search and Discord notifications.

## Roadmap / TODO

> **Status (2026-11-13):** this checklist tracks **remaining** work, not all ideas ever discussed. The TUI, workspace manifests, Markdown preview and Greenlight rename landed in PRs #6–#9. For rationale and proposed designs see [the dated TUI UX analysis](docs/12-tui-ux-analysis.md); for settled decisions see [the decision log](docs/16-decision-log.md), and for a repeatable manual exercise see [the smoke-test guide](docs/17-smoke-test.md). The current executable behavior is described in the specialized docs and Rust sources.

### Implemented baseline (not the finished UX)

- [x] CLI and Ratatui TUI share the engine, branch-aware event store, approvals and guarded restart/restore paths (PR #6).
- [x] Native-write journal, file diffs and tracked-file restoration; step-boundary SHA-256 workspace manifests and read-only `workspace-status` (PR #7 for manifests; earlier PRs for journal/restore).
- [x] Rendered Markdown subset with raw toggle, focus-based Left/Right/Enter/Escape navigation, `i` to compose, and Up/Down or `j/k` scrolling (PR #8).
- [x] Greenlight executable/configuration/state naming, with guarded migration of legacy project state (PR #9).
- [x] Local user validation of the pre-rename `master` integration: 57 passing Rust tests, one environment-dependent sandbox test ignored, and clean Clippy. The rename was subsequently reported good locally; automated CI evidence is not available. See [validation](docs/06-validation.md).

### P0 — stabilize the current interactive baseline
- [x] Use typed, explicit approval choices with inert Enter and ignored key repeats; project/global grants require a separate scope confirmation. Cancellation, disconnection or save failure never authorizes the request.
- [x] Surface worker errors, blocked steps, malformed structured events, missing artifact bodies and partial restores in scrollable diagnostics. Preserve raw evidence; failed restores report changed/journaled counts and cannot silently fork or claim success.
- [x] Enforce one execution owner per session with an OS lock and persisted owner record; recover interrupted runs explicitly with `recover-session` or TUI `/recover`. Competing CLI/TUI resumes cannot change the live owner's state.
- [x] Discard unanswered permission requests on worker loss; disconnected TUI workers clear pending dialogs, and session recovery preserves already-saved grants. A real durable session grant (today `a` only covers the process) remains separate work.
- [x] Publish artifact metadata and its success event in one SQLite transaction after syncing the blob; test blob, database insertion and commit failures, preserving prior versions and allowing retries. A crash before publication may leave an unreferenced blob for future cleanup.
- [x] Consolidate deviation detection into one shared evaluator and retain its verdict through routing. Accepted reports are checked once; rejected agent reports continue in the same conversation, while detector errors block. Tests cover pause/retry/goto and engine-generated failures.
- [x] Persist native-write intent/preimage before atomic file replacement; reconcile interrupted writes explicitly with `recover-writes`, preserving manual edits and blocking execution/restore while unresolved. Process-death and journal-failure tests cover recovery.
- [x] Add engine-backed 80×24 terminal-buffer acceptance tests for deny/confirm/cancel, native writes, failed verification, artifact inspection, revised instructions, human gates, worker loss/recovery and completion, using a scripted provider. Separate tests cover process death and injected partial-restore failures.

### P1 — TUI redesign for daily use

- [x] Persist input/cached-input/output usage for every current coding, verification and detector request, including failures and retries. Prefer provider counts; use labelled tiktoken estimates otherwise, retaining unknown cache/billing values. Report per-request/attempt/model totals in CLI/TUI and distinguish session, branch and inherited history. See [usage and pricing](docs/18-token-usage-and-pricing.md).
- [x] Support explicit per-model pricing with currency, source/effective date and per-request snapshots. `model-info`, `models --pricing`, `usage` and TUI `/usage` expose configured or unknown rates/counts and estimated costs without counting cached input twice.
- [ ] Discover authoritative provider pricing where supported, handle tiered/cache-creation billing and flag pricing freshness automatically. Current rates are explicit configuration, never silently fetched, refreshed or invented; cost budgets remain separate P2 work.
- [x] Replace the raw log-centric layout with a session workspace: **status + stepper + context + primary action**, projected from persisted attempts, input manifests, verification events and execution ownership. `c` / `/context` shows saved attempt context; legacy gaps remain explicit.
- [x] Add a session picker with goal names, blocked-first sorting, last recorded activity (UTC), case-insensitive search (`f`) and an in-TUI creation flow (`n`). Choose a validated workflow and goal to create a ready session, then resume through the existing engine. Selection stays stable as activity reorders the list.
- [x] Implement contextual blocked-state cards with recorded reason/evidence and guarded resume/restart guidance. `e` opens the exact blocker, `h` opens history, `2` opens artifacts, and `i` / `r` use the existing resume/restart controls. Interrupted ownership and pending writes require recovery first.
- [ ] Extend the **implemented** pane/focus keyboard model with discoverable help, reliable scroll-follow, draft preservation and small-terminal/resize behavior; do not regress AZERTY-friendly arrow/`j/k` scrolling.
- [ ] Harden the **existing** Markdown renderer with control-sequence/Unicode-width/large-document tests and improve preview ergonomics as actual usage requires; full CommonMark/GFM and inline images are not a goal.
- [x] Persist exact selected-input versions and approval references per attempt; human gates approve displayed versions and downstream consumers require reapproval after changes.
- [ ] Add artifact history/provenance navigation and version diffs to the TUI, building on recorded version IDs and approval evidence.
- [ ] Upgrade activity to semantic cards with expandable tool calls, verification results, file diffs, artifacts and structured failures.
- [ ] Explore model-authored **activity titles** at each response/action boundary (e.g. `Exploring call sites`, `Testing first fix`); record titles as annotations, not as verified execution facts. Optionally allow multiple logical phases within one workflow step.
- [x] Build a workflow stepper showing latest recorded outcome per step, retry/loop attempt counts, current attempt ID, aggregate verification and next action. Long workflows window around the current step. This is an overview, not a full graph or attempt browser.
- [x] Persist a UUID for each new step attempt with definition/start/end event references and outcome; expose branch-visible history through `attempts`. Worker recovery marks interrupted attempts without replaying approvals. Historical sessions are not backfilled.
- [x] Capture each attempt's input manifest with exact artifact creation events and applicable human approval events, linked to its saved step definition. Agent context uses the selected snapshots; a future UI must not reconstruct them from latest-by-name state.
- [ ] Correlate the **already implemented** SHA-256 step manifests with native-write journal events; classify explained/unexplained drift and measure scanning overhead on real projects. Hashes alone cannot restore file contents.
- [ ] Design a conversational coordinator that can answer a question or pass an instruction straight to the coding agent; preserve visible routing and normal engine permissions. Define live steering as cancellation/reissue or next-model-call guidance, not modification of an in-flight LLM request.
- [ ] Split the large TUI controller incrementally into tested focus routing, activity, Markdown reader, approval and recovery components; render artifacts without blocking input.
- [ ] Preserve draft instructions and per-tab selection/scroll position; support copy/open-in-editor and useful filtering.

### P2 — supervised intelligence and stronger recovery

- [ ] Consider an inexpensive **optional summarizer** for long steps or completed workflows, run asynchronously and display evidence-linked output when ready; compare it against coding-agent-authored short activity titles. Never block execution on summarization.
- [ ] Maintain the [agent YAML contract reference](docs/13-agent-action-protocol.md), embedded model prompt, runtime parser and protocol tests together; implement optional `activity_title` only after formalizing its validation.
- [ ] Add a **manager/coordinator** role that answers grounded questions, forwards coding requests, and suggests valid workflow navigation; actual transitions remain checked by the harness. Implement separately from the TUI foundation.
- [ ] Make the message recipient explicit (**Instruct coding agent** vs **Ask manager**) to avoid unexpected mutations.
- [ ] Support manager **side questions** that cite a consistent evidence snapshot without changing the coding-agent context; make question history/retention explicit.
- [ ] Add richer restart controls: choose a specific historical attempt/checkpoint, inspect changed files, compare branches, understand untracked/manual edits.
- [ ] Evaluate **snapshotting changed file bodies** at step boundaries only after hash manifests reveal a useful gap; define size/privacy/caches policy and whether external edits should be restored or simply protected from overwrite.
- [ ] Add real **session-scoped saved permissions**, separate from one-time/process/project/global grants; no persistence of unanswered approval prompts.
- [ ] Support artifact review comments and safe new-version editing (external editor first, optionally embedded later).
- [ ] Improve diff review (file list, hunk navigation, copied patch), final workflow recap and exportable verification evidence.
- [ ] Add **opt-in grouped review** of multiple already-journaled native writes (combined diff by step/attempt). Don't build a separate staging engine unless a concrete workflow requires pre-apply group approval.
- [ ] Introduce model-role configuration and usage/cost budgets without assuming multiple provider accounts.

### P3 — longer-term capabilities

- [ ] Implement **manager-proposed** retry/goto/restart after deterministic recovery actions are available; validate proposed target/checkpoint in Rust and show the normal restore/confirmation screen.
- [ ] Optimize artifact loading when profiling warrants it: fetch lightweight metadata, load the selected body on demand, and cache by immutable version ID. Keep as a documented scaling task, not an immediate blocker.
- [ ] Revisit full-workspace / multi-file atomic rollback only if measured drift and restoration failures justify the extra complexity; document the current restore's partial-error handling.

- [ ] Model response streaming, clean cancel/interrupt/queue semantics and resumable sessions across process restarts.
- [ ] Safe Git-aware checkpoints and patch export; do not imply native restore captures arbitrary workspace changes.
- [ ] Parallel/subworkflow orchestration with explicit session ownership and approval routing.
- [ ] Provider flexibility, context compaction, bounded code retrieval/indexing and opt-in MCP tool integrations.
- [ ] More complete sandbox resource limits and secret-handling defenses, plus security regression tests.
- [ ] Optional desktop/Discord notifications, authenticated remote decisions and customizable keybindings/themes.
- [ ] Branch graph/time-travel visualization, saved views, accessibility/contrast testing and performance profiling.
- [ ] Keep design docs and the decision log synchronized with implemented behavior as each feature lands; preserve historical proposals as clearly dated snapshots.

**Prioritization rule:** build and validate one feature at a time. Harden native-write crash consistency, and evaluate recorded workspace manifests against the native journal before expanding restoration; broader rollback can wait for real coverage evidence. Keep the ambitious TUI direction, but distinguish execution facts from model-authored UI titles, preserve saved grants while dropping pending requests after crashes, and keep the existing native-write rollback scope explicit. Models may coordinate, summarize and propose; the harness applies permissions, verification and transitions.

## Caveats (read before use)

This MVP has deliberately limited capabilities: no cross-platform shell sandbox, no full Git checkpoint/restore, no parallel/subworkflow execution, no remote Discord decisions, no MCP, no streaming, no provider-specific reasoning parameters, no adaptive context compaction, and no transactional code edits. The YAML protocol is a best-effort model instruction validated by the runtime, not a guarantee that every model responds correctly. Bubblewrap does not provide resource quotas, comprehensive project-source secret scanning or immunity from kernel vulnerabilities.

The OpenAI-compatible DeepInfra HTTP API requires **JSON transport**; only the prompts, workflow files and *model-generated structured actions* use YAML. The latter is not JSON on the agent-facing surface.

The `optimize-performance` example loops on `target_not_met` and stops on regressions; automatic code rollback is intentionally **not** implemented. For safe rollback, restore the repository from a known Git checkpoint before resuming.

See [documentation and decisions](docs/16-decision-log.md), the [manual smoke-test guide](docs/17-smoke-test.md), and the rest of `docs/` for execution contracts, workflow syntax and engineering tradeoffs.
