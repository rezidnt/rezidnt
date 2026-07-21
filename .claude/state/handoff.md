# Handoff — 2026-07-21 (session 18: c3-op-secrets DONE — 1Password op backend; C3 credential-brokering arc complete; next = owner-gated live-op proof / Platform backends)

## State of play
**`c3-op-secrets` is DONE.** The C3 credential-brokering arc is now COMPLETE end-to-end: a shipped `rezidnt open`
run mediates egress AND brokers a secret the agent never holds, sourced from EITHER a host file OR **1Password**
(`op read op://vault/item/field`, exec'd not linked, vault-scoped service account) — both behind the DR-029
`SecretSource` I4 seam, dispatched by reference scheme. DR-030 ratified, implementation landed + audited + a
seam-cleanup round (removed trait pollution). Host `/vet` PASS, op suites green host+WSL, WSL crit-5 (live op)
honest-skips, `/debrief` PASS (no blocking defects). High autonomy ON ([[autonomy-high-trust]]). `current-slice`
= `c3-op-secrets` (**done**). Pushed to `origin/main` (push is the last step of this handoff).

## Current slice & criteria
`c3-op-secrets` — DONE. DR-030's five criteria: (1) scheme-dispatch (`op://` → OpSecretSource, plain →
HostFileSecretSource, Composite mixes both); (2) OpSecretSource resolves via exec'd `op read` + newline-trim +
redaction; (3) honest degrade taxonomy — op absent ⇒ UNAVAILABLE (spawn err), token unset ⇒ AUTH_FAIL (op exit 1),
op read other-nonzero ⇒ RESOLUTION_FAIL — each DROPS with a DISTINGUISHING `credential.dropped` reason, never a
fake token; (4) leak-discipline — `OP_SERVICE_ACCOUNT_TOKEN` + resolved value in NO fact/log/trace/RunError/agent-env,
`.expose()` single call-site. Crit 1–4 → GREEN (21 host op tests; also green on WSL). (5) live op-injected mediated
run → WSL `#[cfg(unix)]` `egress_fold_op_mediated_run_c3_wire`, **OWNER-GATED** (honest-skips 2/2 until the owner
provides live op + token). `/debrief` = PASS. Auditor's one flagged nit (trait pollution) was FIXED this session.

## THE OWNER-GATED PROOF (the one open thread on C3)
Criterion 5 — a LIVE `op`-injected mediated run — is the only C3 arm not yet proven, and it needs the OWNER to
provision on the WSL box: **(a)** the `op` CLI installed on PATH, **(b)** a vault-scoped `OP_SERVICE_ACCOUNT_TOKEN`
in the daemon env, **(c)** `REZIDNT_TEST_OP_REF` pointing at a real vault item (e.g. `op://rezidnt/GitHub/token`).
Then it's a one-command proof:
`cargo build -p rezidnt-run --example op_fake` (already built) then
`cargo test -p rezidentd --test egress_fold_op_mediated_run_c3_wire -- --test-threads=1` (currently honest-SKIPs
without a/b/c, exactly like the pasta/bwrap gates). Until provisioned, it skips — NOT a failure. **NB the "1Password
for Claude" connector (Desktop/Chrome, human-in-the-loop) is the WRONG tool here — the daemon needs the `op` CLI +
a service account; see [[secret-source-1password-direction]].**

## What changed this session (git log since c3-wire handoff `2e43ef3`)
- `7737b8e`/`f2c7fc9`/`a9e443d` **c3-egress-fold** (DR-029): `[egress]` block + `SecretSource` seam + host-file MVP;
  live run reaches the Mediated arm (DR-026 crit 4 at run-loop level); 5 `egress.*`/`credential.*` subjects minted.
- `2d77d23` **DR-030** (c3-op-secrets): the `op` backend design + the connector-vs-op-CLI distinction.
- `09aaf53` **c3-op-secrets impl**: `OpSecretSource` (exec `op read`), `CompositeSecretSource` scheme-dispatch,
  per-floor drop reasons; `fold_c3_policies` composite wiring; `examples/op_fake.rs` (dev-only cross-platform fake op);
  testkit `start_daemon_with_op_secrets`/`op_ref_available`. `with_binary` inherent-only (seam un-polluted). No new
  linked dep.

