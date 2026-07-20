//! rezidnt materialized state: pure reducers folding the event log into the
//! entity graph (CQRS-lite, doc §6). I3: the log is truth, this is derived —
//! the whole crate can be deleted and rebuilt from the log.
//!
//! ## S0 graph scope (deliberate)
//!
//! S0 materializes only what the *envelope itself* provides plus the
//! `workspace.*` lifecycle; payload-schema-driven entities (worktrees, agent
//! runs, dossiers) arrive with their slices (S1/S2) as additive fields. The
//! S0 reducer semantics pinned by the oracle tests and golden fixtures:
//!
//! - every event: `events_folded += 1`, `last_event = Some(event.id)`,
//!   `counts_by_subject[subject] += 1`;
//! - `workspace.opened` with an envelope workspace id: status → `Open`;
//! - `workspace.closed` with an envelope workspace id: status → `Closed`
//!   (inserted even if never opened — the log is truth);
//! - every other subject: counters only.

use std::collections::BTreeMap;

use rezidnt_types::{Event, Subject, WorkspaceId};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Workspace lifecycle status derived from `workspace.opened` / `.closed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceStatus {
    Open,
    Closed,
}

/// One gate's recorded verdict state on a run (S4 — the ORACLE SCAFFOLD:
/// fields exist so the board is assert-red; the reducer arm is implementer
/// work).
///
/// S4 reducer semantics (pinned by `tests/s4_gates.rs` and the
/// `s4_verified_run` / `s4_vet_refusal` golden fixtures; keyed under
/// `AgentRunState::gates` by the payload `gate` name, last write wins):
/// - `gate.entered` `{run, gate}` → `verdict = "entered"`;
/// - `gate.passed` `{run, gate, verifiers: [{verifier, cost_ms, evidence,
///   inputs}]}` → `verdict = "pass"`, `verifier = None`, `evidence` = the
///   verifiers' evidence hashes flattened in order;
/// - `gate.failed` `{run, gate, verifier, evidence, inputs}` →
///   `verdict = "fail"`, the FAILING verifier named, evidence hashes copied;
/// - `gate.inconclusive` (failed shape + `reason`) →
///   `verdict = "inconclusive"` plus `reason` — never coerced (I6).
///
/// Verdicts stay payload-strings (never an enum gate) for the same I3 reason
/// as statuses: reducers fold every live payload version, they never choke.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GateState {
    /// `"entered" | "pass" | "fail" | "inconclusive"` — recorded verbatim.
    pub verdict: String,
    /// The FAILING verifier's name (fail/inconclusive only).
    pub verifier: Option<String>,
    /// Evidence blob hashes (blake3 hex) — refs, never bytes (I2).
    #[serde(default)]
    pub evidence: Vec<String>,
    /// `gate.inconclusive` v1 `reason` (`timeout | nonzero_exit |
    /// malformed_output`), recorded verbatim.
    pub reason: Option<String>,
}

/// One replay-divergence alarm folded onto a run's dossier (DR-006 — the
/// ORACLE SCAFFOLD: the type + field exist so `tests/dr006_integrity_alarms.rs`
/// is assert-red, mirroring the S4 `GateState` scaffold; the reducer arm for
/// `integrity.alarm` is implementer work).
///
/// DR-006 reducer semantics (pinned by `tests/dr006_integrity_alarms.rs`):
/// `integrity.alarm` `{run, gate, verifier, recorded, replayed}` folds onto
/// `AgentRunState::integrity_alarms`, DEDUPED by (gate, verifier) — the log is
/// append-only and debrief is re-runnable, so duplicate facts collapse to one
/// queryable record. Deterministic order (by (gate, verifier)) keeps
/// whole-graph equality stable. Payload shape is an ORACLE PROPOSAL pending
/// the warden `/subject` for `integrity.alarm` (flagged in the work order);
/// verdicts stay payload-strings (never an enum gate) for the same I3 reason
/// as gate verdicts — reducers fold every live payload version.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntegrityAlarmRecord {
    /// The gate the diverging verifier ran under (`vet` | `pre_merge` | …).
    pub gate: String,
    /// The diverging verifier's name.
    pub verifier: String,
    /// Recorded verdict, verbatim (`pass` | `fail` | `inconclusive`).
    pub recorded: String,
    /// Replayed verdict, verbatim — divergence means `recorded != replayed`.
    pub replayed: String,
}

/// One entry in a run's permit ledger: an authorization request and, once a
/// decision lands, its outcome (DR-008/DR-009 — the pre-hoc "may" axis). SP5
/// REDUCER SCAFFOLD: the type + fields exist so the permit subjects have a
/// folding consumer (no consumer-less subjects — DR-006 precedent); the
/// contextual C1/C7 permit-verifiers that *read* this ledger are SP0–SP4.
///
/// Reducer semantics (keyed under [`AgentRunState::permit_ledger`] by the
/// payload `request_id`, so request and decision fold onto the same entry):
/// - `permit.requested` `{run, request_id, action, target, ...}` → insert with
///   `action` recorded, `decision = None` (pending);
/// - `permit.granted`/`permit.denied`/`permit.escalated`
///   `{run, request_id, ...}` → set `decision` to `"granted"`/`"denied"`/
///   `"escalated"` on the matching entry (created if the decision is seen
///   before the request — the log is truth, I3, never gatekeeps).
///
/// Decisions stay payload-strings (never an enum gate) for the same I3 reason
/// as gate verdicts — reducers fold every live payload version. `granted`=allow
/// (`pass`), `denied`=deny (`fail`), `escalated`=inconclusive→human (`ask`);
/// `escalated` is never coerced to `granted` (I6, DR-008 §4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PermitLedgerEntry {
    /// The requested action kind (`permit.requested.action`, e.g.
    /// `"tool.invoke"`), recorded verbatim.
    pub action: String,
    /// The decision, once one lands: `"granted" | "denied" | "escalated"`.
    /// `None` while the request is pending (requested but not yet decided).
    pub decision: Option<String>,
    /// The deciding policy's CAS hash (`policy_ref.hash`), recorded so
    /// `gate why` / `gate_explain` can resolve the deciding policy (I6). Set
    /// on the decision fact; `None` while pending.
    pub policy_ref: Option<String>,
    /// Denial / escalation reason, recorded verbatim (`permit.denied.reason`
    /// / `permit.escalated.reason`); `None` for grants and while pending.
    pub reason: Option<String>,
}

