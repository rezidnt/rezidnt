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
use rezidnt_gate::permit::PermitVerifierSpec;
use rezidnt_run::RunId;
use rezidnt_run::adapter::{ClaudeCodeAdapter, MESSAGE_INLINE_CAP, MappedFact};
use rezidnt_run::badge::{Caveat, Macaroon, RootKey};
use rezidnt_run::capture::{DEFAULT_CHUNK_BYTES, DEFAULT_RING_BYTES, RingBuffer, chunk_into_cas};
use rezidnt_run::compose::{ComposedChild, ComposedDegrade, compose_degrade, degrade_fact};
use rezidnt_run::egress::{EgressPolicy, EgressProxy, PastaProxy};
use rezidnt_run::sandbox::{Bind, SandboxPolicy, SandboxSubstrate, bwrap_argv};
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
    /// DR-017 §Decision 6 (SP4b): the daemon's process-lifetime macaroon root
    /// key — minted ONCE at construction, held for the process, NEVER on the
    /// fabric (design §4: like the operator-badge secret). The daemon MINTS
    /// agent badges against it (`launch_agent`); the shared `McpCore` VERIFIES
    /// them against a clone of it (`main.rs` → `with_root_key`). A restart
    /// re-mints — run-scoped agent badges never survive a restart, the accepted
    /// DR-017 §Decision 4/6 model.
    pub root_key: RootKey,
    /// The HOST-LEVEL admin permit layer (SP4c-wire, DR-020 §Decision 1): the
    /// resolved `[gates.permit]` verifier set sourced from a host config file
    /// OUTSIDE any workspace spec (via `REZIDNT_ADMIN_PERMIT`), STAMPED
    /// [`PermitLayer::Admin`]. `permit_config_for` merges it FIRST via
    /// `compose_layers(admin, dev, session)`, so a dev cannot edit or reorder it
    /// (the authority boundary). EMPTY when no admin source is wired — the
    /// pre-SP4c single-source (dev-only) behavior, no regression.
    pub admin_permit: Vec<PermitVerifierSpec>,
}

impl Daemon {
    pub fn new(fabric: Arc<Fabric>, cas: Arc<Cas>, registry: Arc<RunRegistry>) -> Self {
        Self {
            fabric,
            cas,
            registry,
            workspaces: tokio::sync::Mutex::new(HashMap::new()),
            // One key per Daemon instance = process-lifetime (a test that builds
            // a fresh Daemon gets its own key; the production daemon builds one).
            root_key: RootKey::mint(),
            // No admin source unless `main.rs` wires one (DR-020): absent env var
            // ⇒ empty admin layer ⇒ unchanged single-source path.
            admin_permit: Vec::new(),
        }
    }

