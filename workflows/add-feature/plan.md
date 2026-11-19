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

Register `plan.md` with `action: artifact`; a project source file named plan.md is not required. Trace the actual caller/constructor and displayed instance, including monitor/context arguments. Distinguish unused instances from rendered ones. Name the integration files needed to carry those arguments; verify assumptions against source before asking for approval.
