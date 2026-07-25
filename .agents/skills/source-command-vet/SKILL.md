---
name: "source-command-vet"
description: "Run the local verifier gauntlet (fmt, clippy, tests, fixture replay)"
---

# source-command-vet

Use this skill when the user asks to run the migrated source command `vet`.

## Command Template

Run `bash .Codex/hooks/vet.sh` and report the JSON verdict verbatim. During inner rework loops (fixing findings, not gating done), `bash .Codex/hooks/vet.sh --fast` runs preflight+fmt+clippy only — it can fail conclusively but can never pass; the full gauntlet remains the only done gate. If the verdict is `fail`, list each failing stage and route remediation to the implementer agent. If `inconclusive`, name what could not be verified (usually absent fixtures) and what would make it conclusive. Do not describe a fail or inconclusive verdict as success.
