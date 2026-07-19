# Handoff — 2026-07-19 (session 8/9: SP1→SP2→SP3 all COMPLETE — permit engine feature-complete)

## State of play
The permit engine is now **feature-complete across its core arc**: SP1 *decides* → SP2 *enforces
mid-run* → **SP3 *any policy DSL decides*** (all done this session, every `/vet` + `/debrief`
**pass**). SP3 shipped: design sketch (`830276a`) → DR-015 ACCEPTED (`f77cc07`) → oracle → implementer
→ vet pass → debrief **pass** (first try) → resolver-coverage gap closed → **committed `f07b86b`**.
Pointer = **SP3 (done)**. **`main` ahead 1 of origin** (`f07b86b`) — auto-push classifier-gated,
**ask before pushing.**

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

## Next action — permit core done; choose direction (owner priority)
The permit "may" axis (SP1–SP3) is feature-complete. Remaining permit-stream + roadmap options:
- **SP4 — roles + macaroon-attenuated delegation** (promotes DR-005 PROVISIONAL; folds in C8
  layered admin/dev/session precedence). Not spec'd — fresh design→/dr→oracle arc.
- **Exec debrief-replay wiring** — the one SP3 `#[ignore]` deferral: `rezidnt_gate::replay` reports
  exec verifiers as `replayed: None` (v1); threading `policy_ref` back to re-execute recorded §8
  stdin + raise a DR-006 integrity alarm on divergence. A focused follow-on (its own oracle pass).
- **Decision fast-path cache** (permit-engine §10.2) — the latency answer for exec-on-hot-path,
  deferred by DR-015; a likely slice once measured.
- **Concrete OPA/Cedar adapter** — demand-gated follow-on to SP3, its own DR (+ maybe `/intel`).
- **C3 — sole-chokepoint enforcement** (OS sandbox + egress + credential brokering). DR-009 fenced;
  own design sketch + implementation DR before build.
- **Carried debt / cleanup** instead of a new slice (see below).
Each new slice is oracle-first after its spec + DR. Advance the pointer once the next slice is chosen.

## Open /debrief residuals & carried notes (non-blocking)
- SP1–SP3 all auditor **pass**; SP3 residual coverage gap (resolver un-filter) was CLOSED before commit.
- Exec debrief-replay is the one honest SP3 deferral (see Next action) — `#[ignore]` panics, not faked.

## Decisions still needing a /dr (permit stream + beyond)
- **SP4 / C3 / concrete OPA-Cedar adapter / exec-replay** — each its own spec + DR before build.
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
**NEXT ACTION → SP3 COMPLETE (committed `f07b86b`, auditor pass). Permit core (SP1–SP3)
feature-complete. Choose the next direction with the owner — SP4 (roles+delegation), exec
debrief-replay wiring, the decision cache, a concrete OPA/Cedar adapter, C3 (fenced), or carried
cleanup — then run its design→/dr→/oracle arc. ALSO PENDING: owner ok to push (`main` ahead 1).**
