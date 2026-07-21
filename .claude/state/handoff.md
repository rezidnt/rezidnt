# Handoff — 2026-07-21 (session 17: c3-egress-fold DONE — live run actually mediates + brokers; next = 1Password op SecretSource backend)

## State of play
**`c3-egress-fold` is DONE.** A real `rezidnt open` governed run now ACTUALLY mediates egress and brokers a
token the agent never holds — **DR-026 crit 4 at the run-loop level**, not just the substrate suite. This
activated the Mediated arm that DR-028's honestly-minimal empty deny-all fold left dead. DR-029 ratified,
warden `/subject` minted the real fact taxonomy, implementation landed + audited. Host `/vet` PASS, WSL
live-mediated suite 2/2 (re-run by me), `/debrief` PASS (no blocking defects). High autonomy ON
([[autonomy-high-trust]]). `current-slice` = `c3-egress-fold` (**done**). NOT yet pushed at time of writing —
push is the last step of this handoff.

## Current slice & criteria
`c3-egress-fold` — DONE. DR-029's five criteria: (1) `[egress]` block parses, absent⇒deny-all, malformed⇒honest
error; (2) `SecretSource` host-file backend resolves, absent⇒empty, missing/malformed⇒honest error,
unresolvable⇒drop with a loud `credential.dropped` fact (never a fake/empty secret); (3) fold folded-only,
C6/DR-024 preserved end-to-end (`from_folded_authority` sole door). All host-provable → GREEN (26 host tests).
(4) live mediated run reaches the run-loop Mediated arm end-to-end (`egress.mediated`/`egress_enforceable=true`,
`credential.injected` by-ref, **token value in NO log fact**) → WSL `#[cfg(unix)]` `egress_fold_mediated_run_c3_wire`
2/2. (5) facts ride the real minted subjects (paired warden `/subject`) → done. Enforce 4/4, composed +
spawn_composed + open_flow green (no regressions). `/debrief` = PASS.

## What changed this session (git log since c3-wire handoff `2e43ef3`)
- `7737b8e` **DR-029** (c3-egress-fold): `[egress]` block + `SecretSource` I4 seam; host-file MVP now, **1Password
  `op`-CLI backend fenced as the next backend**. Rides DR-026 posture + DR-020 host-authority-file precedent.
- `f2c7fc9` **/subject mint**: 5 real subjects — `egress.mediated`/`egress.unavailable`/`egress.denied`/
  `credential.injected`/`credential.dropped` (sandbox posture folded INTO `egress.*` as a field; `credential.*`
  its own noun, facts structurally value-free). 4 folding reducers named (DR-006), wired by the impl.
- `a9e443d` **c3-egress-fold impl**: NEW `crates/rezidnt-run/src/secret.rs` (`SecretSource` + `HostFileSecretSource`
  reading `REZIDNT_EGRESS_SECRETS`); `spec.rs` (`EgressSpec` on `ProjectSpec`); `egress.rs` (`fold_egress_policy`,
  `CredentialDrop`, `denied_fact`/`dropped_fact`, `injected_refs`); `compose.rs` (placeholder→real-subject swap);
  `rezidnt-state` (4 `AgentRunState` fields + 5 apply arms); `runs.rs` (`fold_c3_policies` real fold, Mediated arm
  activated, `credential.injected`/`credential.dropped` emission); testkit helpers. No new linked dep. 6 new test
  files (1 WSL-only).