    /// Wire the host-level admin permit layer (SP4c-wire, DR-020 §Decision 1;
    /// builder-style, `main.rs` calls it after reading `REZIDNT_ADMIN_PERMIT`).
    /// The specs are already resolved + STAMPED [`PermitLayer::Admin`].
    pub fn with_admin_permit(mut self, admin: Vec<PermitVerifierSpec>) -> Self {
        self.admin_permit = admin;
        self
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
    //    A permit-gated agent (its spec declares a `[gates.permit]` gate) also
    //    gets the PEP wired at spawn (DR-014 §Decision 2): `REZIDNT_RUN` +
    //    `REZIDNT_SOCKET` in the env and a `PreToolUse` hook config naming
    //    `rezidnt permit-hook`, written into the worktree's `.claude/settings.json`.
    //    A non-permit agent spawns exactly as today (no injection).
    //
    //    SP4b (DR-017): the badge is now a MACAROON, not a DR-005 opaque token.
    //    The BASE run capability (the "lead" badge) is minted against the daemon
    //    root key over the run's SCOPE — `base_caveats` = workspace + expiry, both
    //    deterministic from the run (permissive enough that the run's own governed
    //    spawn/open/merge calls verify; NARROWING verbs/roles is attenuation's job,
    //    not the base mint). When the spec declares an RBAC `role` (SP4a — the
    //    sub-agent narrowing signal, DR-016 §Decision 3), the injected badge is the
    //    base ATTENUATED with a `Role` caveat, and the daemon records the
    //    capability edge as a `permit.delegated` fact (I3, the replayable chain).
    //    The token INJECTED under REZIDNT_BADGE is `.to_wire()` — inline, never CAS
    //    (I2). `badge_id` on agent.spawned is `hex(blake3(sig)[..8])`
    //    (`macaroon.badge_id()`, DR-018 §Decision (a)), the unchanged loggable
    //    shape (8-byte hex prefix) over a sig-derived pre-image.
    let base_expiry = rezidnt_run::badge::expiry_from_now(rezidnt_run::badge::DEFAULT_BADGE_TTL);
    let base_caveats = rezidnt_run::badge::base_caveats(&workspace.ulid().to_string(), base_expiry);
    let base_badge = Macaroon::mint(&daemon.root_key, run_str.clone(), base_caveats);
    // The badge the sub-agent actually runs under: the base, narrowed by the
    // declared role if any, via a TRUE OFFLINE ATTENUATION (DR-018 §Decision (a)).
    // The child SHARES the base's identifier — `attenuate` appends the `Role`
    // caveat and re-keys the running sig with NO root key, NO fresh identifier.
    // Distinct ends of the capability edge come from the sig-derived `badge_id`
    // (`hex(blake3(sig)[..8])`), which re-keys per hop, NOT from a different
    // identifier — so parent ≠ child while the offline property (the reason
    // SP4b exists) is preserved and the child caveat set is a strict superset
    // of the base (monotone narrowing — I6). A roleless spawn injects the base
    // badge directly (no delegation, no fact).
    let role_delegation = agent.role.as_ref().map(|role| {
        let added = Caveat::Role { role: role.clone() };
        let child = base_badge.attenuate(added.clone());
        (child, added)
    });
    let injected_badge = role_delegation
        .as_ref()
        .map(|(child, _)| child)
        .unwrap_or(&base_badge);
    let badge_wire = injected_badge.to_wire();

    let pep_enforced = agent.gates.iter().any(|g| g == "permit");
    let plan = if pep_enforced {
        let socket = rezidnt_proto::socket_path();
        SpawnPlan::for_claude_code_permit(
            agent,
            &badge_wire,
            std::env::vars(),
            &run_str,
            &socket.to_string_lossy(),
        )
    } else {
        SpawnPlan::for_claude_code(agent, &badge_wire, std::env::vars())
    };
    // Write the PreToolUse hook settings into the worktree so claude-code loads
    // them (design §3(2)). Best-effort surface: a settings-write failure must
    // not silently drop enforcement, so it is a hard error here.
    if let Some(hook_config) = plan.permit_hook_config() {
        let settings_dir = worktree.join(".claude");
        tokio::fs::create_dir_all(&settings_dir)
            .await
            .with_context(|| format!("create {}", settings_dir.display()))?;
        let settings_path = settings_dir.join("settings.json");
        tokio::fs::write(&settings_path, hook_config)
            .await
            .with_context(|| format!("write permit hook settings {}", settings_path.display()))?;
    }
    // C3 COMPOSED SPAWN (DR-028): the raw `Command::new(&plan.bin)` bypass is GONE.
    // The harness spawns THROUGH the composed confinement path — pasta -> bwrap ->
    // agent on one shared netns when both backends are up, degrading to confined +
    // CLOSED (bwrap alone, sealed netns) or loud unsandboxed otherwise. The binds /
    // allowlist fold ONLY through `from_folded_authority` (C6 preserved), and the
    // decided composed state is recorded as a distinct loud fact below.
    let (mut composed_child, composed_degrade) = compose_spawn(&plan, &worktree)
        .with_context(|| format!("composed spawn of harness {}", plan.bin.display()))?;
    let started = std::time::Instant::now();

    // The composed-spawn/degrade fact — the durable record that the spawn went
    // THROUGH the composition decision (not a silent raw spawn). Each of the three
    // states carries its distinct loud posture (DR-028 §Decision 4). WARDEN-GATED
    // placeholder subject (the `sandbox.*`/`egress.*` family is a deferred
    // `/subject`, DR-028 §Consequences); keyed off the posture fields the fold
    // suites pin, not a ratified name.
    let (degrade_subject, mut degrade_payload) = degrade_fact(&composed_degrade, &run_str);
    if let Some(obj) = degrade_payload.as_object_mut() {
        obj.insert("agent".to_string(), json!(agent.name));
        obj.insert("backend".to_string(), json!(composed_child.backend()));
    }
    publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("rezidnt-run"),
            Some(workspace),
            Subject::new(degrade_subject),
            correlation,
            Some(allocated_id),
            1,
            degrade_payload,
        )?,
    )
    .await?;

    let mut spawned_payload = json!({
        "run": run,
        "agent": agent.name,
        "harness": agent.harness,
        // SP4b: the loggable id of the badge the agent actually runs under (the
        // role-narrowed child when a role was declared, else the base),
        // `hex(blake3(sig)[..8])` — the sig-derived shape (DR-018 §Decision (a)).
        "badge_id": injected_badge.badge_id(),
    });
    if let (Some(pid), Some(obj)) = (
        composed_child.child_mut().id(),
        spawned_payload.as_object_mut(),
    ) {
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
    // DR-014 §Decision 5 / ontology `agent.spawned.pep?`: record `pep:
    // "enforced"` iff the permit PEP was wired at spawn (the agent declared a
    // `[gates.permit]` gate), so `gate_explain` distinguishes a
    // mid-run-PEP-enforced run from an edge-gated-only one (I4). ABSENT when no
    // PEP was wired — never synthesized to `false`/`"unenforced"` (DR-012
    // declared-vs-absent discipline; absence is the honest "no PEP wired").
    if pep_enforced && let Some(obj) = spawned_payload.as_object_mut() {
        obj.insert("pep".to_string(), json!("enforced"));
    }
    // SP4a permit input axis (DR-016 §Decision 2 / ontology `agent.spawned.role?`):
    // record the RBAC role VERBATIM iff the spec declared one, so a role-keyed
    // permit policy can decide on it (role rides the run's derived state). ABSENT
    // when no role was declared — never synthesized to a default like
    // "contributor" (DR-012 declared-vs-absent; mirrors the `pep`/`harness_version`
    // absent-is-honest gate). A declared empty string is emitted as `role: ""`,
    // distinct from absent.
    if let (Some(role), Some(obj)) = (&agent.role, spawned_payload.as_object_mut()) {
        obj.insert("role".to_string(), json!(role));
    }

    // SP4b (DR-017 §Decision 2): the capability-chain fact. When the injected
    // badge is a role-NARROWED child of the run's base (lead) badge, record the
    // delegation edge as a durable `permit.delegated` fact BEFORE `agent.spawned`
    // — the reducer folds it onto the run's dossier so the chain replays (I3).
    // `parent_badge_id`/`child_badge_id` are the two `hex(blake3(sig)[..8])`
    // sig-derived ends of the edge — distinct because `attenuate` re-keys the
    // sig, under a SHARED identifier (DR-018 §Decision (a); NEVER the token —
    // I2/§12); `added_caveats` folds VERBATIM
    // the tagged first-party `Caveat` JSON. A roleless spawn has no delegation
    // and emits no fact (no consumer-less noise).
    if let Some((child, added)) = &role_delegation {
        let added_caveats = vec![serde_json::to_value(added).unwrap_or(serde_json::Value::Null)];
        publish(
            &daemon.fabric,
            Event::new(
                SourceId::new("rezidnt-run"),
                Some(workspace),
                Subject::new("permit.delegated"),
                correlation,
                Some(allocated_id),
                1,
                json!({
                    "run": run,
                    "parent_badge_id": base_badge.badge_id(),
                    "child_badge_id": child.badge_id(),
                    "added_caveats": added_caveats,
                }),
            )?,
        )
        .await?;
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
    // Drain the composed child's PIPED stdout (the capture seam) off the borrow,
    // then move the OWNED `tokio::process::Child` into the run task where the
    // daemon reaper adopts it (S1 — the daemon owns the composed process, DR-028
    // §Decision 2). No detached orphan waiter: the same child the run loop drains
    // is the child the reaper `wait()`s.
    let stdout = composed_child
        .child_mut()
        .stdout
        .take()
        .context("composed child stdout must be piped")?;
    let child = composed_child.into_child();
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

/// Fold the honestly-minimal C3 policies for one run FROM FOLDED AUTHORITY ONLY
/// (DR-028 §Decision 3; C6/DR-024 preserved end-to-end). The `[gates.permit]`/role
/// bind+allowlist+secret fold field does not exist yet, so this slice's
/// honestly-scoped FIRST source is the run state the daemon already holds:
/// - the SANDBOX binds fold to the allocated worktree (writable) plus the
///   read-only toolchain roots the confined harness needs to resolve — the minimal
///   confinement that lets a real run execute, NEVER a run-supplied bind.
/// - the EGRESS allowlist is EMPTY: the spec carries no egress config, so absent
///   means DENY-ALL (a sealed netns whose proxy allows nothing), never open.
///
/// Both go through `from_folded_authority` — the sole door; no `SpawnPlan`/request
/// value reaches either policy (the private-field guard). The confined program's
/// OWN binary directory is folded read-only too, so bwrap can `execvp` the harness
/// inside its sealed mount-ns (DR-028 §Decision 1); the harness identity is
/// DECLARED authority computed daemon-side (`plan.bin`), not a request-time value —
/// `compose::confined_program_binds` is the single shared definition of "bind what
/// you're about to exec". Returns `(sandbox, egress)`.
fn fold_c3_policies(plan: &SpawnPlan, worktree: &Path) -> (SandboxPolicy, EgressPolicy) {
    // Sandbox binds, in bwrap APPLICATION ORDER (later binds override earlier ones
    // on overlapping paths — so the WRITABLE worktree must come LAST, or a broader
    // read-only bind that happens to be an ANCESTOR of the worktree would shadow it
    // read-only). `unshare_all = true` — the composed spawn drops the net unshare
    // only when egress is active (DR-028 §Decision 1), inside compose.
    //
    // 1. Read-only toolchain roots (the C3a usr-merged discipline — the confined
    //    harness's interpreter/libs resolve inside the namespace).
    let mut binds = vec![
        Bind::read_only("/usr"),
        Bind::read_only("/bin"),
        Bind::read_only("/lib"),
        Bind::read_only("/lib64"),
        Bind::read_only("/etc"),
    ];
    // 2. The harness binary's own directory (read-only) so bwrap can exec it inside
    //    the sealed mount-ns — a harness living outside the toolchain binds (e.g. a
    //    cargo-target example, or an installed `claude` under $HOME) is otherwise
    //    `No such file or directory` under bwrap even though it exists on the host.
    //    Folded from the DECLARED harness path daemon-side (C6 holds — same door).
    //    Placed BEFORE the worktree so that when the harness dir is an ancestor of
    //    the worktree (e.g. a test tmpdir holding both), the writable worktree bind
    //    below still WINS — the confined harness must be able to write its worktree.
    binds.extend(rezidnt_run::compose::confined_program_binds(plan));
    // 3. The WRITABLE worktree — LAST, so it overrides any read-only ancestor bind
    //    above. The confined harness edits its worktree; a shadowing read-only bind
    //    would break the golden-path diff (the harness could not write its change).
    binds.push(Bind::writable(worktree));
    let sandbox = SandboxPolicy::from_folded_authority(binds, true);
    // Egress: empty spec ⇒ empty allowlist ⇒ deny-all (absent never means open).
    let egress = EgressPolicy::from_folded_authority(Vec::new(), std::collections::BTreeMap::new());
    (sandbox, egress)
}

/// The composed spawn (DR-028 §Decision 1/2/4) — the C3 wiring that REPLACES the
/// raw `Command::new(&plan.bin)` bypass. Probes the sandbox + egress backends,
/// decides the composed degrade state, and spawns the harness THROUGH the composed
/// confinement path, returning a daemon-owned [`ComposedChild`] (the S1 handle the
/// run loop drains + the reaper adopts) plus the [`ComposedDegrade`] the caller
/// records as a distinct loud fact.
///
/// - **Mediated** (sandbox-up + egress-up): `pasta -> bwrap(minus-net) -> harness`
///   over one shared netns (composed argv).
/// - **ConfinedClosed** (sandbox-up + egress-down): `bwrap(--unshare-all) -> harness`
///   — confined + a sealed netns, no network (DR-026's CLOSED degrade composed).
/// - **Unsandboxed** (sandbox-down): the harness spawns raw (DR-025's loud-OPEN
///   degrade) — the caller's fact declares egress un-enforceable, never a silent
///   claim of mediation.
///
/// Binds/allowlist reach the wrapper ONLY via `fold_c3_policies` (C6 preserved).
fn compose_spawn(
    plan: &SpawnPlan,
    worktree: &Path,
) -> anyhow::Result<(ComposedChild, ComposedDegrade)> {
    // Resolve the harness binary FIRST — a nonexistent harness is a spawn-time
    // failure (`open-failed`), exactly as the raw `Command::new(&plan.bin).spawn()`
    // it replaces: `Command::new("/nonexistent")` returned ENOENT. Routing through
    // bwrap would otherwise turn that into `Command::new("bwrap")` (which exists)
    // succeeding and the confined child failing later — a run-time error, not a
    // spawn error, breaking the S1 partial-open contract. Also prevents
    // `confined_program_binds` from binding a nonexistent directory (bwrap would
    // reject it with a confusing error). A bare name is resolved via PATH.
    if !harness_binary_resolves(&plan.bin) {
        anyhow::bail!(
            "harness binary {} does not exist or is not on PATH",
            plan.bin.display()
        );
    }
    let (sandbox_policy, egress_policy) = fold_c3_policies(plan, worktree);

    // Probe the two backends (a missing tool is a VERDICT, never a crash — the
    // could-not-run discipline).
    let bwrap = rezidnt_run::sandbox::BwrapSubstrate::default();
    let pasta = PastaProxy::default();
    let sandbox_avail = bwrap.availability();
    let egress_avail = pasta.availability();
    let mut degrade = compose_degrade(&sandbox_avail, &egress_avail);

    // HONESTY: the Mediated posture requires a LIVE proxy the sealed netns routes
    // to. This slice's honestly-minimal fold is deny-all (an EMPTY egress
    // allowlist), so there is NO allowlisted host and the daemon starts no per-run
    // proxy dataplane here (the live proxy is the `start_composed_dataplane` path
    // the enforce/shared-netns suite drives, not the real agent run yet). With
    // nothing to mediate and no proxy address to route to, the honest posture is
    // confined + CLOSED — a SEALED netns with no network — NOT a claim of
    // mediation over a proxy that does not exist (the overclaim DR-028 forbids).
    // When the folded allowlist is empty, downgrade a Mediated decision to
    // ConfinedClosed for the SPAWN + the fact. A non-empty allowlist (a later
    // slice that folds real egress config) takes the true Mediated pasta-outer arm.
    if degrade == ComposedDegrade::Mediated && egress_policy.allowlist().is_empty() {
        degrade = ComposedDegrade::ConfinedClosed;
    }

    let bwrap_bin = "bwrap";
    // The proxy address the sealed netns would route to under true mediation. Unused
    // in the current arms (Mediated with a live proxy is a later slice); kept for
    // the composed-argv render seam.
    let proxy_addr = "127.0.0.1:9";

    // Build the composed command per the decided state. Every arm pipes stdout
    // (the capture seam), nulls stdin/stderr, and runs in the worktree — the same
    // S1 lifecycle the raw spawn had, now through confinement.
    let mut cmd = match degrade {
        ComposedDegrade::Mediated => {
            // pasta -> bwrap(minus-net) -> harness, one shared netns. The composed
            // argv is argv[0]=pasta … -- bwrap … -- <harness> <args>. bwrap resets
            // cwd inside its sealed mount-ns, so inject `--chdir <worktree>` after
            // the bwrap token — the confined harness runs IN the worktree (its
            // relative paths resolve), matching the raw spawn's `.current_dir`.
            let mut argv = rezidnt_run::compose::composed_argv(
                plan,
                &sandbox_policy,
                /* egress_active */ true,
                proxy_addr,
            );
            insert_bwrap_chdir(&mut argv, worktree);
            argv_to_command(&argv)
        }
        ComposedDegrade::ConfinedClosed => {
            // bwrap alone with the full --unshare-all (net sealed, no route) — the
            // confined + CLOSED spawn. No pasta: the sealed netns has no network.
            let mut argv = vec![bwrap_bin.to_string()];
            argv.extend(bwrap_argv(plan, &sandbox_policy));
            // Run the confined harness IN the worktree (bwrap resets cwd otherwise).
            argv.push("--chdir".to_string());
            argv.push(worktree.to_string_lossy().into_owned());
            argv.push("--".to_string());
            argv.push(plan.bin.to_string_lossy().into_owned());
            argv.extend(plan.args.iter().cloned());
            argv_to_command(&argv)
        }
        ComposedDegrade::Unsandboxed => {
            // Sandbox down ⇒ the harness spawns raw (DR-025 loud-OPEN degrade). The
            // caller emits the loud egress-un-enforceable fact; NO silent claim of
            // confinement or mediation is made.
            let mut cmd = tokio::process::Command::new(&plan.bin);
            cmd.args(&plan.args);
            cmd
        }
    };

    let child = cmd
        .env_clear()
        .envs(plan.env.iter().cloned())
        .current_dir(worktree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("composed spawn ({:?}) of {}", degrade, plan.bin.display()))?;

    let backend = match degrade {
        ComposedDegrade::Mediated => "pasta+bwrap",
        ComposedDegrade::ConfinedClosed => "bwrap",
        ComposedDegrade::Unsandboxed => "none",
    };
    Ok((ComposedChild::new(backend, child), degrade))
}

/// Does the harness binary `bin` resolve to an existing executable — either an
/// existing path (absolute/relative with a directory component) or a bare name on
/// `PATH`? Reproduces the ENOENT semantics of the raw `Command::new(bin).spawn()`
/// the composed spawn replaces, so a nonexistent harness fails at SPAWN time (the
/// S1 `open-failed` contract) rather than turning into a bwrap run-time failure.
fn harness_binary_resolves(bin: &Path) -> bool {
    // A path with a directory component (absolute or `./x`) must exist on disk.
    if bin.components().count() > 1 || bin.is_absolute() {
        return bin.exists();
    }
    // A bare name resolves via PATH — search each entry for an existing file.
    let name = bin.as_os_str();
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).exists()))
        .unwrap_or(false)
}

/// Build a `tokio::process::Command` from a rendered argv (`argv[0]` is the
/// program, the rest are its args). The composed wrapper argv is a flat vector;
/// this lifts it back into a command the run loop spawns.
fn argv_to_command(argv: &[String]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd
}

/// Inject `--chdir <worktree>` into the bwrap layer of a composed argv so the
/// confined harness runs IN the worktree (bwrap otherwise resets cwd to `/` inside
/// its sealed mount-ns, breaking a harness's relative paths). Inserts right after
/// the `bwrap` program token — the first `"bwrap"`-suffixed argv element after the
/// pasta handoff. A no-op if no bwrap token is found (defensive; the Mediated arm
/// always renders one). `--chdir` is a bwrap directive, folded from the worktree
/// the daemon allocated — not a run-supplied value (C6 holds).
fn insert_bwrap_chdir(argv: &mut Vec<String>, worktree: &Path) {
    if let Some(pos) = argv.iter().position(|a| a.ends_with("bwrap")) {
        let wt = worktree.to_string_lossy().into_owned();
        argv.insert(pos + 1, "--chdir".to_string());
        argv.insert(pos + 2, wt);
    }
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
