# Handoff — 2026-07-18 (session 6: permit-engine pivot + SP0 COMPLETE)

## State of play
Big strategic pivot this session: rezidnt now **replaces Omnigent's permission axis** (pre-hoc
"may") natively, not just composes alongside it. Ratified in **DR-008**; scope of what to match
set in **DR-009**; sized by **intel memo 001** (clean-room, cited). Then built the first slice.

**Current slice: SP0 (permit gate lifecycle point) — DONE.** Passed `/vet`
(`{"verdict":"pass"}`) and `/debrief` (auditor **pass** — the I6 "inconclusive never coerced"
guarantee is *structural*, not just tested). Pointer advanced to **SP1**. All pushed; tree clean.

## What changed this session — 6 commits, ALL PUSHED (`9d4d424..4d8fcd8`)
- **`b97a056`** IA cleanup: decision records extracted from the architecture doc into
  `docs/decisions/` (one file each, DR-001..007), `§20` index added, `/dr` workflow rewired
  (scribe + command now write to `docs/decisions/`), stale-section "amended by" pointers.
- **`127ee97` DR-008** — the pivot: rezidnt owns both axes via a permit engine. Design sketch
  `docs/design/permit-engine.md` (PDP/PEP split, `permit` gate, policy-as-verifier, macaroons).
- **`3a1a0b3` intel memo 001** — `intel/001-omnigent-permission-governance.md`: 12-row capability
  matrix + 10-scenario benchmark seed. Omnigent = Databricks meta-harness (Apache-2.0).
- **`d6e48eb` DR-009** — folds four memo gaps into scope: C1 spend/rate→SP1, C7 intent-lock→new
  SP-intent, C8 layered precedence→SP4, C3 sandbox/egress→own later phase (fenced behind its own DR).
- **`ae85b2a` /subject (warden)** — mints `permit.requested/granted/denied/escalated` +
  pure reducer (per-run ledger + session accumulators: cumulative_spend_usd, risk_score, counts).
  `SUBJECTS_V0` 35→39, drift guard green.
- **`4d8fcd8` SP0 (oracle→impl→vet→debrief)** — `crates/rezidnt-gate/src/permit.rs`:
  `LIFECYCLE_POINT="permit"`, `PermitDecision`, total non-coercing `decision_for`
  (Pass→Grant/Fail→Deny/Inconclusive→Escalate), `decided_fact`/`requested_fact` (policy_ref +
  optional evidence_ref/reason omitted-not-null; bulk context as CasRef, I2). Golden fixtures pin
  the folds; producer/reducer/ontology wire keys agree (auditor-verified, no drift).

## Next action
**Start SP1 with `/oracle`.** SP1 = `request_permission` MCP tool + socket path, and the native
permit-verifiers: **tool-allowlist, path-scope, and C1 spend/rate limits**. Oracle-first as always.

## Open /debrief findings (SP0 — one flag carried to SP1, not a defect)
- `decided_fact` (`crates/rezidnt-gate/src/permit.rs`) does **not** yet emit
  `spend_delta_usd`/`risk_delta`/`cost_ms`. The reducer reads them and fixtures carry them
  (additive optionals), so SP0 is internally consistent — but no emit-side oracle pins those two
  keys, so a future producer/consumer drift on them would slip past `permit_emit.rs`.
  **SP1 fix:** add spend/risk params to `decided_fact` + an emit-side pin when the C1 spend-cap
  verifier lands (that verifier is the producer).

## Decisions still needing a /dr (permit stream)
- **C3 sole-chokepoint enforcement** (OS sandbox + L7 egress proxy + credential brokering):
  committed to the roadmap by DR-009 but **fenced** — needs its own design sketch + its own
  implementation DR before any build. Do not start it as a slice.
- Any design change motivated by memo 001 needs its own DR citing it (DR-002 rule 3).
- Pre-permit carried debt still open: DR-007 GitError→associated-type (2nd RepoSubstrate impl);
  badge.issued emitter / badge_id on other mutations; release items (root README, `rezident`
  fallback note, crates.io needs owner `cargo login`); Phase 3 stays demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). Host test/bin names must avoid substring `update` (UAC os error 740,
[[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask before pushing.

---
**NEXT ACTION → Start SP1 with `/oracle`: `request_permission` MCP tool + socket, and native
permit-verifiers (tool-allowlist, path-scope, C1 spend/rate). Carry the SP0 flag: add
spend_delta_usd/risk_delta to `decided_fact` with an emit-side pin.**
