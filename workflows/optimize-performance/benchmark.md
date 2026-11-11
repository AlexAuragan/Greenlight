---
id: benchmark
inputs:
  from:
    - source: session
outputs:
  required: [benchmark.md]
deviations:
  - id: measurement_failure
    action: pause
transitions:
  success: evaluate
---
# Benchmark after the change

Run the same benchmark used in baseline.md, using equivalent conditions. Record exact command, metric, variance and interpretation in `benchmark.md`. Report `measurement_failure` if measurements are not comparable.