/// A run's per-session permit accumulators: the running state the *contextual*
/// (stateful) permit-verifiers read to make C6/C7 decisions — a pure fold over
/// the log (I3), not held imperatively as Omnigent does (intel memo 001 C6).
/// SP5 REDUCER SCAFFOLD: fields exist so the accumulator inputs on the permit
/// decision payloads have a folding consumer; the verifiers that read these
/// are SP0–SP4.
///
/// Reducer semantics: every `permit.granted`/`.denied`/`.escalated` fold adds
/// the payload's optional `spend_delta_usd` / `risk_delta` (when present) to
/// the running totals and increments the decision counters. Sized per the
/// intel-memo C6/C7 note so the payload supports contextual decisions without
/// a `v+1`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PermitAccumulators {
    /// Running cumulative spend charged to this run's granted/denied actions
    /// (`sum(spend_delta_usd)`); read by the C1 spend-cap verifiers.
    #[serde(default)]
    pub cumulative_spend_usd: f64,
    /// Running per-session risk score (`sum(risk_delta)`); read by the C6
    /// contextual policy.
    #[serde(default)]
    pub risk_score: f64,
    /// Count of decisions folded, by outcome — cheap contextual signal
    /// (e.g. escalation rate) and a rebuild-stable counter.
    #[serde(default)]
    pub granted: u64,
    #[serde(default)]
    pub denied: u64,
    #[serde(default)]
    pub escalated: u64,
}

