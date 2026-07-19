# Handoff — 2026-07-19 (session 8/9: SP1–SP3 COMPLETE; SP4 sliced, SP4a roles in build)

## State of play
Permit core (SP1 *decides* → SP2 *enforces mid-run* → SP3 *any DSL decides*) is **feature-complete**
(all done this session, every `/vet` + `/debrief` **pass**, pushed ≤`2d3d239`). **SP4 is now sliced
and ratified:** design sketch (`5a042dd`) → **DR-016 ACCEPTED** (owner) splits SP4 into SP4a roles →
SP4b macaroon delegation → SP4c C8 precedence. **SP4a (roles) is in build** (owner said "ratify it,
then start SP4a"): warden `/subject` → `/oracle` → implementer. Pointer = **SP4** (SP4a immediate).

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

## Next action — build SP4a (roles), DR-016 §Decision 2 is the spec
Sequence (owner-directed, autonomous through the loop):
  1. Warden **`/subject`** — `role` on `agent.spawned` (additive field, mirroring `pep`/`bare`;
     no new subject; drift-guard stays green since subject list is unchanged).
  2. **`/oracle`** — failing tests: `role: Option<String>` on `AgentSpec` parses; recorded on
     `agent.spawned`; folded to `AgentRunState`; injected into `decide_permit` per-run params; a
     role-keyed policy (native or exec reference) decides a permit DIFFERENTLY by role (the headline).
  3. **Implementer:** add `role` to `AgentSpec` + emit on `agent.spawned` + fold + inject into
     `decide_permit`'s content-pinned params (DR-011 §2 discipline) as a new input axis.
  4. **`/vet`** → **`/debrief`** → commit.
Then SP4b/SP4c are separate later slices (SP4b needs its OWN DR — macaroon crate/dep choice +
badge migration + monotonicity property; SP4c = C8 layered precedence). **Reminder: `/vet` is
host-side** ([[vet-is-host-side-wsl-insufficient]]).

## Open /debrief residuals & carried notes (non-blocking)
- SP1–SP3 all auditor **pass**; SP3 residual coverage gap (resolver un-filter) was CLOSED before commit.
- Exec debrief-replay is the one honest SP3 deferral (see Next action) — `#[ignore]` panics, not faked.

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
**NEXT ACTION → Build SP4a (roles), DR-016 §Decision 2. Warden `/subject` (role on `agent.spawned`)
→ `/oracle` (role parses + folds + injects; a role-keyed policy decides differently by role) →
implementer (`role` on `AgentSpec` + emit + fold + inject into `decide_permit` params) → `/vet` →
`/debrief`. SP4b (macaroon, own DR) + SP4c (C8) are later slices.**
