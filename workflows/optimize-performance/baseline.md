---
id: baseline
outputs:
  required: [baseline.md]
deviations:
  - id: benchmark_unavailable
    action: pause
transitions:
  success: optimize
---
# Establish a reproducible baseline

Identify a benchmark command and record exact environment, metric, measurement variance, and stopping target in `baseline.md`. Do not optimize until the baseline is reproducible. If it cannot be measured, report `benchmark_unavailable`.
