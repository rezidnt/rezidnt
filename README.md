# rezidnt development harness

A Claude Code agent team, skill set, command set, and enforcement hooks for building **rezidnt** —
the local-first resident daemon (event fabric + run substrate + gate engine + MCP surface).

This is not the product. It is the harness that builds the product: it encodes the architecture's
invariants, the maker-checker discipline, and the standing gates directly into the tools so they hold
across sessions instead of living in your head.

## Install (Claude Code)
Drop the `.claude/` directory and `CLAUDE.md` at the root of the rezidnt repo. Also copy
`docs/rezidnt-architecture.md` into `docs/` — the skills reference it as canonical. Then in Claude Code:
`/slice` should print the current slice (S0) and its acceptance criteria.

## The team (agents)
| Agent | Role | Access | Why it exists |
|---|---|---|---|
| **implementer** | maker — writes Rust for the current slice | edit | builds against oracle tests, inside the invariants |
| **auditor** | checker — verdict on the diff | read-only | a checker that can edit is a rubber stamp |
| **oracle** | writes failing tests from criteria | edit (tests) | the sequencing law is real only if tests come first |
| **warden** | sole ontology custodian | edit (gated) | taxonomy drift is unrecoverable after events ship |
| **analyst** | DR-002 competitor intel → memos | read-only | keeps external influence auditable, not banned |
| **scribe** | decision records | read-only draft | a decision that isn't recorded didn't happen |

## The loop (commands)
```
/slice                 # what are we building, and what does done mean
/oracle <component>    # oracle writes failing tests from the slice criteria
   (implementer builds to green, inside I1–I8)
/vet                   # fmt + clippy -D + tests + fixture replay → pass|fail|inconclusive
/debrief               # auditor renders a verdict on the diff
   (fix findings, or advance the slice)
/subject <name>        # warden-gated ontology change
/intel <question>      # analyst files a competitor memo (DR-002)
/dr <decision>         # scribe drafts a decision record for any BINDING change
/handoff               # write session state so the next run resumes cold
```

## The guardrails (hooks — verified blocking, not advisory)
- **firewall** — blocks Read/Fetch/clone of herdr (AGPL) sources everywhere (I8 / DR-002). Omnigent docs pass.
- **ontology-gate** — blocks direct `spec/ontology.md` edits outside a `/subject` session.
- **fmt** — auto-rustfmt on every edited `.rs` file.

All were behavioral-smoke-tested — they consume real tool-call JSON on stdin and were
confirmed to block what must block and pass what must pass.

## The skills (shared knowledge, auto-loaded)
`rezidnt-constitution` (the eight invariants + golden path) · `event-fabric` (envelope, subjects, log, CQRS) ·
`rust-conventions` (error/async/deps) · `gate-authoring` (verdict contract, determinism, interrogability) ·
`testing-oracles` (property tests, fixtures, transcript contracts, benchmark seam) ·
`prior-art-protocol` (DR-002 mechanics) · `slice-discipline` (roadmap + per-slice acceptance criteria).

## First session
1. Clear the one thing the harness can't do for you: register the name across crates.io/npm/domains
   (`rezident` is the fallback string).
2. `/slice` → `/oracle fabric` → build S0 to green → `/vet` → `/debrief`.
3. `/handoff` before you stop.

## Maintenance
Bump skill `version:` when you change a skill body. When the architecture changes, `/dr` first, then update
the constitution skill to match `docs/rezidnt-architecture.md`. The doc is canonical; skills are its distillation.
