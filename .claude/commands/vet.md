---
description: Run the local verifier gauntlet (fmt, clippy, tests, fixture replay)
allowed-tools: Bash(bash .claude/hooks/vet.sh), Bash(bash .claude/hooks/vet.sh --fast)
---
Run `bash .claude/hooks/vet.sh` and report the JSON verdict verbatim. During inner rework loops (fixing findings, not gating done), `bash .claude/hooks/vet.sh --fast` runs preflight+fmt+clippy only — it can fail conclusively but can never pass; the full gauntlet remains the only done gate. If the verdict is `fail`, list each failing stage and route remediation to the implementer agent. If `inconclusive`, name what could not be verified (usually absent fixtures) and what would make it conclusive. Do not describe a fail or inconclusive verdict as success.