/// A run's declared intent — the initiating task + the intent-scoped tool set
/// the `intent-lock` permit-verifier enforces (DR-010; the run-intent axis).
/// SP-intent REDUCER SCAFFOLD: the type + field exist so `run.intent.declared`
/// has a folding consumer (no consumer-less subjects — DR-006 precedent); the
/// `intent-lock` verifier that *reads* this pinned state is the NEXT slice
/// (SP-intent, oracle-first — NOT built here).
///
/// Reducer semantics (folded onto [`AgentRunState::intent`], keyed by the
/// payload `run`, last write wins):
/// - `run.intent.declared` `{run, intent_ref: CasRef, allowed_tools: [string]}`
///   → set `intent = Some(IntentState { allowed_tools, intent_ref })`.
///
/// The enforced set is the DECLARED list read verbatim (never re-derived — the
/// determinism BINDING forbids a live inference at decision time; DR-010 §3).
/// **Distinct from `agent.spawned.allowed_tools?`**: that is the *composed
/// harness allowlist* (what the agent was configured with); this is the
/// *intent-derived least-privilege set* the verifier checks against (DR-010 §4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntentState {
    /// The intent-scoped tool names the `intent-lock` verifier enforces
    /// (in-set → allow; off-task → escalate / deny under the knob).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// blake3 hex of the initiating task/prompt text persisted to the CAS
    /// (`intent_ref.hash`) — a ref, never inline bytes (I2). The interrogable
    /// "what was this run for" `gate_explain` names alongside the off-task tool.
    pub intent_ref: Option<String>,
}

