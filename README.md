# Stepwright (v0.1 / experimental)

A **human-supervised, step-centric AI harness in Rust**. Each workflow is a directory containing a `workflow.yaml` and one Markdown-with-YAML-frontmatter file per step. Every step starts with fresh model context, explicitly selects artifacts, and cannot advance until its verification policy has passed.

This is an **experimental harness**, not a production-ready agent. DeepInfra is the first provider. The CLI remains available, alongside a Ratatui conversation TUI that can resume blocked work, present approvals, and restart from earlier recorded steps. Linux with Bubblewrap is now required for shell execution. See `docs/10-shell-sandbox.md`.

## Requirements

- Rust toolchain (Cargo, Rust 2021 edition)
- Linux, Bubblewrap at `/usr/bin/bwrap`, and enabled unprivileged user namespaces
- DeepInfra API key
- Optionally, a Brave Search API key for web searches
- Git is recommended for checkpointing your code; this MVP **does not automatically roll back files**

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

Run `stepwright restore SESSION_ID --event EVENT_ID --dry-run` to preview restoring tracked files to a historical event. `fork` now offers workspace restoration before creating a new execution branch; see `docs/09-workspace-restore.md` for limitations.

`stepwright.yaml` specifies the provider model and default host tool approval policies. When asked to approve shell commands or native writes, use `[p]` to save a project whitelist entry or `[g]` to save a global entry. Shell grants match the exact command string; write grants cover native writes to permitted project files. `stepwright approvals` shows stored grants and their paths (edit the YAML files to revoke grants). See `docs/08-persistent-approvals.md` for details. Replace the default model with one available to your account if needed, using `cargo run -- models` to discover models.

**Important:** Agent shell commands, workflow command steps and command verification now run **inside Bubblewrap**, with a read-only `/workspace`, private `HOME`, no inherited API keys and no network. Shell writes are limited to an isolated `/tmp` and a project-local build cache under `.stepwright/state/sandbox-cache`. Source edits must use native `write` to keep diffs and history. There is **no unsandboxed fallback** if Bubblewrap is unavailable. This is not complete protection against untrusted programs or resource exhaustion; see `docs/10-shell-sandbox.md`.

## Commands

```text
stepwright sandbox-check
stepwright validate fix-bug
stepwright start fix-bug --goal "..."
stepwright start add-feature --goal "..." --model "provider/model"
stepwright resume SESSION_ID --instruction "Do not change dependencies"
stepwright sessions
stepwright tui                  # interactive session conversation
stepwright tui SESSION_ID       # focus on a session, resume or restart
stepwright history SESSION_ID
stepwright branches SESSION_ID
stepwright switch SESSION_ID --branch BRANCH_ID --step implement
stepwright artifacts SESSION_ID
stepwright artifact SESSION_ID plan.md > plan.md
stepwright put-artifact SESSION_ID plan.md --file plan.md
stepwright fork SESSION_ID --event 42 --step plan
stepwright models
```

For a globally installed binary, run `stepwright ...` instead of `cargo run -- ...`.

Run with `--root /path/to/project` to select the project directory containing `stepwright.yaml`, `workflows/` and `.stepwright/instructions.md`. Workflows may use includes in `shared/`. Config and workflow reload for each *new step run*.

Use `stepwright tui` to open a session conversation, send instructions to resume blocked work, approve tools and human gates, inspect edit diffs, browse workflow artifacts in the **[2] Artifacts** tab, and restart an earlier workflow step. The TUI polls the existing event database, so it can also observe a workflow running in another terminal. To restart, type `/restart`, choose a recorded step and confirm the guarded workspace restore before forking; see `docs/11-tui.md`.

When execution blocks, use `history`, inspect artifacts and then `resume --instruction`. If an attempt should be superseded without losing history, use `fork` from the desired event. Forking offers to restore **tracked native edits** first. Shell-only or manual changes cannot be reconstructed; preview with `restore --dry-run`.

## Design summary

- YAML frontmatter, YAML agent action protocol, Markdown prompt bodies.
- Fresh model context for each step, populated with selected registered artifacts.
- Required artifacts checked per attempt; additional artifacts are always allowed.
- Explicit verification, including `type: none`; composite `all` and `any` supported at one level.
- Deviations named by step, actions `pause`, `goto`, or bounded `retry`; optional independent model-based detector prompt.
- Human approval for risky tools, with once, process, project or global trust scopes; no automatic hidden error recovery.
- Append-only SQLite event stream with fork ancestry, artifact snapshots and immutable historical branches.
- DeepInfra/OpenAI-compatible chat-completions transport and model listing.
- Read-only, network-disabled sandboxed shell; project read/journaled native write tools; optional Brave web search and Discord notifications.

## Roadmap / TODO

> **Status:** proposed, prioritized backlog — not a list of finished features. PR #6 is an evolving TUI baseline, **not** the end-state UX. The detailed product rationale, hot paths, interaction rules, [competitive harness benchmark](docs/12-tui-ux-analysis.md#15-external-harness-benchmark-research-snapshot-2026-11-12) and acceptance criteria live in [the TUI UX analysis](docs/12-tui-ux-analysis.md).

### P0 — stabilize the current interactive baseline

