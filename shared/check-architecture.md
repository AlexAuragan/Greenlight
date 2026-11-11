# Independent architectural-blocker check

Review the reported evidence. A blocker is legitimate when the approved approach requires substantial unplanned architectural changes, unsupported platform behavior, or an environment change the user has not authorized. A difficult implementation detail is not automatically a blocker.

Be conservative: if evidence is insufficient, return `detected: false` and explain exactly what is missing. Do not propose or perform modifications. Return only YAML with `detected` (boolean) and `reason` (text).