/// One agent run's derived state (S1: the dossier's accounting seed).
///
/// S1 reducer semantics (pinned by `tests/s1_agent_runs.rs` and the
/// `s1_agent_run` golden fixture; payload schemas pending warden ratification):
/// - `agent.spawned` `{run, ...}` → insert with `status = "spawning"`;
/// - `agent.status.changed` `{run, from, to}` → `status = to`;
/// - `agent.completed` `{run, status, cost{total_usd,input_tokens,
///   output_tokens}, session_id, ...}` → `status = "completed"`, accounting
///   fields recorded.
///
/// Statuses stay payload-strings in the graph: reducers must fold any live
/// payload version, so they do not gatekeep through an enum (I3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentRunState {
    pub status: String,
    pub total_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub session_id: Option<String>,
    /// S4: recorded gate verdicts, keyed by gate name (see [`GateState`]).
    /// `#[serde(default)]` keeps every pre-S4 golden fixture parsing
    /// unedited; a gate fact on a run the log never spawned still creates
    /// the entry (default `status: ""`) — the log is truth (I3), and a
    /// pre-spawn vet refusal is exactly such a run.
    #[serde(default)]
    pub gates: BTreeMap<String, GateState>,
    /// DR-006: replay-divergence alarms on this run (see
    /// [`IntegrityAlarmRecord`]). ORACLE SCAFFOLD — field present so the
    /// DR-006 board is assert-red; the `integrity.alarm` reducer arm is
    /// implementer work. `#[serde(default)]` keeps every pre-DR-006 golden
    /// fixture parsing (and comparing equal) unedited. Deduped by
    /// (gate, verifier), deterministic order.
    #[serde(default)]
    pub integrity_alarms: Vec<IntegrityAlarmRecord>,
    /// DR-008/DR-009: this run's permit ledger, keyed by `request_id`
    /// (request→decision folds onto one entry). SP5 REDUCER SCAFFOLD — the
    /// permit subjects' folding consumer (no consumer-less subjects, DR-006
    /// precedent). `#[serde(default)]` keeps every pre-permit golden fixture
    /// parsing (and comparing equal) unedited. `BTreeMap` for deterministic
    /// whole-graph equality.
    #[serde(default)]
    pub permit_ledger: BTreeMap<String, PermitLedgerEntry>,
    /// DR-008/DR-009: this run's per-session permit accumulators (running
    /// spend / risk / decision counts) — the state the contextual C1/C7
    /// permit-verifiers read, folded purely from the log (I3). SP5 REDUCER
    /// SCAFFOLD. `#[serde(default)]` keeps every pre-permit golden fixture
    /// unedited.
    #[serde(default)]
    pub permit_accumulators: PermitAccumulators,
    /// DR-010: this run's declared intent (see [`IntentState`]) — the pinned
    /// state the future `intent-lock` permit-verifier reads. SP-intent REDUCER
    /// SCAFFOLD. `None` until a `run.intent.declared` fact folds. `#[serde(default)]`
    /// keeps every pre-DR-010 golden fixture parsing (and comparing equal)
    /// unedited.
    #[serde(default)]
    pub intent: Option<IntentState>,
    /// DR-014 §Decision 5: this run's PEP enforcement mode, folded from
    /// `agent.spawned.pep?`. `Some("enforced")` iff the spawn wired the permit
    /// PEP (the spec declared a `[gates.permit]` gate); `None` = edge-gated-only
    /// (no mid-run interception). The value is recorded VERBATIM as a string
    /// (never a bool) so a future degraded/partial mode arrives additively; the
    /// ABSENT case is never synthesized to a truthy value (DR-012 declared-vs-
    /// absent; the honest "no PEP wired"). `#[serde(default)]` keeps every
    /// pre-DR-014 golden fixture parsing (and comparing equal) unedited (I3
    /// rebuild-stability). Read through [`AgentRunState::pep_enforced`].
    #[serde(default)]
    pub pep: Option<String>,
    /// DR-016 §Decision 2 (SP4a): this run's RBAC role, folded from
    /// `agent.spawned.role?` (ontology line 195). `Some(role)` iff the spawn
    /// carried a role — taken VERBATIM (an opaque string the policy interprets;
    /// rezidnt mints no role vocabulary). `None` = no role declared: ABSENT,
    /// never synthesized to a default like `"contributor"` (DR-012 declared-vs-
    /// absent; the honest "no role"). This is the permit input axis
    /// `decide_permit` injects into its content-pinned per-run params so a
    /// role-keyed policy can decide on role + workspace + action. `#[serde(default)]`
    /// keeps every pre-DR-016 golden fixture parsing and comparing equal
    /// unedited (I3 rebuild-stability), exactly as `pep` does.
    #[serde(default)]
    pub role: Option<String>,
}

impl AgentRunState {
    /// Whether this run was mid-run-PEP-enforced — `true` iff the spawn folded
    /// `pep == "enforced"` (DR-014 §Decision 5). ABSENCE folds `false`: a run
    /// with no `pep` on its spawn is edge-gated-only, NEVER synthesized to
    /// enforced (the honesty the `gate_explain` distinction rests on, I4).
    pub fn pep_enforced(&self) -> bool {
        self.pep.as_deref() == Some("enforced")
    }
}

