# Handoff — 2026-07-20 (session 11: SP4c core+wire, SP5, S5b, DR-021+cost_ms shipped)

## State of play
Pointer = **cost_ms** (now DONE). This session shipped a long run end-to-end and pushed all through
**`dcf5c23`**: DR-019 (SP4c core), DR-020 (SP4c-wire), SP5 close-out, S5b (fleet-board permit column),
**DR-021 (live spend-cap C1, ratified)**, and the **cost_ms slice**. The permit-engine SP0–SP5 arc is
COMPLETE and surfaced on the board; decision-latency is now recorded on every permit fact. Tree clean,
synced to origin/main. `current-slice` reads cost_ms — **advancing past it is an owner action**; the
natural next is the **C1 build** that DR-021 just ratified (see next action).

## What shipped this session (pushed)
- **`4b97d4f`/`2bbaddb`/`a6b2c44`** SP4c core+wire (DR-019/DR-020) · **`7d6a833`** SP5 close-out ·
  **`4f08964`** S5b fleet-board permit column. (Details in prior handoff sections / DRs.)
- **`b05d495` DR-021 (ACCEPTED)** — live spend-cap (C1). Ratified **B2**: a permit decision folds NO
  spend; measured actuals ride a **post-action metering fact**; C1 `SpendCap` (verifier exists, currently
  inert) enforces on a lagging-but-truthful cumulative. `spend_delta_usd` retires from the `permit.*`
  reducer fold source. **No producer seam needed (C=iii)** — `VerifierOutput`/`PermitOutcome` unchanged,
  no §8 exec-verifier contract change. Lagging enforcement disclosed (blocks the action AFTER the one that
  crossed). **cost_ms is separate** (shipped this session); **C6 risk fenced to its own later DR**.
- **`dcf5c23` cost_ms slice** — `decide_permit` times the `aggregate_async` span (policy latency only,
  NOT CAS/publish I/O) and emits `DecisionDeltas.cost_ms` on every `permit.granted/.denied/.escalated`.
  Recorded-only (folds into no accumulator; reducer regression-locks pin it). `/vet` pass, `/debrief` PASS.

## Next action — the DR-021 C1 build (owner-ratified direction; needs a /subject first)
DR-021 ratified the DIRECTION; building live C1 spend enforcement is the follow-on, in order:
1. **Warden `/subject`: `action.metered`** (or per-action attribution off `agent.completed`) — the
   post-action fact carrying MEASURED spend ($ + tokens) at per-action grain. `agent.completed` already
   carries `cost.total_usd`/tokens but only as a per-RUN terminal total (too coarse) — the per-action grain
   is the new thing. This is warden-gated; design the subject + its reducer arm there.
2. **Move the spend fold source:** `rezidnt-state/src/lib.rs:725-726` (`spend_delta_usd`→`cumulative_spend_usd`)
   moves from the `permit.*` reducer to the new metering fact's reducer. `spend_delta_usd?` retires from
   `permit.granted`/`.denied` payload (ontology `:364,370`) — part of the same /subject session.
3. **Wire caps + make SpendCap live:** inject `soft_cap_usd`/`hard_cap_usd`/`rate_limit`/`window_action_count`
   from `[gates.permit]` config into permit params (mirror the `role` injection at `rezidnt-mcp/src/lib.rs:828`),
   so `SpendCap` (`rezidnt-gate/src/lib.rs:680`) runs instead of cannot-run.
   Then `/oracle` (DR-021 §Acceptance-criteria sketch: soft→ask, hard→deny, rate-limit→deny, denied action
   folds ZERO spend, metering-fact folds spend not the permit fact) → implementer → `/vet` → `/debrief`.

## Other candidates (owner's pick)
- **C6 risk verifier** — own DR (reuses the DR-021 post-action seam; needs a deterministic risk-scoring
  function, I6 no live inference).
- **Benchmark** (vs Omnigent) · deferred `/dr` items: holder-offline attenuation (DR-018 §b) · decision
  fast-path cache · OPA/Cedar adapter · C3 sole-chokepoint (DR-009 fenced).

## Open /debrief residuals — NONE from this session
All slices closed clean. Reusable pattern captured this session (from the cost_ms /debrief): a
NON-DETERMINISTIC decision-fact payload field (like `cost_ms`) must be STRIPPED from the MCP/socket
byte-identity assertion (`crates/rezidnt-mcp/tests/permit_single_decision_path.rs`) AND guarded by a
present-and-typed check before the strip — the guard is what keeps the exclusion from masking a dropped-key
regression. Reject a bare strip with no guard. (`cost_ms` is the first such field; `ts` was the envelope
precedent.)

## Decisions still needing a /dr
- **C6 risk** · holder-offline attenuation (DR-018 §b) · decision fast-path cache · OPA/Cedar adapter ·
  C3 sole-chokepoint. Carried debt: DR-007 GitError→associated-type; `badge.issued` emitter; release items.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`.
**Quote the WSL PATH export** (`export PATH="$HOME/.cargo/bin:$PATH"`) — interop PATH has `Program Files
(x86)`; unquoted it breaks on the parens. Vet host-side (`bash .claude/hooks/vet.sh`); **host + WSL
SEQUENTIAL** ([[vet-concurrency-flake]]). **`/vet` is host-side; WSL-green NOT sufficient**
([[vet-is-host-side-wsl-insufficient]]); `#![cfg(unix)]` daemon suites don't run on host `/vet` (verify on
WSL); `rezidnt-tui` + the permit-decision-path/cost_ms suites ARE platform-neutral (host runs them).
**`clippy::doc_lazy_continuation`** ([[clippy-doc-lazy-continuation-trap]]) bites `//!` headers. Golden
fixtures auto-discovered (`<name>.jsonl`+`.expected.json`); ratatui goldens blessed via
`REZIDNT_BLESS_GOLDEN=1` (never hand-edit / never overwrite a shipped golden). ULIDs 26-char Crockford
(no I/L/O/U). Auto-push to `main` classifier-gated — ask first (owner approved this session's pushes).

---
**NEXT ACTION → cost_ms + DR-021 shipped (`dcf5c23`). DR-021 ratified live spend-cap C1 (B2: spend
post-action, off the permit fact). The C1 BUILD is the ratified follow-on and starts with a warden
`/subject` for a per-action `action.metered` fact (agent.completed is per-run, too coarse), then moves the
spend fold source off the permit reducer + wires SpendCap caps live → /oracle → impl → /vet → /debrief.
C6 risk is a separate later DR. Advancing `current-slice` past cost_ms is an owner action.**
