# Handoff — 2026-07-22 (session 20: DR-034 live-unblock + DR-035 escalation TTL & grant-all — ALL shipped)

## State of play
Cold-started from session 19's handoff (operator-client arc DR-031/032/033 complete). Owner steered three
consecutive design commitments this session, each run through the full loop and shipped to `origin/main`
(synced, `6a8a44b`). High autonomy ON ([[autonomy-high-trust]]). `current-slice` = `escalation-grant-all`
(**done** — last sub-slice of the DR-035 arc).

Three arcs closed, each DR → (/subject) → /oracle → implement → /vet → /debrief, all /vet + /debrief PASS:
1. **DR-034 operator live-unblock** — resume the currently-stalled agent on resolve (pulled forward DR-033's
   demand-gated deferral). One /debrief came back INCONCLUSIVE (a real scope gap), remediated in-session, re-PASSED.
2. **DR-035 sub-slice 1 `escalation-ttl`** — log-derived resolution expiry. Clean PASS.
3. **DR-035 sub-slice 2 `escalation-grant-all`** — broad-grant predicate + the structural coupling. Clean PASS.

## What shipped (git log since `f9c2948`)
- `cc7e46d` **DR-034 ACCEPTED** + `042740b` impl (both halves) + `bbad69e` handoff.
- `686e19d` **DR-035 ACCEPTED** (one DR, both axes — TTL + grant-all — designed together because the risk is
  DURATION × BLAST-RADIUS) + §20 row (next record now **DR-036**) + §16 pointer.
- `7d2989c` **escalation-ttl** impl · `e21107d` slice advance · `6a8a44b` **escalation-grant-all** impl.

## The two features now live on `permit.resolved` v1 (both additive, v1 unchanged, drift green)
- **`ttl_ms?: u64`** — optional duration; a resolution applies only while `incoming_request_ULID_ms <=
  resolution_ULID_ms + ttl_ms`, else re-escalates. Expiry is a PURE FOLD of two event-ULID timestamps already on
  the log (no decision-time wall-clock → I3-clean, replay-deterministic). Absent = permanent (DR-033 behavior).
- **`scope?: "run_tool"`** — optional single-axis wildcard; a broad resolution matches ANY action on its
  `(run, tool)` (tool stays EXACT). Absent = DR-033 exact request-scoped match. Unknown scope values fail closed to
  exact. **Coupling (structural, DR-035 §Decision 3):** `resolve_permit` REFUSES `scope="run_tool"` with no
  `ttl_ms` (code `codes::SCOPE_REQUIRES_TTL`, badge→validate→emit, no fact on refusal) — broad-and-permanent is
  UNMINTABLE, not merely discouraged.

## The shared seam (where both features live)
`AgentRunState::resolution_for(action, tool, incoming_ms)` (`crates/rezidnt-state/src/lib.rs` ~482/507/550) — the
one function the DR-033 PDP path (`apply_folded_resolution`) AND DR-034 live-unblock (`recheck_resolution`) both
route through, so TTL + grant-all flow to both automatically. `action_matches` (free fn) does the action-axis
wildcard; the TTL deadline branch composes with it; `expired_resolution_for` mirrors both for I6 expiry
interrogation. Operator surface: `resolve_permit` gained optional `ttl_ms`/`scope`; `rezidnt operator resolve-permit`
gained `--ttl-ms` (a `--scope` flag was NOT added — flag if the CLI needs it; the MCP arg exists).

## Next action (owner's steer — DR-035 arc complete, nothing gated)
No forced next. `current-slice` sits at `escalation-grant-all` (done). Natural options:
1. **`--scope` CLI flag** — small gap: the MCP `resolve_permit` takes `scope`, but the `rezidnt operator
   resolve-permit` subcommand only wired `--ttl-ms`. A broad grant is currently only reachable via raw MCP, not the
   CLI. Trivial follow-up (no DR) if operator ergonomics want it.
2. **Onboarding** ([[onboarding-future-focus]]) — flagged by owner; still needs audience + DR before it's a slice.
3. Other roadmap phase: benchmark harness (DR-022), macOS/Windows sandbox+egress backends.
4. A live-op end-to-end demo of all the operator actions (kill-run + resolve + live-unblock + TTL/broad grants).

## Open /debrief findings (NON-BLOCKING, none blocks done)
- DR-034: the INCONCLUSIVE (PEP client 250ms read-timeout unlifted, so the real client couldn't collect the held
  reply) was remediated (deadline split) and re-PASSED. Recorded as DR-034 §Decision 3 erratum (wake keys on
  action-identity via the ledger re-decide, not request_id equality — cleaner, ledger is truth).
- grant-all non-blocking notes (auditor): `saturating_add` silently clamps a pathological `u64::MAX` ttl to
  "always applies" (no criterion requires refusing it); `scope` is a closed single-value enum — a future second
  axis needs an `action_matches` arm, not just a schema value.

## Decisions still needing a /dr
- None outstanding from this arc. DR-035 closed DR-033's two "MAY add if demand shows" deferrals (TTL, grant-all).
- Prior carried (unrelated): macOS/Windows sandbox+egress backends; MCP-based 1Password backend.

## Environment (essentials)
Host `/vet` = `bash .claude/hooks/vet.sh` (definition-of-done). DR-035's core (fold + `resolution_for` + coupling
guard) is in the **platform-neutral state + mcp crates** — host-lintable, and all DR-035 tests are host-side (no
`#[cfg(unix)]`). DR-034 live-unblock DID add `#[cfg(unix)]` bodies (daemon `await_unblock` + PEP `ask_daemon`) —
host clippy can't reach them ([[vet-is-host-side-wsl-insufficient]]); for any future work there, lint on WSL
(`wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, quote the PATH export [[wsl-dev-environment]]), host+WSL
SEQUENTIAL ([[vet-concurrency-flake]]). Watch [[clippy-doc-lazy-continuation-trap]] in doc headers. Untracked
`.playwright-mcp/` + `docs/site/` are stray, not part of the project — leave them.

---
**NEXT ACTION → DR-034 (live-unblock) + DR-035 (escalation TTL + grant-all) all shipped, every slice /vet +
/debrief PASS, pushed to origin/main (`6a8a44b`). `current-slice` = escalation-grant-all (done); DR-035 arc
complete. NO forced next — owner's steer. Strongest candidates: (1) wire `--scope` on the resolve-permit CLI
(trivial, no DR — the MCP arg exists but the CLI only got --ttl-ms), (2) onboarding (needs audience + DR), (3) a
live-op end-to-end demo of the operator actions, (4) a different roadmap phase (benchmark DR-022; macOS/Windows
backends). High autonomy ON. DR-035 work is host-lintable; DR-034's #[cfg(unix)] bodies need WSL clippy.**
