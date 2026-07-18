# Design note — SP2 PEP hook binary (the thing that actually blocks a live tool call)

**Status:** PROPOSED (design-first, DR-002 rule 1) · **Feeds:** DR-014 (owner sign-off — see §8) · **Builds on:** [DR-013](../decisions/DR-013-permit-pep-sp2-integration.md) (which deferred exactly the two questions §3/§5 answer), [permit-pep-sp2 sketch](permit-pep-sp2.md), [permit-engine](permit-engine.md) §3/§10 · **Owner:** TwofoldTech LLC

> Completes SP2. The socket-PDP half (committed `bb7afe3`) makes the daemon *answer* a permit request; nothing yet *asks* mid-run. This note designs the PEP — the claude-code `PreToolUse` hook that intercepts a live tool call, asks the daemon over the socket, and enforces the answer. It answers DR-013's two explicitly-deferred questions (where the hook lives + opt-in; the timeout budget) and closes the auditor's path-scope-asymmetry note. Not BINDING until DR-014.

## 1. Scope — the SP2 headline criterion, made real

DR-013 §11-SP2 acceptance is *"a real mid-run tool call is blocked by policy, one take."* Today that criterion is two `#[ignore]`/`unimplemented!()` stubs (`permit_socket_decision.rs` fail-posture, `permit_pep_enforcement.rs` script-leg) — honest placeholders for the part that doesn't exist: the hook. This note designs it. Non-goal (unchanged): the sole-execution-chokepoint posture (C3) stays fenced.

## 2. Where the hook lives — a `rezidnt` CLI subcommand, not a new binary (I7)

**Recommendation:** the PEP is a subcommand of the existing `rezidnt` CLI — `rezidnt permit-hook` — **not** a new executable. I7 (one static binary, no telemetry) says we don't multiply binaries; the hook is small (read stdin JSON → one socket round-trip → write hook output) and reuses `rezidnt-proto` (`Request::RequestPermission`, `Reply::PermitDecision`, `socket_path()`) already linked into the CLI. claude-code's hook config invokes `rezidnt permit-hook`; no second artifact ships.

## 3. Opt-in + injection — the daemon wires the hook at spawn, keyed on `[gates.permit]`

Run enforcement opts in through the **config that already exists** — no new knob. A run whose spec declares a `[gates.permit]` gate gets the PEP wired automatically at spawn; a run without one spawns exactly as today (degradation, §6).

The seam is `SpawnPlan::for_claude_code` (`crates/rezidnt-run/src/spawner.rs:22`) — a pure, inspectable struct (tests pin it without spawning). Extend it, when the agent has a `permit` gate, to:

1. **Inject env** into the (currently badge-only, `env_clear`'d) scrubbed env: `REZIDNT_SOCKET` (the daemon's UDS) and `REZIDNT_RUN` (this run's ULID). Run discovery is then **deterministic** — the hook reads `REZIDNT_RUN`, never guesses from cwd.
2. **Point claude-code at the hook** — write a `PreToolUse` hook config into the worktree's claude settings (`.claude/settings.json`, the same mechanism the repo's own hooks use) naming `rezidnt permit-hook`, or pass it via the CLI's settings flag. The spawn already `current_dir`s into the worktree (`runs.rs:691`), so a worktree-local settings file is clean and per-run.

Because `SpawnPlan` is pure, the injection is unit-testable: assert that a permit-gated agent's plan carries `REZIDNT_RUN`/`REZIDNT_SOCKET` + the hook config, and a non-permit agent's plan does not.

## 4. The claude-code `PreToolUse` contract

- **Input (stdin JSON):** claude-code passes `tool_name`, `tool_input`, session/cwd context. The hook maps `tool_name` → `tool`, extracts path arguments from `tool_input` → `paths`, and (I2) if `tool_input` is bulky, pins it to CAS and carries `context_ref` — never inline bytes over the socket. `run` comes from `REZIDNT_RUN`.
- **Ask:** connect `REZIDNT_SOCKET`, read hello, send one `Request::RequestPermission { run, request_id, action, tool, paths, context_ref }`, read one `Reply::PermitDecision` (or `Reply::Error`).
- **Enforce (hook output):** map the daemon decision to claude-code's `hookSpecificOutput.permissionDecision` — `allow` → `allow`, `deny` → `deny` (+ `permissionDecisionReason` = the daemon's `reason`), `ask` → `ask` (route to the human decision surface; the escalate path is a client, I1). Never coerce (I6): `ask`/`deny` never become proceed.