## Next action (owner's steer — C3 core is complete)
The C3 sole-chokepoint (sandbox + inescapable egress + credential brokering, host-file + 1Password) is DONE on
Linux/WSL. Natural next options, owner to pick:
1. **Finish the live-op proof** — once the owner provisions op + token (above), run it + fold the evidence in (tiny).
2. **macOS/Windows sandbox+egress backends** — each its own DR behind the ratified traits; Windows coupled to the
   deferred native-Windows Platform phase (Phase 3). Demand-gated.
3. **A different phase** — e.g. S5 ratatui read-only fleet board (the I1 proof, can precede Phase 3), or the
   benchmark harness (DR-022). 
There is no forced next slice; C3 can stop here (like the roadmap's "may stop after any primitive", DR-025).

## Open /debrief findings (NON-BLOCKING, carried — none blocks done)
1. **No-widening test is a compile-time interface pin, not fail-first-on-ADDED-door** (`egress_fold_no_widening_fold.rs`,
   DR-029) — a `trybuild` compile-fail fixture would close it; the private-field guard genuinely holds.
2. **Stale `sandbox.unavailable` doc-strings in `sandbox.rs`** (~152/160/204/402) — cosmetic; composed path rides `egress.*`.
3. **`op_fake_bin()` doesn't assert the example exists** (`secret_source_op_resolve.rs:73`) — when `op_fake` isn't
   built the op suites fail LOUDLY (expect-Some-got-drop) rather than with a clear "run `cargo build --example op_fake`"
   message. A diagnostic-clarity nit (not a false-pass); an existence-assert would sharpen it.

## Decisions still needing a /dr or /subject
- macOS/Windows sandbox+egress backends (each own DR; Windows ↔ deferred Platform phase) · an MCP-based 1Password
  backend (heavier alt behind the same seam; DR-030 fenced it) · the richer role-layer/`[gates.permit]`-precedence
  egress fold (only if demanded) · smaller carried: bench.completed, holder-offline (DR-018 §b), fast-path cache,
  OPA/Cedar.

## Environment (essentials)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`, **quote the
PATH export** ([[wsl-dev-environment]]). Vet host-side; **host+WSL SEQUENTIAL** ([[vet-concurrency-flake]]);
**WSL-green NOT sufficient, /vet is host-side** ([[vet-is-host-side-wsl-insufficient]]) — and **lint `#[cfg(unix)]`
test BODIES on WSL** (`cargo clippy -p <pkg> --test <name> -- -D warnings`; host can't reach the unix body).
**Dev-only example binaries must be built before their suites run OFF the full gauntlet:** host `vet.sh` builds them
via `clippy --workspace --all-targets`, but a bare WSL `cargo test` does NOT — build first:
`cargo build -p rezidnt-run --example op_fake` (op suites) and `--example egress_c3bc_probe` (egress/mediation suites),
else the exec-based tests fail (spawn err → UNAVAILABLE, masking the real path). WSL op suites:
`cargo test -p rezidnt-run --test secret_source_composite_dispatch --test secret_source_op_resolve --test op_secret_degrade_taxonomy_fold --test op_secret_leak_discipline -- --test-threads=1`.
`REZIDNT_EGRESS_SECRETS` = host-file secrets TOML; `OP_SERVICE_ACCOUNT_TOKEN` + `REZIDNT_TEST_OP_REF` = the live-op arm.
**For WSL-only evidence, re-run it yourself — the auditor can't.** [[clippy-doc-lazy-continuation-trap]] keeps biting
doc headers (a wrapped line starting with `+`/`-`). **Sweep stray `*.rlib` from the repo root before committing.**
[[secret-source-1password-direction]] records the connector-vs-op-CLI distinction.

---
**NEXT ACTION → C3 core is COMPLETE (sandbox + inescapable egress + credential brokering via host-file AND 1Password
`op`, Linux/WSL). The one open thread is the OWNER-GATED live-op proof (crit 5): once the owner puts `op` +
`OP_SERVICE_ACCOUNT_TOKEN` (vault-scoped) + `REZIDNT_TEST_OP_REF` on the WSL box, run
`cargo test -p rezidentd --test egress_fold_op_mediated_run_c3_wire -- --test-threads=1` (honest-skips until then).
Otherwise the roadmap MAY STOP after C3 (DR-025 precedent) — next slice is the owner's steer: macOS/Windows backends,
S5 fleet board, or the benchmark harness. `current-slice` = c3-op-secrets (done). High autonomy ON. For WSL-only
evidence re-run it yourself; build dev-only example bins first; lint `#[cfg(unix)]` bodies on WSL.**
