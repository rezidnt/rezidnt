# Handoff — 2026-07-18 (session 7: SP1 + SP-intent COMPLETE; permit engine deepening)

## State of play
Long session: closed **SP1** (request_permission + native permit pack), then fully spec'd and
built **SP-intent** (C7 intent-lock) end to end. Both passed `/vet` + `/debrief` (auditor **pass**;
SP-intent took one remediation round — see below). All work **PUSHED** to origin/main
(`4d8edac..982e358`); tree clean but for the pointer edit. Pointer advanced to **SP-wire**
(owner's choice — the PDP verifier-selection consolidation).

## What changed this session — 5 commits, ALL PUSHED (`4d8edac..982e358`)
- **`c430caf` SP1** — `request_permission` MCP tool (PDP: logs `permit.requested` + one decision
  fact, maps verdict via `decision_for`), native pack `tool-allowlist`/`path-scope`/`spend-cap`
  (SpendCap three-valued: soft-band→escalate, hard/rate→deny, never coerced, I6), proto
  `Request::RequestPermission`/`Reply::PermitDecision` wire shape, `gate_explain` permit leg,
  carried SP0 flag closed (`decided_fact` emits spend/risk/cost deltas, omit-when-None).
- **`b1540bf` handoff** (session-6→7 bridge).
- **`6f9d47c` DR-010** — ratifies SP-intent scope + criteria; design sketch
  `docs/design/intent-lock.md`. Load-bearing fork: intent→allowlist is DECLARED + content-pinned,
  never inferred live (determinism BINDING + I6). (a) explicit manifest in-scope; (b) recorded
  derivation FENCED. Off-task → escalate default (`on_off_task = escalate|deny` knob). §20 indexed.
- **`02fe7fe` /subject (warden)** — mints new noun `run` for the run-intent axis:
  `run.intent.declared v1 {run, intent_ref: CasRef, allowed_tools: [string]}` + folding reducer
  (`AgentRunState.intent`). Distinct from `agent.spawned.allowed_tools?` (composed harness allowlist
  vs intent-derived least-privilege). `SUBJECTS_V0` 39→40, drift guard green.
- **`982e358` SP-intent** — `IntentLock` native (rezidnt-gate): reads `inputs.params` only, in-intent
  →Pass / off-task→escalate / off-task+deny→Fail / intent-absent→escalate-never-pass; evidence names
  off-task tool + declared intent (CAS ref). Accept demo (memo scenario 5) folds rebuild-stable.

## Next action
**Start SP-wire with `/oracle`.** SP-wire = a focused consolidation: give `request_permission`
(the PDP) a **verifier-selection seam** so it dispatches the configured permit-verifier SET
(tool-allowlist, path-scope, spend-cap, intent-lock) from the project spec `[gates.permit].verifiers`
block — today it **hardcodes `ToolAllowlist.verify()`** (`crates/rezidnt-mcp/src/lib.rs:454`), so
the other three natives are registered + tested but never run live. Design is already ratified
(permit-engine §6 TOML shape; DR-008), so this is likely **oracle-first directly, no new DR**. The one
design point to pin: **multi-verifier verdict aggregation → permit decision** — follow the gate
engine's existing first-fail-short-circuit + three-valued precedence (any `Fail`→deny, else any
`Inconclusive`→escalate, else allow). If aggregation proves contentious, a quick `/dr`; otherwise
oracle → implementer → `/vet` → `/debrief`. Requires parsing `[gates.permit].verifiers` from the
§13 spec if SP1 didn't already — check first.

## Open /debrief residuals (SP-intent — carried, non-blocking)
- **Live-PDP dispatch gap** (`crates/rezidnt-mcp/src/lib.rs:454`): `request_permission` runs ONLY
  `ToolAllowlist`; IntentLock/PathScope/SpendCap never execute on the live path. **This is exactly
  what SP-wire closes** — it is the next slice, not a defect.
- **Empty-vs-absent allowlist collapse** (`crates/rezidnt-gate/src/lib.rs:~785`): IntentLock treats a
  DECLARED-empty `allowed_tools: []` identically to intent-absent (both → cannot-run/Inconclusive).
  Defensible under DR-010 as written (the record didn't distinguish them), but a real latent semantic:
  is "declared no tools" the same as "no declaration"? **Candidate DR-011** if the answer should be
  "declared-empty = escalate/deny everything as off-task." Owner deferred it this session.

## Decisions still needing a /dr (permit stream)
- **Empty-vs-absent intent semantic** — DR-011 candidate (above).
- **SP2 harness PEP integration** — claude-code `PreToolUse` hook → permit endpoint; real mid-run
  block. Roadmap-committed (permit-engine §11); comes after SP-wire. Also wires the socket path that
  SP1 left as `op.not_served`.
- **C8 layered admin/dev/session precedence** → folds into SP4 (roles); not yet designed.
- **C3 sole-chokepoint enforcement** (OS sandbox + L7 egress + credential brokering) — DR-009 fenced;
  needs its own design sketch + implementation DR before any build.
- Any design change motivated by memo 001 needs its own DR citing it (DR-002 rule 3).
- Pre-permit carried debt: DR-007 GitError→associated-type (2nd RepoSubstrate impl); badge.issued
  emitter / badge_id on other mutations; release items (root README, crates.io owner `cargo login`);
  Phase 3 demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). Host test/bin names must avoid substring `update` (UAC os error 740,
[[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask before pushing.

---
**NEXT ACTION → Start SP-wire with `/oracle`: give `request_permission` a verifier-selection seam
that dispatches the configured `[gates.permit]` verifier set (tool-allowlist/path-scope/spend-cap/
intent-lock), aggregating verdicts by the gate engine's first-fail + three-valued precedence.
Closes the live-PDP-dispatch residual. Check whether `[gates.permit]` spec parsing exists first.**
