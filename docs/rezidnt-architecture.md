# rezidnt — Architecture Plan

**Version:** 0.1 (pre-code) · **Owner:** TwofoldTech LLC · **License posture:** Apache-2.0 (see §17)
**Status flags used throughout:** **BINDING** (change requires a written decision record), **DEFAULT** (best current call; cheap to revisit), **PROVISIONAL** (expected to change; do not build against).

> Standing gates, restated once: the name ships only after registry checks pass (`rezident` is the fallback string), and the repository stays private until the employer IP memo and carve-out letter are executed. Nothing in this document overrides either gate.

---

## 1. Thesis and scope

rezidnt is a local-first **resident daemon** that runs, verifies, and audits a fleet of coding agents across workspaces. It is one Rust binary that owns three things no current tool unifies: a **typed event fabric** (every fact about the fleet, append-only, replayable), a **supervisor** (substrates and agents run under restart-with-backoff discipline, like a session-scoped init), and a **verifier gate engine** (deterministic checks that produce evidence, not vibes). Everything else — terminals, harnesses, UIs — attaches to those three through narrow seams.

Positioning, compressed from the strategy record: Omnigent governs what agents *may* do (pre-hoc permissions); rezidnt proves what agents *did* (post-hoc evidence). herdr owns the terminal substrate today; rezidnt integrates it at arm's length and replaces it in Phase 3 with a permissively-licensed kernel it assembles itself. The wedge buyers are (a) the single operator running a fleet on their own hardware and (b) Microsoft-stack enterprises that need an audit trail a compliance reviewer will sign.

**The golden path is the product contract (BINDING):** cold machine → `curl` install → `rezidnt open <repo>` → worktrees allocated, agents spawned under gates, fleet state visible, first verified diff merged — one take, zero config edits, single-digit minutes. Every phase exit is judged against this demo, not a feature list.

**Twelve-month non-goals (BINDING):** writing a VT parser from scratch (Phase 3 *assembles* permissive components), real-time multi-device session sync, mobile clients, hosted/cloud control plane, plugin marketplace.

## 2. Design invariants (BINDING)

**I1 — Zero pixels in core.** `rezidentd` renders nothing. Every UI (CLI, TUI, Tauri, web) is a client of the socket/MCP surface. A renderer decision must never force a daemon change.

**I2 — Control plane and data plane never mix.** The event fabric carries facts and references. PTY bytes, transcripts, diffs, and artifacts move out-of-band (substrate-direct or via the CAS, §10). Payload hard cap 32 KiB (DEFAULT); larger content becomes a CAS ref. Rationale: terminal output is tens of MB/s aggregate under agent load; the bus is designed for ≤ ~10³ events/min. Mixing them is the classic event-sourcing death.

**I3 — The log is the truth; state is derived.** Materialized state can be deleted and rebuilt from the log at any time (`rezidnt rebuild`). Reducers are pure. Any feature that cannot be reconstructed from events + CAS is misdesigned.

