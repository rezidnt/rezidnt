---
name: slice-discipline
description: >-
  The rezidnt phased roadmap, per-slice acceptance criteria, and the definition of done.
  This skill should be used to determine what the current slice requires, whether a task is
  in scope, and what "done" means — done equals slice criteria passing vet and debrief,
  never a feature list. Load for slice/handoff commands and by every implementation agent.
user-invocable: false
version: 0.2.0
---

# Slice discipline

Definition of done for every task: the current slice's acceptance criteria pass `/vet` and `/debrief`. Nothing more. Anything not advancing the current slice's exit demo is scope gravity — name it and stop. Current slice ID lives in `.claude/state/current-slice`. Estimates below are the project's own, moderate confidence, dominated by available hours.

## Phase 1 — fabric + run substrate (5–9 weeks)
- **S0 (2–3 days):** ontology v0 + envelope + log + broadcast + `rezidnt tail`.
  *Exit:* two concurrent subscribers observe the stream; `kill -9` the daemon mid-stream, restart, `rebuild` reproduces identical graph state; chain verifies. Property test `fold(log)==snapshot` green.
- **S1 — native run:** `rezidnt open` materializes a workspace and spawns claude-code headless (`claude -p --output-format stream-json --verbose`, `--bare` for governed runs) under capture.
  *Exit:* golden path minus gates on a clean VM ≤ 5 min, zero config edits; kill the client mid-run and the run survives (daemon owns the PTY); `attach` replays the tail; every step visible in `tail`.
- **S2 — git adapter:** gix reads, git-CLI mutations, notify watcher, sole-allocator worktree registry.
  *Exit:* `diff.ready` within 1 s of write (post-debounce); deliberate out-of-band worktree collision emits exactly one `worktree.conflict`.
- **S3 — MCP + attach:** rmcp (or hand-rolled JSON-RPC) surface; stdio + loopback HTTP.
  *Exit:* Claude Code, via MCP only, opens a project, spawns an agent, reads its dossier, and receives a `gate_explain` for a forced failure; `attach` byte-proxy demonstrated over the socket. **Phase-1 exit = golden-path-minus-gates demo, one take, recorded.**

## Phase 2 — gates (5–10 weeks)
Verifier engine v1 (native pack + exec contract); `vet` enforces bare-mode/pinned-version/allowedTools pre-spawn; `pre_merge` and `debrief` on the golden path.
- **S4 exit:** an agent spawned under rezidnt gates produces a VERIFIED merged diff with replayable `debrief` and recorded cost. **Golden path completes here.**
- **S5 (may precede Phase 3):** ratatui read-only fleet board consuming only watch channels — proof I1 held. Primary visibility surface beyond the CLI.

## Phase 3 — interactive fidelity layer (demand-gated, NOT scheduled)
Assemble a permissive VT kernel (libghostty-vt / alacritty_terminal family — licenses verified at kickoff) + scrollback re-render + rich attach + optional pane UI as a client behind the reserved TerminalSubstrate trait. Pulled only when attach-fidelity friction is measured. The parity race with herdr is deleted from the plan.

## Sequencing law
fabric → gates → terminal. Reordering pressure gets the phase-exit-demo test.
