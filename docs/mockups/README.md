# Static UI design studies

These are documentation assets for the [running-screen review](../24-running-screen-design.md), [approval-dialog review](../25-approval-dialog-design.md) and [completed-screen review](../26-completed-screen-design.md). They are not screenshots, executable prototypes or implemented application screens. Data, paths and code snippets are fictional. Exact colors, spacing and glyphs are illustrative; the images do not prove terminal-cell fit at 80×24.

| Study | Preview | Editable vector source |
| --- | --- | --- |
| Default running view | [PNG](running-wide.png) | [SVG](running-wide.svg) |
| Inspector explicitly opened | [PNG](running-inspector.png) | [SVG](running-inspector.svg) |
| Compact-width view | [PNG](running-compact.png) | [SVG](running-compact.svg) |
| Shell approval, all valid choices visible | [PNG](approval-shell.png) | [SVG](approval-shell.svg) |
| Pending native write and diff | [PNG](approval-write.png) | [SVG](approval-write.svg) |
| Global native-write confirmation | [PNG](approval-global-write.png) | [SVG](approval-global-write.svg) |
| Read-only Help above approval | [PNG](approval-help.png) | [SVG](approval-help.svg) |
| Completed, measurement unavailable | [PNG](completed-unmeasured.png) | [SVG](completed-unmeasured.svg) |
| Completed, recorded checks passed | [PNG](completed-checks.png) | [SVG](completed-checks.svg) |
| Historical result after restore | [PNG](completed-restored.png) | [SVG](completed-restored.svg) |
| Blocked step | [PNG](attention-blocked.png) | [SVG](attention-blocked.svg) |
| Provider authentication failure | [PNG](attention-provider.png) | [SVG](attention-provider.svg) |
| Interrupted worker | [PNG](attention-interrupted.png) | [SVG](attention-interrupted.svg) |
| Interrupted native write | [PNG](attention-write.png) | [SVG](attention-write.svg) |
| New session: workflow selection | [PNG](create-workflow.png) | [SVG](create-workflow.svg) |
| New session: goal editor | [PNG](create-goal.png) | [SVG](create-goal.svg) |
| Read-only manager composer | [PNG](compose-manager.png) | [SVG](compose-manager.svg) |
| Resume-step composer | [PNG](compose-resume.png) | [SVG](compose-resume.svg) |

The studies use the agreed semantic color roles and example RGB values from the main UI draft. The SVGs contain only drawing/text elements, not scripts or external resources. No runtime or project dependencies were added to create them.

See the [attention and recovery review](../27-attention-and-recovery-design.md) for the four recovery studies.

See the [session creation and composer review](../28-session-creation-and-composer-design.md) for the four input-flow studies.
