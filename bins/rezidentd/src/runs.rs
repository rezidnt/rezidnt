//! S1 run substrate wiring: `open` materialization, the daemon-owned run
//! registry, and per-run capture (ring + CAS chunks).
//!
//! Ownership rule (S1 exit criterion): a run belongs to the daemon, never to
//! the connection that requested it — `materialize_open` is spawned as a
//! detached task and every run gets its own detached task, so a client
//! disconnect mid-run kills nothing but the socket.
#![cfg(unix)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use rezidnt_cas::Cas;
use rezidnt_fabric::Fabric;
use rezidnt_gate::Verdict;
use rezidnt_run::RunId;
use rezidnt_run::adapter::{ClaudeCodeAdapter, MESSAGE_INLINE_CAP, MappedFact};
use rezidnt_run::badge::Badge;
use rezidnt_run::capture::{DEFAULT_CHUNK_BYTES, DEFAULT_RING_BYTES, RingBuffer, chunk_into_cas};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::{AgentSpec, GateSpec, ProjectSpec};
use rezidnt_types::{Event, SourceId, Subject, WorkspaceId};
use serde_json::json;

use crate::gates;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::Instrument;
use ulid::Ulid;

/// Byte-broadcast ring per run (chunks, not bytes): a lagging attach client
/// skips ahead rather than back-pressuring the run task; the authoritative
/// stream copy is the ring + the CAS chunk manifest, never this channel.
const CAPTURE_BROADCAST_CAPACITY: usize = 1024;

/// Live byte feed handed to an attach subscriber.
pub type CaptureFeed = tokio::sync::broadcast::Receiver<Arc<[u8]>>;

/// One live (or finished) run's capture state. The mutex orders ring pushes
/// against attach snapshots: a subscriber that locks, subscribes, snapshots,
/// and unlocks observes every byte exactly once across snapshot + live feed
/// (the run task sends to the broadcast while holding the same lock).
pub struct RunCapture {
    inner: Mutex<CaptureInner>,
}

struct CaptureInner {
    ring: RingBuffer,
    full: Vec<u8>,
    tx: tokio::sync::broadcast::Sender<Arc<[u8]>>,
    finished: bool,
}

impl RunCapture {
    fn new() -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(CAPTURE_BROADCAST_CAPACITY);
        Self {
            inner: Mutex::new(CaptureInner {
                ring: RingBuffer::with_capacity(DEFAULT_RING_BYTES),
                full: Vec::new(),
                tx,
                finished: false,
            }),
        }
    }

    /// Poison recovery: the capture state is bytes and a flag; no invariant
    /// spans the lock, so continuing with the inner value is sound.
    fn lock(&self) -> std::sync::MutexGuard<'_, CaptureInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn push(&self, bytes: &[u8]) {
        let mut inner = self.lock();
        inner.ring.push(bytes);
        inner.full.extend_from_slice(bytes);
        // No live attach subscribers is not an error.
        let _ = inner.tx.send(Arc::from(bytes));
    }

    fn finish(&self) -> Vec<u8> {
        let mut inner = self.lock();
        inner.finished = true;
        // Zero-length sentinel: live attach loops close on it.
        let _ = inner.tx.send(Arc::from(&[][..]));
        std::mem::take(&mut inner.full)
    }

    /// Atomic subscribe + replay-snapshot for `attach`: everything pushed
    /// before the call is in the snapshot, everything after arrives on the
    /// receiver — no gap, no duplicate.
    pub fn attach(&self) -> (Vec<u8>, Option<CaptureFeed>) {
        let inner = self.lock();
        let rx = (!inner.finished).then(|| inner.tx.subscribe());
        (inner.ring.snapshot(), rx)
    }
}

/// Daemon-owned map of run id → capture handle. Entries persist after
/// completion so a late `attach` still replays the ring (DR-001 dtach model).
#[derive(Default)]
pub struct RunRegistry {
    runs: Mutex<HashMap<Ulid, Arc<RunCapture>>>,
}

impl RunRegistry {
    /// Poison recovery: plain map, no cross-key invariant.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Ulid, Arc<RunCapture>>> {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn insert(&self, run: Ulid, capture: Arc<RunCapture>) {
        self.lock().insert(run, capture);
    }

    pub fn get(&self, run: &Ulid) -> Option<Arc<RunCapture>> {
        self.lock().get(run).cloned()
    }
}

/// One opened workspace's daemon-side working state: what `spawn_agent`
/// needs to launch spec agents after the fact. Derived bookkeeping only —
/// the log remains the source of record (I3).
pub struct OpenedWorkspace {
    /// Repo root as the spec gave it (canonicalized again at use).
    pub root: PathBuf,
    pub agents: Vec<AgentSpec>,
    /// `[gates.<name>]` verifier sets from the applied spec (S4). The vet /
    /// pre_merge gates run these; empty for pre-S4 specs.
    pub gates: BTreeMap<String, GateSpec>,
    /// §9 idempotency: key → the run it minted. A retried `spawn_agent` with
    /// a known key returns this run and spawns nothing new.
    pub spawn_keys: HashMap<String, Ulid>,
}

