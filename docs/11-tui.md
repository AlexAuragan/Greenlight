# Greenlight interactive TUI

Greenlight's Ratatui interface is a session conversation and a controller for **the existing Engine**. Both CLI and TUI use the same persisted histories, approval policy, workspace edit journal and fork/restore implementation.

```bash
cargo run -- --root /path/to/project tui
cargo run -- --root /path/to/project tui SESSION_ID
```

## Main interaction

Open a blocked or ready session. The TUI starts in navigation mode, even for blocked sessions: the composer never automatically captures shortcuts. From Sessions use **Right** or **Enter** to focus the conversation/artifact list; from a list use **Right** or **Enter** to focus the detail pane. Press **i** from any navigation pane to focus the message composer immediately, or press **Enter** from the detail pane. Type instructions and press Enter in the composer to resume the agent. While the agent works, its activity appears in the conversation timeline; the TUI refreshes automatically. The composer is disabled while the current worker runs, but events can still be inspected.

Example instruction: `Fix the existing add() bug and rerun verification.`

The TUI executes resumed sessions in a background worker thread with a separate Tokio runtime and database connection. It does **not** fork a CLI process or bypass the Engine. All shell commands still use the mandatory Bubblewrap sandbox.

When the Engine requests permission to execute a shell command, write a file, use a tool, or approve a human verification gate, it sends the **same** permission prompt to the TUI. The foreground TUI captures a decision and sends it back over channels. The TUI now shows a separate color-coded choice menu. Press **`y`** to approve once, **`a`** for the process, **`p`** for the project or **`g`** globally (where offered), or **`n`** to deny. **A keypress submits immediately; Enter does not approve or deny.** Escape denies. Only `y/n` are valid for human verification. The available choices are enforced from typed permission metadata rather than inferred from tool text. Proposed native file diffs and required human review artifacts are shown in the approval pane.

## Artifacts tab

Press **2** in navigation mode to open the **Artifacts** tab; press **1** to return to the conversation. The left pane lists the most recent version of each artifact on the selected session's active branch, along with its producing step and byte size. The right pane renders headings, emphasis, inline/fenced code, lists, blockquotes and simple pipe tables. Press **m** in navigation mode to toggle between rendered Markdown (the default) and exact raw text; the original stored artifact is never rewritten. Other CommonMark features, hyperlinks and complex table layouts are not yet supported. **j/k**, arrows and **g/G** select a saved artifact; **Up/Down** or **j/k** scroll the artifact when the detail pane is focused; **PageUp/PageDown** also scroll its contents. The selected artifact remains selected when newer versions are registered.

Artifacts are inputs to later workflow steps and an important human-review surface. This tab deliberately uses the same branch-aware `Store::artifacts` lookup as the CLI and does not alter artifact content. It currently shows **latest versions only**. To register a manually edited version, use `stepwright put-artifact SESSION_ID NAME --file PATH` from another terminal; version browsing and direct artifact editing are follow-on features.

## Restart from a historical step

Type `/restart` in the composer. Use `j/k` to select an earlier **executed** workflow step and press Enter. Greenlight selects the checkpoint immediately before the first recorded attempt at that step, constructs a **guarded workspace restoration plan** from its native edit history, and displays the file changes. Press `y` to apply and fork at that event; `n` or Escape cancels with no modification. This is a real workflow branch, not a text instruction asking the model to pretend it is at a different step.

The restore planner refuses mismatched file snapshots, manual drift, symlinks, and unsupported paths. Its guarantees apply only to recorded native writes/restores, not manual edits or untracked shell operations. Restoration applies individual files; if an error occurs partway through, the error is displayed and the user must inspect the workspace/history. Never retry blindly after a partial restore. Restart does not automatically execute the new branch; send a message or `/resume` from the composer.

## Keys

- **Left/Right** (or **Escape/Enter**) navigate from Sessions → event/artifact list → detail pane; focused detail content scrolls independently of the selected item.
- **i** focuses the composer directly; **Enter** from the detail pane also focuses it, and **Enter** in the composer sends. **Escape** from the composer returns to the previous pane.
- **Ctrl+J** inserts a newline into the composer; **Escape** returns to event navigation.
- **1 / 2** switch between Conversation and Artifacts tabs while in navigation mode; **m** toggles rendered Markdown/raw text in Artifacts.
- **Tab** moves focus between Sessions, the current tab's list, detail and composer.
- **j/k**, Up/Down and **g/G** or Home/End navigate sessions, events and artifacts when a list is focused; Up/Down or j/k scroll line by line when detail is focused.
- **d** displays a journal-backed unified diff for a selected native write or restore; **v** toggles concise/full event visibility.
- **PageUp/PageDown** scrolls event details, artifact content, permission requests or restoration previews.
- **/restart** chooses the step to restart; **/resume** resumes without extra instructions; **/events** toggles raw event visibility; **/help** lists shortcuts.
- **q** quits from navigation mode when no agent is running. The TUI intentionally prevents quitting while its own worker is active, to avoid silently abandoning a pending prompt.

## Limitations

Initial TUI control is intentionally serial: only one runner per TUI, no streaming token-by-token model output, no direct interruption of an in-progress model call, and no interactive control of another process that happens to be running a session. Read-only observation of external sessions remains possible. The underlying CLI remains available. Concurrent execution of the **same session in separate processes** is not yet prevented by a robust per-session lock; don't run CLI `resume` simultaneously with a TUI resume.

Approval choices and restoration actions retain their existing security semantics. The TUI requires an interactive terminal and uses Ratatui's terminal restoration. Raw events may contain private project information.

After adding Ratatui/Crossterm dependencies, regenerate and commit `Cargo.lock` from a supported Rust installation.
