[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-001 — Native substrates from day one

**Date:** 2026-07-04 · **Status:** ACCEPTED · **Doc version:** 0.1 → 0.2
**Amends:** §1 (non-goals), §3 (topology), §4 (crates), §7 (adapters), Invariant I8, §16 (roadmap), §18 (risks). **Supersedes:** the herdr integration path and the Omnigent runtime adapter/donor port.

## Decision

rezidnt ships with **zero external substrate dependencies**. herdr is not integrated at any phase. Omnigent code is neither executed nor ported. All substrates are native, built in-repo, from first principles.

## Basis

The v0.1 Phase 1 inherited herdr's *frame*: it assumed agents live inside terminal sessions because the incumbent tool is a terminal multiplexer. At the root, **agents are processes, not terminal sessions.** The golden path requires process supervision, output capture, lifecycle facts, worktree isolation, and gates — none of which require VT emulation, pane multiplexing, or screen re-rendering. Decomposing what herdr actually bundles:

| herdr capability | rezidnt need | native answer |
|---|---|---|
| PTY allocation + process persistence | **required** | `portable-pty` + daemon-owned PTY masters |
| VT emulation / screen re-render | **not required** (Phase 1–2) | raw byte capture; client's own terminal emulates on attach (dtach model) |
| Multiplexed pane UI | **never in core** (I1) | fleet board is a state view, not a terminal grid |
| Socket control API | already ours | §9 unchanged |

The decisive upgrade nobody gets from a multiplexer: harness CLIs now expose **structured telemetry natively**. Claude Code's headless mode (`claude -p`) emits newline-delimited JSON events — assistant messages, tool calls, tool results, session ids — and its JSON envelope reports `total_cost_usd` with a per-model breakdown (high confidence; verified against official headless docs). Terminal-scraping status heuristics were always a downgrade we were about to inherit. Native integration is not merely independent; it is *higher fidelity*.

## New component: `rezidnt-run` (the ProcessSubstrate)

Replaces `rezidnt-adapters/herdr` in the workspace. Four parts:

**Spawner.** `tokio::process` for headless children; `portable-pty` when a child demands a TTY (ConPTY on Windows arrives in Phase 1 as a consequence — aligned with the Windows wedge, WSL2-first topology unchanged). Environment scrubbing and badge injection at spawn.

**Capture.** Per-run ring buffer (256 KiB DEFAULT) for live tail; full stream chunked into the CAS with manifest events carrying refs only (I2 holds — bytes never touch the fabric).

**Persistence.** The daemon owns the PTY master; client disconnects never kill runs. `rezidnt attach <run>` is a raw byte proxy over the socket — the dtach model: no scrollback re-render, resize is SIGWINCH passthrough, redraw is the child's job. Documented limitation, and the explicit trigger metric for Phase 3.

**Reaper.** Exit status, signal escalation (TERM→KILL with grace), orphan cleanup on daemon restart via pidfile reconciliation.

## Trait changes

`AgentSubstrate` absorbs spawn-with-PTY. `TerminalSubstrate` is **removed from Phases 1–2** and reserved as the Phase 3 seam. `RepoSubstrate` unchanged. The worktree registry simplifies: rezidnt is now the **sole allocator**; `worktree.observed`/`worktree.conflict` are retained only to guard against out-of-band human git activity — the two-allocator reconciliation problem is deleted, not solved.

## Native harness adapters (`AgentSubstrate` impls)

**claude-code (S1).** Invocation: `claude -p --output-format stream-json --verbose`, with `--include-partial-messages` where token-level streaming matters. Governed runs add `--bare` — skips ambient hooks, skills, MCP servers, and CLAUDE.md discovery, requiring explicit credentials — which makes it the **determinism knob a `vet` gate can require**: same result on every machine, no inherited ambient config (high confidence; per official docs, `--bare` is recommended for scripted calls and slated to become the `-p` default). Telemetry mapping: stream-json events → `agent.tool.invoked`, `agent.message`, `agent.status.changed`; cost fields → dossier accounting, which makes the benchmark metric *cost per merged verified diff* free. Steering: `--input-format stream-json` (+ `--replay-user-messages`) is the bidirectional channel; its wire contract is thinly documented upstream (open docs issue; third parties have reverse-engineered it) — **moderate confidence in stability**, so it lives behind the trait with recorded-transcript contract tests. Permission composition: `--allowedTools`/permission modes constrain the harness per AgentSpec; badges attribute the writes; both recorded in events. Session resume (`--resume <session_id>`) maps to run checkpointing.

**codex (S4-era).** `codex exec` non-interactive path; moderate confidence on current flag surface — verify at implementation, same recorded-transcript discipline.

## Invariant I8 rewritten (BINDING)

*Old:* AGPL firewall around a herdr runtime dependency. *New — the clean-room rule:* copyleft sources (herdr, any AGPL) are **never read** for implementation purposes. Permissive sources (Omnigent, Apache-2.0) **may be read as prior art** but nothing is ported or vendored — so NOTICE obligations never arise and the tree stays provenance-clean. First principles means no inherited frames and no inherited code; it does not mean ignorance of prior art, and it does not license NIH at the component level: `portable-pty`, `tokio`, `rusqlite`, and (Phase 3) a permissive VT kernel remain mandatory boring parts. "Build our own system" is a claim about the *system*, not about syscall wrappers.

## Roadmap (supersedes §16)

**Phase 1 — fabric + run substrate. Estimate: 5–9 weeks part-time (moderate confidence; my numbers, not anchored).**
*S0 unchanged* (ontology, log, broadcast, `tail`; same acceptance).
*S1 — native run:* `rezidnt open` → worktree allocated → claude-code spawned headless under capture, lifecycle + stream-json telemetry on the fabric. Accept: golden path *minus gates* on a clean VM ≤ 5 min; kill the client mid-run and the run survives; `attach` replays the tail; every step visible in `tail`.
*S2 — git adapter:* unchanged, minus two-allocator machinery.
*S3 — MCP + attach:* unchanged acceptance, plus `attach` byte proxy demonstrated over the socket.

**Phase 2 — gates. Estimate: 5–10 weeks.** Verifier engine v1; `vet` enforces bare-mode/pinned-version/allowedTools policy pre-spawn; `pre_merge` and `debrief` on the golden path. *S4 accept:* an agent produces a **verified merged diff** with replayable debrief and recorded cost. **The golden path completes at end of Phase 2.**
*S5 — fleet board* (ratatui, read-only, watch-channels only) moves earlier — it is now the primary visibility surface beyond the CLI.

**Phase 3 — rescoped from "replace herdr" to "interactive fidelity layer."** VT kernel assembly (libghostty-vt or alacritty_terminal family — licenses verified at kickoff), scrollback re-render, rich attach, optional pane UI as a *client*. **Demand-gated**: pulled only when attach-fidelity friction is measured, not scheduled. The forced parity race with herdr is deleted from the plan entirely.

## Risk register deltas

**Deleted:** herdr API churn / adversarial maintainer; AGPL contamination; Omnigent alpha churn (runtime).
**Added:** harness CLI churn — claude-code flags and stream-json shape drift, and `--bare` becoming the `-p` default is pre-announced → pin harness versions per AgentSpec, recorded-transcript contract tests, adapter refuses untested majors. PTY/ConPTY edge cases arrive in Phase 1 → carried by `portable-pty` (high-confidence component), WSL2-first unchanged. Attach-fidelity expectations → documented limitation + tail replay; complaint volume is the Phase 3 trigger. NIH creep → the clean-room rule's component clause; violations get the phase-exit-demo test.

## Honest ledger

**Buys:** typed telemetry and per-run cost accounting instead of terminal-scraping heuristics; zero dependency on a rival's roadmap or license; sole-allocator worktree model; a cleaner install story (one binary, no external substrates, nothing to shell out to except git and the harnesses themselves); Phase 3 becomes optional instead of obligatory. **Costs:** attach without screen re-render until demand pulls Phase 3; no multi-pane interactive UX in v1; every PTY edge case is ours from day one.

*Amendments to this record require DR-002.*
