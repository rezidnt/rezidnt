[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-006 — Replay-divergence integrity signal

**Date:** 2026-07-17 · **Status:** ACCEPTED (owner) · **Amends:** §8 (replay/integrity alarm), §14 (self-observation). Mints a new subject (warden follow-on). I3 is the axis.

## Context

`rezidnt debrief <run>` replays recorded verdicts (`crates/rezidnt-gate/src/lib.rs:727`); recorded≠replayed raises an `IntegrityAlarm` (lib.rs:709). Today this is **CLI-report-only**: `debrief` runs in `bins/rezidnt` (main.rs:483), reads the SQLite log + CAS directly, prints `report.alarms[]`, and exits 3 — it **emits nothing to the log**. I3 says the log is truth, so an integrity check that fires but leaves no trace is invisible to `rebuild` and to any auditor querying the log — and log tampering is precisely the signal I3 most needs durable. Load-bearing constraint: `debrief` is a standalone CLI read with **no fabric writer handle**, so emitting a fact requires either routing debrief through the daemon or standing up a second log writer — a real architecture decision, not a one-liner.

## Decision (durability now, dedicated subject)

Divergence **lands a durable fact on the log** via a new dedicated subject, chosen over reusing the broad `daemon.error` bucket so the integrity signal is precisely queryable and the gate vocabulary stays clean (integrity-of-log ≠ gate verdict). Two-part follow-on, both required:

1. **Resolve the writer path.** `debrief` becomes daemon-routed for the append (the daemon owns the single writer); the CLI keeps its direct read for the report. The replay itself stays a deterministic read over log+CAS.
2. **Warden `/subject` mints `integrity.alarm`** with a **folding reducer** (no consumer-less subject). Payload carries the run, the diverging verifier, and both verdicts (recorded vs replayed). The `SUBJECTS_V0` companion edit lands same-commit (taxonomy-drift precedent).

## Consequences

- The S4 pin `golden_path.rs::cli_debrief_divergence_raises_integrity_alarm` (CLI report + exit 3) stays correct; the durable fact is strictly additive to it.
- New work stream: an oracle board for the daemon-routed append + the reducer, a warden `/subject` for `integrity.alarm`, then implementation. Retires the divergence `/dr` item.
- Risk delta: adds the daemon-routed-debrief path to the architecture surface; closes the I3 gap for the one event class (possible tampering) where invisibility was least acceptable.

*Amendments to this record require DR-008.*
