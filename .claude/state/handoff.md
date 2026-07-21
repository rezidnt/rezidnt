# Handoff — 2026-07-21 (session 14: C3 sole-chokepoint — C3a shipped, C3b+c folded→split, c3bc-decide landed)

## State of play
Continuing the **C3 sole-chokepoint** arc (owner picked it over pulling Platform forward; Phase 2 already
exited). This session shipped **C3a (Linux sandbox)** end-to-end, then took **C3b+c (egress + credential
brokering)** — which the owner chose to build as **full L7 MITM in one folded slice** (DR-026). The
implementation pass revealed the enforcement **dataplane can't land + verify in one go**; owner re-scoped
(DR-027) to **decide-then-enforce**, and **c3bc-decide** (the egress decision/governance layer, explicitly
ENFORCEMENT-INERT) landed clean. All pushed to `origin/main` at **`5c8be98`**. Tree clean, synced.
`current-slice` = `c3bc-decide` (done). **Next slice = `c3bc-enforce`** (the real dataplane). High autonomy ON
([[autonomy-high-trust]]).

## What shipped this session (all pushed)
- **`924c86b` — C3a Linux OS-sandbox (DR-025).** `bwrap`-backed `SandboxSubstrate` (I4) wrapping the S1 spawn
  seam; `PathConfinement` verifier; folded-policy binds (private field, no-widening in the type system); loud
  `sandbox.unavailable` degrade; zero new linked dep. /vet host PASS, WSL bwrap 3/3, /debrief PASS.
- **`fa84f1a` — DR-026 (C3b+c full L7 MITM, owner-ratified).** Folded egress proxy + credential brokering:
  TLS-terminating `EgressProxy`, `pasta` netns inescapability, rezidnt CA (rustls/rcgen), secrets injected
  upstream/never-held/logged-by-ref. First C3 linked deps. Threat model + I7 dep delta owned honestly.
- **`5c8be98` — DR-027 split + c3bc-decide landed.** The impl pass built the decision/type/CA-scaffold layer
  green (18/18 host) but NOT the enforcement dataplane. Owner split: **decide** (landed, inert) + **enforce**
  (next). c3bc-decide: `EgressScope` verifier + `EgressPolicy` (allowlist AND injection_map private,
  folded-only), redacted `BrokeredSecret` (value private, `.expose()` sole accessor, zero non-test
  call-sites), degrade-CLOSED, rustls+rcgen dep-scan. /vet host PASS, /debrief PASS.

## The honesty discipline earned its keep HARD this session
1. **C3a criterion-1 fixture under-bound the toolchain** → implementer flagged (didn't patch oracle's
   artifact) → oracle repaired + caught its OWN criterion-2 vacuity (denial passing because the shell never
   execed). Rule: **a sandbox confinement fixture must bind the toolchain, and a denial test must prove the
   shell RAN (inside-bind sentinel) before asserting the escape was blocked.**
2. **C3b+c folded scope was too big to land the dataplane.** Implementer honestly reported `inject_and_proxy`
   returns only `secret_ref` — no byte-path, no real injection; crit 3 (inescapability) + crit 4 (real
   capture) are `unimplemented!()`/`#[ignore]`'d, exit demo unachievable. Rather than declare a green
   checkmark on a half-built chokepoint, the gap was surfaced → owner re-scoped (DR-027). Auditor confirmed
   the split is **honest, not a relabel**: crit 3/4 genuinely moved to c3bc-enforce, NO host test vacuously
   covers inescapability/real-injection. Rule: **a decision layer landed as done must be labeled
   enforcement-INERT and MUST NOT be wired into a live run loop as if it enforced** (DR-022 no-half-measuring
   -stick discipline; DR-027 honesty guard).
3. Reusable type-safety patterns now proven twice: **no-widening = private field + folded-only constructor**
   (C3a `binds`; C3b+c `allowlist` AND `injection_map`); **secret-never-in-log = a redacted newtype whose
   value is reachable only via one `.expose()` call-site** (zero in production code).

## Autonomy — [[autonomy-high-trust]] (owner granted 2026-07-20)
Proceed WITHOUT asking: full loop, commit+push green+debrief-PASS increments, draft+self-ratify routine
engineering DRs. **This session TWO owner sign-offs were required and given** (DR-025 C3a, DR-026 C3b+c — each
a sole-chokepoint posture commitment, fenced by DR-009, NOT self-ratifiable). **Direction/scope forks got
checkpoints:** Platform-vs-core (owner→core/C3), C3a-first (owner→Linux), C3b+c TLS scope (owner→full MITM),
and the dataplane gap (owner→re-scope decide/enforce). DR-027 was self-ratified as it RECORDS the owner's
just-made re-scope decision. **Still surface:** irreversible/destructive git, a DR amending BINDING invariant
TEXT or licensing/clean-room, firewalled sources, publishing beyond a main push, one-way doors, and each C3
posture commitment / new linked-dep slice.

