# Greenlight documentation

Use these documents for current behavior. The [indexed backlog](../TODO.md) tracks remaining work. Completed UI research/plans and proportional mockups were removed after implementation; Git retains their history. Settled decisions and unfinished requirements remain below.

## Start and use Greenlight

| Topic | Reference |
| --- | --- |
| Install, update, initialize and select a project | [Installation and projects](22-installation-and-projects.md) |
| Current TUI controls, popups, drafts, artifacts and Edits | [Interactive TUI](11-tui.md) |
| Project-only file reads, directory listing and path filtering | [Native file discovery](36-native-file-discovery.md) |
| Explicit user reruns of historical tool calls | [Tool reruns](37-tool-reruns.md) |
| Saved exact requests across app restarts | [Provider continuations](38-provider-continuations.md) |
| Rate limits, provider errors and explicit retry | [Provider recovery](19-provider-failures.md) |
| Full model/tool evidence and local exports | [Conversation dumps](35-conversation-dump.md) |
| Input/cached-input/output counts and model pricing | [Usage and pricing](18-token-usage-and-pricing.md) |
| Read-only project/pipeline questions | [Manager Q&A](21-manager-qa.md) |
| Actual application renders | [Ratatui captures](screenshots/README.md) |

## Runtime contracts

| Topic | Reference |
| --- | --- |
| Execution, attempts, ownership and verification | [Execution contract](01-execution-contract.md) |
| Workflow definitions and configuration precedence | [Workflow format](02-workflow-format.md) |
| Events, branches and immutable artifacts | [Persistence](03-persistence.md) |
| Modules and trust boundaries | [Architecture](04-architecture.md) |
| Native write journal and interrupted-write recovery | [Edit history](07-edit-history.md) |
| One-time/process/project/global grants | [Approvals](08-persistent-approvals.md) |
| Guarded restoration, forks and scope limits | [Workspace restore](09-workspace-restore.md) |
| Mandatory shell isolation and its limits | [Linux sandbox](10-shell-sandbox.md) |
| Coding-action fields, envelope handling and corrections | [Agent protocol](13-agent-action-protocol.md) |
| Hash manifests, drift and journal correlation | [Workspace recording](14-workspace-recording.md) |
| Legacy Stepwright compatibility | [Name migration](15-name-migration.md) |

## Decisions, future work and validation

- [Settled decisions](16-decision-log.md): rationale and current boundaries, including explicit deferred work.
- [Conversational coordinator contract](20-conversational-coordinator.md): retains unimplemented targeted delivery, guarded routing and live-owner inbox requirements. Only its read-only Q&A stage is currently implemented.
- [Code review](code-review.md): readability changes, corrected oversights and deferred findings.
- [Validation](06-validation.md): current evidence and reproducible checks.
- [Manual smoke tests](17-smoke-test.md): acceptance scenarios for real terminals and disposable projects.

Files under [prompts](prompts/) are compiled runtime resources, not disposable planning documents. Changes there can alter model behavior and require the protocol/manager checks. Numbering gaps are intentional; surviving files retain stable paths.