/// Shared daemon context handed to connection and run tasks.
pub struct Daemon {
    pub fabric: Arc<Fabric>,
    pub cas: Arc<Cas>,
    pub registry: Arc<RunRegistry>,
    /// Opened workspaces by ULID (tokio mutex: held across the spawn await
    /// so same-key `spawn_agent` retries can never double-spawn).
    pub workspaces: tokio::sync::Mutex<HashMap<Ulid, OpenedWorkspace>>,
}

impl Daemon {
    pub fn new(fabric: Arc<Fabric>, cas: Arc<Cas>, registry: Arc<RunRegistry>) -> Self {
        Self {
            fabric,
            cas,
            registry,
            workspaces: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

/// Rebuild the open-workspace map from log + CAS at daemon startup (S3-T1
/// remediation, I3: the map is derived state and anything that cannot be
/// rebuilt from log + CAS is misdesigned). Eager rather than lazy: one
/// blocking scan before the transports come up keeps the spawn path simple
/// and never holds the daemon-wide workspaces lock across a log read — the
/// same restart-derived-state pattern as the S2 git adapter's
/// reconcile-on-open.
pub async fn rebuild_workspaces(daemon: &Arc<Daemon>) -> anyhow::Result<()> {
    let fabric = Arc::clone(&daemon.fabric);
    let cas = Arc::clone(&daemon.cas);
    let rebuilt = tokio::task::spawn_blocking(move || fold_workspaces(&fabric, &cas))
        .await
        .context("workspace rebuild task panicked")??;
    let count = rebuilt.len();
    let mut workspaces = daemon.workspaces.lock().await;
    for (ws, entry) in rebuilt {
        workspaces.entry(ws).or_insert(entry);
    }
    drop(workspaces);
    tracing::info!(workspaces = count, "open-workspace map rebuilt from log");
    Ok(())
}

/// The pure fold behind [`rebuild_workspaces`]: `workspace.opened` opens an
/// entry (root from the payload); `workspace.spec.applied` resolves the
/// applied spec TOML from the CAS and fills the agent list; a keyed
/// `agent.spawned` rebuilds the §9 idempotency map, scoped to the envelope
/// `workspace` per the ontology. A ghost open (acked but `workspace.opened`
/// never reached the log) is absent by construction.
fn fold_workspaces(fabric: &Fabric, cas: &Cas) -> anyhow::Result<HashMap<Ulid, OpenedWorkspace>> {
    let mut map: HashMap<Ulid, OpenedWorkspace> = HashMap::new();
    let events = fabric
        .replay_since(None)
        .context("replay log for workspace rebuild")?;
    for event in events {
        let Some(ws) = event.workspace else {
            continue;
        };
        let ws = ws.ulid();
        match event.subject.as_str() {
            "workspace.opened" => {
                let Some(root) = event.payload()["root"].as_str() else {
                    continue;
                };
                map.insert(
                    ws,
                    OpenedWorkspace {
                        root: PathBuf::from(root),
                        agents: Vec::new(),
                        gates: BTreeMap::new(),
                        spawn_keys: HashMap::new(),
                    },
                );
            }
            "workspace.spec.applied" => {
                let Some(entry) = map.get_mut(&ws) else {
                    continue;
                };
                let spec_ref = serde_json::from_value::<rezidnt_types::refs::CasRef>(
                    event.payload()["spec_ref"].clone(),
                );
                let spec = spec_ref
                    .ok()
                    .and_then(|r| cas.get(&r).ok())
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .and_then(|toml| ProjectSpec::from_toml_str(&toml).ok());
                match spec {
                    Some(spec) => {
                        entry.agents = spec.agents;
                        entry.gates = spec.gates;
                    }
                    // Degraded, never fatal: the workspace stays open with no
                    // spawnable agents; spawn answers agent.unknown.
                    None => tracing::warn!(
                        workspace = %ws,
                        "applied spec unresolvable from CAS; agents not rebuilt"
                    ),
                }
            }
            "agent.spawned" => {
                let payload = event.payload();
                let (Some(key), Some(run)) =
                    (payload["idempotency_key"].as_str(), payload["run"].as_str())
                else {
                    continue; // keyless spawns never enter the key map
                };
                let Ok(run) = Ulid::from_string(run) else {
                    continue;
                };
                if let Some(entry) = map.get_mut(&ws) {
                    entry.spawn_keys.insert(key.to_string(), run);
                }
            }
            _ => {}
        }
    }
    Ok(map)
}

/// Publish one event off the async threads (SQLite append is blocking);
/// returns the event id for causation chaining.
pub async fn publish(fabric: &Arc<Fabric>, event: Event) -> anyhow::Result<Ulid> {
    let id = event.id;
    let fabric = Arc::clone(fabric);
    tokio::task::spawn_blocking(move || fabric.publish(event))
        .await
        .context("publish task panicked")?
        .context("append to log")?;
    Ok(id)
}

/// Record replay-divergence integrity alarms on the log through the daemon's
/// SINGLE writer (DR-006, I3). For each requested alarm, dedup by
/// (run, gate, verifier) against `integrity.alarm` facts ALREADY on the log
/// (log-derived, no side table) — a re-run of `debrief` over an
/// already-alarmed divergence appends nothing. Every new alarm is published
/// as an `integrity.alarm` v1 fact through the Fabric, so it lands on the log
/// AND broadcasts to live tail subscribers (proving legitimate-writer
/// emission). Returns the count actually appended.
///
/// Emission is at-least-once on the wire (the ontology ratifies this): a crash
/// between the log read and the append can still double a fact on the raw log;
/// the fold dedups by (run, gate, verifier), so derived state stays honest.
pub async fn record_alarms(
    daemon: &Arc<Daemon>,
    alarms: &[rezidnt_proto::AlarmRecord],
) -> anyhow::Result<usize> {
    // Read the existing (run, gate, verifier) alarm keys off the log once,
    // on a blocking thread (SQLite read is blocking; no blocking in async).
    let fabric = Arc::clone(&daemon.fabric);
    let existing: std::collections::HashSet<(String, String, String)> =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let events = fabric
                .replay_since(None)
                .context("replay log for integrity.alarm dedup")?;
            let keys = events
                .into_iter()
                .filter(|e| e.subject.as_str() == "integrity.alarm")
                .filter_map(|e| {
                    let p = e.payload();
                    Some((
                        p["run"].as_str()?.to_string(),
                        p["gate"].as_str()?.to_string(),
                        p["verifier"].as_str()?.to_string(),
                    ))
                })
                .collect();
            Ok(keys)
        })
        .await
        .context("alarm dedup task panicked")??;

    // Dedup within this batch too, so a request carrying the same divergence
    // twice appends once (the fold would collapse it, but the log stays honest).
    let mut seen = existing;
    let mut appended = 0usize;
    for alarm in alarms {
        let key = (
            alarm.run.clone(),
            alarm.gate.clone(),
            alarm.verifier.clone(),
        );
        if !seen.insert(key) {
            continue; // already on the log (or already appended this batch)
        }
        let event = Event::new(
            SourceId::new("rezidnt-gate"),
            None,
            Subject::new("integrity.alarm"),
            Ulid::new(),
            None,
            1,
            json!({
                "run": alarm.run,
                "gate": alarm.gate,
                "verifier": alarm.verifier,
                "recorded": alarm.recorded,
                "replayed": alarm.replayed,
            }),
        )
        .context("construct integrity.alarm")?;
        publish(&daemon.fabric, event).await?;
        appended += 1;
    }
    Ok(appended)
}

/// A refused `open`: the request never materialized anything. Answered as a
/// machine-readable `spec.invalid` frame/refusal by the caller (S3 board).
#[derive(Debug)]
pub struct OpenRefusal {
    pub message: String,
}

/// Emit the `daemon.warning {what: "open-failed"}` fact that keeps a failed
/// open visible in `tail`, never silent (S1 pin).
///
/// `correlation` threads the OPEN CHAIN's correlation through so a
/// post-materialization warning is scoped by causal chain, not by a time
/// marker — two concurrent failed opens then carry distinct correlations and
/// are distinguishable (S3-T6). A pre-materialization refusal (no chain minted
/// yet) passes `None` and gets a fresh correlation.
async fn warn_open_failed(daemon: &Arc<Daemon>, correlation: Option<Ulid>, error: &str) {
    // The open chain's correlation when this failure has one; otherwise a FRESH
    // random ULID (NOT `Ulid::default()`, which is the nil id) for a
    // pre-materialization refusal that has no chain.
    let correlation = match correlation {
        Some(correlation) => correlation,
        None => Ulid::new(),
    };
    let warning = Event::new(
        SourceId::new("daemon"),
        None,
        Subject::new("daemon.warning"),
        correlation,
        None,
        1,
        json!({"what": "open-failed", "error": error}),
    );
    match warning {
        Ok(event) => {
            if let Err(e) = publish(&daemon.fabric, event).await {
                tracing::error!(error = %e, "could not publish open-failure warning");
            }
        }
        Err(e) => tracing::error!(error = %e, "could not construct open-failure warning"),
    }
}

/// Begin an `open` (S3 request-scoped ack shape): validate the spec, mint the
/// workspace + correlation ids, register the workspace for later
/// `spawn_agent` calls, and DETACH the materialization task — the run chain
/// is daemon-owned and survives the requesting client (S1 exit criterion).
///
/// Returns the (workspace, correlation) pair the ack names; every
/// materialization fact of this open carries exactly that correlation.
///
/// `warn_on_refuse`: the socket path mirrors refusals as
/// `daemon.warning {what: "open-failed"}` (S1 pin); the MCP path refuses
/// without touching the log (§12 refusal-before-effect, S3 board).
pub async fn begin_open(
    daemon: &Arc<Daemon>,
    spec_toml: &str,
    warn_on_refuse: bool,
) -> Result<(WorkspaceId, Ulid), OpenRefusal> {
    let checked = check_open_spec(spec_toml);
    let spec = match checked {
        Ok(spec) => spec,
        Err(message) => {
            tracing::warn!(error = %message, "open refused");
            if warn_on_refuse {
                // Pre-materialization refusal: no open chain exists yet.
                warn_open_failed(daemon, None, &message).await;
            }
            return Err(OpenRefusal { message });
        }
    };

    let correlation = Ulid::new();
    let workspace = WorkspaceId::new(Ulid::new());
    daemon.workspaces.lock().await.insert(
        workspace.ulid(),
        OpenedWorkspace {
            root: spec.repo.clone(),
            agents: spec.agents.clone(),
            gates: spec.gates.clone(),
            spawn_keys: HashMap::new(),
        },
    );

    let task = materialize_open(
        Arc::clone(daemon),
        spec,
        spec_toml.to_string(),
        workspace,
        correlation,
    );
    let span = tracing::info_span!("open", workspace = %workspace.ulid());
    tokio::spawn(task.instrument(span));
    Ok((workspace, correlation))
}

/// Pre-materialization validation: §13 parse + harness gate. Pure — nothing
/// touches the fabric here.
fn check_open_spec(spec_toml: &str) -> Result<ProjectSpec, String> {
    let spec =
        ProjectSpec::from_toml_str(spec_toml).map_err(|e| format!("parse project spec: {e}"))?;
    // I4: refusal keys on the harness NAME, before anything materializes —
    // a spec naming an unknown harness produces no workspace, no worktree,
    // and no agent.spawned.
    for agent in &spec.agents {
        if !SUPPORTED_HARNESSES.contains(&agent.harness.as_str()) {
            return Err(format!(
                "unknown harness {:?} for agent {:?} — this daemon speaks {SUPPORTED_HARNESSES:?}; \
                 refused at open, nothing materialized",
                agent.harness, agent.name,
            ));
        }
    }
    Ok(spec)
}

/// The detached materialization chain (S1 exit criterion). One correlation
/// ULID spans the whole chain; causation chains each fact to its trigger.
/// Every post-ack failure is traced and mirrored as `daemon.warning` so a
/// failed open is visible in `tail`, not silent.
async fn materialize_open(
    daemon: Arc<Daemon>,
    spec: ProjectSpec,
    spec_toml: String,
    workspace: WorkspaceId,
    correlation: Ulid,
) {
    if let Err(e) = try_materialize_open(&daemon, spec, &spec_toml, workspace, correlation).await {
        tracing::warn!(error = %e.source, "open materialization failed");
        // EVICTION DISCIPLINE (S4 remediation, I3): evict ONLY the ghost case
        // — `workspace.opened` never reached the log, so the workspace is not
        // open and `spawn_agent` must answer `workspace.unknown`. A POST-FACT
        // failure (a later agent could not launch) after `workspace.opened`
        // published is NOT a ghost: the log OPENED the workspace, so it stays
        // spawnable (fold(log) == live map). Evicting it was the over-reach
        // that made the live daemon disagree with a restarted one.
        if !e.opened {
            daemon.workspaces.lock().await.remove(&workspace.ulid());
        }
        // Thread the open chain's correlation so the warning is scoped to THIS
        // open, distinguishable from a concurrent failed open (S3-T6).
        warn_open_failed(&daemon, Some(correlation), &format!("{:#}", e.source)).await;
    }
}

/// A materialization failure that remembers whether `workspace.opened` reached
/// the log — the eviction decision (ghost vs. post-fact failure) turns on it.
struct MaterializeError {
    opened: bool,
    source: anyhow::Error,
}

/// Harnesses this daemon's S1 run substrate can drive. The AgentSubstrate
/// trait seam (I4) is S2+ architecture; until it lands this name gate is the
/// refusal point.
const SUPPORTED_HARNESSES: &[&str] = &["claude-code"];

async fn try_materialize_open(
    daemon: &Arc<Daemon>,
    spec: ProjectSpec,
    spec_toml: &str,
    workspace: WorkspaceId,
    correlation: Ulid,
) -> Result<(), MaterializeError> {
    // Pre-opened phase: any failure here is a GHOST (workspace.opened never
    // reached the log). `opened = false` → the caller evicts.
    let ghost = |e: anyhow::Error| MaterializeError {
        opened: false,
        source: e,
    };

    // Workspace root = the spec's repo path (relative paths resolve against
    // the daemon cwd in S1 — the spec arrived over the wire, not from a file).
    let root = tokio::fs::canonicalize(&spec.repo)
        .await
        .with_context(|| format!("canonicalize workspace root {}", spec.repo.display()))
        .map_err(ghost)?;

    // 1. workspace.opened — the envelope workspace id is the entity key. Once
    //    this reaches the log the workspace IS open; every later failure is
    //    post-fact (opened = true → the caller keeps it spawnable, I3).
    let opened_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("daemon"),
            Some(workspace),
            Subject::new("workspace.opened"),
            correlation,
            None,
            1,
            json!({"name": spec.name, "root": root.display().to_string()}),
        )
        .map_err(|e| ghost(e.into()))?,
    )
    .await
    .map_err(ghost)?;

    // From here on the workspace is on the log — post-fact failures never
    // evict.
    let post_fact = |e: anyhow::Error| MaterializeError {
        opened: true,
        source: e,
    };

    // 2. workspace.spec.applied — the applied spec TOML persists to the CAS.
    let spec_ref = {
        let cas = Arc::clone(&daemon.cas);
        let bytes = spec_toml.as_bytes().to_vec();
        tokio::task::spawn_blocking(move || cas.put(&bytes, "application/toml"))
            .await
            .context("cas put task panicked")
            .map_err(post_fact)?
            .context("store spec in cas")
            .map_err(post_fact)?
    };
    let agent_names: Vec<&str> = spec.agents.iter().map(|a| a.name.as_str()).collect();
    let applied_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("daemon"),
            Some(workspace),
            Subject::new("workspace.spec.applied"),
            correlation,
            Some(opened_id),
            1,
            json!({"spec_ref": spec_ref, "agents": agent_names}),
        )
        .map_err(|e| post_fact(e.into()))?,
    )
    .await
    .map_err(post_fact)?;

    // 3–6. Per agent: vet (pre-spawn), allocate a worktree, spawn under
    //       capture, detach. A single agent's launch failure is recorded as a
    //       daemon.warning but does NOT roll back the (already-opened)
    //       workspace: the log opened it, so it stays spawnable (I3; the S4
    //       partial-failure remediation). The whole open only errors out of
    //       here — and even then, with `opened = true` — if every agent fails
    //       so a warning is warranted.
    let mut launch_error: Option<anyhow::Error> = None;
    for agent in &spec.agents {
        if let Err(e) = launch_agent(
            daemon,
            agent,
            &root,
            workspace,
            correlation,
            Some(applied_id),
            &spec.gates,
            // Spec-driven open-chain spawns are keyless (ontology: the key is
            // never synthesized).
            None,
        )
        .await
        {
            tracing::warn!(agent = %agent.name, error = %e, "agent launch failed; workspace stays open");
            launch_error.get_or_insert_with(|| e.context(format!("launch agent {:?}", agent.name)));
        }
    }
    match launch_error {
        // A post-fact launch failure surfaces as daemon.warning (the caller),
        // but never evicts — the workspace is on the log.
        Some(e) => Err(post_fact(e)),
        None => Ok(()),
    }
}

