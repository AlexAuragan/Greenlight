# Terminal UI foundation

Greenlight has a read-only Ratatui session dashboard in addition to its existing CLI. This is the first incremental TUI milestone, not a second workflow/approval engine.

```bash
cargo run -- --root /path/to/project tui
cargo run -- --root /path/to/project tui SESSION_ID
```

The dashboard reads existing SQLite session and event history, including inherited branch ancestry. It refreshes automatically about every 600 ms, so you can keep it open in **one terminal** while starting or resuming a workflow in **another**. It does not modify the workflow, its events, files, or permissions, apart from the existing Store initialization when the local history database does not yet exist.

## Navigation

- `Tab` switches focus between sessions and event history; `Enter`, `Right` focus events; `Esc`, `Left` focus sessions.
- `j/k` and arrow keys move the focused selection; `g/G` and Home/End jump to first or last.
- The right-bottom pane shows the complete selected event payload; `PageUp/PageDown` scroll it.
- `d` toggles the unified diff when the selected event represents a native write or a recorded workspace restore.
- `v` toggles full and concise event modes. Concise mode hides repetitive `step.definition`, `context.assembled`, `model.output`, and `tool.request` events but retains failures, tool results, approvals, and change events.
- `r` requests an immediate refresh; `q` quits without stopping any running session.

## Safety and next iteration

The TUI intentionally does **not** collect approvals, resume sessions, fork branches, or run agent commands yet. The existing CLI is the only authority for those operations. This avoids introducing inconsistent approval behavior or losing terminal prompts in a subprocess. A later milestone can connect the UI to the engine through a single event/approval interface.

The screen uses the current terminal's alternate buffer and raw input mode; Ratatui restores terminal state when it exits. It requires a real interactive TTY, not redirected input/output. All event contents are local and may include commands, model output or private project details recorded by the harness; use the TUI only in a trusted terminal.

Ratatui 0.30.2 requires Rust 1.88 or newer. After adding the dependencies, regenerate and commit `Cargo.lock` from a supported Rust/Cargo installation.