- [ ] Finish local PR #6 validation: tests, Clippy, format check, generated Cargo.lock, and real-terminal smoke tests of the composer, permissions, artifacts and restore/fork.
- [ ] Verify that approval keys, denials, persisted grant scope and human verification are unmistakable in the TUI, with no accidental Enter approval.
- [ ] Ensure worker errors, interrupted sessions, malformed event content and partial restores surface prominently instead of appearing successful.
- [ ] Add robust per-session execution ownership and stale-run recovery; prevent simultaneous CLI/TUI resumes of the same session.
- [ ] Give each pending approval a durable request ID, decision receipt, and safe reconnection/replay behavior; reject stale/duplicate decisions.
- [ ] Create end-to-end tests for blocked → inspect → instruct → approve/deny → verify/recover, including an 80×24 terminal.

### P1 — TUI redesign for daily use

- [ ] Replace the raw log-centric layout with a session workspace: **status + stepper + context + primary action**, backed by typed events.
- [ ] Add a session home/picker with readable names, blocked-first visibility, last activity, search and an in-TUI start flow.
- [ ] Implement contextual blocked-state cards: show reason/evidence, inspect related artifacts, retry, resume or restart safely.
- [ ] Make focus, keyboard shortcuts, composer mode, help, scroll-follow behavior and pane resizing coherent and discoverable.
- [ ] Render artifacts as **terminal Markdown** (headings, lists, code, tables, links); support Preview/Raw, search, safe escaping and long documents.
- [ ] Add artifact version history and provenance (step, branch, attempt/event, timestamp), with version diffs and pinned review/approval evidence.
- [ ] Upgrade activity to semantic cards with expandable tool calls, verification results, file diffs, artifacts and structured failures.
- [ ] Build a workflow stepper with retry/loop/verification state, current attempt and a clear explanation of what comes next.
- [ ] Introduce durable step/attempt IDs, typed pending decisions and reliable interruption/reconnection semantics.
- [ ] Provide a per-attempt **context manifest**: exact step definition, selected artifact versions, code/evidence sources and accepted plan version.
- [ ] Define explicit **Ask / Instruct / Steer / Queue / Stop** composer behavior, with safe boundaries and no wrong-branch delivery.
- [ ] Split the large TUI controller into tested focus routing, activity, Markdown reader, approval and recovery components; cache/lazily render long content.
- [ ] Preserve draft instructions and per-tab selection/scroll position; support copy/open-in-editor and useful filtering.

### P2 — supervised intelligence and stronger recovery

- [ ] Add an optional **summarizer** model role: cache evidence-linked step/workflow summaries, clearly distinguish claims from verified results, and fall back to facts when unavailable.
- [ ] Add an optional read-only **manager** model role: answer questions using relevant code, artifact versions, workflow definitions and event evidence, with sources.
- [ ] Let the manager **propose** retry/goto/restart actions via a typed schema; require Rust-side validation, guarded restore previews and explicit user confirmation. No direct manager tool execution or permission grants.
- [ ] Make the message recipient explicit (**Instruct coding agent** vs **Ask manager**) to avoid unexpected mutations.
- [ ] Add read-only manager **side questions** that cite a consistent evidence snapshot without changing the coding-agent context or silently persisting private questions.
- [ ] Add richer restart controls: choose a specific historical attempt/checkpoint, inspect changed files, compare branches, understand untracked/manual edits.
- [ ] Support artifact review comments and safe new-version editing (external editor first, optionally embedded later).
- [ ] Improve diff review (file list, hunk navigation, copied patch), final workflow recap and exportable verification evidence.
- [ ] Evaluate optional **staged multi-file changesets** with preview, review, conflict-checked apply and verified-result linkage; keep the native edit journal.
- [ ] Introduce model-role configuration and usage/cost budgets without assuming multiple provider accounts.

### P3 — longer-term capabilities

- [ ] Model response streaming, clean cancel/interrupt/queue semantics and resumable sessions across process restarts.
- [ ] Safe Git-aware checkpoints and patch export; do not imply native restore captures arbitrary workspace changes.
- [ ] Parallel/subworkflow orchestration with explicit session ownership and approval routing.
- [ ] Provider flexibility, context compaction, bounded code retrieval/indexing and opt-in MCP tool integrations.
- [ ] More complete sandbox resource limits and secret-handling defenses, plus security regression tests.
- [ ] Optional desktop/Discord notifications, authenticated remote decisions and customizable keybindings/themes.
- [ ] Branch graph/time-travel visualization, saved views, accessibility/contrast testing and performance profiling.
- [ ] Consolidate older v0.1 docs whose pre-sandbox/pre-TUI wording is now obsolete.

**Prioritization rule:** reliable execution and human decisions before UI polish; usable artifact/recovery paths before optional AI-manager autonomy. Anything that can alter permissions, workflow history, or files requires clear provenance and a deliberate user decision.

## Caveats (read before use)

This MVP has deliberately limited capabilities: no cross-platform shell sandbox, no full Git checkpoint/restore, no parallel/subworkflow execution, no remote Discord decisions, no MCP, no streaming, no provider-specific reasoning parameters, no adaptive context compaction, and no transactional code edits. The YAML protocol is a best-effort model instruction validated by the runtime, not a guarantee that every model responds correctly. Bubblewrap does not provide resource quotas, comprehensive project-source secret scanning or immunity from kernel vulnerabilities.

The OpenAI-compatible DeepInfra HTTP API requires **JSON transport**; only the prompts, workflow files and *model-generated structured actions* use YAML. The latter is not JSON on the agent-facing surface.

The `optimize-performance` example loops on `target_not_met` and stops on regressions; automatic code rollback is intentionally **not** implemented. For safe rollback, restore the repository from a known Git checkpoint before resuming.

See `docs/` for execution contracts, workflow syntax and engineering tradeoffs.
