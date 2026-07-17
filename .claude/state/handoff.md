# Handoff — 2026-07-17 (session 4 close, S3 machinery done)

## State of play
**Current slice: S3** (MCP + attach) — machinery DONE, full loop on record: pickup triage (6 items in, rest deferred with destinations) → `/oracle mcp` (27-test board: 18 host rezidnt-mcp, 9 WSL daemon; 2 gate fixture pairs; rmcp 2.2.0 verified viable) → warden ratified `gate.entered/failed/inconclusive/explained` v1 additively over pins, operator-badge blessed as prose → implementer chose **hand-rolled JSON-RPC** over rmcp (board pins raw shapes + schemars byte-equality; I7) → all 27 green → `/vet` pass independently (host `{"verdict":"pass","evidence":[]}` + WSL workspace green, two runs beyond the implementer's own) → `/debrief` **PASS** with six tracked items. **Remaining for S3 close: the Phase-1 exit demo — golden-path-minus-gates, one take, RECORDED. Owner action.** Slice pointer stays S3 until the demo exists.

## Session log
Board+ratification `c19c267` → implementation `1df0fa8` (vet+debrief pass). Both LOCAL ONLY — push is on owner order, `origin/main` is at `92efd44`. Tree clean at close.

## Next action
**Owner records the Phase-1 exit demo** (Claude Code via MCP only: open project → spawn agent → read dossier → gate_explain forced failure; attach byte-proxy over the socket; lockfile-discovered HTTP endpoint, `REZIDNT_MCP_LOCKFILE` env). Then advance slice pointer to S4 and start Phase 2 planning — where the pre-S4 fixes below go first.

## Open /debrief findings (S3 debrief, all tracked, verdict pass)
- **T1 (med, I3, fix before S4):** workspace/spawn-key maps are process-lifetime; restart → acked workspace answers `workspace.unknown` despite `workspace.opened` on log. Fails SAFE (refusal, not duplicate facts). Rebuild from log on start. `runs.rs:180-196`, `mcp.rs:58-66`.
- **T2 (med, I3, fix before S4):** ghost-workspace window — registry entry precedes detached materialization; post-ack failure leaves a spawnable entry with no `workspace.opened` fact; never evicted. Gate spawn on the fact or evict. `runs.rs:302-320`.
- **T3 (med, warden/scribe not implementer):** `gate_explain` writes `gate.explained` unbadged — board-ratified, but §12 tension. Badge the interrogation or record interrogations as read-class.
- **T4–T7 (low):** at-least-once `worktree.allocated` on retry-after-failed-launch + daemon-wide lock held across spawn (liveness); unbounded HTTP body read (cap it); `daemon.warning` open-failed mints fresh correlation (thread the open's through); lockfile tmp `create(true)` not `create_new`.
- **T8 (scribe):** silent DEFAULTs — protocol version `2025-06-18` no negotiation, tail limit 1024 oldest-first, 202 notifications, dedicated HTTP runtime, schemars runtime dep.

## /dr and warden queue (owner/session decisions)
- **Carried, twice-tracked:** `/dr` or §7 note for `release_worktree` extending BINDING RepoSubstrate. **Carried:** warden one-liner conflict at-least-once wording; `/dr` exit-code collision REQUIRED before Phase 2; capture-chunk subject `/dr` flag.
- **New this session (warden deferrals, one session should settle together):** `gate.passed` v1 (needs S4 emitter); `badge_id` additive on mutation facts vs correlation-only; `badge.issued` emit-or-drop incl. daemon-lifetime operator-badge scope beyond §12's per-AgentRun framing. Scribe note wanted: hand-rolled-over-rmcp as formal DEFAULT.
- Carried non-slice: RepoSubstrate/GitError seam (I4); S1 hardening list; `daemon.warning` payload ratification; fixture housekeeping (tool_use transcript PROVISIONAL, s0_rebuild_equality line 3); root README; crates.io placeholder (needs `cargo login`); `rezident` fallback-string doc note.
- Deferred at triage with destinations: T1(S2)/T3(S2)/T5(S2) worktree hardening → pre-Phase-2; T4(S2) ingest helper → next git-adapter touch.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. Vet hook host-side; daemon tests WSL. Guardrails held all session (firewall, ontology-gate via warden session, fmt).
