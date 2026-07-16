# rezidnt — project memory

Rust workspace implementing the rezidnt daemon: event fabric, run substrate, gate engine, MCP surface.
Canonical design: `docs/rezidnt-architecture.md` (v0.2 + DR-001 + DR-002). BINDING items change only via `/dr`.

## Non-negotiable invariants (full text: rezidnt-constitution skill)
I1 zero pixels in core · I2 control/data plane never mix (payload ≤32KiB, bytes→CAS) · I3 log is truth, state derived ·
I4 substrates behind traits · I5 MCP-first · I6 verifiers deterministic+interrogable (pass/fail/inconclusive, never coerce) ·
I7 one static binary, no telemetry · I8 clean-room (AGPL never read; permissive read-only via /intel; nothing ported).

## Team (agents)
implementer (maker, green) · auditor (checker, read-only, red) · oracle (failing-tests-first, cyan) ·
warden (ontology custodian, yellow) · analyst (DR-002 intel, magenta) · scribe (decision records, blue).
Maker and checker are different agents on purpose: a checker that can edit is a rubber stamp.

## Commands (the workflow)
/slice show current slice + acceptance criteria · /oracle write failing tests from criteria · /vet run the gauntlet ·
/debrief auditor verdict on the diff · /subject change the ontology (warden-gated) · /intel scoped competitor read (memo) ·
/dr draft a decision record · /handoff write session state for the next run.

## The loop (per slice)
/slice → /oracle <component> → implementer builds to green → /vet → /debrief → (fix or) advance. Definition of done = slice
criteria pass /vet and /debrief. Nothing else counts as done.

## Guardrails (hooks, enforced)
- `spec/ontology.md` edits are blocked outside a /subject session (ontology-gate).
- herdr/AGPL sources are blocked from Read/Fetch/clone everywhere (firewall, DR-002).
- edited .rs files are auto-rustfmt'd (fmt, PostToolUse).

## Style (full text: rust-conventions skill)
Rust edition 2024. thiserror in libs, anyhow in bins, no unwrap/expect outside tests, no blocking in async,
tracing span on every adapter task. Lore vocabulary capped at vet/debrief/dossier.

## Build
cargo per crate; workspace layout in docs §4. The verifier gauntlet is `bash .claude/hooks/vet.sh`.
