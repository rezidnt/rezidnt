# Handoff — 2026-07-18 (session 7: permit "may" axis COMPLETE through SP-empty)

## State of play
Marathon session. The pre-hoc **"may" axis is now feature-complete and internally honest
end to end** — four slices closed (SP1, SP-intent, SP-wire, SP-empty), five DRs ratified
(DR-008..012). Every slice passed `/vet` + `/debrief` (auditor **pass**); SP-intent and
SP-wire each took ONE remediation round (evidence-string divergence; evidence_ref
metadata fidelity), SP-empty took a doc-staleness cleanup — all caught by the auditor,
all fixed before commit. **All pushed** to origin/main (`4d8edac..0f5496e`); tree clean.
Pointer advanced to **SP2** — but SP2 needs a design pass first (see Next action).

## What the permit engine now does (shipped, on the live PDP)
- `request_permission` MCP tool + proto `Request::RequestPermission`/`Reply::PermitDecision`
  wire shape (SP1). PDP logs `permit.requested` + one aggregate decision fact.
- Native permit pack: **tool-allowlist, path-scope, spend/rate cap, intent-lock** (SP1+SP-intent).
- Live PDP dispatches the CONFIGURED `[gates.permit]` set via `permit::aggregate` — first-Fail
  short-circuit, else any Inconclusive→escalate, else grant; maps via `decision_for` (I6). Config
  resolved daemon-side via `McpSubstrate::permit_config_for(run)`; per-run accumulators/intent
  folded from the fabric and injected as content-pinned params (SP-wire, DR-011).
- Intent semantics: declared-empty `allowed_tools: []` = lockdown (every tool off-task,
  deny-capable under `on_off_task=deny`); genuinely-absent = cannot-run/escalate (SP-empty, DR-012).
- `gate_explain` surfaces every permit decision (granted/denied/escalated) with policy_ref +
  evidence_ref, interrogable (I6). `run.intent.declared` subject + reducer folded (warden, SP5-era).

## Commits this session — ALL PUSHED (`4d8edac..0f5496e`)
`c430caf` SP1 · `b1540bf` handoff · `6f9d47c` DR-010 · `02fe7fe` subject(run.intent.declared) ·
`982e358` SP-intent · `d0e7b7c` handoff · `4293dcd` DR-011 · `8baebff` SP-wire ·
`3b7fff5` DR-012 · `0f5496e` SP-empty.

## Next action
**Spec SP2 BEFORE any `/oracle`.** SP2 = **harness PEP integration**: the claude-code `PreToolUse`
hook calls the permit endpoint mid-run so a real tool call is BLOCKED by policy (permit-engine
§11 SP2; the "make it actually enforce" slice). Everything above DECIDES; SP2 is what makes an
agent's mid-run action get stopped. It crosses a real external boundary and has open design
questions that warrant a **design sketch + likely a DR** first (like SP-intent/SP-wire did):
- **The socket path SP1 stubbed as `op.not_served`** (`bins/rezidentd/src/main.rs`) must now
  actually service a permit decision over the socket for the harness hook (SP1 left only the
  wire shape). Design where the hook reaches the PDP: socket `Request::RequestPermission` →
  daemon → `permit::aggregate` (reuse SP-wire's resolver/fold), returning `Reply::PermitDecision`.
- **The PEP contract:** claude-code `PreToolUse` hook shape (stdin/stdout JSON), how the daemon
  is addressed (socket path/lockfile), timeout/fail-closed-or-open posture on the hot path
  (permit-engine §10.1/§10.2 — latency + "enforcement only as strong as the PEP" honesty).
- **Degradation (I4):** harnesses without a hook fall back to pre-spawn vet + post-hoc evidence,
  stated explicitly (design §3) — do not overclaim interception breadth.
Sequence: design sketch → `/dr` (owner sign-off) → `/oracle` → implementer → `/vet` → `/debrief`.

## Open /debrief residuals & carried notes (non-blocking)
- **`sp_wire_aggregate_deny` fixture** (`spec/fixtures/`) green-locks only the reducer fold, not
  decision production (SP-wire debrief). Either wire it into a debrief-replay assertion or drop it
  — a committed golden with a thin assertion. Low; fold into SP2 or a cleanup.
- Live PDP now runs the full native set (SP-wire closed the SP1/SP-intent live-dispatch residual).

## Decisions still needing a /dr (permit stream + beyond)
- **SP2 needs a design sketch + /dr** (the gating next action above).
- **C8 layered admin/dev/session precedence** → folds into SP4 (roles + macaroon delegation);
  not yet designed.
- **C3 sole-chokepoint enforcement** (OS sandbox + L7 egress + credential brokering) — DR-009
  fenced; needs its own design sketch + implementation DR before any build.
- **SP3 policy-as-exec-verifier** (OPA/Rego or Cedar) — permit-engine §11; not yet spec'd.
- Any design change motivated by memo 001 needs its own DR citing it (DR-002 rule 3).
- Pre-permit carried debt: DR-007 GitError→associated-type (2nd RepoSubstrate impl); badge.issued
  emitter / badge_id on other mutations; release items (root README, crates.io owner `cargo login`);
  Phase 3 stays demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). Host test/bin names must avoid substring `update` (UAC os error 740,
[[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask before pushing.

---
**NEXT ACTION → Spec SP2 (harness PEP integration) first: a design sketch covering the socket
PDP path (un-stub `op.not_served`), the claude-code PreToolUse hook contract, hot-path
timeout/fail-posture, and graceful degradation (I4) — then a `/dr`, THEN oracle-first. Do NOT
`/oracle` SP2 until the design + DR land.**