/// Allocate a worktree and spawn one agent under capture; returns the minted
/// run id. `causation` is the triggering fact (`workspace.spec.applied` on
/// the open chain; `None` for a standalone MCP `spawn_agent`).
/// `idempotency_key` is the caller-supplied spawn key, recorded on the
/// `agent.spawned` payload so the key→run map is log-derivable (I3; ontology
/// v1 additive field, ratified 2026-07-17) — `None` for keyless paths (socket
/// `open` chain), never synthesized.
#[allow(clippy::too_many_arguments)]
pub async fn launch_agent(
    daemon: &Arc<Daemon>,
    agent: &AgentSpec,
    repo: &Path,
    workspace: WorkspaceId,
    correlation: Ulid,
    causation: Option<Ulid>,
    gate_defs: &BTreeMap<String, GateSpec>,
    idempotency_key: Option<&str>,
) -> anyhow::Result<RunId> {
    let run = RunId::new(Ulid::new());
    let run_str = run.ulid().to_string();

    // 0. vet — PRE-SPAWN enforcement (doc §8, S4). If the agent's `gates`
    //    name `vet`, run it BEFORE anything materializes: a non-conforming
    //    spec (no bare / no pinned version / no allowed_tools) is refused at
    //    the gate — NO worktree, NO agent.spawned. The refusal is a
    //    machine-readable fact (gate.failed), never a log-less error.
    let mut vet_causation = causation;
    if agent.gates.iter().any(|g| g == "vet") {
        let spec_ref = gates::pin_agent_spec(daemon, agent).await?;
        let refs = BTreeMap::from([("spec".to_string(), spec_ref)]);
        let outcome = gates::run_gate(
            daemon,
            workspace,
            correlation,
            causation,
            &run_str,
            "vet",
            refs,
            &gates::vet_verifiers(),
        )
        .await?;
        if outcome.verdict != Verdict::Pass {
            // Pre-spawn refusal: the gate fact is the record. No spawn.
            anyhow::bail!("vet gate refused agent {:?} (verdict not pass)", agent.name);
        }
        vet_causation = Some(outcome.verdict_id);
    }

    // 3. worktree.allocated — minimal git-CLI allocation (S2 owns the full
    // RepoSubstrate adapter; S1 keeps this to allocate-and-emit).
    let (worktree, branch) = allocate_worktree(repo, agent, run).await?;
    let allocated_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("daemon"),
            Some(workspace),
            Subject::new("worktree.allocated"),
            correlation,
            vet_causation,
            1,
            json!({
                "path": worktree.display().to_string(),
                "branch": branch,
                "allocator": "rezidnt",
            }),
        )?,
    )
    .await?;

    // 4. agent.spawned — badge minted, env scrubbed, badge injected (§12).
    let badge = Badge::mint().context("mint badge")?;
    let plan = SpawnPlan::for_claude_code(agent, &badge, std::env::vars());
    let mut child = tokio::process::Command::new(&plan.bin)
        .args(&plan.args)
        .env_clear()
        .envs(plan.env.iter().cloned())
        .current_dir(&worktree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawn harness {}", plan.bin.display()))?;
    let started = std::time::Instant::now();

    let mut spawned_payload = json!({
        "run": run,
        "agent": agent.name,
        "harness": agent.harness,
        "badge_id": badge.id(),
    });
    if let (Some(pid), Some(obj)) = (child.id(), spawned_payload.as_object_mut()) {
        obj.insert("pid".to_string(), json!(pid));
    }
    if let (Some(key), Some(obj)) = (idempotency_key, spawned_payload.as_object_mut()) {
        obj.insert("idempotency_key".to_string(), json!(key));
    }
    // Governed-spawn fields (S4, additive, DR-001: enforcement decisions
    // recorded in events). A governed spawn is one that ran through the vet
    // gate — the posture the gate checked is now log-derivable. Absent on
    // ungoverned/legacy spawns (never synthesized to `false` — absence is
    // honest, per the ontology).
    let governed = agent.gates.iter().any(|g| g == "vet");
    if governed && let Some(obj) = spawned_payload.as_object_mut() {
        obj.insert("bare".to_string(), json!(agent.bare));
        if let Some(v) = &agent.harness_version {
            obj.insert("harness_version".to_string(), json!(v));
        }
        if !agent.allowed_tools.is_empty() {
            obj.insert("allowed_tools".to_string(), json!(agent.allowed_tools));
        }
    }

    // Register BEFORE the fact hits the fabric: a client that sees
    // agent.spawned must be able to attach without racing the registry.
    let capture = Arc::new(RunCapture::new());
    daemon.registry.insert(run.ulid(), Arc::clone(&capture));

    let spawned_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("rezidnt-run"),
            Some(workspace),
            Subject::new("agent.spawned"),
            correlation,
            Some(allocated_id),
            1,
            spawned_payload,
        )?,
    )
    .await?;

    // 5–6. The run task: daemon-owned, detached from every connection. The
    //       pre_merge plan (if the agent's gates name it) travels with the
    //       run so the post-completion diff.ready → pre_merge → diff.merged
    //       chain runs where the worktree state is known.
    let pre_merge = if agent.gates.iter().any(|g| g == "pre_merge") {
        Some(PreMergePlan {
            repo: repo.to_path_buf(),
            gate: gate_defs.get("pre_merge").cloned().unwrap_or_default(),
        })
    } else {
        None
    };
    let stdout = child.stdout.take().context("child stdout must be piped")?;
    let ctx = RunTaskContext {
        daemon: Arc::clone(daemon),
        run,
        workspace,
        correlation,
        spawned_id,
        capture,
        worktree: worktree.clone(),
        pre_merge,
    };
    let span = tracing::info_span!("run", run = %run.ulid(), agent = %agent.name);
    tokio::spawn(
        async move {
            if let Err(e) = drive_run(ctx, child, stdout, started).await {
                tracing::warn!(error = %e, "run task failed");
            }
        }
        .instrument(span),
    );
    Ok(run)
}

