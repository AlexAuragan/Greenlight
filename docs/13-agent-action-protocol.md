# Agent action protocol — authoritative field reference (current v0.1)

**Status:** documentation of the **currently implemented** model-to-harness YAML contract on the `feat/ratatui-session-viewer` branch. This is the canonical **human-readable reference for agent responses**, not a second permitted YAML syntax. The runtime implementation remains the enforcement authority: [`src/tools.rs`](../src/tools.rs) (`AgentAction`, `parse_action`, `Tools::call`) and [`src/engine.rs`](../src/engine.rs) (`PROTOCOL` and action dispatch). If this file, the prompt and the parser disagree, **the running code determines what is accepted** and all three should be reconciled.

Scope: *one model response describing one action*. This document is **not** the schema for `stepwright.yaml`, `workflows/*/workflow.yaml` or step Markdown frontmatter; those are independent authoring interfaces covered by [`02-workflow-format.md`](02-workflow-format.md) and the strict serde types in `src/config.rs`. The deviation detector's `detected`/`reason` response is likewise a separate internal schema.

## 1. Wire format and validation

- Each coding-agent response is **one YAML mapping**. A plain mapping or a ```yaml / ```yml fenced mapping is recognized. **No JSON or explanatory prose outside YAML** is the agent prompt contract.
- `action` is required. It must be one of **`tool`**, **`artifact`**, **`deviation`**, **`finish`**. There is no fifth action called `thought`, `plan`, `message` or `progress`.
- All other defined properties are optional **strings at deserialization time**. The engine/tool handler enforces presence when needed for a particular action.
- **Unknown property names are rejected** by `serde(deny_unknown_fields)`. Currently, extra *known* property names on the wrong action type may deserialize but be ignored (for example, `tool: read` on a `finish`). **Do not rely on this**. A stricter action-specific union/schema is a future compatibility improvement.
- The parser takes the first closing fence when parsing fenced output; strict rejection of text *after* the closing fence is not yet enforced. That is a parser gap, not permission to append commentary.
- A malformed/unknown action produces a recorded `protocol.error`; the model is asked to retry within its turn budget. A syntactically valid action with missing runtime-required fields can result in a step error or a returned tool error, depending on the handler.
- There is **no protocol version field** currently. `schema_version`, `activity_title`, `title`, `confidence`, `reasoning`, etc. are **not accepted** until the Rust type and prompt are explicitly changed.

### Complete list of currently permitted top-level fields

| Field | YAML type | Required where? | Implemented meaning |
| --- | --- | --- | --- |
| `action` | string | **Every response** | Discriminator: `tool`, `artifact`, `deviation` or `finish` |
| `tool` | string | `action: tool` | Tool name: `read`, `write`, `shell`, `web`; also must be enabled by the step and host policy |
| `input` | string | `tool: shell` or `tool: web` | Exact shell command or web-search query |
| `path` | string | `tool: read` or `tool: write` | Project-relative file path, subject to path checks |
| `content` | string | `tool: write` or `action: artifact` | Full replacement file content for a native write, or the complete registered artifact body |
| `name` | string | `action: artifact` | Session artifact name (relative name/path with no `..` or absolute components) |
| `summary` | string | Optional for `finish`, `deviation` | Human-readable completion summary or blocker/evidence narrative; an omitted value becomes empty in current handling |
| `deviation_id` | string | `action: deviation` | Named deviation; it should match a configured rule for normal handling |

**Only these eight keys are currently recognized.** Requiredness in the table is **semantic**, not an independent per-action serde schema.

## 2. Actions

### 2.1 Tool requests

**Read a file**:

```yaml
action: tool
tool: read
path: src/lib.rs
```

Reads a UTF-8 file under the project root, up to 100,000 bytes. Permission defaults to `allow` for read but can be configured otherwise. Sensitive paths and traversal are restricted.

**Write a file**:

```yaml
action: tool
tool: write
path: src/lib.rs
content: |
  pub fn answer() -> u32 {
      42
  }
```

`content` is the **complete new file**, not a patch. The write is limited to 100,000 bytes, existing parent directories and checked project-relative paths. The harness compares old/new content, previews a diff, asks permission when policy requires, and rechecks for concurrent changes before applying. Native writes are intended to be journaled for restore. **Current crash gap:** the tool applies the edit before the engine records its journal entry; a process failure in between can leave an unrecorded modification. This is a recording TODO, not an accepted guarantee.

**Run a command**:

```yaml
action: tool
tool: shell
input: cargo test
```

The exact command goes through an approval policy and the mandatory Bubblewrap sandbox: project files mounted read-only, no network and private HOME. A shell command cannot be used to modify project source and will not be captured as a native edit.

**Web search**:

```yaml
action: tool
tool: web
input: Rust serde_yaml deny_unknown_fields
```

Uses the configured Brave Search API (requires its API key), independently permissioned, with returned text treated as untrusted.

**Common semantics:** the step must enable the selected tool; configured `deny` wins over saved approvals; `ask` requests a decision. Tool responses and failures are recorded and sent back to the model as facts. `tool` actions do **not** complete a workflow step.

### 2.2 Register an artifact

```yaml
action: artifact
name: investigation.md
content: |
  # Investigation

  Root cause: ...
  Evidence: ...
```