/// One worktree's derived state (S2: the sole-allocator registry's shadow in
/// the graph — the log is truth, the `.rezidnt/worktrees` file is the
/// adapter's working copy).
///
/// S2 reducer semantics (pinned by `tests/s2_worktrees.rs` and the
/// `s2_worktree_conflict` / `s2_diff_ready` golden fixtures). Entries key on
/// the payload's canonicalized path string:
/// - `worktree.allocated` `{path, branch?, allocator}` → insert with
///   `status = "allocated"`, `branch`/`allocator` copied from the payload;
/// - `worktree.observed` `{path, allocator}` → insert with
///   `status = "observed"`, allocator copied (out-of-band guard, DR-001);
/// - `worktree.conflict` `{path}` → `conflicts += 1` on the existing entry
///   (first claim's fields untouched — never double-tracked); the
///   exactly-once emission obligation is the ADAPTER's, so the reducer counts
///   every logged conflict honestly (I3);
/// - `worktree.released` `{path}` → `status = "released"` (inserted even if
///   never allocated — the log is truth);
/// - `diff.ready` `{worktree, diff: CasRef}` → `last_diff = Some(diff.hash)`
///   on the entry keyed by `worktree`.
///
/// S4 addition (pinned by `tests/s4_gates.rs` and the `s4_verified_run`
/// fixture; payload shape PENDING warden ratification — `diff.merged` is a
/// proposed subject, flagged in the S4 work order):
/// - `diff.merged` `{run, worktree, diff: CasRef}` → `status = "merged"`,
///   `last_diff = Some(diff.hash)` (inserted even if never allocated — the
///   log is truth, I3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorktreeState {
    pub status: String,
    pub branch: Option<String>,
    /// `"rezidnt"` (sole allocator) or `"human"` (out-of-band observation).
    pub allocator: Option<String>,
    #[serde(default)]
    pub conflicts: u64,
    /// blake3 hex of the most recent `diff.ready` summary ref.
    pub last_diff: Option<String>,
}

/// The entity graph. `BTreeMap` everywhere so equality and serialization
/// are deterministic (the property tests compare whole graphs).
///
/// S1 adds `agent_runs`, S2 adds `worktrees` — both additively:
/// `#[serde(default)]` keeps every earlier golden fixture parsing (and
/// comparing equal) unedited.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub events_folded: u64,
    pub last_event: Option<Ulid>,
    pub counts_by_subject: BTreeMap<Subject, u64>,
    pub workspaces: BTreeMap<WorkspaceId, WorkspaceStatus>,
    /// Keyed by the run ULID's canonical text form (payload `run` field).
    #[serde(default)]
    pub agent_runs: BTreeMap<String, AgentRunState>,
    /// Keyed by the canonicalized worktree path (payload `path` /
    /// `worktree` field).
    #[serde(default)]
    pub worktrees: BTreeMap<String, WorktreeState>,
}

