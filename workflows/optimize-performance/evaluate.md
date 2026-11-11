---
id: evaluate
inputs:
  required: [baseline.md, benchmark.md]
  from:
    - source: session
outputs:
  required: [performance-evaluation.md]
deviations:
  - id: target_not_met
    action: goto
    target: optimize
  - id: regression
    action: pause
  - id: architecture_limit
    action: pause
---
# Evaluate the result

Compare benchmark.md against baseline.md, and explain the evidence in performance-evaluation.md. If target met, finish. If target not met but a *new* authorized optimization has evidence, report `target_not_met` to loop. If regression occurs, report `regression` to request restoration (the MVP does not automatically restore files). If no meaningful improvement is possible without redesign, report `architecture_limit`. Avoid looping merely to spend more tokens.
