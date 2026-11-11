# Terminal experience — v0.1

Current interface is a scriptable CLI, not a full-screen TUI. Output includes current workflow/step, tool requests, short tool-result previews, registered artifacts, deviations, verification and session/branch IDs.

- `start`: creates a session and runs until complete or blocked.
- `resume`: adds a user instruction and attempts the current blocked step again with fresh context. Without `--instruction`, retains workflow instructions and artifacts.
- `history`: displays branch-aware event IDs, useful for deciding where to fork.
- `artifacts` / `artifact`: inspect registered outputs. Redirect `artifact` to a Markdown file, edit externally and `put-artifact` to register a new version.
- `fork`: creates a branch with events/artifacts as of an earlier event; does not touch the project workspace.
- `models`: API model discovery.

Approval prompts default to No and allow a single operation or all operations of a tool category for the life of the CLI process. This should be refined to command-pattern-based trusted permissions in a future UX version.

Notifications: Discord webhook when configured, on step blocked or session completed. Remote Discord approval is not implemented.

Future TUI design should offer roadmap, live conversation, artifact tree, branch graph, and a decision panel with clear evidence for any deviation. All actions must remain available via CLI before TUI-specific integration.
