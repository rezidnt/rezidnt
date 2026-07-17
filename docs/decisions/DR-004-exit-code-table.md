[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-004 — Stable exit-code table: resolve the gate-fail/local-input collision

**Date:** 2026-07-17 · **Status:** ACCEPTED (owner) · **Amends:** §9 command surfaces (CLI stable exit codes). No invariant text touched; I6 unaffected (verdicts stay `pass|fail|inconclusive` on the fabric — exit codes are the CLI projection only).

## Context

§9 pinned "stable exit codes (0 ok, 2 gate-fail, 3 substrate-fault, 4 daemon-unreachable)." The S1/S2 CLI as built uses **exit 2 for local input errors** (unreadable/unparseable spec, malformed run ULID — before any daemon traffic) and **exit 3 for daemon-side refusals** at open (`Reply::Error`, `open-failed` warning), stretching "substrate-fault." The collision was flagged in `bins/rezidnt/src/main.rs` module docs and `cli_verbs.rs` at implementation time, carried twice, and was REQUIRED before Phase 2: when S4 lands real gates, `rezidnt vet`/`debrief` in CI must distinguish gate-fail from local misuse by exit code alone.

Forcing fact: **clap emits exit 2 for usage errors** (unknown subcommand, bad flags) regardless of our table. §9's gate-fail=2 was unimplementable without overriding the argument parser's error path.

**Options considered.**
- **A — Keep §9's table; move local-input to a new code (e.g. 64/EX_USAGE).** Honors the ratified text. Cost: override clap's error exit everywhere, change 3 exit sites, rewrite the `Some(2)` pin in `cli_open_missing_spec_is_exit_2_family`, and fight the ecosystem convention forever.
- **B — Re-ratify §9 with implemented semantics only (2=local-input, 3=substrate-fault-incl-refusal), gate-fail folded into 3.** Zero migration, but loses the very distinction S4 needs: gate-fail vs daemon fault.
- **C — Widen the table: 0 ok · 1 unexpected internal error · 2 local input/usage (clap-aligned) · 3 substrate-fault (daemon-side refusal or failure) · 4 daemon-unreachable · 5 gate-fail.** Zero code migration (no gate emitter exists yet; 5 is unclaimed), clap-aligned, and gate-fail gets a dedicated code before anything builds on it.

**Dissent recorded:** the counterargument for A is that gate-fail is the golden-path outcome and deserved the low conventional number, and that scripts written against §9's published table would break. Answer: no such scripts exist yet (no gate emitter has shipped), and the published table was never achievable under clap.

## Decision

**Option C** (owner-ratified 2026-07-17). §9's exit-code sentence is replaced with:

> Global `--json` on every verb; stable exit codes (BINDING once ratified): **0** ok · **1** unexpected internal error · **2** local input/usage error (clap convention; daemon never reached) · **3** substrate fault, including daemon-side refusals · **4** daemon unreachable · **5** gate-fail (`vet`/`debrief`/`pre_merge` verdict `fail`; `inconclusive` is NOT 5 — it is 3, never coerced toward pass or fail, per I6).

## Consequences

- Migration cost: **zero code, zero tests.** The sole pinned failure code (`cli_verbs.rs::cli_open_missing_spec_is_exit_2_family`, `Some(2)`) remains correct. Exit-3 refusal paths in `main.rs` are ratified as-is; its module-doc `/dr` flags are retired by citing this record.
- S4 oracle board MUST pin exit 5 for gate-fail and exit-3-for-`inconclusive` before the gate engine lands. Risk-register delta: removes the twice-carried Phase-2 blocker; adds a small doc-drift risk (any external notes quoting the old table are now stale).
- No test or criterion is weakened; this tightens the table before anyone depends on the broken one.

*Amendments to this record require DR-005.*
