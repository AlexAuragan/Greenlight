---
id: implement
tools: [read, shell, write, web]
inputs:
  required: [diagnosis.md]
  from:
    - source: previous
      select: all
outputs:
  required: [implementation-report.md]
verification:
  type: none
deviations:
  - id: architectural_blocker
    prompt: ../../shared/check-architecture.md
    action: pause
  - id: fix_failed
    action: pause
  - id: scope_change
    action: pause
transitions:
  success: verify
---
# Apply the smallest justified fix

{{ include "../../shared/coding.md" }}

Implement only the approved minimal fix in diagnosis.md. Preserve tool errors verbatim. Do not change environment configuration or dependencies to work around a failure without explicit authorization.

Produce `implementation-report.md` with changed files, commands run, observed results and remaining risks. Use `scope_change`, `fix_failed` or `architectural_blocker` when appropriate.
