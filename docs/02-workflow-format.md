# Workflow format — v0.1

## File layout

```text
greenlight.yaml                    global/provider/security settings
.greenlight/instructions.md        optional project instructions
shared/*.md                        reusable prompt fragments
workflows/fix-bug/workflow.yaml     workflow graph + step defaults
workflows/fix-bug/investigate.md    step YAML frontmatter + prompt
.greenlight/state/history.db       private runtime event database
.greenlight/state/blobs/           immutable registered artifact bodies
```

`workflow.yaml` declares `id`, `title`, `start`, `steps: [id...]` and `defaults` containing optional `model`, `max_turns`, `tools`, `verification`.

Each `.md` file begins with `---` YAML frontmatter; the required `id` matches the filename. Fields:

- `kind`: `agent` (default), `human`, or `command` (`command` field mandatory for command steps).
- `model`: provider model ID, optional override of default.
- `max_turns`: limit of provider calls for one agent run.
- `tools`: subset of `read`, `shell`, `write`, `web`; a step can override defaults.
- `inputs.required`: required named artifacts.
- `inputs.from`: list of `{source: previous | session | step_id, select: all | required}`.
- `outputs.required`: named documents that MUST be registered during the current step run.
- `verification`: explicit or inherited verification mapping with `type`.
- `deviations`: list of `{id, prompt?, action: pause | goto | retry, target?, max_retries?}`.
- `transitions.success`: next step; omitted ends workflow.

`{{ include "../../shared/coding.md" }}` recursively includes a file restricted to the project root, with maximum include expansion depth 12. Other expression templates are deliberately not supported yet.

Additional produced artifacts are permitted and versioned. `source: previous` refers to the last *completed* step run. `source: session` includes all latest registered artifacts visible on the active branch. `select: required` includes only artifacts declared as required by the producing step. Values are injected into the agent's fresh context, bounded by `session.max_artifact_bytes`.

## Precedence and trust

1. Global `greenlight.yaml` provides provider defaults and absolute permission policies.
2. Workflow defaults fill unset step frontmatter.
3. Step frontmatter overrides workflow defaults.
4. `.greenlight/instructions.md` augments step prompt; it does not override permissions.
5. CLI `--model` and `--instruction` apply to the current command's step runs.

Tool permissions are **not** overridable from workflow frontmatter. Default policies are `read: allow`, `shell/write/web: ask` in absence of explicit global settings. An `allow` policy executes directly; `deny` rejects; `ask` offers one-time or process-lifetime approval. Restarting clears process approvals.

## Example

```markdown
---
id: implement
tools: [read, shell, write, web]
inputs:
  required: [plan.md]
  from:
    - source: previous
      select: all
outputs:
  required: [implementation-report.md]
verification:
  type: all
  checks:
    - type: command
      run: uv run basedpyright
    - type: human
deviations:
  - id: architectural_blocker
    prompt: ../../shared/check-architecture.md
    action: pause
transitions:
  success: verify
---
# Implement the approved plan

{{ include "../../shared/coding.md" }}
```

## Validation

Unknown config and step fields are rejected. Workflow graph references and supported tools/deviation actions are checked. Full graph reachability, recursive detector checking, rich template variables, static schema export and quantitative benchmark expressions remain future work.