/// The pure reducer (doc §6: `fn apply(&mut Graph, &Event)`). No IO, no
/// clocks, no randomness — same event, same graph delta, every time.
pub fn apply(graph: &mut Graph, event: &Event) {
    graph.events_folded += 1;
    graph.last_event = Some(event.id);
    *graph
        .counts_by_subject
        .entry(event.subject.clone())
        .or_insert(0) += 1;

    match event.subject.as_str() {
        "workspace.opened" => {
            if let Some(ws) = event.workspace {
                graph.workspaces.insert(ws, WorkspaceStatus::Open);
            }
        }
        "workspace.closed" => {
            // Inserted even if never opened — the log is truth (I3).
            if let Some(ws) = event.workspace {
                graph.workspaces.insert(ws, WorkspaceStatus::Closed);
            }
        }
        // S1 agent-run reducers, keyed by the payload `run` string. A payload
        // without a `run` (pre-ratification fixture lines, foreign versions)
        // folds as counters-only — reducers never choke, never guess (I3).
        "agent.spawned" => {
            if let Some(run) = payload_run(event) {
                let state = graph.agent_runs.entry(run).or_default();
                state.status = "spawning".to_string();
                // DR-014 §Decision 5: fold the enforcement mode VERBATIM when
                // present (`pep: "enforced"`); ABSENT stays `None` — never
                // synthesized to a truthy value (DR-012; the honest "no PEP
                // wired"). A pre-DR-014 spawn (no `pep`) folds edge-gated-only.
                if let Some(pep) = event.payload()["pep"].as_str() {
                    state.pep = Some(pep.to_string());
                }
                // DR-016 §Decision 2 (SP4a): fold the RBAC role VERBATIM when
                // present; ABSENT stays `None` — never synthesized to a default
                // (DR-012; the honest "no role declared"). A pre-DR-016 spawn (no
                // `role`) folds role-less, keeping rebuild stable (I3).
                if let Some(role) = event.payload()["role"].as_str() {
                    state.role = Some(role.to_string());
                }
            }
        }
        "agent.status.changed" => {
            if let Some(run) = payload_run(event)
                && let Some(to) = event.payload()["to"].as_str()
            {
                graph.agent_runs.entry(run).or_default().status = to.to_string();
            }
        }
        "agent.completed" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let state = graph.agent_runs.entry(run).or_default();
                state.status = "completed".to_string();
                state.total_usd = payload["cost"]["total_usd"].as_f64();
                state.input_tokens = payload["cost"]["input_tokens"].as_u64();
                state.output_tokens = payload["cost"]["output_tokens"].as_u64();
                state.session_id = payload["session_id"].as_str().map(String::from);
            }
        }
        // S2 worktree reducers, keyed by the payload's canonicalized path
        // string. A payload without its key folds as counters-only —
        // reducers never choke, never gatekeep (I3).
        "worktree.allocated" => {
            if let Some(path) = payload_path(event) {
                let payload = event.payload();
                let wt = graph.worktrees.entry(path).or_default();
                wt.status = "allocated".to_string();
                wt.branch = payload["branch"].as_str().map(String::from);
                wt.allocator = payload["allocator"].as_str().map(String::from);
            }
        }
        "worktree.observed" => {
            if let Some(path) = payload_path(event) {
                let payload = event.payload();
                let wt = graph.worktrees.entry(path).or_default();
                wt.status = "observed".to_string();
                wt.branch = payload["branch"].as_str().map(String::from);
                wt.allocator = payload["allocator"].as_str().map(String::from);
            }
        }
        "worktree.conflict" => {
            // Never mints a second entry (no double-tracking) and never
            // touches the first claim's fields; every logged fact counts
            // once — exactly-once emission is the adapter's obligation (I3).
            if let Some(path) = payload_path(event) {
                graph.worktrees.entry(path).or_default().conflicts += 1;
            }
        }
        "worktree.released" => {
            // Inserted even if never allocated — the log is truth (I3).
            if let Some(path) = payload_path(event) {
                graph.worktrees.entry(path).or_default().status = "released".to_string();
            }
        }
        "diff.ready" => {
            if let Some(path) = event.payload()["worktree"].as_str()
                && let Some(hash) = event.payload()["diff"]["hash"].as_str()
            {
                graph
                    .worktrees
                    .entry(path.to_string())
                    .or_default()
                    .last_diff = Some(hash.to_string());
            }
        }
        "diff.merged" => {
            // Golden-path merge/worktree-lifecycle-close fact; inserted even
            // if the worktree was never allocated — the log is truth (I3).
            if let Some(path) = event.payload()["worktree"].as_str() {
                let wt = graph.worktrees.entry(path.to_string()).or_default();
                wt.status = "merged".to_string();
                if let Some(hash) = event.payload()["diff"]["hash"].as_str() {
                    wt.last_diff = Some(hash.to_string());
                }
            }
        }
        // S4 gate reducers, keyed under `AgentRunState::gates` by the payload
        // `gate` name (last write wins). A gate fact creates the run entry
        // with no synthesized status — a pre-spawn vet refusal is exactly such
        // a run (I3: the log is truth, gate facts do not require a spawn).
        "gate.entered" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "entered".to_string(),
                    ..GateState::default()
                };
            }
        }
        "gate.passed" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                // Flatten every verifier record's evidence hashes in order.
                let evidence = event.payload()["verifiers"]
                    .as_array()
                    .map(|records| {
                        records
                            .iter()
                            .flat_map(|r| r["evidence"].as_array().cloned().unwrap_or_default())
                            .filter_map(|e| e["hash"].as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "pass".to_string(),
                    verifier: None,
                    evidence,
                    reason: None,
                };
            }
        }
        "gate.failed" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "fail".to_string(),
                    verifier: event.payload()["verifier"].as_str().map(String::from),
                    evidence: evidence_hashes(event),
                    reason: None,
                };
            }
        }
        "gate.inconclusive" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "inconclusive".to_string(),
                    verifier: event.payload()["verifier"].as_str().map(String::from),
                    evidence: evidence_hashes(event),
                    // Recorded verbatim — never coerced (I6).
                    reason: event.payload()["reason"].as_str().map(String::from),
                };
            }
        }
        // DR-006 replay-divergence alarm, folded onto the run's dossier. A
        // payload without a `run` folds as counters-only (never guesses a key,
        // I3). Deduped by (gate, verifier) within the run — the log is
        // append-only and debrief is re-runnable, so duplicate facts collapse
        // to one queryable record. Deterministic order (by (gate, verifier))
        // keeps whole-graph equality stable.
        "integrity.alarm" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let payload = event.payload();
                if let Some(verifier) = payload["verifier"].as_str() {
                    let alarms = &mut graph.agent_runs.entry(run).or_default().integrity_alarms;
                    // Dedup by (gate, verifier); duplicate facts do not grow
                    // the vec (the raw log still holds every fact).
                    if !alarms
                        .iter()
                        .any(|a| a.gate == gate && a.verifier == verifier)
                    {
                        let record = IntegrityAlarmRecord {
                            gate,
                            verifier: verifier.to_string(),
                            recorded: payload["recorded"].as_str().unwrap_or_default().to_string(),
                            replayed: payload["replayed"].as_str().unwrap_or_default().to_string(),
                        };
                        // Insert in deterministic (gate, verifier) order so
                        // whole-graph equality is interleaving-independent.
                        let pos = alarms
                            .binary_search_by(|a| {
                                (a.gate.as_str(), a.verifier.as_str())
                                    .cmp(&(record.gate.as_str(), record.verifier.as_str()))
                            })
                            .unwrap_or_else(|e| e);
                        alarms.insert(pos, record);
                    }
                }
            }
        }
        // DR-008/DR-009 permit reducers (the pre-hoc "may" axis). Keyed under
        // the run by the payload `request_id` (request and decision fold onto
        // one ledger entry). A payload without `run`/`request_id` folds as
        // counters-only — reducers never choke, never guess a key (I3). The
        // decision facts also accumulate per-session running state (spend/risk)
        // that the contextual C1/C7 verifiers read (SP0–SP4 work; this is the
        // fold that feeds them).
        "permit.requested" => {
            if let Some(run) = payload_run(event)
                && let Some(request_id) = payload_request_id(event)
            {
                let entry = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .permit_ledger
                    .entry(request_id)
                    .or_default();
                // Record the requested action; a decision may already have
                // folded first (out-of-order log), so only fill the action —
                // never clobber a decision (the log is truth, I3).
                entry.action = event.payload()["action"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
            }
        }
        "permit.granted" => apply_permit_decision(graph, event, "granted"),
        "permit.denied" => apply_permit_decision(graph, event, "denied"),
        "permit.escalated" => apply_permit_decision(graph, event, "escalated"),
        // DR-010 run-intent reducer (the run-intent axis). Keyed by the payload
        // `run`; folds onto `AgentRunState::intent` the pinned state the future
        // `intent-lock` verifier reads. A payload without `run` folds as
        // counters-only — the reducer never guesses a key, never chokes (I3,
        // the permit-reducer discipline). The enforced set is the DECLARED list
        // read verbatim (never re-derived; DR-010 §3).
        "run.intent.declared" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let allowed_tools = payload["allowed_tools"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let intent_ref = payload["intent_ref"]["hash"].as_str().map(String::from);
                graph.agent_runs.entry(run).or_default().intent = Some(IntentState {
                    allowed_tools,
                    intent_ref,
                });
            }
        }
        _ => {} // every other subject: counters only (S0 scope)
    }
}

