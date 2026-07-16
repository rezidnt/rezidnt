---
description: Auditor verdict on the current diff against invariants and slice criteria
argument-hint: "[files-or-blank-for-staged]"
---
First run `git diff` (staged and unstaged; or scope to $ARGUMENTS if provided) and capture the output. Then delegate to the auditor agent with that diff and the current slice criteria. Return the auditor's JSON verdict verbatim. On `fail`, route findings to the implementer. This is the post-hoc evidence gate — the same maker-checker separation the product itself enforces.
