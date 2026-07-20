# Handoff — 2026-07-20 (session 9: SP4b shipped end-to-end incl. DR-018 rework)

## State of play
Pointer = **SP4**. **SP4a (roles) + SP4b (macaroon-attenuated delegation) both DONE**; **SP4c
(C8 layered precedence) remains and is `/dr`-gated (owner) before build.** SP4b closed the full loop
this session — `/subject` → `/oracle` → implementer → `/vet`+`/debrief` — twice: the first debrief came
back **inconclusive** (the "delegation" was a root-key re-mint, not an offline attenuation), which
forced **DR-018** (owner-ratified A2), then a rework to a real `attenuate`. Final `/vet` **pass** +
`/debrief` **pass**. **Committed + pushed to origin/main through `e8301af`; tree clean.**

## What shipped this session (commit `e8301af`, pushed)
- **SP4b (DR-017 built + DR-018):** agent badges are first-party-caveat **macaroons over
  `blake3::keyed_hash`** (zero new dep, I7). `check_badge` flips id-equality → **crypto-verify +
  caveat-eval** on the §12 mutating-call door (operator badge = DR-005 opaque class, untouched).
  Delegation is a **real offline `base_badge.attenuate(Caveat::Role)`** at the daemon boundary
  (`runs.rs:730`), emitting **`permit.delegated {parent_badge_id, child_badge_id, added_caveats, run}`**
  — the replayable capability chain (I3), folded onto `AgentRunState.delegations`.
- **DR-018 (ACCEPTED 2026-07-20):** `Macaroon::badge_id()` derives from the **running sig**
  (`hex(blake3(sig)[..8])`, `badge.rs:317`), not the identifier — so a shared-identifier `attenuate`
  yields **distinct** parent/child edge ids while preserving the offline property. Amends DR-017 §Dec 2/4.
- **Load-bearing tests (I6):** monotonicity `verify(M+c) ⊆ verify(M)` + forgery/tamper/reorder/
  foreign-root/one-bit + proptest sig-sweep + constant-time compare (`blake3::Hash` PartialEq, verified
  `constant_time_eq_32`) + expiry-vs-passed-in-ts (no ambient `now()` in verify).

## Next action — SP4c is owner-gated; start with a /dr
SP4c (**C8 layered precedence** — admin/dev/session, stricter-wins, in `permit_config_for`) is its own
slice and needs a **`/dr`** (owner) before build. Draft the C8 precedence-model DR (scribe), get owner
ratification, then `/oracle` → implementer → `/vet`+`/debrief`. Nothing here is unblocked build work.

## Open /debrief residuals (all DR-018-tracked deferrals; none blocking)
- **Holder-offline process boundary** — SP4b ships a real `attenuate` at the *daemon* boundary; moving
  attenuation into a *lead-agent process* (no daemon round-trip) is a **named later SP4 sub-slice** (DR-018 §b).
- **Inert door-role caveat** — `check_badge` builds no `ctx.role`, so a child `Role` caveat is not
  enforced at the §12 door; role enforcement lives in the SP4a permit PDP. Documented (`lib.rs:540`), not a defect.
- **Absent-`now` replayability** — a `check_badge` call with no `args["now"]` reads wall-clock not on any
  fact, so its exact expiry eval isn't log-replayable. Documented (`lib.rs:560`), acceptable edge read.

## Decisions still needing a /dr
- **SP4c — C8 layered precedence** (next action, above).
- **Holder-offline attenuation sub-slice** (DR-018 §b) · **exec debrief-replay wiring** · **decision
  fast-path cache** (permit §10.2) · **concrete OPA/Cedar adapter** — each a focused follow-on w/ own DR.
- **C3 — sole-chokepoint enforcement** (DR-009 fenced; own sketch + DR).
- Carried pre-permit debt: DR-007 GitError→associated-type; `badge.issued` emitter / `badge_id` on other
  mutations; release items (root README, crates.io `cargo login`); Phase 3 demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY)**. Vet hook host-side (`bash .claude/hooks/vet.sh`); daemon/gate/state tests WSL. **Host
vet.sh and WSL workspace SEQUENTIAL, never concurrent** ([[vet-concurrency-flake]]). **`/vet` is
host-side; WSL-green is NOT sufficient** for platform-cfg code ([[vet-is-host-side-wsl-insufficient]]).
Host test/bin names avoid substring `update` (UAC 740, [[windows-test-binary-update-uac]]). Auto-push to
`main` classifier-gated — ask first.

---
**NEXT ACTION → SP4c is the remaining SP4 sub-slice and is owner-gated: draft a `/dr` for the C8
layered-precedence model (admin/dev/session, stricter-wins, `permit_config_for`) → owner ratify →
`/oracle` → implementer → `/vet`+`/debrief`. No unblocked build work; SP4a+SP4b are DONE and pushed (`e8301af`).**
