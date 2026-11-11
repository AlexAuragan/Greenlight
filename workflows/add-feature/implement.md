---
id: implement
tools: [read, shell, write, web]
inputs:
  required: [plan.md]
  from:
    - source: session
outputs:
  required: [implementation-report.md]
deviations:
  - id: scope_change
    action: pause
  - id: cannot_implement
    action: pause
transitions:
  success: review
---
# Implement the approved plan

{{ include "../../shared/coding.md" }}

Follow the approved `plan.md`. If required changes differ substantially from the plan, report `scope_change` and stop. Produce `implementation-report.md` listing actual code changes and tests.
