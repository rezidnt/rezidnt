---
name: rezidnt-constitution
description: >-
  The non-negotiable invariants, decision-status labels, and golden-path contract for the
  rezidnt project. This skill should be used as background whenever writing, reviewing, or
  designing any rezidnt code or architecture — it defines what BINDING means and what the
  eight invariants forbid. Load when working on the daemon, fabric, gates, or any component.
user-invocable: false
version: 0.2.0
---

# rezidnt constitution

Canonical source: `docs/rezidnt-architecture.md` (v0.2 + DR-001 + DR-002). This skill is the always-loaded distillation; when they disagree, the doc wins and the skill is stale.

## Decision-status labels
- **BINDING** — changing it requires a decision record (`/dr`). The eight invariants and the trait seams are BINDING.
- **DEFAULT** — current best call, cheap to revisit; change freely with a note.
- **PROVISIONAL** — do not build against it yet.

## The eight invariants (violations are the auditor's first check)
- **I1 Zero pixels in core.** `rezidentd` renders nothing. Every UI is a socket/MCP client. A rendering need never justifies a daemon change.
- **I2 Control and data plane never mix.** The fabric carries facts and refs. Payloads ≤ 32 KiB; anything larger becomes a CAS ref. PTY bytes, transcripts, diffs move out-of-band. This is the event-sourcing death if violated.
- **I3 The log is truth; state is derived.** Reducers are pure. Anything that cannot be rebuilt from log + CAS is misdesigned. Derived state is never the source of record.
- **I4 Substrates behind traits.** ProcessSubstrate/AgentSubstrate/RepoSubstrate (and the reserved Phase-3 TerminalSubstrate) are traits; implementations are swappable.
- **I5 MCP-first, UI-second.** Every capability is an MCP tool/resource before it is a keybinding. Agents are first-class operators.
- **I6 Verifiers are deterministic and interrogable.** Same content-hashed inputs → same verdict, replayable from the log. `pass|fail|inconclusive`; inconclusive is never coerced to pass. "Why blocked" returns the failing verifier and evidence.
- **I7 One static binary, no runtime deps, no telemetry.** `curl | sh` or `cargo install`. Phones home to no one.
- **I8 Clean-room rule (DR-001/002).** AGPL (herdr) sources are never read for implementation. Permissive (Omnigent) sources may be read read-only via `/intel`; nothing is ported. Boring components (portable-pty, tokio, rusqlite) remain mandatory — "our own system" is about the system, not syscall wrappers.

## Golden path (BINDING contract)
Cold machine → `curl` install → `rezidnt open <repo>` → worktree allocated, agent spawned under gates, fleet state visible, first verified diff merged — one take, zero config edits, single-digit minutes. Every slice is judged against advancing this demo, never a feature list. Completes at end of Phase 2.

## Sequencing law (most-violated in the wild)
fabric → gates → (optional) terminal. Any pressure to build interactive terminal fidelity before the fabric and gates exist is scope gravity; apply the phase-exit-demo test.

## Lore vocabulary cap
Product/user-facing terms stop at `vet`, `debrief`, `dossier`. Everything else stays boring. (Internal harness roles — warden, scribe, analyst — are not product surface.)
