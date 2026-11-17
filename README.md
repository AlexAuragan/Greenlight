# Greenlight (v0.1 / experimental)

A **human-supervised, step-centric AI harness in Rust**. Each workflow is a directory containing a `workflow.yaml` and one Markdown-with-YAML-frontmatter file per step. Every step starts with fresh model context, explicitly selects artifacts, and cannot advance until its verification policy has passed.

This is an **experimental harness**, not a production-ready agent. DeepInfra is the first provider. The CLI remains available, alongside a Ratatui conversation TUI that can resume blocked work, present approvals, and restart from earlier recorded steps. Linux with Bubblewrap is now required for shell execution. See `docs/10-shell-sandbox.md`.

## Requirements

- Rust 1.89 or newer, Cargo, Git and a C compiler
- Linux, Bubblewrap at `/usr/bin/bwrap`, and enabled unprivileged user namespaces
- DeepInfra API key
- Optionally, a Brave Search API key for web searches
- Git is recommended for checkpoints. Greenlight can **explicitly restore recorded native writes**, but it does not automatically restore arbitrary project files, Git state, or external effects

## Quick start

Install or upgrade from GitHub (no sudo):

```bash
git clone https://github.com/AlexAuragan/Greenlight.git
cd Greenlight
bash install.sh
```

During review, switch to `feat/install-and-project-layout` and pass `--ref feat/install-and-project-layout` to the installer. After merge, `master` is the default. The installer remembers your selected ref and binary location; rerunning it fetches and builds upgrades. Use `--ref master` to switch back after merge. Add the printed binary directory to PATH if needed.

Then, in any project:

```bash
cd /path/to/your/project
greenlight init
# Configure .greenlight/config.yaml and export DEEPINFRA_API_KEY,
# or copy .greenlight/.env.example to .greenlight/.env and fill it in.
greenlight sandbox-check
greenlight validate fix-bug
greenlight                     # opens the TUI
greenlight start fix-bug --goal 'Find and fix the intermittent test failure'
```

Settings, workflows, instructions and runtime data live under that project's `.greenlight/`. Initialization embeds the three example workflows, preserves existing files and needs no source checkout. Upgrading the binary never rewrites project files. Exported environment variables take precedence over `.greenlight/.env`, then the selected project's root `.env`. An unrelated caller directory's `.env` is not loaded. See [installation and projects](docs/22-installation-and-projects.md) for paths, prerequisites and update behavior.

For source development, use `cargo build`, `cargo test --locked` and `cargo run -- ...`.

Run `greenlight restore SESSION_ID --event EVENT_ID --dry-run` to preview restoring tracked files to a historical event. `fork` now offers workspace restoration before creating a new execution branch; see `docs/09-workspace-restore.md` for limitations.

`.greenlight/config.yaml` (or legacy `greenlight.yaml`) specifies the provider model and default host tool approval policies. When asked to approve shell commands or native writes, use `[p]` to save a project whitelist entry or `[g]` to save a global entry. Shell grants match the exact command string; write grants cover native writes to permitted project files. `greenlight approvals` shows stored grants and their paths (edit the YAML files to revoke grants). See `docs/08-persistent-approvals.md` for details. Replace the default model with one available to your account if needed, using `cargo run -- models` to discover models.

**Important:** Agent shell commands, workflow command steps and command verification now run **inside Bubblewrap**, with a read-only `/workspace`, private `HOME`, no inherited API keys and no network. Shell writes are limited to an isolated `/tmp` and a project-local build cache under `.greenlight/state/sandbox-cache`. Source edits must use native `write` to keep diffs and history. There is **no unsandboxed fallback** if Bubblewrap is unavailable. This is not complete protection against untrusted programs or resource exhaustion; see `docs/10-shell-sandbox.md`.

## Commands