**I4 — Substrates behind traits.** Terminal, agent-harness, repo, and window-arrangement capabilities are Rust traits. herdr, git, and Omnigent are *implementations*, individually swappable. This is the hedge against both dependencies turning adversarial (herdr's maintainer is heading toward "agent runtime"; Databricks ships on their schedule).

**I5 — MCP-first, UI-second.** Every capability ships as an MCP tool/resource before it gets a keybinding. Agents are first-class operators of rezidnt itself.

**I6 — Verifiers are deterministic and interrogable.** Same inputs (pinned by content hash) → same verdict, replayable from the log. "Why was I blocked" returns the failing verifier and its evidence, machine-readably. A gate that can't explain itself doesn't ship.

**I7 — One static binary, no runtime dependencies, no telemetry.** Install is `curl | sh` or `cargo install`. The daemon phones home to no one; this is a product stance, not a config default.

**I8 — AGPL firewall.** herdr is consumed strictly as a separate process over its socket/CLI. No linking, no vendoring, no source consultation for implementation. CI enforces a denylist (no `herdr` source in-tree). Omnigent (Apache-2.0) may be *ported* with NOTICE preservation; herdr may not.

## 3. System topology

```
┌────────────────────────── clients ──────────────────────────┐
│  rezidnt CLI    TUI board (S5)    Claude Code / MCP agents  │
│  Tauri dashboard (later)          any MCP-speaking tool     │
└───────────────┬──────────────────────────┬──────────────────┘
                │ UDS / named pipe (JSONL)  │ MCP (stdio + localhost HTTP)
┌───────────────▼──────────────────────────▼──────────────────┐
│                     rezidentd (daemon)                       │
│  fabric (broadcast bus) ── event log (SQLite, append-only)   │
│  reducers → materialized workspace graph (watch channels)    │
│  gate engine (native + exec verifiers, evidence → CAS)       │
│  supervisor (adapter tasks, backoff, health events)          │
└───┬──────────────┬───────────────┬───────────────┬───────────┘
    │ herdr socket │ git CLI + gix │ Omnigent WS   │ komorebi/Win32
┌───▼───┐      ┌───▼───┐       ┌───▼────┐      ┌───▼─────┐
│ herdr │      │ repos │       │Omnigent│      │arranger │
│(AGPL, │      │ + FS  │       │(Phase 2│      │(optional│
│ IPC   │      │watcher│       │ donor) │      │ feature)│
│ only) │      └───────┘       └────────┘      └─────────┘
```

Process model: one `rezidentd` per user session, auto-started by the CLI on first use (`rezidnt` connects, spawns the daemon if absent, waits for the hello). Trust boundary is the local user account (§12). Multi-machine is out of scope until Phase 3+; the seam is that clients already speak a socket protocol, so a remote bridge is additive.

## 4. Crate workspace layout

Cargo workspace, one repository. Library crates dual-licensed `MIT OR Apache-2.0` (Rust convention, maximizes reuse); binaries Apache-2.0.

```
rezidnt/
  crates/
    rezidnt-types      # event envelope, subjects, entity models, schemars derives — published first (0.0.1)
    rezidnt-fabric     # bus, log writer/reader, replay, hash chain
    rezidnt-state      # reducers, workspace graph, watch channels, snapshots
    rezidnt-supervise  # adapter runtime: spawn, backoff, health, circuit breaker
    rezidnt-gate       # gate engine, native verifier trait, exec-verifier contract, evidence
    rezidnt-adapters/
      herdr/           # socket client + CLI fallback (IPC only — see I8)
      git/             # gix reads, git-CLI mutations, notify watcher, worktree registry
      omnigent/        # Phase 2; WebSocket client + ported harness adapters (NOTICE)
      arranger/        # feature-flagged; komorebi IPC / Win32
    rezidnt-mcp        # MCP server (resources + tools)
    rezidnt-proto      # socket protocol frames, versioned hello
    rezidnt-cas        # content-addressed store (blake3)
  bins/
    rezidentd          # the daemon
    rezidnt            # the CLI (also: daemon auto-spawn)
  spec/
    ontology.md        # subject taxonomy — the IP; versioned like code
    schemas/           # generated JSON Schema (schemars) → published to @rezidnt npm scope
  bench/
    harness/           # public benchmark harness (held-out cases live elsewhere — §15)
```

## 5. Event fabric

**Envelope (BINDING in shape; additive evolution only):**

```rust
pub struct Event {
    pub id: Ulid,                  // time-ordered, globally unique
    pub ts: OffsetDateTime,        // daemon clock, UTC
    pub v: u16,                    // payload schema version for this subject
    pub source: SourceId,          // adapter/component that emitted
    pub workspace: Option<WorkspaceId>,
    pub subject: Subject,          // e.g. agent.status.changed (Appendix B)
    pub correlation: Ulid,         // groups a causal chain (one `open`, one gate run)
    pub causation: Option<Ulid>,   // the event that directly triggered this one
    pub payload: serde_json::Value // ≤ 32 KiB; larger content → CasRef
}
```

Serialization is JSON Lines on the wire and JSON in the log column (DEFAULT). Control-plane volume makes binary encodings premature optimization; `postcard`/CBOR is a drop-in later because `rezidnt-types` owns serde derives.

**Subjects** are dot-namespaced, never renamed, only deprecated (BINDING). Payloads evolve additively; a breaking change mints `v+1` and reducers must handle all live versions. The taxonomy v0 is Appendix B and lives in `spec/ontology.md` — treat that file as the crown jewel; it is the artifact that outlives every implementation choice here.

**Delivery semantics:** append to the log is the commit point (exactly-once by ULID uniqueness); in-process fan-out is `tokio::sync::broadcast` (at-least-once to live subscribers). A lagged subscriber that overflows its buffer receives `Lagged(n)` and **must resync from the log by last-seen ULID** rather than pretend continuity (BINDING rule for all clients and adapters). This single rule is what keeps slow UIs from ever back-pressuring the daemon.

## 6. Event log and materialized state

**Storage decision (DEFAULT, with reasoning): SQLite via `rusqlite`, WAL mode.** Rejected: `redb` (no ad-hoc SQL — `debrief`, the benchmark, and future compliance exports are all query workloads), Postgres (violates I7), flat JSONL (no indexed reads for replay-from-ULID). SQLite gives single-file backup, `PRAGMA integrity_check` for `doctor`, and a query surface the audit story is literally sold on. Revisit only if write amplification appears at >10⁴ events/min, which is two orders of magnitude above design load.

```sql
CREATE TABLE events (
  seq        INTEGER PRIMARY KEY,          -- monotonic append order
  id         TEXT NOT NULL UNIQUE,         -- ULID
  ts         TEXT NOT NULL,
  v          INTEGER NOT NULL,
  source     TEXT NOT NULL,
  workspace  TEXT,
  subject    TEXT NOT NULL,
  correlation TEXT NOT NULL,
  causation  TEXT,
  payload    TEXT NOT NULL,                -- JSON
  chain      BLOB NOT NULL                 -- blake3(prev.chain || id || payload) — §12
);
CREATE INDEX idx_events_subject ON events(subject, seq);
CREATE INDEX idx_events_ws       ON events(workspace, seq);
CREATE INDEX idx_events_corr     ON events(correlation);
```

Retention: the log is forever by default; compaction/archival is PROVISIONAL and gated on real disk pressure, because the log *is* the compliance artifact and the eval corpus.

**Materialized state** is a CQRS-lite fold. Reducers are pure functions `fn apply(&mut Graph, &Event)` living in `rezidnt-state`; the graph is the entity model below; every entity class exposes a `tokio::sync::watch` channel so clients subscribe to *state*, not raw events, unless they ask for the firehose.

```
Project ─┬─ Workspace (jurisdiction; config, layout intent)
         │    ├─ Worktree (canonical path, branch, allocator, claim state)
         │    ├─ Session (terminal substrate handle: herdr ws/tab/pane ids)
         │    └─ AgentRun (harness, model, status, badge, gate history)
         │         └─ Dossier (derived per-agent view: runs, verdicts, evidence refs, cost)
         └─ GateDef / VerifierDef (named policy points and their verifier lists)
Artifact (CAS ref + mime + provenance)   ·   AdapterHealth (per substrate)
```

Snapshots: periodic (every 5,000 events or 15 min, DEFAULT) into a `snapshots` table keyed by last `seq`; startup = load snapshot, fold the tail. `rezidnt rebuild` drops snapshots and refolds from `seq 0`; a rebuild that diverges from the running graph is a reducer bug and a release blocker (property-tested, §15).

## 7. Substrate adapter layer

**Runtime model:** each adapter is a supervised tokio task owning its connection, receiving commands over `mpsc`, emitting events into the fabric. Supervision policy (DEFAULT): exponential backoff with jitter, base 500 ms, cap 60 s; a crash-loop breaker trips after 5 restarts/5 min and parks the adapter in `Faulted` — visible as `adapter.health.changed`, never a silent retry storm. Health states: `Starting → Healthy → Degraded → Faulted`, all on the bus, because an invisible supervisor is a failed supervisor.

**Trait seams (shape BINDING, signatures DEFAULT):**

```rust
#[async_trait]
pub trait TerminalSubstrate: Send + Sync {
    async fn ensure_workspace(&self, spec: &WorkspaceSpec) -> Result<SessionMap>;
    async fn spawn_pane(&self, ws: &WorkspaceId, cmd: Command) -> Result<PaneId>;
    async fn send_keys(&self, pane: &PaneId, input: &[u8]) -> Result<()>;
    async fn subscribe_lifecycle(&self) -> Result<EventStreamHandle>; // facts only; bytes stay out-of-band (I2)
}

#[async_trait]
pub trait AgentSubstrate: Send + Sync {
    async fn spawn(&self, spec: &AgentSpec, badge: Badge) -> Result<AgentRunId>;
    async fn signal(&self, run: &AgentRunId, sig: AgentSignal) -> Result<()>;
    async fn status(&self, run: &AgentRunId) -> Result<AgentStatus>;
}

#[async_trait]
pub trait RepoSubstrate: Send + Sync {
    async fn alloc_worktree(&self, req: WorktreeReq) -> Result<Worktree>;
    async fn diff_summary(&self, wt: &WorktreeId) -> Result<CasRef>;
}
```

**herdr adapter (Phase 1).** Speaks herdr's JSON socket (Unix socket; named pipe on Windows) for workspace/tab/pane creation, spawn, and state subscription; falls back to the CLI via `HERDR_BIN_PATH` for verbs the socket lacks. Version-gated hello: the adapter records herdr's version and refuses semver-major jumps until its contract tests (recorded socket transcripts, §15) pass against the new version. Confidence that the socket covers Slice 1's needs: moderate-high from documented API surface; the contract-test harness exists precisely because herdr ships weekly.

**git adapter (Phase 1).** Reads via `gix` (status, branch, diff summaries — high confidence in crate maturity for read paths); mutations via the `git` CLI (`worktree add/remove`, fetch, merge) because the CLI is the compatibility truth; filesystem events via `notify`, debounced 250 ms (DEFAULT). Owns the **worktree registry**, which resolves the two-allocators problem: herdr mints worktrees, Omnigent's delegation mints worktrees, and rezidnt itself allocates on `open`. Rule (BINDING): every worktree is registered under its canonicalized path with an `allocator` field; a second claim on the same path emits `worktree.conflict` rather than silently double-tracking. rezidnt is *allocator* for its own spawns and *observer/normalizer* for substrate-minted trees.

**Omnigent adapter (Phase 2).** Two distinct values, kept separate: (a) a live adapter to an Omnigent server over its WebSocket, pinned to an exact alpha version — integration value is real but churn risk is high, so it ships feature-flagged and never on the golden path; (b) **donor value**: Omnigent's Apache-2.0 harness adapters (Claude Code, Codex, Cursor spawn/telemetry logic) are ported into `rezidnt-adapters/omnigent` as the *reference implementation* for our native `AgentSubstrate` impls, with NOTICE preservation. The port pattern is the Bun pattern: reference implementation + our contract tests as the oracle. Their Python never runs in our process.

**Arranger adapter (optional, feature-flagged).** Windows window placement delegates to komorebi over its IPC/CLI (high confidence it exists; moderate on current IPC details — verify at implementation). Never hand-roll `SetWindowPos` choreography. Absent komorebi, the adapter is a no-op that still emits layout-intent events for future clients.

## 8. Gate and verifier engine

This is the differentiation layer; over-invest here, under-invest everywhere else.

**Model.** A **Gate** is a named policy point bound to a lifecycle transition: `vet` (pre-spawn: is this agent spec, badge scope, and workspace state acceptable?), `pre_merge` (is this diff verified?), `post_run` (`debrief`: what did the agent actually do, and does the evidence support the claim?). A **Verifier** is a deterministic check attached to a gate. Verdicts are `pass | fail | inconclusive` — never a bare boolean, because `inconclusive` routed to a human is honest and `fail` faked as `pass` is the product's death.

**Two verifier kinds (BINDING).** *Native* verifiers implement a Rust trait (built-ins: diff-scope check, test-suite runner, forbidden-path touch, secret-leak scan, build-passes). *Exec* verifiers are any argv program speaking a JSON contract — this is the polyglot seam and the ecosystem play; a Roslyn analyzer pack, a Bun script, and a Python linter all plug in identically:

```json
// stdin
{ "gate": "pre_merge", "workspace": "…", "refs": { "diff": "cas:blake3:…", "transcript": "cas:blake3:…" },
  "params": { … }, "timeout_ms": 120000 }
// stdout
{ "verdict": "fail", "evidence": [ { "kind": "finding", "msg": "test regression: auth::login", "ref": "cas:…" } ],
  "cost_ms": 8412 }
```

**Determinism requirements (BINDING):** inputs are pinned by content hash (verifiers receive CAS refs, not mutable paths); no network by default (exec verifiers run with network disabled unless the gate def opts in, recorded in the event); wall-clock timeout 120 s DEFAULT; nonzero exit or malformed output = `inconclusive`, never `pass`. Evidence blobs go to the CAS; `gate.passed|failed` events carry refs.

**Interrogability (I6, the AX feature no one ships):** `rezidnt gate why <run>` and the MCP tool `gate_explain` return the failing verifier, its evidence refs, and the exact inputs — so a blocked agent can read *why* it was blocked and fix the actual defect instead of thrashing against a refusal string. **Replay:** `rezidnt debrief <session>` re-executes recorded verdicts from log + CAS; divergence between the recorded and replayed verdict raises an integrity alarm (either the verifier was nondeterministic — a verifier bug — or the log was altered — see §12). This replay property is what makes the audit trail *evidence* rather than assertion, and it is the sentence that gets a compliance reviewer to sign.

**Composition with Omnigent, stated once for positioning:** their policies gate *permissions* before the act; rezidnt gates *evidence* after it. The models compose; only one of them survives an auditor.

Verifier packs are separate crates/repos. The generic packs are open; domain judgment packs (DXP/Microsoft-stack failure modes) are the commercial seam (§17) and stay out of this repository entirely.

## 9. Command surfaces

**MCP (primary, per I5).** Server via the official Rust SDK (`rmcp` — moderate confidence on crate name and API maturity; verify at Slice 3 and be prepared to write a thin JSON-RPC layer if it disappoints). Transports: stdio (spawned by local clients like Claude Code) and streamable HTTP on `127.0.0.1` (DEFAULT port 0/announced via lockfile, not a fixed port). Resources: workspace graph nodes, dossiers, event ranges by ULID, gate definitions. Tools: `open_project`, `spawn_agent`, `vet`, `debrief`, `gate_explain`, `tail_events`, `alloc_worktree`, `arrange_layout` (PROVISIONAL). Every tool is idempotent or carries an idempotency key; every tool's JSON Schema is generated from `rezidnt-types` via `schemars`, so the MCP surface and the npm-published types can never drift.

**Socket protocol.** UDS at `$XDG_RUNTIME_DIR/rezidnt.sock` (fallback `~/.local/state/rezidnt/`); Windows named pipe `\\.\pipe\rezidnt`. JSONL frames; first frame is a versioned hello `{proto: 1, schema: <ontology hash>, daemon: <semver>}`; mismatched proto majors disconnect with a machine-readable upgrade hint. The CLI, TUI, and future Tauri client are all just consumers of this protocol — no privileged in-process clients (I1).

**CLI.** `rezidnt open <repo|spec>` (materialize), `status`, `tail [--subject …]`, `vet <agent-spec>`, `debrief <session|run>`, `gate why <run>`, `rebuild`, `doctor`, `spec init`. Global `--json` on every verb; stable exit codes (0 ok, 2 gate-fail, 3 substrate-fault, 4 daemon-unreachable). Lore vocabulary stops at `vet`, `debrief`, and `dossier` (BINDING) — everything else stays boring on purpose.

## 10. Data plane

PTY bytes never touch the fabric (I2). In Phase 1, interactive output stays entirely inside herdr (its clients render it); rezidnt consumes only lifecycle facts and, on gate events, captured artifacts. Artifacts — diffs, transcripts, verifier evidence, build logs — land in a content-addressed store at `~/.local/share/rezidnt/cas/<blake3-hex>` (blake3 DEFAULT: fast, incremental-friendly), written once, referenced by `CasRef { hash, bytes, mime }` in events. GC is reachability-from-log and PROVISIONAL — with the log retained forever, unreachable blobs are rare by construction. Phase 3's owned terminal substrate will route PTY streams substrate→client over dedicated pipes/shared memory with the daemon carrying only chunk manifests; that design lands with Phase 3 and is deliberately unspecified here.

## 11. Cross-platform topology

Linux and macOS are native and boring. Windows is the wedge and gets explicit treatment:

**Phase 1 topology (DEFAULT):** the daemon and all substrates (herdr, git worktrees, agents) run inside WSL2; Windows-side clients (CLI.exe, Claude Code on Windows) reach the daemon over loopback TCP, because AF_UNIX sockets do not bridge the Windows/WSL2 boundary (moderate-high confidence) and herdr's native Windows builds are preview-beta. Path canonicalization is a first-class concern: every workspace records its dual mapping (`/mnt/c/...` ↔ `C:\...`) at registration, and all events carry the canonical (WSL) form with translation at the client edge.

**Phase 3 differentiation:** native-Windows daemon with ConPTY as a first-class citizen via `portable-pty`, named-pipe transport, komorebi arrangement, Entra-conscious enterprise packaging. This is the "nobody owns Windows" wedge from the strategy record; it is *sequenced after* the fabric proves itself in WSL2, not before.

## 12. Security and audit posture

Trust boundary: one user on one machine. Socket at mode 0600; named pipe ACL'd to the current user. **Badges** (the one lore term in the security model): per-`AgentRun` capability tokens — 256-bit random, scoped to `{workspace, verb set, expiry}` — required on every mutating socket/MCP call. DEFAULT is opaque bearer checked by the daemon; macaroon-style attenuation is PROVISIONAL and waits for a real delegation use case. The point in Phase 1 is not cryptographic ceremony; it is that *an agent's writes are attributable to that agent* in the log, which is what makes the dossier meaningful.

Secret hygiene: fabric ingress runs a redaction pass (denylist patterns: cloud keys, PATs, connection strings) before append; exec verifiers run with a scrubbed environment. Log integrity: the `chain` column (blake3 over previous chain + id + payload) makes the log tamper-evident at near-zero cost; `doctor` re-walks it. DEFAULT on. No telemetry (I7). Threats explicitly out of scope for now, documented so nobody pretends otherwise: hostile local root, malicious verifier binaries installed by the operator themselves, and multi-tenant isolation.

## 13. Configuration and the project spec

Global config `~/.config/rezidnt/rezidnt.toml` (Windows: `%APPDATA%\rezidnt\`): substrate paths, default gates, arranger toggle. Per-project spec — the file `rezidnt open` materializes from — borrows herdr-plus's proven TOML semantics (independently reimplemented; I8 does not apply to a config shape, but keep it clean anyway):

```toml
[project]
name = "acme-checkout"
repo = "."

[[workspace.tab]]
name = "build"
panes = [{ cmd = "just watch" }, { cmd = "just test --watch" }]

[[agent]]
name = "impl"
harness = "claude-code"          # AgentSubstrate impl
worktree = "auto"                 # allocator: rezidnt
gates = ["vet", "pre_merge"]

[gates.pre_merge]
verifiers = [
  { native = "tests-pass" },
  { exec = "verifiers/scope-check", params = { allow = ["src/checkout/**"] } },
]
```

`rezidnt spec init` generates this interactively; the golden path must work with the generated file untouched.

## 14. Observability

`tracing` + `tracing-subscriber` throughout; WARN and above are mirrored onto the fabric as `daemon.*` events so the system's own misbehavior is queryable with the same tools as everything else. `rezidnt doctor` checks: socket reachability, SQLite `integrity_check` + WAL state, chain verification, adapter health, substrate versions against tested ranges, CAS disk headroom. A `--smoke` self-benchmark (spawn a null agent through a null gate, assert end-to-end latency) is PROVISIONAL.

## 15. Testing, oracles, and the benchmark seam

Ordered by the oracle principle that sequences the whole project — build where a deterministic judge exists:

**Reducer determinism (Phase 1's oracle):** property tests assert `fold(log) == snapshot` under arbitrary event interleavings and that rebuild equals live state; golden log fixtures live in-repo and every release replays them. **Adapter contracts:** herdr and Omnigent adapters are tested against *recorded* socket/WebSocket transcripts (record/replay harness in `rezidnt-supervise` test utils); a substrate version bump that breaks the recording blocks the adapter, not the daemon. **Verifier conformance:** a suite that feeds exec verifiers malformed input, timeouts, and nondeterminism traps, asserting `inconclusive`-not-`pass` behavior. **Phase 3 oracles:** vttest/esctest conformance plus grid-snapshot comparison against a reference emulator, per the phase plan. **The benchmark (GTM-grade testing):** the harness is public in `bench/harness`; the held-out case set is private, full stop — you know exactly what contamination does to a measuring stick. Metrics locked now so the category argument is ours: orchestrated task completion rate, gate precision/recall against labeled defects, worktree merge success rate, cost per merged verified diff.

## 16. Phased roadmap and slice acceptance criteria

Estimates are mine, part-time-founder calibrated, moderate confidence, wide intervals dominated by your available hours.

**Phase 1 — the fabric (4–8 weeks).**
*S0 (2–3 days):* ontology v0 + envelope + log + broadcast + `rezidnt tail`. Accept: two concurrent subscribers; `kill -9` the daemon mid-stream; restart; `rebuild` reproduces identical graph state; chain verifies.
*S1:* herdr adapter + `rezidnt open` materialization from spec. Accept: golden path on a clean VM in ≤ 5 minutes with zero config edits; every materialization step visible in `tail`.
*S2:* git adapter. Accept: `diff.ready` within 1 s of write (post-debounce); two-allocator worktree test produces exactly one registry entry and one `worktree.conflict` on deliberate collision.
*S3:* MCP surface. Accept: Claude Code, using MCP only, opens a project, spawns an agent, reads its dossier, and receives a `gate_explain` for a forced failure. **Phase 1 exit = golden path demo, one take, recorded.**

**Phase 2 — harness and gates (6–12 weeks).** Port Omnigent's Claude Code harness adapter as the native `AgentSubstrate` reference (donor pattern, NOTICE); verifier engine v1 with the built-in native pack + exec contract; `vet` and `pre_merge` live on the golden path. *S4 accept:* an agent spawned under rezidnt gates produces a verified merged diff with replayable `debrief`. Phase 2 exit = the benchmark harness runs end-to-end against rezidnt itself.

**Phase 3 — owned terminal substrate (3–6 months to herdr-2024 parity).** Assemble the permissive kernel — libghostty-vt (MIT, high confidence) or `alacritty_terminal`/`vte` (Apache-2.0/MIT family, moderate — verify at kickoff) + `portable-pty` (WezTerm lineage, high) — then the multiplexer, persistence, and agent-detection layer as a `TerminalSubstrate` impl behind the same trait herdr sits behind today. *S5 (can precede Phase 3):* ratatui read-only fleet board consuming only watch channels — the proof that I1 held. Phase 3 exit = golden path runs with `substrate = "native"` and herdr uninstalled.

Sequencing law, restated because it is the project's most-violated invariant in the wild: **fabric → harness → terminal.** Any pressure to reorder is scope gravity and gets the phase-exit-demo test applied to it.

## 17. Repository, licensing, and the commercial seam

Public repo `rezidnt/rezidnt` from first push (post-memo): Apache-2.0 at root, `MIT OR Apache-2.0` on `crates/*` libs, DCO enforced, NOTICE carrying Omnigent attributions for ported code, TRADEMARKS.md (mark owned by TwofoldTech LLC), SECURITY.md, CONTRIBUTING.md. Excluded from this repo permanently: `rezidnt-enterprise` (RBAC/SSO/audit-export, hosted control plane if ever), domain verifier judgment packs, the benchmark held-out set, and anything employer-affiliated pending the memo. The seam is structural — separate repos, separate crates — so a future dual-license or paid tier requires zero untangling, exercised or not.

## 18. Risk register

| Risk | Signal | Mitigation already in the architecture |
|---|---|---|
| herdr API churn or adversarial turn | weekly releases; maintainer's "agent runtime" ambitions | trait seam (I4), recorded contract tests, version-gated hello, Phase 3 replacement path |
| Omnigent alpha churn | breaking WS/API changes | pinned version, feature flag, donor-not-depend posture |
| Databricks ships server MCP + RBAC | their stated roadmap | differentiation is evidence-gates + local-first + Windows, none of which their model rewards |
| First-party absorption (Anthropic/OpenAI orchestration) | Claude Code teams features | cross-vendor governance seat; rezidnt orchestrates *their* harnesses |
| AGPL contamination | any herdr code in-tree | I8 + CI denylist; IPC-only integration |
| Scope gravity | building layer N+1 before N has users | phase-exit demos are the only definition of done |
| Solo-founder bus factor | — | boring tech, spec/ontology as versioned docs, golden fixtures |
| Name fails a registry check | tonight's lookups | fallback string `rezident`, conventions unchanged |

## 19. Open decisions register

`rmcp` maturity (verify at S3; fallback: thin hand-rolled JSON-RPC). SQLite write amplification (revisit only past 10⁴ events/min). WASM verifiers as a third kind (post-Phase 2; exec contract already covers polyglot). Macaroon-attenuated badges (needs a real delegation use case). Komorebi IPC depth (verify at arranger implementation). First graphical client: ratatui board is S5; Tauri dashboard is demand-gated. Hash-chain externalization (periodic chain-head publication for third-party timestamping) — PROVISIONAL, compliance-driven.

---

## Appendix A — dependency table

| Crate | Purpose | License | Confidence |
|---|---|---|---|
| tokio | async runtime, channels | MIT | high |
| serde / serde_json | envelope + payloads | MIT/Apache | high |
| rusqlite | event log, WAL | MIT | high |
| ulid | event ids | MIT | high |
| schemars | JSON Schema export → npm types | MIT | high |
| gix | git read paths | MIT/Apache | high (reads) |
| notify | FS watching | CC0/Artistic-2.0 family | moderate — verify license detail |
| blake3 | CAS + chain | CC0/Apache | high |
| tracing | observability | MIT | high |
| clap | CLI | MIT/Apache | high |
| rmcp | MCP server SDK | Apache (expected) | **moderate — verify at S3** |
| interprocess or tokio-net | UDS + named pipes | MIT-family | moderate — pick at S0 |
| portable-pty | Phase 3 PTY (incl. ConPTY) | MIT | high |
| libghostty-vt (FFI) or alacritty_terminal | Phase 3 VT kernel | MIT / Apache-2.0 | high / moderate — verify |
| ratatui | S5 TUI board | MIT | high |

## Appendix B — subject taxonomy v0 (excerpt; canonical copy lives in `spec/ontology.md`)

```
workspace.opened | workspace.closed | workspace.spec.applied
worktree.allocated | worktree.observed | worktree.conflict | worktree.released
session.created | session.attached | pane.spawned | pane.exited
agent.spawned | agent.status.changed | agent.blocked | agent.completed | agent.signaled
gate.entered | gate.passed | gate.failed | gate.inconclusive | gate.explained
artifact.captured                  # {ref, mime, bytes, provenance}
diff.ready | merge.completed | merge.rejected
adapter.health.changed | daemon.started | daemon.warning | daemon.error
badge.issued | badge.revoked
```

*End of v0.1. Amendments to BINDING items require a dated decision record appended below this line.*

---

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

---

# Decision Record DR-002 — Prior-art protocol for competitor sources

**Date:** 2026-07-04 · **Status:** ACCEPTED 2026-07-16 (owner) · **Amends:** DR-001 clean-room rule — tightens it; reverses nothing.

## Context

Concern raised: using Omnigent as a "donor" — even read-only — anchors rezidnt's design on Databricks' frame ("plan poisoning"). Assessment: **legal** contamination from Apache-2.0 source is near-zero absent copying (permissive license; no obligations attach to reading; patent exposure is orthogonal to reading, since independent invention is not a patent defense — confirm with counsel). **Cognitive** anchoring is a real mechanism, demonstrated by v0.1 inheriting herdr's terminal frame *without anyone reading herdr source* — the vector was the integration plan and product osmosis, not code access. The mitigation is sequencing and traceability, not ignorance.

## Rules

1. **Design-first.** No competitor source is opened until the corresponding rezidnt design is committed in writing. (Satisfied for fabric, traits, run substrate, and gate model as of v0.2 + DR-001.)
2. **Extraction-scoped reads.** Every competitor read begins with written questions and ends with a findings memo in `/intel/`. Memos feed the benchmark, the risk register, and positioning — never trait or ontology definitions directly.
3. **Traceable influence.** Any design change motivated by an intel memo requires its own DR citing that memo. Anchoring is made *auditable*, not pretended away.
4. **Copyleft unchanged.** herdr and all AGPL sources are never read for implementation purposes (DR-001 stands in full).
5. **Implementation oracles are primary sources.** Harness vendors' own documentation plus recorded-transcript contract tests. Omnigent code is never an implementation reference for any rezidnt component.
6. **Benchmark exception.** Installing, running, and scoring competitor binaries is unrestricted — a benchmark cannot be built blind, and black-box behavior is not source.
7. **Post-freeze gap-diff.** The single sanctioned design-adjacent use: after ontology v1 freezes, one scoped read of Omnigent's event/policy taxonomy to diff for *coverage gaps* ("do they handle a lifecycle fact we lack a subject for?"). Output is a memo per rule 2; additions require a DR per rule 3.

*Amendments to this record require DR-003.*
