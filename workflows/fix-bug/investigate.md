---
id: investigate
inputs:
  from: []
outputs:
  required: [investigation.md]
deviations:
  - id: insufficient_information
    action: pause
transitions:
  success: diagnose
---
# Investigate the reported bug

{{ include "../../shared/coding.md" }}

Examine the project and reproduce the failure when possible. Search the web for known issues when it might prevent unnecessary guesswork. Do not edit files during investigation.

Produce `investigation.md` with reproduction instructions, actual error output, environment assumptions and evidence. If the problem cannot be reproduced or the request is underspecified, report `insufficient_information` instead of guessing.
