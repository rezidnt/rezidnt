# Handoff — 2026-07-18 (session 8: SP2 socket-PDP COMPLETE+PUSHED; hook sub-slice DR-014 awaiting sign-off)

## State of play
SP2 = **harness PEP integration** (make the "may" axis *enforce*, not just decide). This
session took SP2 through the full arc — design sketch → DR-013 (ACCEPTED, owner) → `/oracle`
→ implementer → `/vet` → `/debrief` (fail: 2 findings) → remediation → re-`/vet` → re-`/debrief`
(**pass**) → **committed + pushed** (`bb7afe3`). The **socket-PDP half of SP2 is done, green, on
origin/main**. Then designed the remaining SP2 piece — the claude-code PEP hook binary — in a
committed design note (`b762213`), then drafted and **ratified DR-014** (ACCEPTED, owner) to gate it.
DR-014 + §20 index + this handoff are committed and pushed. **No owner blocker outstanding** — the
SP2 hook sub-slice build is cleared to start (warden `/subject` first; see Next action).

## What SP2 shipped this session (committed, green)
- **One PDP code path (I3).** Extracted transport-neutral `McpCore::decide_permit(PermitRequest)
  → PermitOutcome` from `call_request_permission` (now a thin JSON-RPC adapter). Both on-log facts
  emit from `decide_permit` ONLY; the socket and MCP **decision** facts are byte-identical (the
  `permit.requested` fact differs only on `badge_id`, by §3 — pinned by test).
- **Socket un-stubbed.** `Request::RequestPermission` (was `op.not_served`, `main.rs`) now calls
  `decide_permit` → returns `Reply::PermitDecision`. The PDP `McpCore` is now built
  **unconditionally** at startup (no longer gated on `REZIDNT_MCP_LOCKFILE`); socket + optional
  HTTP transport share the one `Arc`.
- **Socket identity = 0600 UDS (DR-013 §3).** Socket skips the §12 badge door; MCP badge-first
  door unchanged. `badge` stays optional on the socket wire.
- **`Enforcement{Proceed,Block,Escalate}` + `for_decision`** in rezidnt-proto (allow→Proceed,
  deny→Block, ask→Escalate; unknown→Escalate — never coerce, I6).
- **`RequestPermissionArgs`** gained `request_id` + `paths` (optional) so the advertised MCP
  inputSchema matches exactly what the adapter reads (§9 no-drift; closed a /debrief finding).

## Commits this session — NOT pushed (`770c228..bb7afe3`, ahead 2)
`286e2e1` DR-013 + SP2 sketch · `bb7afe3` SP2 socket-PDP slice.
DR-013 is ACCEPTED; §20 index bumped (next is DR-014). Design sketch: `docs/design/permit-pep-sp2.md`.

## Next action
**Build the SP2 hook sub-slice (DR-014 ACCEPTED).** Sequence:
  1. Warden **`/subject`** (§6 enforcement-mode visibility taxonomy) — a field on `agent.spawned`
     or a `permit.enforcement.declared` subject + reducer (warden's design; no consumer-less subjects).
  2. **`/oracle`** the now-live criteria: crit-5 fail-posture (`permit_socket_decision.rs` stub) +
     crit-4 script-leg (`permit_pep_enforcement.rs` stub), both currently `#[ignore]`/`unimplemented!()`,
     plus crit 1 (live tool call blocked, one take) and crit-5-path parity.
  3. **Implementer:** `rezidnt permit-hook` CLI subcommand + `SpawnPlan::for_claude_code` injection
     (`REZIDNT_RUN`/`REZIDNT_SOCKET` + PreToolUse config, keyed on `[gates.permit]`) + add optional
     `paths` to the socket `Request::RequestPermission` wire; 250 ms timeout, fail-closed → `ask`.
  4. **`/vet`** → **`/debrief`** → commit.
DR-014 §Decision is the spec. Ratified decisions: hook = CLI subcommand (I7); opt-in via `[gates.permit]`;
250 ms `REZIDNT_PERMIT_TIMEOUT_MS`; socket `paths` wire; enforcement-visibility `/subject`.

## Open /debrief residuals & carried notes (non-blocking)
- **Criterion 6 (hook-less degradation / `gate_explain` mid-run-enforced vs edge-gated)** — NOT
  encoded; needs the hook-vs-no-hook harness distinction + possibly a degradation-visibility
  `/subject` (DR-013 §6.4). Follow-on once the hook binary lands.
- **MCP-vs-socket path-scope asymmetry** (auditor note, not scored): socket carries no `paths`
  axis, so a `path-scope` gate degrades to escalate over the socket while MCP-with-paths can deny.
  Honest fail-closed, within DR-013 scope — worth a design note before the hook binary.
- `sp_wire_aggregate_deny` fixture residual is **resolved** (folded into criterion-7 golden).

## Decisions still needing a /dr (permit stream + beyond)
- **SP2 hook-binary sub-slice → DR-014 ACCEPTED.** Build cleared; warden `/subject` (§6) is the first step.
- **C8 layered admin/dev/session precedence** → folds into SP4 (roles + macaroon delegation).
- **C3 sole-chokepoint enforcement** (OS sandbox + L7 egress + credential brokering) — DR-009
  fenced; needs its own design sketch + implementation DR before any build.
- **SP3 policy-as-exec-verifier** (OPA/Rego or Cedar) — permit-engine §11; not yet spec'd.
- Any design change motivated by memo 001 needs its own DR citing it (DR-002 rule 3).
- Pre-permit carried debt: DR-007 GitError→associated-type (2nd RepoSubstrate impl); badge.issued
  emitter / badge_id on other mutations; release items (root README, crates.io `cargo login`).

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). Host test/bin names must avoid substring `update` (UAC os error 740,
[[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask before pushing.

---
**NEXT ACTION → Build the SP2 hook sub-slice (DR-014 ACCEPTED). Start with the warden `/subject`
for §6 enforcement-mode visibility, then `/oracle` the two now-live `#[ignore]` stubs + crit 1/4/5,
then implementer (`rezidnt permit-hook` subcommand + `SpawnPlan` injection + socket `paths` wire,
250 ms fail-closed → ask), then `/vet` → `/debrief`.**
