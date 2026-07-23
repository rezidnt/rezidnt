# Handoff — 2026-07-23 (session 22: DR-037 installer arc + §16 S3 MCP surface — Phase-1 golden path now demonstrable END-TO-END)

## State of play
A long, multi-arc session. Two things shipped, both complete: (1) the whole **DR-037 installer arc** (a real `curl|sh`
installer + a published `v0.0.1` pre-release), and (2) **§16 S3, the MCP surface** — the Phase-1 exit. Everything
/vet + /debrief PASS, pushed to `origin/main` (synced, `e465d67`). High autonomy ON ([[autonomy-high-trust]]).
`current-slice` = `s3-exit-demo` (**done**). Untracked `.playwright-mcp/` + `docs/site/` are stray — leave them.

**The headline: the Phase-1 golden path is now demonstrable END-TO-END** — curl-installed static binary → `rezidentd`
→ Claude Code over MCP via `rezidnt mcp` → open_project → spawn_agent → dossier → gate_explain. Both BINDING §1/§18
steps that were aspirational (the `curl` install AND the MCP client path) are now real and proven in tests.

## What shipped this session (each through the full loop)
1. **Two DR-036 follow-ups** (`724068b`) — nested-verb lockstep + socket-precedence pin.
2. **DR-037 installer arc** — ACCEPTED (`0aefd51`), then `release-ci` (`0137bcf`/`21afaef`), `install-script`
   (`90d11f9`), pre-release prep (`4380c77`), and `quickstart-real` (`a50e576`). Cut **`v0.0.1`** (GitHub pre-release,
   static musl assets + SHA256SUMS); the real `curl|sh` was PROVEN end-to-end. Details: [[installer-arc-progress]].
3. **§16 S3 — MCP surface** (owner set as focus; no DR — stdio is §9-specified, proxy shape forced by I3):
   - **`mcp-stdio`** (`69c3385` + `9fd41ea`) — `rezidnt mcp`, a stdio↔loopback-HTTP JSON-RPC PROXY to the resident
     daemon in `bins/rezidnt/src/main.rs` (`mcp_serve`/`inject_operator_badge`/`MUTATING_MCP_TOOLS`). Reuses the
     existing `loopback_post` (I7, no HTTP crate); injects the operator badge into mutating tool calls only; fail-closed
     (exit 4 no-lockfile; JSON-RPC error + keep-serving on mid-session daemon loss). Host-runnable oracle
     `bins/rezidnt/tests/mcp_stdio_proxy.rs` (fake `/mcp` server, 5 tests).
   - **`s3-exit-demo`** (`e465d67`) — `bins/rezidentd/tests/s3_exit_demo_e2e.rs` (`#[cfg(unix)]`/WSL): one real daemon +
     one real `rezidnt mcp` subprocess, drives the full S3 sequence as JSON-RPC over the proxy's stdio, all mutating
     calls UN-BADGED (green proves badge injection — the daemon refuses `badge.required` otherwise). Details:
     [[mcp-surface-s3-state]].

## Key finding worth carrying: the MCP surface was ALREADY BUILT
The survey (see [[mcp-surface-s3-state]]) found all four S3 tools (`open_project`/`spawn_agent`/dossier-resource/
`gate_explain`) already real + tested, both transports (stdio + loopback-HTTP), schemars schemas with a drift oracle,
and dual-path badge auth — built across the operator-client arc (DR-031..035). The "verify rmcp at S3" question was
already resolved: deliberately HAND-ROLLED JSON-RPC, not the SDK. So S3's real gap was only the client CONNECTION PATH
(`rezidnt mcp`) + the recorded e2e demo — both now closed. No production wiring change was needed for the e2e.

## Owner-settled this session
- Installer: Linux/WSL-first; musl x86_64 only; two static binaries now (combined literal-I7 binary = named follow-up);
  raw GitHub-asset endpoint; **pre-release v0.0.1** (golden path not shippable-complete until Phase-1 exit).