/// `git worktree add` under the workspace dir. A repo with commits gets a
/// detached checkout; an empty repo (no HEAD yet) falls back to an orphan
/// worktree on a fresh `rezidnt/<agent>-<run>` branch (git ≥ 2.42).
async fn allocate_worktree(
    repo: &Path,
    agent: &AgentSpec,
    run: RunId,
) -> anyhow::Result<(PathBuf, Option<String>)> {
    let base = repo.join(".rezidnt").join("worktrees");
    tokio::fs::create_dir_all(&base)
        .await
        .with_context(|| format!("create worktree base {}", base.display()))?;
    let path = base.join(format!("{}-{}", agent.name, run.ulid()));

    let detach = git_worktree_add(repo, &["--detach"], &path).await?;
    if detach.status.success() {
        let canonical = tokio::fs::canonicalize(&path).await?;
        return Ok((canonical, None));
    }

    let branch = format!("rezidnt/{}-{}", agent.name, run.ulid());
    let orphan = git_worktree_add(repo, &["--orphan", "-b", &branch], &path).await?;
    if orphan.status.success() {
        let canonical = tokio::fs::canonicalize(&path).await?;
        return Ok((canonical, Some(branch)));
    }

    anyhow::bail!(
        "git worktree add failed for {}: detach: {}; orphan: {}",
        path.display(),
        String::from_utf8_lossy(&detach.stderr).trim(),
        String::from_utf8_lossy(&orphan.stderr).trim(),
    )
}

