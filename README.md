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

## Caveats (read before use)

This MVP has deliberately limited capabilities: no cross-platform shell sandbox, no full Git checkpoint/restore, no parallel/subworkflow execution, no remote Discord decisions, no MCP, no streaming, no provider-specific reasoning parameters, no adaptive context compaction, and no transactional code edits. The YAML protocol is a best-effort model instruction validated by the runtime, not a guarantee that every model responds correctly. Bubblewrap does not provide resource quotas, comprehensive project-source secret scanning or immunity from kernel vulnerabilities.

The OpenAI-compatible DeepInfra HTTP API requires **JSON transport**; only the prompts, workflow files and *model-generated structured actions* use YAML. The latter is not JSON on the agent-facing surface.

The `optimize-performance` example loops on `target_not_met` and stops on regressions; automatic code rollback is intentionally **not** implemented. For safe rollback, restore the repository from a known Git checkpoint before resuming.

See `docs/` for execution contracts, workflow syntax and engineering tradeoffs.