## Next action — `c3bc-enforce` (the real egress dataplane), or a lighter item
**c3bc-enforce** carries DR-026's crit 3/4 + the full-MITM exit demo. Its oracle is HALF-WRITTEN: the
`#[cfg(unix)]` `#[ignore]`'d `crates/rezidnt-run/tests/egress_mediation_c3bc.rs` has `unimplemented!()` bodies
for inescapability + real-injection. The build:
- A live `pasta`-in-netns dataplane with **no default route** — the only outbound is the rezidnt proxy
  (inescapability, crit 3). `passt` installs on WSL (`sudo apt-get install -y passt` → `/usr/bin/pasta`);
  unprivileged user+net namespaces create on the WSL box (proven this session).
- A `rustls` terminating listener + upstream TLS + **real credential injection** into the upstream request
  (crit 4) — the ONE `.expose()` call-site lives here (radioactive: never near a fact/log/trace).
- Fill in `egress_mediation_c3bc.rs`'s `unimplemented!()` bodies (a confined probe binary, agent-request
  capture) + remove `#[ignore]` where the WSL box supports it; the exit demo (agent clones from allowlisted
  GitHub over the broker with an unheld injected token) becomes achievable.
- This is a big subsystem — likely its own oracle→impl→/vet→/debrief, possibly multi-session. It rides
  DR-026's already-ratified design (no new DR needed; DR-027 sequenced it).
Alternatives (lighter, if enforce is too heavy to start): the `egress.*`/`credential.*` warden `/subject`
(mint the taxonomy for C3a's + c3bc's placeholder facts) · the `sandbox.*` warden `/subject` (C3a's deferred
facts) · smaller carried /dr items (bench.completed, holder-offline, fast-path cache, OPA/Cedar).

## Open /debrief residuals — NONE blocking
c3bc-decide auditor PASS. One non-blocking convention finding (stale oracle-era `todo!()`/RED-MODE comments)
was fixed this session (refreshed to GREEN). No carried residue.

## Platform deferral (unchanged, still coherent with C3)
Windows named-pipe transport is Phase-3, owner-deferred. Coupled to C3 at one point: full Windows egress +
sandbox parity waits on the native-Windows daemon (Job Objects/AppContainer). C3 ships Linux-first, degrades
loudly/closed on Windows. No reorder implied.

## Environment (+ C3 additions)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. **Quote
the PATH export** ([[wsl-dev-environment]]). Vet host-side (`bash .claude/hooks/vet.sh`); **host + WSL
SEQUENTIAL** ([[vet-concurrency-flake]]). **`/vet` host-side; WSL-green NOT sufficient**
([[vet-is-host-side-wsl-insufficient]]) — `#[cfg(unix)]` suites (`sandbox_bwrap_confinement_c3a.rs`,
`egress_mediation_c3bc.rs`, daemon golden_path, bench real_driver, permit_role_live) run WSL-only; host → 0
tests. **WSL tooling present:** `bwrap` 0.9.0 at `/usr/bin/bwrap`; `passt`/`pasta` at `/usr/bin/pasta`;
unprivileged user+net namespaces work. **C3a sandbox-fixture gotcha:** bind the toolchain (`/usr,/bin,/lib,
/lib64` ro) or `/bin/sh` won't exec on usr-merged Ubuntu. **clippy::doc_lazy_continuation** still bites
doc/test headers ([[clippy-doc-lazy-continuation-trap]]) — hit repeatedly, watch it. **New deps this session:**
`rustls` + `rcgen` (C3b+c MITM, DR-026 — the first C3 linked deps). New src: `crates/rezidnt-run/src/
sandbox.rs` (C3a), `crates/rezidnt-run/src/egress.rs` (c3bc-decide, ENFORCEMENT-INERT).

## Decisions still needing a /dr or /subject
- **c3bc-enforce** (rides DR-026, no new DR — its own oracle→impl loop) · `egress.*`/`credential.*` warden
  /subject · `sandbox.*` warden /subject · macOS/Windows egress+sandbox backends (each own DR) · smaller
  carried: bench.completed subject, holder-offline attenuation (DR-018 §b), decision fast-path cache,
  OPA/Cedar adapter. Carried debt: DR-007 GitError→associated type; `badge.issued` emitter; release items.
  **Platform / Windows transport** = Phase-3 direction fork, demand-gated.

---
**NEXT ACTION → c3bc-decide landed (`5c8be98`); the egress GOVERNANCE layer is done + inert. Next is
`c3bc-enforce` — the real `pasta`-netns proxy + live TLS byte-path + real credential injection (DR-026 crit
3/4 + exit demo), a big subsystem riding DR-026's ratified design (oracle half-written in
`egress_mediation_c3bc.rs`). `current-slice`=c3bc-decide (done). High autonomy ON ([[autonomy-high-trust]]):
oracle→impl→/vet→/debrief; surface only irreversible/constitution-level/outward-facing calls. The
enforcement-inert substrate MUST NOT be wired live until c3bc-enforce proves the dataplane (DR-027 guard).**