async fn git_worktree_add(
    repo: &Path,
    mode: &[&str],
    path: &Path,
) -> anyhow::Result<std::process::Output> {
    tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "add"])
        .args(mode)
        .arg(path)
        .output()
        .await
        .context("run git worktree add (is git on PATH?)")
}

struct RunTaskContext {
    daemon: Arc<Daemon>,
    run: RunId,
    workspace: WorkspaceId,
    correlation: Ulid,
    spawned_id: Ulid,
    capture: Arc<RunCapture>,
    /// The agent's allocated worktree — the pre_merge chain summarizes and
    /// merges it after the run completes.
    worktree: PathBuf,
    /// Present when the agent's gates include `pre_merge` (the golden path).
    pre_merge: Option<PreMergePlan>,
}

/// What the run task needs to run the `pre_merge` gate and merge after the
/// agent completes: the repo to merge into and the gate's verifier set.
struct PreMergePlan {
    repo: PathBuf,
    gate: GateSpec,
}

/// Read the child's stream-json stdout to EOF: every byte into the capture
/// (ring + live attach + full-stream accumulator), every line through the
/// adapter onto the fabric. On exit: a failure-shaped completion if the
/// harness died without a result line, then the full stream chunks into the
/// CAS as `artifact.captured` manifest facts (I2 — refs only).
async fn drive_run(
    ctx: RunTaskContext,
    mut child: tokio::process::Child,
    stdout: tokio::process::ChildStdout,
    started: std::time::Instant,
) -> anyhow::Result<()> {
    let mut adapter = ClaudeCodeAdapter::new(ctx.run);
    let mut completed_id: Option<Ulid> = None;

    let mut lines = BufReader::new(stdout).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!(error = %e, "stream read failed; treating as EOF");
                break;
            }
        };
        let mut raw = line.clone().into_bytes();
        raw.push(b'\n');
        ctx.capture.push(&raw);

        if line.trim().is_empty() {
            continue;
        }
        let facts = match adapter.map_line(&line) {
            Ok(facts) => facts,
            Err(e) => {
                // A harness emitting a garbage line must not kill the run;
                // the byte stream already captured the evidence verbatim.
                tracing::warn!(error = %e, "unmappable stream line tolerated");
                continue;
            }
        };
        for mut fact in facts {
            cap_message_inline(&ctx.daemon.cas, &mut fact)
                .await
                .context("apply agent.message inline cap")?;
            let is_completion = fact.subject == "agent.completed";
            let event = Event::new(
                SourceId::new("rezidnt-run"),
                Some(ctx.workspace),
                Subject::new(&fact.subject),
                ctx.correlation,
                Some(ctx.spawned_id),
                1,
                fact.payload,
            )?;
            let id = publish(&ctx.daemon.fabric, event).await?;
            if is_completion {
                completed_id = Some(id);
            }
        }
    }

    let status = child.wait().await.context("reap child")?;
    tracing::debug!(?status, "harness exited");

    // Failure-shaped completion when the child died without a result line —
    // the run always terminates on the fabric (accounting zeroed honestly).
    if completed_id.is_none() {
        let mut payload = json!({
            "run": ctx.run,
            "status": "error",
            "cost": {"total_usd": 0.0, "input_tokens": 0, "output_tokens": 0},
            "num_turns": 0,
            "duration_ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        if let (Some(session), Some(obj)) = (adapter.session_id(), payload.as_object_mut()) {
            obj.insert("session_id".to_string(), json!(session));
        }
        let event = Event::new(
            SourceId::new("rezidnt-run"),
            Some(ctx.workspace),
            Subject::new("agent.completed"),
            ctx.correlation,
            Some(ctx.spawned_id),
            1,
            payload,
        )?;
        completed_id = Some(publish(&ctx.daemon.fabric, event).await?);
    }

    // pre_merge — the golden-path verified-merge chain (S4). After the agent
    // completes, summarize its worktree into the CAS (diff.ready, CAS-pinned),
    // run the pre_merge gate against that ref, and on a VERIFIED pass merge
    // the change into the repo (diff.merged). A gate refusal blocks the merge:
    // the verdict fact is the record, the diff is not merged.
    if let Some(plan) = &ctx.pre_merge
        && let Err(e) = run_pre_merge(&ctx, plan, completed_id).await
    {
        // A pre_merge failure never kills the run task's teardown; it is
        // traced and the run still chunks its capture below.
        tracing::warn!(error = %e, "pre_merge chain failed");
    }

    // Capture chunks into the CAS; manifest facts carry refs only (I2).
    let stream = ctx.capture.finish();
    let manifest = {
        let cas = Arc::clone(&ctx.daemon.cas);
        let run = ctx.run;
        tokio::task::spawn_blocking(move || chunk_into_cas(&cas, run, &stream, DEFAULT_CHUNK_BYTES))
            .await
            .context("chunking task panicked")?
            .context("chunk capture into cas")?
    };
    for entry in manifest {
        let event = Event::new(
            SourceId::new("rezidnt-run"),
            Some(ctx.workspace),
            Subject::new("artifact.captured"),
            ctx.correlation,
            completed_id,
            1,
            json!({
                "ref": entry.r#ref,
                "provenance": {
                    "run": entry.run,
                    "kind": "capture-chunk",
                    "chunk": entry.chunk,
                },
            }),
        )?;
        publish(&ctx.daemon.fabric, event).await?;
    }
    Ok(())
}