/// Fold a permit decision (`permit.granted`/`.denied`/`.escalated`) onto the
/// run's ledger + per-session accumulators. `decision` is `"granted"` /
/// `"denied"` / `"escalated"` — `escalated` is NEVER coerced to `granted`
/// (I6, DR-008 §4). Pure: same event, same delta.
fn apply_permit_decision(graph: &mut Graph, event: &Event, decision: &str) {
    let (Some(run), Some(request_id)) = (payload_run(event), payload_request_id(event)) else {
        return; // counters-only; never guess a key (I3)
    };
    let payload = event.payload();
    let state = graph.agent_runs.entry(run).or_default();

    // Ledger: create the entry if the decision arrived before the request
    // (out-of-order log — the log is truth, I3).
    let entry = state.permit_ledger.entry(request_id).or_default();
    entry.decision = Some(decision.to_string());
    entry.policy_ref = payload["policy_ref"]["hash"].as_str().map(String::from);
    entry.reason = payload["reason"].as_str().map(String::from);

    // Accumulators: fold the optional per-session deltas + decision counters,
    // the state the contextual permit-verifiers read (C1/C6/C7).
    let acc = &mut state.permit_accumulators;
    if let Some(spend) = payload["spend_delta_usd"].as_f64() {
        acc.cumulative_spend_usd += spend;
    }
    if let Some(risk) = payload["risk_delta"].as_f64() {
        acc.risk_score += risk;
    }
    match decision {
        "granted" => acc.granted += 1,
        "denied" => acc.denied += 1,
        "escalated" => acc.escalated += 1,
        _ => {}
    }
}

