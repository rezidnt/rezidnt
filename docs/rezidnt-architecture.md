# rezidnt — Architecture Plan

**Version:** 0.1 (pre-code) · **Owner:** TwofoldTech LLC · **License posture:** Apache-2.0 (see §17)
**Status flags used throughout:** **BINDING** (change requires a written decision record), **DEFAULT** (best current call; cheap to revisit), **PROVISIONAL** (expected to change; do not build against).

> Standing gate, restated once: the name ships only after registry checks pass (`rezident` is the fallback string). Nothing in this document overrides it. (A second gate — repository privacy pending an employer IP memo — was retired by DR-003.)

---

## 1. Thesis and scope

> **Amended by [DR-008](decisions/DR-008-permit-engine-pivot.md).** The compose-only framing is dropped: rezidnt takes the pre-hoc "may" axis natively via a permit engine; Omnigent becomes a baseline/adapter, not a required companion.

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

> **I8 was rewritten by [DR-001](decisions/DR-001-native-substrates.md#invariant-i8-rewritten-binding).** The AGPL-firewall wording below is superseded v0.1 text; the current invariant is the clean-room rule in the DR.

**I8 — AGPL firewall.** herdr is consumed strictly as a separate process over its socket/CLI. No linking, no vendoring, no source consultation for implementation. CI enforces a denylist (no `herdr` source in-tree). Omnigent (Apache-2.0) may be *ported* with NOTICE preservation; herdr may not.

## 3. System topology

> **Amended by [DR-001](decisions/DR-001-native-substrates.md).** The native run substrate replaces the herdr-integrated topology described below.

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

> **Amended by [DR-001](decisions/DR-001-native-substrates.md).** `rezidnt-run` replaces `rezidnt-adapters/herdr`; the current crate set is the [repository map](../README.md#repository-map).

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

> **Amended by [DR-001](decisions/DR-001-native-substrates.md) and [DR-007](decisions/DR-007-release-worktree.md).** `TerminalSubstrate` is deferred to Phase 3, `AgentSubstrate` absorbs spawn-with-PTY, and `RepoSubstrate` is the three-method form in DR-007 — not the sketch below. The herdr and Omnigent adapters below are superseded by the native substrates.

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

> **Amended by [DR-006](decisions/DR-006-replay-divergence-signal.md).** Replay divergence now lands a durable `integrity.alarm` fact on the log, not a CLI-only report.
>
> **Amended by [DR-008](decisions/DR-008-permit-engine-pivot.md).** A fourth lifecycle point `permit` joins vet/pre_merge/post_run; permit-verifiers make the gate engine the policy engine (pass→allow, fail→deny, inconclusive→escalate).

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

> **The permit-config resolution seam (`permit_config_for`) is amended by [DR-011](decisions/DR-011-permit-pdp-config-seam.md) (the seam itself) and [DR-020](decisions/DR-020-sp4c-wire-layered-permit-sourcing.md) (three-source admin/dev/session layering, admin sourced outside the workspace spec).**
>
> **Spend (C1) — amended by [DR-021](decisions/DR-021-live-spend-cap-c1.md):** the `SpendCap` verifier becomes live (caps injected via this seam), and spend attribution moves OFF the pre-action permit decision fact onto a post-action metering fact (B2, I3-honest) — so `spend_delta_usd` retires from the `permit.*` reducer fold source. Implies a downstream warden `/subject` (`action.metered`).

**MCP (primary, per I5).** Server via the official Rust SDK (`rmcp` — moderate confidence on crate name and API maturity; verify at Slice 3 and be prepared to write a thin JSON-RPC layer if it disappoints). Transports: stdio (spawned by local clients like Claude Code) and streamable HTTP on `127.0.0.1` (DEFAULT port 0/announced via lockfile, not a fixed port). Resources: workspace graph nodes, dossiers, event ranges by ULID, gate definitions. Tools: `open_project`, `spawn_agent`, `vet`, `debrief`, `gate_explain`, `tail_events`, `alloc_worktree`, `arrange_layout` (PROVISIONAL). Every tool is idempotent or carries an idempotency key; every tool's JSON Schema is generated from `rezidnt-types` via `schemars`, so the MCP surface and the npm-published types can never drift.

**Socket protocol.** UDS at `$XDG_RUNTIME_DIR/rezidnt.sock` (fallback `~/.local/state/rezidnt/`); Windows named pipe `\\.\pipe\rezidnt`. JSONL frames; first frame is a versioned hello `{proto: 1, schema: <ontology hash>, daemon: <semver>}`; mismatched proto majors disconnect with a machine-readable upgrade hint. The CLI, TUI, and future Tauri client are all just consumers of this protocol — no privileged in-process clients (I1).

**CLI.** `rezidnt open <repo|spec>` (materialize), `status`, `tail [--subject …]`, `vet <agent-spec>`, `debrief <session|run>`, `gate why <run>`, `rebuild`, `doctor`, `spec init`. Global `--json` on every verb; stable exit codes (BINDING, ratified by DR-004): **0** ok · **1** unexpected internal error · **2** local input/usage error (clap convention; daemon never reached) · **3** substrate fault, including daemon-side refusals · **4** daemon unreachable · **5** gate-fail (`vet`/`debrief`/`pre_merge` verdict `fail`; `inconclusive` is NOT 5 — it is 3, never coerced toward pass or fail, per I6). Lore vocabulary stops at `vet`, `debrief`, and `dossier` (BINDING) — everything else stays boring on purpose.

## 10. Data plane

PTY bytes never touch the fabric (I2). In Phase 1, interactive output stays entirely inside herdr (its clients render it); rezidnt consumes only lifecycle facts and, on gate events, captured artifacts. Artifacts — diffs, transcripts, verifier evidence, build logs — land in a content-addressed store at `~/.local/share/rezidnt/cas/<blake3-hex>` (blake3 DEFAULT: fast, incremental-friendly), written once, referenced by `CasRef { hash, bytes, mime }` in events. GC is reachability-from-log and PROVISIONAL — with the log retained forever, unreachable blobs are rare by construction. Phase 3's owned terminal substrate will route PTY streams substrate→client over dedicated pipes/shared memory with the daemon carrying only chunk manifests; that design lands with Phase 3 and is deliberately unspecified here.

## 11. Cross-platform topology

Linux and macOS are native and boring. Windows is the wedge and gets explicit treatment:

**Phase 1 topology (DEFAULT):** the daemon and all substrates (herdr, git worktrees, agents) run inside WSL2; Windows-side clients (CLI.exe, Claude Code on Windows) reach the daemon over loopback TCP, because AF_UNIX sockets do not bridge the Windows/WSL2 boundary (moderate-high confidence) and herdr's native Windows builds are preview-beta. Path canonicalization is a first-class concern: every workspace records its dual mapping (`/mnt/c/...` ↔ `C:\...`) at registration, and all events carry the canonical (WSL) form with translation at the client edge.

**Phase 3 differentiation:** native-Windows daemon with ConPTY as a first-class citizen via `portable-pty`, named-pipe transport, komorebi arrangement, Entra-conscious enterprise packaging. This is the "nobody owns Windows" wedge from the strategy record; it is *sequenced after* the fabric proves itself in WSL2, not before.

## 12. Security and audit posture

> **Amended by [DR-005](decisions/DR-005-badge-consolidation.md).** The badge requirement is narrowed to *state-mutating* calls; interrogation (`gate_explain`) and `tail_events` are read-class and unbadged.

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

> **Amended by [DR-006](decisions/DR-006-replay-divergence-signal.md).** Integrity divergence at replay is mirrored onto the fabric as a durable fact.

`tracing` + `tracing-subscriber` throughout; WARN and above are mirrored onto the fabric as `daemon.*` events so the system's own misbehavior is queryable with the same tools as everything else. `rezidnt doctor` checks: socket reachability, SQLite `integrity_check` + WAL state, chain verification, adapter health, substrate versions against tested ranges, CAS disk headroom. A `--smoke` self-benchmark (spawn a null agent through a null gate, assert end-to-end latency) is PROVISIONAL.

## 15. Testing, oracles, and the benchmark seam

> **Amended by [DR-022](decisions/DR-022-benchmark-harness-slice.md).** Slices the benchmark into an in-repo `bench/harness` collating three of the four locked metrics (task-completion, merge success, cost-per-verified-diff — all log-derived); gate precision/recall is structurally fenced behind the permanently-external held-out set (§17), exposed only as a seam that returns `inconclusive` when no labeled set is supplied.

Ordered by the oracle principle that sequences the whole project — build where a deterministic judge exists:

**Reducer determinism (Phase 1's oracle):** property tests assert `fold(log) == snapshot` under arbitrary event interleavings and that rebuild equals live state; golden log fixtures live in-repo and every release replays them. **Adapter contracts:** herdr and Omnigent adapters are tested against *recorded* socket/WebSocket transcripts (record/replay harness in `rezidnt-supervise` test utils); a substrate version bump that breaks the recording blocks the adapter, not the daemon. **Verifier conformance:** a suite that feeds exec verifiers malformed input, timeouts, and nondeterminism traps, asserting `inconclusive`-not-`pass` behavior. **Phase 3 oracles:** vttest/esctest conformance plus grid-snapshot comparison against a reference emulator, per the phase plan. **The benchmark (GTM-grade testing):** the harness is public in `bench/harness`; the held-out case set is private, full stop — you know exactly what contamination does to a measuring stick. Metrics locked now so the category argument is ours: orchestrated task completion rate, gate precision/recall against labeled defects, worktree merge success rate, cost per merged verified diff.

## 16. Phased roadmap and slice acceptance criteria

> **Superseded by [DR-001](decisions/DR-001-native-substrates.md#roadmap-supersedes-16).** The current roadmap lives in the DR; the slices below are the pre-DR-001 (herdr-integrated) plan, kept for provenance.
>
> **Amended by [DR-008](decisions/DR-008-permit-engine-pivot.md).** Adds a permit-engine phase (SP0–SP5) between gates (Phase 2) and terminal fidelity (Phase 3).
>
> **Amended by [DR-009](decisions/DR-009-match-omnigent-scope.md).** Folds four memo-001 capabilities into the permit phase — spend/rate limits (C1→SP1), intent-lock (C7→new SP-intent), layered admin/dev/session precedence (C8→SP4) — and adds a distinct later sole-chokepoint enforcement phase (C3), fenced behind its own design + DR.
>
> **Amended by [DR-022](decisions/DR-022-benchmark-harness-slice.md).** Gives the Phase-2 exit ("the benchmark harness runs end-to-end against rezidnt itself") its first defined acceptance criteria + exit demo — a headless in-repo dogfood harness over the completed S4 golden path.
>
> **Amended by [DR-025](decisions/DR-025-c3a-linux-sandbox.md).** Slices the DR-009 sole-chokepoint (C3) phase: **C3a** — a Linux `bwrap`-backed `SandboxSubstrate` (I4) wrapping the S1 spawn seam, confinement from folded policy, loud `sandbox.unavailable` degrade when absent — ships first with its own acceptance criteria + exit demo. C3b (egress proxy), C3c (credential brokering), and the macOS/Windows backends stay fenced, each behind its own DR; the Windows tier is coupled to the deferred native-Windows Platform phase.

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

> **Amended by [DR-003](decisions/DR-003-retire-ip-memo-gate.md).** The employer-IP-memo gate is retired. Per [DR-001](decisions/DR-001-native-substrates.md), no third-party code is ported, so the NOTICE obligation described below does not arise.
>
> **Amended by [DR-022](decisions/DR-022-benchmark-harness-slice.md).** The permanently-excluded benchmark held-out set (below) is recorded as a slice-level boundary: gate precision/recall cannot be computed in-repo, so `bench/harness` carries only the seam and the labeled data lives external forever.

Public repo `rezidnt/rezidnt` from first push: Apache-2.0 at root, `MIT OR Apache-2.0` on `crates/*` libs, DCO enforced, NOTICE carrying Omnigent attributions for ported code, TRADEMARKS.md (mark owned by TwofoldTech LLC), SECURITY.md, CONTRIBUTING.md. Excluded from this repo permanently: `rezidnt-enterprise` (RBAC/SSO/audit-export, hosted control plane if ever), domain verifier judgment packs, and the benchmark held-out set. The seam is structural — separate repos, separate crates — so a future dual-license or paid tier requires zero untangling, exercised or not.

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

## 20. Decision records

BINDING items change only through a dated decision record. Records live one per file under [`docs/decisions/`](decisions/), numbered sequentially; each closes by naming the number its own future amendment must take. This catalog is the index — **where a section above is marked "amended by DR-00N", the plan's text is no longer the whole truth until you have read that record.**

| DR | Title | Status | Amends |
|---|---|---|---|
| [DR-001](decisions/DR-001-native-substrates.md) | Native substrates from day one | ACCEPTED | §1, §3, §4, §7, I8, §16, §18 |
| [DR-002](decisions/DR-002-prior-art-protocol.md) | Prior-art protocol for competitor sources | ACCEPTED | DR-001 (tightens) |
| [DR-003](decisions/DR-003-retire-ip-memo-gate.md) | Retire the employer IP memo standing gate | ACCEPTED | preamble, §17 |
| [DR-004](decisions/DR-004-exit-code-table.md) | Stable exit-code table | ACCEPTED | §9 |
| [DR-005](decisions/DR-005-badge-consolidation.md) | Badge model consolidation | ACCEPTED | §12 |
| [DR-006](decisions/DR-006-replay-divergence-signal.md) | Replay-divergence integrity signal | ACCEPTED | §8, §14 |
| [DR-007](decisions/DR-007-release-worktree.md) | RepoSubstrate `release_worktree` as-built | ACCEPTED | §7 |
| [DR-008](decisions/DR-008-permit-engine-pivot.md) | Permit-engine pivot (rezidnt owns both axes) | ACCEPTED | §1, §8, §16 |
| [DR-009](decisions/DR-009-match-omnigent-scope.md) | Match-Omnigent scope (four memo-surfaced permit capabilities) | ACCEPTED | §16 |
| [DR-010](decisions/DR-010-intent-lock-scope.md) | SP-intent scope + criteria (C7 intent-lock) | ACCEPTED | §16 |
| [DR-011](decisions/DR-011-permit-pdp-config-seam.md) | Permit PDP config-resolution seam (McpSubstrate method) | ACCEPTED | §8, §9 |
| [DR-012](decisions/DR-012-empty-vs-absent-intent.md) | Declared-empty vs absent intent allowlist (intent-lock) | ACCEPTED | §16 |
| [DR-013](decisions/DR-013-permit-pep-sp2-integration.md) | Permit PEP integration + fail-posture (SP2) | ACCEPTED | §9, §16 |
| [DR-014](decisions/DR-014-permit-pep-hook.md) | Permit PEP hook binary + spawn injection (SP2 hook sub-slice) | ACCEPTED | §9, §16 |
| [DR-015](decisions/DR-015-permit-exec-verifier.md) | Permit axis dispatches exec verifiers (SP3 bring-your-own policy DSL) | ACCEPTED | §8, §9, §16 |
| [DR-016](decisions/DR-016-permit-roles-sp4-slicing.md) | SP4 slicing + SP4a roles scope | ACCEPTED | §8, §9, §16 |
| [DR-017](decisions/DR-017-permit-macaroon-delegation-sp4b.md) | SP4b macaroon-attenuated delegation (crypto + dep choice) | ACCEPTED | §12, §16 |
| [DR-018](decisions/DR-018-delegation-edge-id-and-offline-boundary.md) | Delegation edge id from the running sig + offline-boundary defer (SP4b) | ACCEPTED | §12, §16 |
| [DR-019](decisions/DR-019-c8-layered-precedence-sp4c.md) | SP4c: C8 layered policy precedence via monotone concat | ACCEPTED | §8, §9, §16 |
| [DR-020](decisions/DR-020-sp4c-wire-layered-permit-sourcing.md) | SP4c-wire: three-source layered permit wiring (admin outside the workspace spec) | ACCEPTED | §9, §16 |
| [DR-021](decisions/DR-021-live-spend-cap-c1.md) | Live spend-cap (C1): spend measured post-action, off the permit fact (B2) | ACCEPTED | §8, §9 |
| [DR-022](decisions/DR-022-benchmark-harness-slice.md) | Benchmark-harness slice: in-repo three-metric dogfood, precision/recall fenced external | ACCEPTED | §15, §16, §17 |
| [DR-023](decisions/DR-023-shared-daemon-driving-client.md) | Extract shared `rezidnt-client` socket driver (unblocks DaemonDriver); fixtures stay dev-only test-support | ACCEPTED | §4 |
| [DR-024](decisions/DR-024-running-risk-cap-c6.md) | Running-risk cap (C6): deterministic rule-table scorer, pre-action, granted-only fold, contract-free shared-scorer seam | ACCEPTED | §8, §9 |
| [DR-025](decisions/DR-025-c3a-linux-sandbox.md) | C3a Linux OS-sandbox: bwrap-backed `SandboxSubstrate` (I4), folded-policy binds, loud degrade when absent, `PathConfinement` verdict stays a permit-verifier | ACCEPTED | §16, §18 |

*The next record is DR-026.*

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

*End of the plan body. BINDING changes are recorded as decision records — see [§20](#20-decision-records) and [`docs/decisions/`](decisions/).*
