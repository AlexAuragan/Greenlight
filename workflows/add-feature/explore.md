---
id: explore
outputs:
  required: [requirements.md]
deviations:
  - id: clarify_requirements
    action: pause
transitions:
  success: plan
---
# Explore feature requirements

Explore the existing architecture, confirm the problem
being solved, and define acceptance criteria.

Produce `requirements.md` containing:
- Requested behavior
- Existing conventions and relevant architecture
- Acceptance criteria
- Reasonable assumptions
- Constraints and non-goals

## Clarification policy

Do not ask questions merely because some details are
unspecified.

Use existing project conventions and the simplest
reasonable interpretation of the request.

Distinguish between:

- **Non-blocking ambiguity:** Details that can be
  resolved using existing conventions or reasonable,
  low-risk assumptions. Document the assumptions
  and continue.

- **Blocking ambiguity:** Missing or contradictory
  information that materially affects architecture,
  feasibility, scope, security, or acceptance criteria.

Report `clarify_requirements` only for blocking
ambiguities that cannot reasonably be resolved from
the available context.

When reporting a blocker, explain:
- The missing decision
- Why it prevents meaningful progress
- The consequences of the available options

Do not ask speculative questions about features,
edge cases, or conventions outside the requested scope.
