---
id: plan
inputs:
  required: [requirements.md]
  from:
    - source: previous
outputs:
  required: [plan.md]
deviations:
  - id: architecture_blocker
    prompt: ../../shared/check-architecture.md
    action: pause
transitions:
  success: approve
---
# Design the smallest sound feature

Produce `plan.md` with affected modules, architecture tradeoffs, tests, risks, and explicit non-goals. Do not assume feasibility without evidence. Stop for `architecture_blocker` if the requested feature requires more scope than permitted.