## Secret hygiene (the load-bearing property this slice adds — auditor-confirmed)
`BrokeredSecret` has NO `Serialize` + redacting `Debug`/`Display`, so a secret value is STRUCTURALLY
unserializable onto a fact/CAS/trace. The single `.expose()` stays on the upstream-write path (`egress.rs:1669`);
`credential.*`/`egress.*` facts carry ONLY `secret_ref` (label) + `dest` + `policy_ref`. Values live host-side
only (`REZIDNT_EGRESS_SECRETS`, env-pointed OUTSIDE any workspace spec — a dev can't self-grant). The honesty
guard is DISCHARGED for the mediated path: product copy MAY now claim Linux/WSL egress mediation + credential
brokering for a shipped run with a configured `[egress]` allowlist + resolvable secret.

## Next action — the 1Password `op`-CLI SecretSource backend
The owner directed (2026-07-21) 1Password as the secret-management direction ([[secret-source-1password-direction]]);
the host-file MVP shipped this slice behind the `SecretSource` seam precisely so `op` slots in as a drop-in.
Build an **`OpSecretSource`** backend behind the same trait: `op read op://vault/item/field` **exec'd not linked**
(I7-clean — the pasta/bwrap/git pattern; an MCP-based backend is the heavier alternative). Its own light DR
(DR-030; the `SecretSource` seam + posture are already ratified — the DR is the `op` invocation shape, the
`secret_ref → op://` mapping, availability/degrade when `op` absent or not signed in → honest error not fake
secret), then oracle→impl→/vet→/debrief. Pairs with deciding how `secret_ref` names an `op://` reference (a new
`[egress.secrets]` value grammar, or a side map).

## Open /debrief findings (NON-BLOCKING, carried — none blocks done)
1. **No-widening test is a compile-time interface pin, not fail-first-on-ADDED-door** (`egress_fold_no_widening_fold.rs:232-257`).
   DR-029 crit-3's "fails-FIRST if a SpawnPlan door is added" is slightly overstated — the private-field guard is
   the real (and holding) mechanism, but a newly-added widening ctor wouldn't trip the test. Fix = a `trybuild`
   compile-fail fixture, or narrow the test's claim/comment.
2. **Stale `sandbox.unavailable` doc-strings in `sandbox.rs`** (lines ~152/160/204/402) — historical C3a-alone
   posture; the composed path rides `egress.*` only. Purely cosmetic; could mislead a future reader.

## Decisions still needing a /dr or /subject
- **1Password `op` SecretSource backend** (the next action — DR-030 light DR) · macOS/Windows sandbox+egress
  backends (each own DR; Windows coupled to the deferred Platform phase) · the richer role-layer/`[gates.permit]`-
  precedence egress fold (DR-019/020 style) only if demanded (this slice's `[egress]` is project-level) · smaller
  carried: bench.completed, holder-offline (DR-018 §b), fast-path cache, OPA/Cedar.

## Environment (essentials)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`, **quote the
PATH export** ([[wsl-dev-environment]]). Vet host-side; **host+WSL SEQUENTIAL** ([[vet-concurrency-flake]]);
**WSL-green NOT sufficient, /vet is host-side** ([[vet-is-host-side-wsl-insufficient]]) — BUT note the inverse this
session: `#[cfg(unix)]` test BODIES compile to zero tests on host, so host `/vet` does NOT lint them; **run WSL
`cargo clippy -p <pkg> --test <name> -- -D warnings` on any new `#[cfg(unix)]` suite** (host clippy can't reach the
unix body — bit us on the mediated suite's doc header). The mediated suite:
`cargo test -p rezidentd --test egress_fold_mediated_run_c3_wire -- --test-threads=1` (needs pasta + bwrap + netns,
all present; **build the dev probe example first**: `cargo build -p rezidnt-run --example egress_c3bc_probe`).
`REZIDNT_EGRESS_SECRETS` = host TOML `secret_ref = "value"` (the secret-source env). **For WSL-only evidence, re-run
it yourself — the auditor can't.** [[clippy-doc-lazy-continuation-trap]] bit a WSL test doc header again (a wrapped
line starting with `+`). **Sweep stray `*.rlib` probe artifacts from the repo root before committing** (agents leave
them). [[secret-source-1password-direction]] guides the next slice.

---
**NEXT ACTION → the 1Password `op`-CLI SecretSource backend: build an `OpSecretSource` behind the ratified
`SecretSource` seam (`op read op://…` exec'd not linked, I7), degrading honestly when `op` is absent/not-signed-in
(never a fake secret). Draft DR-030 (light — the seam+posture are ratified; the DR is the `op` invocation + the
`secret_ref → op://` mapping + degrade), then oracle→impl→/vet→/debrief. `current-slice` = c3-egress-fold (done).
High autonomy ON. For WSL-only evidence, re-run it yourself; lint `#[cfg(unix)]` bodies on WSL.**
