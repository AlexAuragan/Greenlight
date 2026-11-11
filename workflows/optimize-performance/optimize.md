---
id: optimize
tools: [read, shell, write, web]
inputs:
  required: [baseline.md]
  from:
    - source: session
outputs:
  required: [optimization.md]
deviations:
  - id: architectural_redesign_required
    prompt: ../../shared/check-architecture.md
    action: pause
  - id: no_viable_change
    action: pause
transitions:
  success: benchmark
---
# Propose and implement one performance change

Select one narrow improvement based on available benchmark evidence. Change only that. Write `optimization.md` explaining the expected benefit and exact changes. If improvement would need architectural redesign, stop with `architectural_redesign_required`.