/// The golden-path verified-merge chain: diff.ready (CAS-pinned) →
/// pre_merge gate over the diff ref → on pass, git merge + diff.merged.
async fn run_pre_merge(
    ctx: &RunTaskContext,
    plan: &PreMergePlan,
    completed_id: Option<Ulid>,
) -> anyhow::Result<()> {
    let run_str = ctx.run.ulid().to_string();

    // 1. Summarize the worktree's change into the CAS and emit diff.ready
    //    (the S2 watcher fact, produced here at completion for the gated run).
    let (diff_ref, cas_ref) = gates::summarize_worktree(&ctx.daemon, &ctx.worktree).await?;
    let diff_ready_id = publish(
        &ctx.daemon.fabric,
        Event::new(
            SourceId::new("rezidnt-adapter-git"),
            Some(ctx.workspace),
            Subject::new("diff.ready"),
            ctx.correlation,
            completed_id,
            1,
            json!({
                "worktree": ctx.worktree.display().to_string(),
                "diff": cas_ref,
            }),
        )?,
    )
    .await?;

    // 2. pre_merge — verify the CAS-pinned diff. gate.entered follows
    //    diff.ready (the test pins this order: the gate verifies the diff).
    let refs = BTreeMap::from([("diff".to_string(), diff_ref)]);
    let verifiers = gates::resolve_verifiers(&plan.gate);
    let outcome = gates::run_gate(
        &ctx.daemon,
        ctx.workspace,
        ctx.correlation,
        Some(diff_ready_id),
        &run_str,
        "pre_merge",
        refs,
        &verifiers,
    )
    .await?;

    // 3. Merge ONLY on a verified pass (the merge happens after the verdict).
    if outcome.verdict == Verdict::Pass {
        gates::merge_worktree(
            &ctx.daemon,
            ctx.workspace,
            ctx.correlation,
            Some(outcome.verdict_id),
            &run_str,
            &plan.repo,
            &ctx.worktree,
            &cas_ref,
        )
        .await?;
    }
    Ok(())
}