Registers a named, versioned **session artifact** with the current producing step and branch. This is **not** a filesystem write. The content can be multiline Markdown. The name must be safe and relative. The registered body is limited by `session.max_artifact_bytes` (100,000 bytes by default). Registering an artifact does not advance the workflow; a `finish` action is still necessary.

The current store writes an `artifact.created` event, the blob and the artifact database record in sequence. Publication atomicity on failure is an explicit TODO.

### 2.3 Report a deviation

```yaml
action: deviation
deviation_id: architectural_blocker
summary: |
  The approved approach cannot satisfy the step.
  Evidence: ...
  Options: ...
```

The model reports a blocker; **it does not choose** whether to retry, jump or stop. The engine looks up `deviation_id` among the step's declared deviation rules, optionally runs an independent detector, then applies the configured `pause`, bounded `retry` or `goto`. An unconfigured ID is reported as an unconfigured deviation and blocks the step. `summary` is optional to the parser but should provide evidence and options.

### 2.4 Finish the current step

```yaml
action: finish
summary: |
  Implementation complete; see implementation-report.md.
```

Requests completion of **the current step only**. The harness checks that every required output artifact was registered **during the current step run** and runs the resolved verification policy. Failure becomes a deviation/blocker, not successful completion. If checks pass, the harness records `step.completed` and follows the workflow's declared `transitions.success`, if any. The model's `summary` is **not verification evidence** by itself.

## 3. What the model does *not* control through these fields

| Concern | Actual owner |
| --- | --- |
| Next step, branch, retry count, goto target | Workflow definition and Rust engine; not `finish.summary` or a model-invented `next_step` |
| Tool availability | Workflow/step `tools` and runtime permission policy |
| Approval lifetime or global grants | Human decision plus permission store; not an action-provided `approval_scope` |
| Required artifacts and verification | Step/workflow configuration and harness |
| Agent/harness model choice | Provider config, step override and CLI, not a response `model` field |
| Direct source modifications | The permissioned native `write` tool; `artifact` only stores registered content |
| Restart, restore or fork | Explicit CLI/TUI control paths with safety checks; no arbitrary `action: restart` |

## 4. Proposed extension: optional activity title (NOT IMPLEMENTED)

The proposed TUI activity stream may use a concise, model-authored title for **each response** or logical phase within a long workflow step.

Illustrative **future** protocol:

```yaml
action: tool
activity_title: Exploring call sites
tool: read
path: src/lib.rs
```

**This example currently fails parsing** because `activity_title` is unknown. It is a design proposal, not a supported key.

Before adding it, update `AgentAction`, `PROTOCOL`, tests and this reference together. Proposed behavior:

- Optional short, plain-text field; limit length and strip terminal control sequences.
- Record in a distinct `activity.title` event or attach to the corresponding model-action event; allow successive titles during one step.
- Show the label as **agent intent**, not an assertion that the tool ran or verification passed.
- Keep `action` limited to one operation per response; **do not** allow multiple YAML actions or arbitrary prose preambles just to display several phase titles.
- Without a title, generate a factual fallback such as `Reading src/lib.rs`, `Writing src/lib.rs` or `Running cargo test` from the parsed action. Do not invent a semantic summary beyond what the action states.

## 5. Related but separate YAML interfaces

| YAML surface | Implemented schema / reference | Difference |
| --- | --- | --- |
| Root `stepwright.yaml` | `Config`, `ProviderConfig`, `SessionConfig`, `Permissions`, `Notifications` in [`config.rs`](../src/config.rs) | Configures default provider, budgets, policies and notifications |
| Workflow `workflow.yaml` | `Workflow`, `StepDefaults` in [`config.rs`](../src/config.rs); [workflow format](02-workflow-format.md) | Declares graph, start and defaults |
| Step Markdown frontmatter | `Step`, `Inputs`, `Outputs`, `Verify`, `Deviation`, `Transition` in [`config.rs`](../src/config.rs); [workflow format](02-workflow-format.md) | Defines step kind, selected artifacts, constraints, verification, deviations and transitions |
| Deviation detector result | Local `Detection { detected: bool, reason: String }` in [`engine.rs`](../src/engine.rs) | Internal response from an independent check, **not** a coding-agent action |
| Agent action response | `AgentAction` in [`tools.rs`](../src/tools.rs); **this document** | Requests exactly one runtime operation or step completion/deviation |

## 6. Maintenance rules for a genuine ground truth

1. **Update this document in the same PR as any field or action semantics change**; document whether the field is required, ignored on other actions and visible in history.
2. Keep the engine's `PROTOCOL` prompt and `AgentAction` aligned. Ideally generate the prompt from a shared protocol declaration, or add tests that compare a canonical fixture/table to the accepted parser fields.
3. Add success/failure fixtures for each action and invalid combinations: unknown keys, missing required fields, wrong tool, wrong types, extra fence commentary and invalid paths.
4. Favor a tagged per-action enum or explicit post-deserialization validation if we later need to reject action-inapplicable known fields; **do not** quietly tighten parsing without compatibility tests.
5. Distinguish **documented/implemented** from **proposed** extensions. The runtime remains the executable authority while this file provides the stable human-facing contract.
