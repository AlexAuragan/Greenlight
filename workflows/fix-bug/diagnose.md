---
id: diagnose
inputs:
  required: [investigation.md]
  from:
    - source: previous
      select: all
outputs:
  required: [diagnosis.md]
deviations:
  - id: architectural_blocker
    prompt: ../../shared/check-architecture.md
    action: pause
  - id: uncertain_root_cause
    action: pause
transitions:
  success: implement
---
# Diagnose before fixing

Form a root-cause hypothesis supported by observations and relevant code. Avoid speculative patches.

Write `diagnosis.md` containing evidence, the proposed minimal fix, tests to validate it, and explicit limits. If root cause is uncertain, report `uncertain_root_cause`; if an architectural redesign is required, report `architectural_blocker`.
