---
id: review
inputs:
  required: [implementation-report.md]
  from:
    - source: previous
outputs:
  required: [review-report.md]
verification:
  type: agent
  prompt: ../../shared/review.md
deviations:
  - id: ci_failure
    action: pause
---
# Review and test

Run the project's relevant test commands. Produce `review-report.md` with exact evidence and a comparison to the approved plan. Report failures as `ci_failure`, do not silently rewrite unrelated code.
