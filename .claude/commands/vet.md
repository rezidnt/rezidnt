---
description: Run the local verifier gauntlet (fmt, clippy, tests, fixture replay)
allowed-tools: Bash(bash .claude/hooks/vet.sh)
---
Run `bash .claude/hooks/vet.sh` and report the JSON verdict verbatim. If the verdict is `fail`, list each failing stage and route remediation to the implementer agent. If `inconclusive`, name what could not be verified (usually absent fixtures) and what would make it conclusive. Do not describe a fail or inconclusive verdict as success.