/// The `run` key every `agent.*` payload carries (ontology v1 baselines).
fn payload_run(event: &Event) -> Option<String> {
    event.payload()["run"].as_str().map(String::from)
}

/// The `path` key every `worktree.*` payload carries (ontology v1 baselines).
fn payload_path(event: &Event) -> Option<String> {
    event.payload()["path"].as_str().map(String::from)
}

/// The `gate` key every `gate.*` payload carries (ontology v1 baselines).
fn payload_gate(event: &Event) -> Option<String> {
    event.payload()["gate"].as_str().map(String::from)
}

/// The `request_id` key every `permit.*` payload carries (the ledger key;
/// ontology v1 permit baselines, DR-008/DR-009).
fn payload_request_id(event: &Event) -> Option<String> {
    event.payload()["request_id"].as_str().map(String::from)
}

/// Evidence blob hashes (blake3 hex) from a `gate.failed`/`gate.inconclusive`
/// fact — refs, never bytes (I2).
fn evidence_hashes(event: &Event) -> Vec<String> {
    event.payload()["evidence"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["hash"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Fold a whole event sequence from scratch. `rezidnt rebuild` is exactly
/// `fold(log from seq 0)`.
pub fn fold<'a, I>(events: I) -> Graph
where
    I: IntoIterator<Item = &'a Event>,
{
    let mut graph = Graph::default();
    for event in events {
        apply(&mut graph, event);
    }
    graph
}

/// Live materializer: incremental fold + snapshot/resume. A snapshot *is* a
/// [`Graph`] (it carries `last_event`/`events_folded`, so startup = load
/// snapshot, fold the tail).
pub struct Materializer {
    graph: Graph,
}

impl Materializer {
    pub fn new() -> Self {
        Self {
            graph: Graph::default(),
        }
    }

    /// Resume from a snapshot taken by [`Materializer::snapshot`].
    pub fn resume(snapshot: Graph) -> Self {
        Self { graph: snapshot }
    }

    /// Apply one live event (delegates to the pure [`apply`]).
    pub fn apply(&mut self, event: &Event) {
        apply(&mut self.graph, event);
    }

    /// Current graph.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Point-in-time snapshot. Property (release-blocking, doc §15):
    /// `fold(log) == snapshot` — resuming from this and folding the tail must
    /// equal folding everything from seq 0.
    pub fn snapshot(&self) -> Graph {
        self.graph.clone()
    }
}

impl Default for Materializer {
    fn default() -> Self {
        Self::new()
    }
}
