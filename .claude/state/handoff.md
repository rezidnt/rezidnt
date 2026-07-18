# Handoff — 2026-07-18 (session 8: SP2 COMPLETE end-to-end — the "may" axis now enforces mid-run)

## State of play
**SP2 is done.** The permit engine now *enforces*, not just decides: a claude-code `PreToolUse`
hook (`rezidnt permit-hook`) asks the daemon before a real tool call and **blocks it on policy,
one take** — the SP2 headline criterion, live. Both halves shipped this session through the full
loop (design → DR → oracle → implement → vet → debrief), each `/debrief` reaching auditor **pass**:
- **Socket-PDP half** (`bb7afe3`) — one transport-neutral `decide_permit`; socket un-stubbed.
- **Hook sub-slice** (`5693aff`) — the PEP hook + `SpawnPlan` injection + socket `paths` wire +
  `agent.spawned.pep` enforcement-mode fold + `gate_explain` visibility.
Two DRs ratified (DR-013, DR-014); one warden `/subject` (`agent.spawned.pep`). Everything committed;
**`main` is ahead 2 of origin** (`de70552`, `5693aff`) — auto-push is classifier-gated, **ask before pushing.**

## What SP2 shipped (all committed, green)
- **PDP:** `McpCore::decide_permit` — one code path for MCP + socket; byte-identical decision facts (I3).
- **PEP:** `rezidnt permit-hook` CLI subcommand (I7, not a new binary). PreToolUse stdin → maps
  tool/paths, run from `REZIDNT_RUN`, bulky `tool_input` → CAS `context_ref` (I2) → one socket
  round-trip (250 ms, `REZIDNT_PERMIT_TIMEOUT_MS`) → `hookSpecificOutput.permissionDecision`.
  **Fail-closed → `ask`** on every failure (unreachable/decode/timeout/malformed-reply/bad-stdin/
  CAS-pin-failure); never coerced to proceed (I6).
- **Opt-in:** `SpawnPlan::for_claude_code_permit` injects the hook + `REZIDNT_RUN`/`REZIDNT_SOCKET`,
  keyed on `[gates.permit]`; `agent.spawned.pep="enforced"` recorded (absent = edge-gated-only).
- **Path parity:** socket `Request::RequestPermission` gained optional `paths` — socket now DENIES
  outside scope (was escalate), identical to MCP.
- **Visibility:** `gate_explain` reports `mid-run-enforced` vs `edge-gated-only` (I4 honesty).

## Commits this session (`770c228..5693aff`) — ahead 2 of origin, NOT fully pushed
`286e2e1` DR-013+sketch · `bb7afe3` SP2 socket-PDP (pushed) · `b762213` hook design note (pushed) ·
`762232a` DR-014 (pushed) · `de70552` subject(agent.spawned.pep) **(unpushed)** ·
`5693aff` SP2 hook sub-slice **(unpushed)**.

## Next action — SP2 done; choose the next slice (owner priority)
The permit "may" axis is now feature-complete end to end (SP1..SP2). Roadmap options (permit-engine §11):
- **SP3 — policy-as-exec-verifier** (OPA/Rego or Cedar as an exec permit-verifier). Not spec'd;
  would open a fresh design→DR→oracle arc like SP2 did.
- **SP4 — roles + macaroon-attenuated delegation** (promotes DR-005 PROVISIONAL; folds in C8
  layered admin/dev/session precedence). Not spec'd.
- **C3 — sole-chokepoint enforcement** (OS sandbox + L7 egress + credential brokering). DR-009
  fenced; needs its own design sketch + implementation DR before any build.
- **Carried debt / cleanup** (below) instead of a new slice.
The pointer is still SP2; advance it once the next slice is chosen. Each new slice is oracle-first
after its spec + DR.

## Open /debrief residuals & carried notes (non-blocking)
- SP2 auditor **pass**; the earlier observations (4 KiB `context_ref` threshold, bad-stdin
  defense-in-depth) were both addressed in remediation — nothing outstanding on SP2.
- **Host-vs-WSL vet lesson (new):** the definition-of-done `/vet` is **host-side**; WSL-green is NOT
  sufficient (a `#[cfg(unix)]` dead-code clippy error passed WSL but failed host `/vet`). Verify
  platform-cfg code against host clippy too. Consider adding to memory.

## Decisions still needing a /dr (permit stream + beyond)
- **SP3 / SP4 / C3** — each needs its own spec + DR before build (see Next action).
- Any design change motivated by memo 001 needs its own DR citing it (DR-002 rule 3).
- Pre-permit carried debt: DR-007 GitError→associated-type (2nd RepoSubstrate impl); `badge.issued`
  emitter / `badge_id` on other mutations; release items (root README, crates.io `cargo login`);
  Phase 3 stays demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo)**. Vet hook host-side (`bash .claude/hooks/vet.sh`);
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent**
([[vet-concurrency-flake]]). Host test/bin names must avoid substring `update` (UAC os error 740,
[[windows-test-binary-update-uac]]). Auto-push to `main` is classifier-gated — ask before pushing.

---
**NEXT ACTION → SP2 is COMPLETE (committed `5693aff`, auditor pass). Choose the next slice with the
owner — SP3 (policy-as-exec-verifier), SP4 (roles + delegation), C3 (sole-chokepoint, fenced), or
carried cleanup — then run its design→/dr→/oracle arc. ALSO PENDING: owner ok to push (`main` ahead 2).**
