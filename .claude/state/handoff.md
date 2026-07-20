# Handoff — 2026-07-19 (session 8/9: SP1–SP3 + SP4a COMPLETE; SP4b/SP4c remain)

## State of play
Permit core (SP1 *decides* → SP2 *enforces mid-run* → SP3 *any DSL decides*) is **feature-complete**,
and **SP4a (roles) is DONE** — every slice this session passed `/vet` + `/debrief`. SP4 was sliced by
**DR-016 ACCEPTED** into SP4a roles → SP4b macaroon delegation → SP4c C8 precedence; **SP4a shipped**:
warden `/subject` (`role?` field `961997c`) → oracle → implementer → vet pass → debrief **pass**
(first try) → **committed `381854a`**. Pointer = **SP4** (SP4a done; SP4b/SP4c remain).
**`main` ahead 2 of origin** (`961997c`, `381854a`) — auto-push classifier-gated, **ask before pushing.**

## What SP3 shipped (committed `f07b86b`, green host+WSL)
An external policy (OPA/Rego, Cedar, or ANY argv speaking the §8 JSON contract) decides a permit as
an exec permit-verifier — reusing the existing `ExecVerifier`, no new machinery/vocabulary, no
bundled engine (I7).
- `PermitVerifierSpec` gains a `kind` (Native | Exec{argv}) + `::native`/`::exec`/`kind()`.
- New async `permit::aggregate_async` — natives sync (extracted `native_verdict`, keeps `dyn
  NativeVerifier` off the Send future), exec via `ExecVerifier::run().await`; ordered first-`Fail`→
  Deny short-circuit across kinds preserved; sync `aggregate` kept for native tests.
- `decide_permit` lifts aggregation to the async layer (no `block_on`).
- `permit_config_for` un-filtered — exec entries dispatched, not dropped; two resolver tests walk
  the real `begin_open`→resolver seam.
- Determinism BINDING (I6): exec sealed (network-off + scrubbed), policy pinned as `policy_ref`.
  Never-coerce: nonzero/malformed/timeout → `ask`. Reference policies = local shell argv.

## Commits this session (`770c228..f07b86b`) — push state
SP2 (all pushed ≤`e6ed589`): DR-013/socket-PDP/hook-note/DR-014/pep-subject/hook-sub-slice.
SP3: `830276a` sketch · `f77cc07` DR-015 (both pushed) · **`f07b86b` SP3 slice — NOT pushed (ahead 1).**

## Next action — SP4a done; choose direction (owner priority)
SP4a (roles) shipped. Remaining SP4 sub-slices + roadmap options:
- **SP4b — macaroon-attenuated delegation.** Needs its OWN DR first (DR-016 §Dec 3 recorded only the
  direction): evaluate a permissive Rust macaroon crate vs hand-roll (approved-dep set + I7),
  badge→macaroon migration, `permit.delegated`-vs-`agent.spawned`-field (`/subject`), and the
  monotonicity property `verify(M+c) ⊆ verify(M)` (a widening bug = privilege escalation). The crypto
  slice — the biggest remaining permit work. Sequence: crate-eval → design → /dr → /subject → oracle.
- **SP4c — C8 layered precedence** (admin/dev/session, stricter-wins) in the `permit_config_for`
  resolution seam. Policy logic, no crypto. Its own design→/dr→oracle.
- **Exec debrief-replay wiring** (SP3 `#[ignore]` deferral) · **decision fast-path cache**
  (permit-engine §10.2) · **concrete OPA/Cedar adapter** — each a focused follow-on with its own DR.
- **C3 — sole-chokepoint enforcement** (DR-009 fenced; own sketch + DR).
- **Carried cleanup** instead of a new slice.
**Reminder: `/vet` is host-side** ([[vet-is-host-side-wsl-insufficient]]).

## Open /debrief residuals & carried notes (non-blocking)
- SP1–SP3 + SP4a all auditor **pass**; SP3 resolver-un-filter gap CLOSED before its commit.
- Exec debrief-replay is the one honest SP3 deferral (see Next action) — `#[ignore]` panics, not faked.
- **SP4a reference-policy nit (auditor note, non-blocking):** `spec/fixtures/policies/permit_role_policy.sh`
  matches on the SUBSTRING `"role":"reviewer"` in the serialized VerifierInput rather than the
  structured `params.role` — robust for SP4a's fixed inputs, but prefer keying on the structured field
  if that policy ever grows. Test-fixture only, not production code.

## Decisions still needing a /dr (permit stream + beyond)
- **SP4b (macaroon delegation)** — its OWN DR: permissive macaroon crate vs hand-roll (approved-dep
  set + I7), badge→macaroon migration, `permit.delegated`-vs-`agent.spawned`-field (`/subject`),
  monotonicity property. DR-016 §Decision 3 records the direction; the concrete choice is SP4b's DR.
- **SP4c (C8 layered precedence)** — its own slice (admin/dev/session, stricter-wins).
- **C3 / concrete OPA-Cedar adapter / exec-replay wiring** — each its own spec + DR before build.
- Any memo-001-motivated change needs its own DR (DR-002 rule 3).
- Pre-permit carried debt: DR-007 GitError→associated-type; `badge.issued` emitter / `badge_id` on
  other mutations; release items (root README, crates.io `cargo login`); Phase 3 demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). **`/vet` is host-side; WSL-green is NOT sufficient for platform-cfg code**
([[vet-is-host-side-wsl-insufficient]]). Host test/bin names must avoid substring `update` (UAC os
error 740, [[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask first.

---
**NEXT ACTION → SP4a COMPLETE (committed `381854a`, auditor pass). Choose the next direction with
the owner — SP4b (macaroon delegation; crate-eval → own DR → crypto slice), SP4c (C8 layered
precedence), exec debrief-replay, the decision cache, an OPA/Cedar adapter, C3 (fenced), or carried
cleanup. SP4b is the biggest remaining permit work and needs its own DR before build.
ALSO PENDING: owner ok to push (`main` ahead 2).**
