# Handoff — 2026-07-19 (session 8/9: SP2 COMPLETE; SP3 spec ratified, build in progress)

## State of play
The permit stream went from "decides" (SP1) → "enforces mid-run" (SP2, DONE) → "any policy DSL
decides" (SP3, in build). **SP2 shipped complete** this session (socket-PDP `bb7afe3` + hook
sub-slice `5693aff`), each `/vet` + `/debrief` **pass**. **SP3 is now spec'd and ratified**:
design sketch (`830276a`), **DR-015 ACCEPTED** (owner). Currently: `/oracle` → implementer (owner
said "let the oracle finish, then hand to implementer"). Pointer = **SP3**.

## SP3 = policy-as-exec-verifier (DR-015, the spec)
Let an external policy file (OPA/Rego, Cedar, or ANY argv speaking the §8 JSON contract) decide a
permit as an **exec permit-verifier**. The machinery exists on both sides — `ExecVerifier`
(`rezidnt-gate/src/lib.rs:867`, used on vet/pre_merge) and `VerifierSpec.exec` (already parsed);
SP3 joins them on the permit axis. Ratified decisions:
1. **Un-filter + dispatch:** `permit_config_for` drops exec entries today (`mcp.rs:157`); un-filter,
   extend `PermitVerifierSpec` to carry an exec kind, dispatch through `ExecVerifier` in
   `permit::aggregate` — ordered first-`Fail`→Deny short-circuit across native+exec preserved.
2. **Async dispatch (option A):** lift permit aggregation to `decide_permit`'s async layer (natives
   stay in `spawn_blocking`, exec runs via `await`). Reject `block_on`.
3. **Determinism/replay BINDING (I6):** policy content-pinned (`policy_ref`); sealed env (network-off,
   doc §12) — a policy that fetches at decision time is NON-CONFORMING; `debrief` replays same bytes.
4. **I7 no bundled engine:** operator brings the argv; SP3's judge is a tiny reference policy program.
5. **Latency:** one-shot argv + stated ceiling (cold eval 10s–100s ms ≥ SP2's 250ms); cache DEFERRED.
6. **`/intel` skipped** (§8 contract is engine-agnostic). **No wire/ontology change, no `/subject`.**
Acceptance (sketch §8): exec policy DENIES a forced breach → `deny`; allowing policy → `allow`;
ordered short-circuit across kinds; never-coerce (nonzero/malformed/timeout → `ask`); replay-stable.

## Commits this session (`770c228..` HEAD) — check push state
SP2: `286e2e1` DR-013 · `bb7afe3` socket-PDP · `b762213` hook note · `762232a` DR-014 ·
`de70552` pep subject · `5693aff` hook sub-slice · `e6ed589` handoff (all PUSHED through e6ed589).
SP3: `830276a` sketch (pushed) · **DR-015 ACCEPTED + index + this handoff — commit + push pending.**

## Next action
**`/oracle` SP3, then hand to implementer** (owner-directed, autonomous through both). Oracle writes
failing tests for sketch §8 criteria (exec deny/allow headline via a reference policy argv;
un-filtered+dispatched; ordered short-circuit across native+exec; never-coerce on nonzero/malformed/
timeout; determinism/replay pinning; I7 no vendored binary). Then implementer: un-filter
`permit_config_for`, extend `PermitVerifierSpec` + `permit::aggregate` for exec, async-lift
`decide_permit`, add the reference policy program. Then `/vet` → `/debrief` → commit.
**Reminder: `/vet` is host-side — verify host clippy, not just WSL** ([[vet-is-host-side-wsl-insufficient]]).

## Open /debrief residuals & carried notes (non-blocking)
- SP2 fully closed (auditor pass; earlier observations all remediated).
- Decision fast-path cache (permit-engine §10.2) deferred by DR-015 — the latency answer for exec
  on the hot path; a likely future slice once measured.

## Decisions still needing a /dr (permit stream + beyond)
- **SP4 — roles + macaroon delegation** (promotes DR-005 PROVISIONAL; folds C8). Not spec'd.
- **C3 — sole-chokepoint enforcement** (OS sandbox + egress + credential brokering). DR-009 fenced;
  own design sketch + implementation DR before build.
- **Concrete OPA/Cedar adapter** — demand-gated follow-on to SP3, its own DR (+ maybe `/intel`).
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
**NEXT ACTION → `/oracle` SP3 (failing tests from sketch §8), then hand to the implementer
(un-filter resolver + exec dispatch in permit::aggregate + async-lift decide_permit + reference
policy argv), then `/vet` → `/debrief`. DR-015 §Decision is the spec.**
