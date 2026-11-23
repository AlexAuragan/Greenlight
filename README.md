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

`.greenlight/config.yaml` (or legacy `greenlight.yaml`) specifies the provider model and default host tool approval policies. When asked to approve shell commands or native writes, use `[s]` to save a session-only grant (including across restarts), `[p]` to save a project whitelist entry or `[g]` to save a global entry. Shell grants match the exact command string; write grants cover native writes to permitted project files. Supported read-only commands can use the bounded project inspection profile without prompting when read policy allows; see [inspection permissions](docs/46-project-inspection-permissions.md) for supported forms, scope and opt-out. `greenlight approvals` shows project/global grants; `greenlight approvals --session ID` includes that session’s grants and file path (edit the YAML files to revoke grants). See `docs/08-persistent-approvals.md` for details. Replace the default model with one available to your account if needed, using `cargo run -- models` to discover models.

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
greenlight dump SESSION_ID --output conversation.md
greenlight dump SESSION_ID --json --output conversation.json
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

For complete model/tool evidence, use [conversation dumps](docs/35-conversation-dump.md) (TUI: More → Conversation dump). New calls retain exact prepared messages; legacy gaps remain labelled.

When execution blocks, use `history`, inspect artifacts and then `resume --instruction`. If an attempt should be superseded without losing history, use `fork` from the desired event. Forking offers to restore **tracked native edits** first. Shell-only or manual changes cannot be reconstructed; preview with `restore --dry-run`.

## Design summary

- YAML frontmatter, YAML agent action protocol, Markdown prompt bodies. **Agent output fields and semantics:** [docs/13-agent-action-protocol.md](docs/13-agent-action-protocol.md) (current parser is the enforcement authority).
- Fresh model context for each step, populated with selected registered artifacts.
- Required artifacts checked per attempt; additional artifacts are always allowed.
- Explicit verification, including `type: none`; composite `all` and `any` supported with the nesting limits described in [the execution contract](docs/01-execution-contract.md).
- Deviations named by step, actions `pause`, `goto`, or bounded `retry`; optional independent model-based detector prompt.
- Human approval for risky tools, with once, process, session, project or global trust scopes; no automatic hidden error recovery.
- Append-only SQLite event stream with fork ancestry, artifact snapshots and immutable historical branches.
- DeepInfra/OpenAI-compatible chat-completions transport and model listing.
- Read-only, network-disabled sandboxed shell; project read/journaled native write tools; optional Brave web search and Discord notifications.

## Documentation

Start with the [documentation index](docs/README.md), [current TUI controls](docs/11-tui.md), [settled decisions](docs/16-decision-log.md) and [actual Ratatui captures](docs/screenshots/README.md). Completed UI plans and proportional mockups have been removed; their history remains in Git. The unfinished [coordinator contract](docs/20-conversational-coordinator.md) is retained for P2.

## Roadmap / TODO

The indexed [open backlog](TODO.md) is the single source of remaining work, ordered by recovery/correctness, supervised workflows, then scale and integrations. P0/P1 implementation is complete on the feature branch; review and merge remain separate. Completed milestones are recoverable from Git and the [decision log](docs/16-decision-log.md).

Current evidence: [validation](docs/06-validation.md). Repeatable checks: [smoke tests](docs/17-smoke-test.md). The [code review](docs/code-review.md) explains the latest cleanup and outstanding findings.

## Caveats (read before use)

This MVP has deliberately limited capabilities: no cross-platform shell sandbox, no full Git checkpoint/restore, no parallel/subworkflow execution, no remote Discord decisions, no MCP, no provider-specific reasoning parameters, no adaptive context compaction, and no atomic multi-file edits. The YAML protocol is a best-effort model instruction validated by the runtime, not a guarantee that every model responds correctly. Bubblewrap does not provide resource quotas, comprehensive project-source secret scanning or immunity from kernel vulnerabilities.

The OpenAI-compatible DeepInfra HTTP API requires **JSON transport**; only the prompts, workflow files and *model-generated structured actions* use YAML. The action interface requests YAML mappings; its parser also accepts YAML’s JSON-compatible mapping syntax.

The `optimize-performance` example loops on `target_not_met` and stops on regressions; automatic code rollback is intentionally **not** implemented. For safe rollback, restore the repository from a known Git checkpoint before resuming.

See the [documentation index](docs/README.md) for runtime contracts, current controls, verification and unfinished design work. [Actual Ratatui captures](docs/screenshots/README.md) show implementation renders; they supplement the manual smoke tests and do not establish real-terminal or live-model compatibility on their own.

Recorded verification evidence can be exported without rerunning checks: `greenlight verification-evidence SESSION_ID`, or **More → Export verification evidence** in the TUI. See [diff hunks and verification evidence](docs/50-diff-hunks-and-verification-evidence.md).
