---
id: verify
inputs:
  required: [implementation-report.md]
  from:
    - source: previous
      select: all
outputs:
  required: [verification-report.md]
verification:
  type: agent
  prompt: ../../shared/review.md
deviations:
  - id: tests_failed
    action: pause
---
# Verify the fix

Run the relevant tests and the project's established CI commands where available. Compare results against the original reproduction and diagnosis. Report missing dependencies as blockers, not success.

Create `verification-report.md` with exact commands, return codes and results. When tests fail, report `tests_failed`.