- S3 forced-failure: **accept the escalation demo as S3-done** (owner agreed 2026-07-23). The demo interrogates a live
  `permit.escalated` (verdict "ask", asserted NOT allow/pass, I6) — an honest demonstration of the interrogability
  property; a true `gate.failed` demo is the natural **S4** deliverable (its verifier engine does not exist yet).

## Open follow-ups (NON-BLOCKING, none blocks done)
- **S4 is the natural next phase focus**: the verifier engine (native pack + exec contract), `vet`/`pre_merge` on the
  golden path, and a true `gate.failed` that `gate_explain` interrogates (strengthens the S3 exit's "forced failure"
  from an escalation to a real fail). §16 Phase 2. A true permit-DENY variant of the S3 demo is reachable NOW (denying
  verifier config + a role) if a harder forced-failure recording is wanted before S4 — additive, owner's call.
- `s3_exit_demo_e2e.rs::read_response` (~:220) doesn't enforce the wall-clock deadline on the blocking `read_line`
  itself (only on blank lines) — sound here (bounded by the daemon's 500ms unblock budget) but a foot-gun for a future
  beat that holds a request open without a daemon-side timeout.
- Combined multi-call single binary (literal-I7 installer form) — a daemon-crate extraction; named in DR-037. Its own
  slice/DR. `aarch64-linux-musl` + the `rezidnt.dev` vanity domain are deferred (DR-037).
- quickstart "What you just saw" narration reads more finished than Phase-1 status warrants (only line 25 hedges);
  worth a scribe pass when S1/S3 fully close. Prior: macOS/Windows backends; 1Password egress backend.

## Decisions still needing a /dr
- None outstanding. A true-deny S3 variant or the combined-binary form would each be small DRs if pursued.

## Environment (essentials)
Host `/vet` = `bash .claude/hooks/vet.sh` (definition-of-done). `rezidnt mcp` + its oracle are cross-platform
(host-lintable); the S3 e2e (`s3_exit_demo_e2e.rs`) + the installer e2e are `#[cfg(unix)]` → WSL clippy+test
([[vet-is-host-side-wsl-insufficient]]). WSL: `wsl.exe -d Ubuntu-24.04 -e bash -lc 'cd /mnt/d/github/rezidnt && export
CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target PATH=$HOME/.cargo/bin:$PATH && cargo …'` ([[wsl-dev-environment]]).
Build BOTH bins on WSL before an e2e (the harness locates the sibling `rezidnt`/`rezidentd`). Host+WSL SEQUENTIAL
([[vet-concurrency-flake]]). **musl toolchain is installed on WSL** (musl-tools + the target); build recipe in
[[installer-arc-progress]]. Traps hit this session: the `clippy doc_lazy_continuation` trap (blank `//!` needed after a
`//!` list before a prose paragraph) bit twice; the Windows UAC install-name trap (`*install*` exe → os error 740)
forced `install_script_unix.rs` → `curl_sh_unix.rs` ([[windows-test-binary-update-uac]]). `gh` is authed (`smithdak`);
`gh workflow run release.yml --ref main` = build+verify dispatch (no publish).

---
**NEXT ACTION → DR-037 installer arc + §16 S3 MCP surface both COMPLETE this session; Phase-1 golden path is now
demonstrable END-TO-END (curl install → daemon → Claude-over-MCP-via-`rezidnt mcp` → open/spawn/dossier/gate_explain),
every slice /vet + /debrief PASS, pushed to origin/main (`e465d67`). `v0.0.1` pre-release live. `current-slice` =
s3-exit-demo (done). NO forced next — owner's steer. Strongest candidate: **S4 / Phase-2** (the verifier engine — native
pack + exec contract, `vet`/`pre_merge` on the golden path, and a true `gate.failed` that upgrades the S3 exit's
forced-failure from an escalation to a real fail). Alternatives: the benchmark harness (DR-022), macOS/Windows backends,
or the small named follow-ups above. High autonomy ON.**