## 5. Fail-posture + timeout (DR-013 ratified the posture; this fixes the number)

DR-013 ratified **fail-closed → `ask`** with a bounded timeout but left the number to this slice. **Recommendation: a 250 ms default, overridable via `REZIDNT_PERMIT_TIMEOUT_MS`.** A local UDS round-trip + a run-state log fold is typically single-digit ms; 250 ms is generous headroom while keeping the hot path hot (permit-engine §10.2). On unreachable-socket / decode-error / timeout, the hook emits `ask` (escalate) — never a silent proceed (DR-011 §3). This is the daemon-independent half of criterion 5, now testable against a hook process with no daemon behind the socket.

## 6. Degradation visibility (criterion 6) — needs a `/subject`

A run with no `[gates.permit]` gate (or a harness with no `PreToolUse` hook) gets **no mid-run interception** — it still gets pre-spawn `vet` + post-hoc `debrief` evidence, and we say so (I4, design §3/§10.1). To make `gate_explain` honestly distinguish *mid-run-enforced* from *edge-gated*, the spawn should record whether the PEP was wired. **This wants a warden `/subject` pass** — either a field on `agent.spawned` (enforcement mode) or a small `permit.enforcement.declared` subject with a reducer (no consumer-less subjects, DR-006 precedent). Flagged as gated taxonomy work, not decided here.

## 7. Close the path-scope wire gap (auditor note, /debrief on `bb7afe3`)

The socket `Request::RequestPermission` carries **no `paths` axis** today, so a `path-scope` verifier degrades to escalate over the socket while the MCP path (which just gained `paths` on `RequestPermissionArgs`) can deny — a live MCP-vs-socket asymmetry. Since the hook (§4) extracts `paths` from `tool_input`, **add `paths: Option<Value>` (optional, additive) to the socket `Request::RequestPermission`** so `path-scope` decides identically on both transports. Small proto change; belongs in this slice.

## 8. Does it need its own DR? — recommend a light DR-014

Recommend **yes, a short DR-014**, because this slice touches surfaces DR-013 didn't ratify: the **proto wire** (`paths` on the socket op, §7), the **spawn-injection opt-in** mechanism (§3), the **hook-as-CLI-subcommand** decision (§2), the **250 ms timeout** default (§5), and it triggers a **`/subject`** (§6). DR-013's deferred bucket named the questions but did not decide them; DR-014 ratifies these answers. Alternatively the owner may fold §2/§3/§5 (pure impl choices) into DR-013's deferred bucket and DR-only the wire + ontology deltas — owner's call.

## 9. Acceptance criteria (turns the two `#[ignore]` stubs live)

1. **Headline (crit 1/SP2):** a permit-gated run's live tool call outside the allowlist is **blocked** — the hook emits `deny` with reason; the log carries `permit.requested` + `permit.denied`. One take.
2. **Fail-posture (crit 5, now live):** hook with an unreachable/timed-out socket emits `ask`, never proceed.
3. **Script-leg (crit 4, now live):** stdin `tool_input` → correct `permissionDecision` output + reason, per decision.
4. **Opt-in:** a permit-gated `SpawnPlan` carries `REZIDNT_RUN`/`REZIDNT_SOCKET` + hook config; a non-permit plan does not (pure-struct test).
5. **Path parity (§7):** `path-scope` decides identically over socket and MCP for the same paths.
6. **Degradation visibility (crit 6):** `gate_explain` distinguishes mid-run-enforced from edge-gated (pending the §6 `/subject`).

Sequence: DR-014 (owner) → warden `/subject` (§6) → `/oracle` the live criteria → implementer (`rezidnt permit-hook` subcommand + `SpawnPlan` injection + socket `paths` wire) → `/vet` → `/debrief`.
