# Terminal experience — v0.1

Greenlight has **both** a scriptable CLI and a full-screen Ratatui TUI. The CLI exposes individual operations, while the TUI lets a human inspect a branch-aware session, read artifacts, send instructions to resume work and approve tools or guided restarts. Both use the same engine, store, permission policy and native-write journal. See [current TUI controls](11-tui.md) and [the dated UX research](12-tui-ux-analysis.md).

- `start`: creates a session and runs until complete or blocked.
- `resume`: adds a user instruction and attempts the current blocked step again with fresh context. Without `--instruction`, retains workflow instructions and artifacts.
- `history`: displays branch-aware event IDs, useful for deciding where to fork.
- `artifacts` / `artifact`: inspect registered outputs. Redirect `artifact` to a Markdown file, edit externally and `put-artifact` to register a new version.
- `fork`: previews and optionally restores tracked native file edits before creating a branch from an earlier event; `--no-restore` explicitly skips inspection/restoration.
- `restore`: previews or applies a guarded restore of journaled native edits to a visible historical event.
- `changes` / `diff`: inspect native-write journal history and historical unified diffs.
- `workspace-status`: compare step-boundary SHA-256 manifests with the current project; this is read-only drift detection, not a backup.
- `models`: API model discovery.

For tools requiring permission, the CLI defaults to denial and supports one-time (`y`), process (`a`), project (`p`) and global (`g`) scopes where offered. Persistent shell approvals match the **exact command string**; native-write grants cover permitted writes at their stated scope. The TUI uses an explicit single-key choice; **Enter does not approve or deny** in its approval dialog. A deliberate extra confirmation for broad persistent grants is still TODO. See [permission semantics](08-persistent-approvals.md).

Notifications: Discord webhook when configured, on step blocked or session completed. Remote Discord approval is not implemented.

The implemented TUI has a session list, conversation-like event list, artifact Markdown reader with raw toggle, composer, typed approval prompt and guarded `/restart` flow. Navigation follows a **Sessions → event/artifact list → detail** focus hierarchy using Left/Right (or Escape/Enter), Up/Down or `j/k` for selection/scrolling, and `i` to enter the composer; it does not autofocus the composer, even for blocked sessions. PageUp/PageDown bindings were intentionally removed for easier keyboard use (including AZERTY layouts). A workflow stepper, contextual blocker cards, version browsing, structured manager Q&A and branch graph remain future work.
