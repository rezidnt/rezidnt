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
}

/// The entity graph. `BTreeMap` everywhere so equality and serialization
/// are deterministic (the property tests compare whole graphs).
///
/// S1 adds `agent_runs` additively: `#[serde(default)]` keeps every S0
/// golden fixture parsing (and comparing equal) unedited.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub events_folded: u64,
    pub last_event: Option<Ulid>,
    pub counts_by_subject: BTreeMap<Subject, u64>,
    pub workspaces: BTreeMap<WorkspaceId, WorkspaceStatus>,
    /// Keyed by the run ULID's canonical text form (payload `run` field).
    #[serde(default)]
    pub agent_runs: BTreeMap<String, AgentRunState>,
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
                graph.agent_runs.entry(run).or_default().status = "spawning".to_string();
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
        _ => {} // every other subject: counters only (S0 scope)
    }
}

/// The `run` key every `agent.*` payload carries (ontology v1 baselines).
fn payload_run(event: &Event) -> Option<String> {
    event.payload()["run"].as_str().map(String::from)
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