```text
greenlight init
greenlight                      # open the TUI
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

`workspace-status SESSION_ID` compares project files with recorded step-boundary manifests and checks the branch-visible native journal for explained/unexplained changes and coverage gaps. `--json` exports the report; `workspace-benchmark --runs 7 --json` measures read-only scans. It records **paths, sizes and SHA-256 hashes, not file contents**. The recording omits sensitive paths, symlinks, caches and files above its size limit. Hashes detect drift; they do not restore files or identify whether a human or another process made a change. See [workspace recording](docs/14-workspace-recording.md).

Without `--root`, commands discover the nearest initialized project from the current directory, stopping at Git repository boundaries. `greenlight init` initializes the current directory exactly. Use `--root /path/to/project` to select a project explicitly. Config and workflows reload for each *new step run*.

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

## UI design review

The [2026-11-17 screen audit and redesign draft](docs/23-ui-audit-and-redesign.md) inventories the current screens and proposes a clearer layout, semantic colors, popups, buttons and contextual actions. It is for joint review before UI implementation.

The [running-screen design and static mockups](docs/24-running-screen-design.md) record the first focused screen review and agreed layout behavior.

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

- [x] Add **LLM provider failure management** with safe CLI/TUI guidance for missing/invalid credentials, credit/quota exhaustion, rate limits, permission/request errors and temporary outages. Retry only explicit rejected rate limits without usage and pre-response connection failures (3 requests maximum); respect short `Retry-After` waits and allow TUI `x` cancellation. Preserve one usage record per request and uncertain billing. Unknown 429s, timeouts and server errors require explicit resume. See [provider failures](docs/19-provider-failures.md).

- [x] Persist input/cached-input/output usage for every current coding, verification and detector request, including failures and retries. Prefer provider counts; use labelled tiktoken estimates otherwise, retaining unknown cache/billing values. Report per-request/attempt/model totals in CLI/TUI and distinguish session, branch and inherited history. See [usage and pricing](docs/18-token-usage-and-pricing.md).
- [x] Support explicit per-model pricing with currency, source/effective date and per-request snapshots. `model-info`, `models --pricing`, `usage` and TUI `/usage` expose configured or unknown rates/counts and estimated costs without counting cached input twice.
- [x] Discover default on-demand token rates from DeepInfra's authoritative public catalog, with local caching, explicit overrides and automatic freshness warnings. Support explicit whole-request context tiers and cache-creation/TTL rates without double billing. Unsupported providers/billing modes stay unknown; no guessed rates or historical repricing. See [pricing discovery and limits](docs/18-token-usage-and-pricing.md#public-pricing-discovery-and-freshness). Budgets remain P2.
- [x] Replace the raw log-centric layout with a session workspace: **status + stepper + context + primary action**, projected from persisted attempts, input manifests, verification events and execution ownership. `c` / `/context` shows saved attempt context; legacy gaps remain explicit.
- [x] Add a session picker with goal names, blocked-first sorting, last recorded activity (UTC), case-insensitive search (`f`) and an in-TUI creation flow (`n`). Choose a validated workflow and editable goal; Enter creates and starts the session through the existing engine. Active input fields use blinking carets, and TUI-owned model requests show a separate live streaming preview. Selection stays stable as activity reorders the list.
- [x] Implement contextual blocked-state cards with recorded reason/evidence and guarded resume/restart guidance. `e` opens the exact blocker, `h` opens history, `2` opens artifacts, and `i` / `r` use the existing resume/restart controls. Interrupted ownership and pending writes require recovery first.
- [x] Extend pane/focus navigation with scrollable `?` / `/help`, explicit activity follow/pause (`End` resumes), per-session in-memory drafts, wrapped-content scroll bounds and compact/resize behavior. Arrows and `j/k` remain primary scrolling keys; optional PgUp/PgDn and Home/End work in reading panels. Drafts are not persisted across exits.
- [x] Harden Markdown and literal artifact previews: visibly escape terminal/bidirectional controls, align ordinary tables by terminal width, handle longer fences, and bound preview bytes/lines/table padding with explicit truncation. Tests cover ANSI/OSC/C1 controls, CJK/combining/emoji widths, malformed markup and oversized documents. Stored source stays unchanged; full CommonMark/GFM and inline images are not a goal.
- [x] Persist exact selected-input versions and approval references per attempt; human gates approve displayed versions and downstream consumers require reapproval after changes.
- [x] Add branch-visible artifact history (`v` in Artifacts), exact creation/attempt/branch provenance and recorded consumer/human-approval references (`p`), plus adjacent-version diffs (`d`). Inspect immutable versions without following latest updates; `artifact --event ID` exports a visible historical version. Missing historical bodies retain metadata access.
- [x] Show semantic activity cards for correlated tool calls, verification checks/verdicts, native file changes, artifact versions and structured failures. Enter expands recorded evidence; `d` opens journal diffs, `o` opens an artifact card’s exact version, and `v` / `/events` retains raw audit history. Legacy/unlinked outputs remain explicit; model reports do not become verification facts.
- [x] Support optional model-authored **activity titles** at each coding response/action boundary. Validate bounded plain text, record exact response links and show separate **Agent intent** cards; permissions, outcomes and verification remain factual. Successive responses may have different titles within a step; no separate phase lifecycle. See [agent protocol](docs/13-agent-action-protocol.md#4-optional-activity-title-implemented).
- [x] Build a workflow stepper showing latest recorded outcome per step, retry/loop attempt counts, current attempt ID, aggregate verification and next action. Long workflows window around the current step. This is an overview, not a full graph or attempt browser.
- [x] Persist a UUID for each new step attempt with definition/start/end event references and outcome; expose branch-visible history through `attempts`. Worker recovery marks interrupted attempts without replaying approvals. Historical sessions are not backfilled.
- [x] Capture each attempt's input manifest with exact artifact creation events and applicable human approval events, linked to its saved step definition. Agent context uses the selected snapshots; a future UI must not reconstruct them from latest-by-name state.
- [x] Correlate SHA-256 manifests with the branch-visible native write/restore journal, checking every before/after hash in the interval. Report journal-explained, unexplained and coverage-unknown paths in CLI/TUI, preserve net-zero journal/gap evidence, and record scan metrics. Real-project measurements and limits are in [workspace recording](docs/14-workspace-recording.md#measured-scan-overhead). Hashes do not restore file contents or prove authorship.
- [x] Draft the [conversational coordinator routing contract](docs/20-conversational-coordinator.md) and [manager prompt](docs/prompts/manager-v1.md). The first [read-only Q&A stage](docs/21-manager-qa.md) is implemented; executing targeted forwards, guarded restart proposals and next-call delivery remain P2 work subject to the contract's acceptance scenarios.
- [x] Split the TUI into focus/routing, activity, layout, reader, approval and recovery components. Large artifact/version previews parse, wrap and paint on a bounded background worker; the UI paints only a ready viewport and discards stale results. Storage refresh and explicit diff construction retain their existing synchronous paths.
- [x] Preserve per-session drafts and per-session/branch tab selections, scroll, follow state and journal-diff reading in memory. `F` filters activity metadata/summaries; `C` sends full selected text to a local clipboard utility; `E` opens a private temporary review snapshot in VISUAL/EDITOR, without importing edits. New artifact versions reset stale offsets. See [TUI reading/export controls](docs/11-tui.md#reading-state-filtering-and-export).

### P2 — supervised intelligence and stronger recovery

- [ ] Consider an inexpensive **optional summarizer** for long steps or completed workflows, run asynchronously and display evidence-linked output when ready; compare it against coding-agent-authored short activity titles. Never block execution on summarization.
- [x] Formalize optional `activity_title` validation and update the [agent YAML contract reference](docs/13-agent-action-protocol.md), embedded model prompt, runtime parser and protocol tests together. Keep these synchronized with future protocol changes.
- [x] Implement grounded read-only [manager Q&A](docs/21-manager-qa.md), context preview, separate saved exchanges, strict JSON/citation/destination validation and accounted `manager` requests. Ordinary TUI messages route to the manager when enabled; explicit CLI/TUI ask works without enabling the default route. Forwards remain visibly unexecuted.
- [ ] Continue the [coordinator design](docs/20-conversational-coordinator.md) with targeted idle delivery/guarded restart proposals, then a durable live-owner inbox. Preserve original message bytes, attempt/branch scope, execution ownership, permission checks and verification.
- [ ] Complete delivery status UI (**Queued for STEP**, **Included in request**, **Restart proposed**) as those stages land. Current manager cards show answers, clarification, unavailable proposals and stale-state review. Keep deterministic commands/approval controls separate; explicit direct-agent mode is a later option.
- [ ] Support manager **side questions** that cite a consistent evidence snapshot without changing the coding-agent context; make question history/retention explicit.
- [ ] Add richer restart controls: choose a specific historical attempt/checkpoint, inspect changed files, compare branches, understand untracked/manual edits.
- [ ] Evaluate **snapshotting changed file bodies** at step boundaries only after hash manifests reveal a useful gap; define size/privacy/caches policy and whether external edits should be restored or simply protected from overwrite.
- [ ] **Design command-level shell grants together before implementation:** allow selected executables (e.g. `ls`) across approved argument/path variations without approving a whole shell string by prefix. Define argument/option restrictions, path and executable identity, wrappers/interpreters, pipelines/AND/OR/sequences, redirections, substitutions and environment assignments; validate every executed component or request approval for the whole compound command. Regression case: granting `cat` must never auto-approve `cat something.txt | ./harmful_script.sh` (nor hidden substitutions). Keep exact-command grants until this design is agreed.
- [ ] Add real **session-scoped saved permissions**, separate from one-time/process/project/global grants; no persistence of unanswered approval prompts.
- [ ] Support artifact review comments and safe new-version editing (external editor first, optionally embedded later).
- [x] Add completed-run results with agent conclusions, pinned reports, verification scope and recorded edit counts; **3 Edits** shows per-file net native diffs and guarded single/all-file revert previews.
- [ ] Extend diff review with hunk navigation and exportable verification evidence.
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

The [approval-dialog design and static mockups](docs/25-approval-dialog-design.md) show all valid grant choices, explicit scope confirmation and read-only Help. These remain design studies.

The [completed-workflow design and static mockups](docs/26-completed-screen-design.md) propose an outcome-first result view with scoped verification, edits and historical-result warnings.

The [attention and recovery design](docs/27-attention-and-recovery-design.md) proposes distinct blocked, provider-error, interrupted-owner and unfinished-write views.
