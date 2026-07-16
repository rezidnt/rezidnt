//! S1 run substrate wiring: `open` materialization, the daemon-owned run
//! registry, and per-run capture (ring + CAS chunks).
//!
//! Ownership rule (S1 exit criterion): a run belongs to the daemon, never to
//! the connection that requested it — `materialize_open` is spawned as a
//! detached task and every run gets its own detached task, so a client
//! disconnect mid-run kills nothing but the socket.
#![cfg(unix)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use rezidnt_cas::Cas;
use rezidnt_fabric::Fabric;
use rezidnt_run::RunId;
use rezidnt_run::adapter::{ClaudeCodeAdapter, MESSAGE_INLINE_CAP, MappedFact};
use rezidnt_run::badge::Badge;
use rezidnt_run::capture::{DEFAULT_CHUNK_BYTES, DEFAULT_RING_BYTES, RingBuffer, chunk_into_cas};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::{AgentSpec, ProjectSpec};
use rezidnt_types::{Event, SourceId, Subject, WorkspaceId};
use serde_json::json;
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

/// Shared daemon context handed to connection and run tasks.
pub struct Daemon {
    pub fabric: Arc<Fabric>,
    pub cas: Arc<Cas>,
    pub registry: Arc<RunRegistry>,
}

/// Publish one event off the async threads (SQLite append is blocking);
/// returns the event id for causation chaining.
async fn publish(fabric: &Arc<Fabric>, event: Event) -> anyhow::Result<Ulid> {
    let id = event.id;
    let fabric = Arc::clone(fabric);
    tokio::task::spawn_blocking(move || fabric.publish(event))
        .await
        .context("publish task panicked")?
        .context("append to log")?;
    Ok(id)
}

/// The `rezidnt open` materialization chain (S1 exit criterion). One
/// correlation ULID spans the whole chain; causation chains each fact to its
/// trigger. Every failure is traced and mirrored as `daemon.warning` so a
/// failed open is visible in `tail`, not silent.
pub async fn materialize_open(daemon: Arc<Daemon>, spec_toml: String) {
    if let Err(e) = try_materialize_open(&daemon, &spec_toml).await {
        tracing::warn!(error = %e, "open materialization failed");
        let warning = Event::new(
            SourceId::new("daemon"),
            None,
            Subject::new("daemon.warning"),
            Ulid::new(),
            None,
            1,
            json!({"what": "open-failed", "error": format!("{e:#}")}),
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
}

async fn try_materialize_open(daemon: &Arc<Daemon>, spec_toml: &str) -> anyhow::Result<()> {
    let spec = ProjectSpec::from_toml_str(spec_toml).context("parse project spec")?;
    let correlation = Ulid::new();
    let workspace = WorkspaceId::new(Ulid::new());

    // Workspace root = the spec's repo path (relative paths resolve against
    // the daemon cwd in S1 — the spec arrived over the wire, not from a file).
    let root = tokio::fs::canonicalize(&spec.repo)
        .await
        .with_context(|| format!("canonicalize workspace root {}", spec.repo.display()))?;

    // 1. workspace.opened — the envelope workspace id is the entity key.
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
        )?,
    )
    .await?;

    // 2. workspace.spec.applied — the applied spec TOML persists to the CAS.
    let spec_ref = {
        let cas = Arc::clone(&daemon.cas);
        let bytes = spec_toml.as_bytes().to_vec();
        tokio::task::spawn_blocking(move || cas.put(&bytes, "application/toml"))
            .await
            .context("cas put task panicked")?
            .context("store spec in cas")?
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
        )?,
    )
    .await?;

    // 3–6. Per agent: allocate a worktree, spawn under capture, detach.
    for agent in &spec.agents {
        launch_agent(daemon, agent, &root, workspace, correlation, applied_id)
            .await
            .with_context(|| format!("launch agent {:?}", agent.name))?;
    }
    Ok(())
}

async fn launch_agent(
    daemon: &Arc<Daemon>,
    agent: &AgentSpec,
    repo: &Path,
    workspace: WorkspaceId,
    correlation: Ulid,
    applied_id: Ulid,
) -> anyhow::Result<()> {
    let run = RunId::new(Ulid::new());

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
            Some(applied_id),
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

    // 5–6. The run task: daemon-owned, detached from every connection.
    let stdout = child.stdout.take().context("child stdout must be piped")?;
    let ctx = RunTaskContext {
        daemon: Arc::clone(daemon),
        run,
        workspace,
        correlation,
        spawned_id,
        capture,
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
    Ok(())
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