/// Ontology v1 baseline for `agent.message`: text stays inline only up to
/// [`MESSAGE_INLINE_CAP`]; bulk bodies go to the CAS and the payload carries
/// `ref` instead (exactly one of `text`/`ref`).
async fn cap_message_inline(cas: &Arc<Cas>, fact: &mut MappedFact) -> anyhow::Result<()> {
    if fact.subject != "agent.message" {
        return Ok(());
    }
    let Some(text) = fact.payload["text"].as_str() else {
        return Ok(());
    };
    if text.len() <= MESSAGE_INLINE_CAP {
        return Ok(());
    }
    let bytes = text.as_bytes().to_vec();
    let cas = Arc::clone(cas);
    let r = tokio::task::spawn_blocking(move || cas.put(&bytes, "text/plain; charset=utf-8"))
        .await
        .context("cas put task panicked")?
        .context("store bulk message in cas")?;
    let Some(obj) = fact.payload.as_object_mut() else {
        return Ok(());
    };
    obj.remove("text");
    obj.insert("ref".to_string(), serde_json::to_value(r)?);
    Ok(())
}

/// Serve an `attach`: replay the capture ring, then proxy live bytes until
/// the run finishes or the client goes away (DR-001 dtach model).
pub async fn serve_attach<W>(daemon: &Arc<Daemon>, run: Ulid, out: &mut W) -> anyhow::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let Some(capture) = daemon.registry.get(&run) else {
        anyhow::bail!("attach: unknown run {run}");
    };
    let (snapshot, live) = capture.attach();
    out.write_all(&snapshot).await?;
    out.flush().await?;
    let Some(mut rx) = live else {
        return Ok(()); // run already finished: the ring replay is the whole story
    };
    loop {
        match rx.recv().await {
            Ok(chunk) => {
                if chunk.is_empty() {
                    return Ok(()); // completion sentinel
                }
                out.write_all(&chunk).await?;
                out.flush().await?;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                // Best-effort live proxy: a slow attach client skips ahead;
                // the authoritative bytes live in the ring + CAS manifest.
                tracing::debug!(dropped = n, "attach subscriber lagged; skipping ahead");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}
