# Handoff — 2026-07-18 (session 7: SP1 COMPLETE — request_permission + native permit pack)

## State of play
**SP1 is DONE.** Passed `/vet` (`{"verdict":"pass"}`) and `/debrief` (auditor **pass** — the
I6 non-coercion guarantee is *structural*, not just tested). Committed as **`c430caf`** and
**PUSHED** to origin/main (`4d8edac..c430caf`; owner-authorized this session). Tree clean.
Pointer advanced to **SP-intent** (owner's choice for next slice — but see the blocker below).

## What changed this session — 1 commit, PUSHED (`c430caf`)
Full oracle→implementer→vet→debrief loop on SP1 (permit-engine, DR-008/009; design
`docs/design/permit-engine.md` §5/§6/§11 + DR-009 folding C1 into SP1):
- **`crates/rezidnt-types/src/mcp.rs`** — `RequestPermissionArgs` (schemars; required
  `badge`/`run`/`action`/`tool`, optional `context_ref` as `cas:blake3:` ref-string). Badge
  required per design §5 (result authorizes a mutation → caller identity).
- **`crates/rezidnt-gate/src/lib.rs`** — native permit pack `ToolAllowlist` / `PathScope` /
  `SpendCap`, registered in `builtin_natives()` as `tool-allowlist`/`path-scope`/`spend-cap`.
  `SpendCap` (C1) is the load-bearing I6 test: under-soft→Pass, **soft-band→Inconclusive
  (never coerced)**, hard-cap/rate→Fail, caps-missing→Inconclusive. Inputs come from
  `inputs.params` (content-hash-pinned, determinism BINDING), never live state.
- **`crates/rezidnt-gate/src/permit.rs`** — carried SP0 flag CLOSED: `decided_fact` takes a
  trailing `DecisionDeltas { spend_delta_usd, risk_delta, cost_ms }` and emits each key
  omitted-when-`None` (never JSON null). Emit-side pinned + folded through the real reducer.
- **`crates/rezidnt-proto/src/lib.rs`** — `Request::RequestPermission` + `Reply::PermitDecision`
  (`allow|deny|ask`; `ask` carried verbatim, never coerced). Wire shape only (see blocker A).
- **`crates/rezidnt-mcp/src/lib.rs`** — `request_permission` MCP tool = the PDP: publishes
  `permit.requested` + one decision fact (I3), runs `ToolAllowlist`, maps verdict via
  `decision_for`. `gate_explain` taught to also resolve `permit.granted|denied|escalated`
  (honest absence → `GATE_NO_VERDICT`, never a synthesized pass). New `with_cas` seam.
- **`bins/rezidentd/src/main.rs`** — `Request::RequestPermission` arm answers honest
  `op.not_served` (socket servicing is SP2, not this slice).
- Tests/fixtures: `permit_natives.rs` (12), `request_permission.rs` (7), `permit_request.rs`
  (4), extended `permit_emit.rs` (8), golden `permit_deny_demo.{jsonl,expected.json}`.

## Next action
**Spec SP-intent BEFORE any `/oracle`.** SP-intent (C7 intent-lock: bind an agent's tool
allowlist to the run's initiating intent, block off-task tool use / anti-prompt-injection) is a
**roadmap note only** — DR-009 added it "after SP1's native verifiers land" (now) but wrote NO
acceptance criteria. C7 came from intel memo 001, so **DR-002 rule 3 requires its own DR citing
the memo before any design change.** So the next action is a design sketch + a `/dr` (scribe)
that sets SP-intent's scope and criteria — NOT jumping to the oracle. Only after criteria exist:
`/oracle` → implementer → `/vet` → `/debrief`.

## Open /debrief findings (SP1 — two coverage notes carried, neither a defect)
- **Only `ToolAllowlist` is wired into the live `request_permission` path.** `PathScope` and
  `SpendCap` exist, are registered, and are unit-pinned, but are NOT reachable through the MCP
  surface this slice. So criterion-4's "the spend-cap verifier is the producer of the deltas" is
  demonstrated **structurally** (direct `decided_fact` + reducer fold in `permit_emit.rs`), not
  end-to-end through a live decision. Closes when the permit gate gains verifier-config wiring.
- **Live path passes `DecisionDeltas::default()`**, so a real surface decision emits no `cost_ms`
  even when the native measured evidence. Not a violation (all delta keys optional/omitted), but
  thread `output.cost_ms` into `DecisionDeltas` on the live path when convenient.
- (Owner declined an "SP1.5 wire-natives-live" consolidation slice; both notes fold into SP2's
  PEP wiring instead.)

## Blockers / decisions still needing a /dr (permit stream)
- **SP-intent (C7) needs its own /dr** (design sketch + scope/criteria, citing memo 001) before
  it can be sliced — this is the gating next action above.
- **C8 layered policy precedence** → folds into SP4 (roles); not yet designed.
- **C3 sole-chokepoint enforcement** (OS sandbox + L7 egress + credential brokering): committed
  by DR-009 but **fenced** — needs its own design sketch + implementation DR before any build.
- Any design change motivated by memo 001 needs its own DR citing it (DR-002 rule 3).
- Pre-permit carried debt still open: DR-007 GitError→associated-type (2nd RepoSubstrate impl);
  badge.issued emitter / badge_id on other mutations; release items (root README, crates.io
  owner `cargo login`); Phase 3 stays demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). Host test/bin names must avoid substring `update` (UAC os error 740,
[[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask before pushing.

---
**NEXT ACTION → Spec SP-intent (C7 intent-lock) first: a design sketch + a `/dr` citing intel memo
001 to set scope + acceptance criteria (DR-002 rule 3). Do NOT `/oracle` until criteria exist.**
