# Handoff — 2026-07-19 (session 8/9: SP2+SP3+SP4a shipped; SP4b DR ratified, build pending)

## State of play
Marathon session. The permit "may" axis went **decides (SP1) → enforces mid-run (SP2) → any DSL
decides (SP3) → role-keyed (SP4a)**, and the **crypto delegation slice (SP4b) is spec'd + ratified,
not yet built.** Four slices closed this session (SP2 socket-PDP, SP2 hook, SP3, SP4a), each `/vet`
+ `/debrief` **pass**; five DRs ratified (DR-013..017). **All pushed to origin/main** through
`f7c1376`; tree clean. Pointer = **SP4** (SP4a done; **SP4b is the next build**).

## What shipped this session (all committed + pushed)
- **SP2 (DR-013/014):** the PEP enforces mid-run — `decide_permit` (one PDP path, byte-identical
  facts), socket un-stubbed, `rezidnt permit-hook` CLI subcommand (fail-closed→ask, 250ms), socket
  `paths` wire, `agent.spawned.pep` + `gate_explain` enforcement visibility.
- **SP3 (DR-015):** permit axis dispatches **exec verifiers** — any argv/OPA/Cedar policy decides via
  `aggregate_async`; determinism-pinned; no vendored engine (I7); reference policies are local sh.
- **SP4a (DR-016):** **roles** — `AgentSpec.role` → `agent.spawned.role` → `AgentRunState` → injected
  into `decide_permit` params; a role-keyed policy decides differently by role.
- **SP4b SPEC (DR-017 ACCEPTED):** macaroon-attenuated delegation, ready to build — see Next action.

## SP4b is READY TO BUILD (DR-017 §Decision is the spec)
Crate eval done: **hand-roll a first-party-caveat macaroon over the already-vendored
`blake3::keyed_hash` MAC — ZERO new dependency (I7)** (rejected: stale `macaroon` crate, over-fit
`biscuit-auth`). Design sketch `docs/design/permit-macaroon-delegation-sp4b.md`. Ratified:
- Construction: process-lifetime root key (`rand`); caveats = workspace/verb/expiry/role predicates;
  mint at spawn under `REZIDNT_BADGE`; attenuate = append caveat + re-key sig (offline, no root key);
  verify in `check_badge` = recompute chain + constant-time compare + eval caveats. First-party only.
- **Monotonicity is BINDING (I6): `verify(M+c) ⊆ verify(M)`; widening = privilege escalation** —
  the most-tested surface (property test + forgery/tamper/reorder rejection + constant-time compare).
- Agent badges → macaroons; `check_badge` flips id-equality → crypto-verify on the §12 door;
  operator badge stays the DR-005 opaque class. Expiry as a caveat vs a passed-in timestamp (no
  ambient `now()`, replayable).
- **New `permit.delegated {parent_badge_id, child_badge_id, added_caveats, run}` subject + reducer.**

## Next action — build SP4b (owner-directed slice; owner-only gates already cleared)
Sequence: (1) warden **`/subject`** — mint `permit.delegated` (a real event in time → its own
subject + folding reducer; NOT a field). (2) **`/oracle`** — property tests FIRST: monotonicity
(`verify(M+c) ⊆ verify(M)`), forgery/tamper/reorder rejection, constant-time verify, mint/attenuate/
verify round-trip, expiry-as-caveat against a passed-in ts, `check_badge` crypto-verify on a
mutating call, delegation logged. (3) Implementer: the ~80-line macaroon over `blake3::keyed_hash`
in `rezidnt-run` (`badge.rs`), `check_badge` verify, `SpawnPlan` mint/inject, `permit.delegated`
emit + reducer. (4) `/vet` → `/debrief` → commit. **`/vet` is host-side** ([[vet-is-host-side-wsl-insufficient]]).

## Open /debrief residuals & carried notes (non-blocking)
- SP2–SP4a all auditor **pass**; no open findings.
- SP4a reference policy `permit_role_policy.sh` matches SUBSTRING `"role":"reviewer"` not structured
  `params.role` — fine for fixed inputs; tighten if it grows (test-fixture only).
- SP3 exec debrief-replay is a named `#[ignore]` deferral (`replay()` reports exec as `replayed:None`).

## Decisions still needing a /dr (permit stream + beyond)
- **SP4c — C8 layered precedence** (admin/dev/session, stricter-wins, in `permit_config_for`) — own slice.
- **Exec debrief-replay wiring** · **decision fast-path cache** (permit §10.2) · **concrete OPA/Cedar
  adapter** — each a focused follow-on with its own DR.
- **C3 — sole-chokepoint enforcement** (DR-009 fenced; own sketch + DR).
- Pre-permit carried debt: DR-007 GitError→associated-type; `badge.issued` emitter / `badge_id` on
  other mutations; release items (root README, crates.io `cargo login`); Phase 3 demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY)**. Vet hook host-side (`bash .claude/hooks/vet.sh`); daemon/gate tests WSL. **Host vet.sh
and WSL workspace SEQUENTIAL, never concurrent** ([[vet-concurrency-flake]]). **`/vet` is host-side;
WSL-green is NOT sufficient for platform-cfg code** ([[vet-is-host-side-wsl-insufficient]]). Host
test/bin names avoid substring `update` (UAC 740, [[windows-test-binary-update-uac]]). Auto-push to
`main` classifier-gated — ask first.

---
**NEXT ACTION → Build SP4b (macaroon delegation, DR-017 ACCEPTED). Warden `/subject`
(`permit.delegated`) → `/oracle` (monotonicity `verify(M+c) ⊆ verify(M)` + forgery/tamper rejection
FIRST — this is the security-critical slice) → implementer (hand-roll macaroon over
`blake3::keyed_hash` in `badge.rs`, `check_badge` crypto-verify, `SpawnPlan` mint/inject,
`permit.delegated` emit+reducer) → `/vet` → `/debrief`. No owner gate outstanding.**
