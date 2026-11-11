# Greenlight interactive TUI

Greenlight's Ratatui interface is a session conversation and a controller for **the existing Engine**. Both CLI and TUI use the same persisted histories, approval policy, workspace edit journal and fork/restore implementation.

```bash
cargo run -- --root /path/to/project tui
cargo run -- --root /path/to/project tui SESSION_ID
```

## Main interaction

Open a blocked or ready session. Press **Enter** to focus the persistent message composer, type instructions and press Enter again to resume the agent. A blocked session opens with the composer focused. While the agent works, its activity appears in the conversation timeline; the TUI refreshes automatically. The composer is disabled while the current worker runs, but events can still be inspected.

Example instruction: `Fix the existing add() bug and rerun verification.`

The TUI executes resumed sessions in a background worker thread with a separate Tokio runtime and database connection. It does **not** fork a CLI process or bypass the Engine. All shell commands still use the mandatory Bubblewrap sandbox.

When the Engine requests permission to execute a shell command, write a file, use a tool, or approve a human verification gate, it sends the **same** permission prompt to the TUI. The foreground TUI captures a decision and sends it back over channels. For shell and write prompts, choose `y` once, `a` for the process, `p` for the project or `g` globally (where offered), or `n` to deny; press **Enter** to confirm your selection. The default is **n**. For human verification, only `y/n` apply. Proposed native file diffs and required human review artifacts are shown in the approval pane.

## Restart from a historical step

Type `/restart` in the composer. Use `j/k` to select an earlier **executed** workflow step and press Enter. Greenlight selects the checkpoint immediately before the first recorded attempt at that step, constructs a **guarded workspace restoration plan** from its native edit history, and displays the file changes. Press `y` to apply and fork at that event; `n` or Escape cancels with no modification. This is a real workflow branch, not a text instruction asking the model to pretend it is at a different step.

The restore planner refuses mismatched file snapshots, manual drift, symlinks, and unsupported paths. Its guarantees apply only to recorded native writes/restores, not manual edits or untracked shell operations. Restoration applies individual files; if an error occurs partway through, the error is displayed and the user must inspect the workspace/history. Never retry blindly after a partial restore. Restart does not automatically execute the new branch; send a message or `/resume` from the composer.

## Keys

- **Enter** on a selected session/event focuses the message composer; **Enter** in the composer sends.
- **Ctrl+J** inserts a newline into the composer; **Escape** returns to event navigation.
- **Tab** switches between sessions, conversation history and composer.
- **j/k**, arrow keys and **g/G** or Home/End navigate sessions and events.
- **d** displays a journal-backed unified diff for a selected native write or restore; **v** toggles concise/full event visibility.
- **PageUp/PageDown** scrolls event details, permission requests, or restoration previews.
- **/restart** chooses the step to restart; **/resume** resumes without extra instructions; **/events** toggles raw event visibility; **/help** lists shortcuts.
- **q** quits from navigation mode when no agent is running. The TUI intentionally prevents quitting while its own worker is active, to avoid silently abandoning a pending prompt.

## Limitations

Initial TUI control is intentionally serial: only one runner per TUI, no streaming token-by-token model output, no direct interruption of an in-progress model call, and no interactive control of another process that happens to be running a session. Read-only observation of external sessions remains possible. The underlying CLI remains available. Concurrent execution of the **same session in separate processes** is not yet prevented by a robust per-session lock; don't run CLI `resume` simultaneously with a TUI resume.

Approval choices and restoration actions retain their existing security semantics. The TUI requires an interactive terminal and uses Ratatui's terminal restoration. Raw events may contain private project information.

After adding Ratatui/Crossterm dependencies, regenerate and commit `Cargo.lock` from a supported Rust installation.
