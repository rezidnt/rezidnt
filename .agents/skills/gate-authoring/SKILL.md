---
name: gate-authoring
description: >-
  How rezidnt gates and verifiers work — the gate model, verdict contract, determinism
  requirements, native vs exec verifier kinds, and interrogability. This skill should be
  used when implementing or reviewing the gate engine, writing verifiers, or designing the
  vet/pre_merge/debrief lifecycle points. Load for work in rezidnt-gate. Also read by the
  auditor, whose own verdict shape mirrors this contract.
user-invocable: false
version: 0.1.0
---

# Gate authoring

The differentiation layer. Full text: architecture doc §8. Over-invest here.

## Model
A **Gate** is a named policy point bound to a lifecycle transition:
- `vet` — pre-spawn: is this agent spec, badge scope, and workspace state acceptable? (e.g. require `--bare` + pinned harness version for determinism.)
- `pre_merge` — is this diff verified? (tests pass, scope respected, no secret leak.)
- `debrief` / `post_run` — what did the agent actually do, and does the evidence support the claim?

A **Verifier** is a deterministic check attached to a gate.

## Verdict contract (BINDING)
```json
{"verdict":"pass|fail|inconclusive","evidence":[{"kind":"...","msg":"...","ref":"cas:blake3:..."}],"cost_ms":0}
```
`inconclusive` is first-class and honest: emit it when a check cannot run or cannot decide, and route it to a human. NEVER coerce inconclusive to pass — that is the single defect that kills the product's credibility (I6).

## Two verifier kinds (BINDING)
- **Native**: implement a Rust trait. Built-ins: diff-scope, tests-pass, forbidden-path-touch, secret-leak-scan, build-passes.
- **Exec**: any argv program speaking the JSON contract over stdin/stdout. This is the polyglot seam — a Roslyn pack, a Bun script, a Python linter all plug in identically. stdin carries the gate, CAS refs, params, timeout; stdout carries the verdict.

## Determinism requirements (BINDING)
- Inputs pinned by content hash — verifiers receive CAS refs, not mutable paths.
- No network by default; an exec verifier that needs it opts in via the gate def, recorded in the event.
- Wall-clock timeout (120 s DEFAULT). Nonzero exit or malformed output ⇒ `inconclusive`, never `pass`.
- Evidence blobs go to the CAS; `gate.passed|failed|inconclusive` events carry refs only (I2).

## Interrogability (I6 — the AX feature)
`gate why <run>` / MCP `gate_explain` returns the failing verifier, its evidence refs, and the exact inputs — so a blocked agent fixes the real defect instead of thrashing a refusal string. `debrief <session>` re-executes recorded verdicts from log + CAS; a divergence between recorded and replayed verdict is an integrity alarm (nondeterministic verifier = verifier bug; altered log = tamper). This replay property is what makes the audit trail evidence rather than assertion.

## Packaging
Generic verifier packs are open, separate crates. Domain judgment packs (DXP/Microsoft-stack failure modes) are the commercial seam and live OUTSIDE the public repo entirely.
