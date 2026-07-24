---
description: The done gate in one call — run /vet and /debrief concurrently, report both verdicts
argument-hint: "[files-or-blank-for-staged]"
---
Both halves of the definition of done, in parallel instead of in sequence. The auditor is read-only over the diff and does not need a green vet to render its verdict — serializing them doubles the gate tail for nothing.

1. Launch `bash .claude/hooks/vet.sh` via Bash with `run_in_background: true`.
2. While it runs, capture `git diff` (staged and unstaged; or scope to $ARGUMENTS) and delegate to the auditor agent with that diff and the current slice criteria — the same contract as /debrief. Do NOT tell the auditor the vet result or that vet is running; independence of the two verdicts is the point.
3. When both return, report the two JSON verdicts verbatim, vet first. DONE requires both pass. If either fails, route findings to the implementer; after the fix, re-run BOTH gates (the auditor re-judges the amended diff — never carry a stale verdict forward).

Concurrency guard: never launch /gauntlet while another lane's cargo tests are running (host or WSL) — core contention flakes the exec-verifier spawn tests. One gauntlet at a time, whole machine.
