//! The daemon's implementation of the MCP substrate seam (I4): the
//! `rezidnt-mcp` core stays transport- and substrate-agnostic; this bridge
//! maps its two mutating tools onto the S1 run substrate in `runs.rs`.
//!
//! Refusal discipline (§12, S3 board): every failure is a machine-readable
//! `ToolRefusal` code — the MCP surface never answers with prose alone, and a
//! refused mutation leaves the log untouched.
#![cfg(unix)]

use std::sync::Arc;

use rezidnt_gate::permit::{PermitLayer, PermitVerifierSpec};
use rezidnt_mcp::{
    BoxFuture, FanOutOutcome, KillAck, McpSubstrate, OpenAck, PermitConfig, ToolRefusal, codes,
};
use rezidnt_run::RunId;
use rezidnt_types::WorkspaceId;
use rezidnt_types::mcp::FanOutTask;
use ulid::Ulid;

use crate::runs::{Daemon, LeadDelegation, begin_open, launch_agent};

/// Ontology cap on `agent.spawned.idempotency_key` (DEFAULT, ratified
/// 2026-07-17): a key is a short opaque token, trivially inside I2.
const IDEMPOTENCY_KEY_MAX_BYTES: usize = 256;

/// Bridges [`McpSubstrate`] onto the daemon's run substrate.
pub struct McpBridge {
    pub daemon: Arc<Daemon>,
}

impl McpSubstrate for McpBridge {
    fn open_project(&self, spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>> {
        let daemon = Arc::clone(&self.daemon);
        Box::pin(async move {
            // warn_on_refuse = false: an MCP refusal happens before any side
            // effect and puts nothing on the log (S3 board pin); post-ack
            // materialization failures still surface as daemon.warning.
            match begin_open(&daemon, &spec_toml, false).await {
                Ok((workspace, correlation)) => Ok(OpenAck {
                    workspace: workspace.ulid().to_string(),
                    correlation: correlation.to_string(),
                }),
                Err(refusal) => Err(ToolRefusal::new(codes::SPEC_INVALID, refusal.message)),
            }
        })
    }

    fn spawn_agent(
        &self,
        workspace: String,
        agent: String,
        idempotency_key: String,
    ) -> BoxFuture<Result<String, ToolRefusal>> {
        let daemon = Arc::clone(&self.daemon);
        Box::pin(async move {
            let ws = Ulid::from_string(&workspace).map_err(|_| {
                ToolRefusal::new(
                    codes::WORKSPACE_UNKNOWN,
                    format!("workspace {workspace:?} is not a ULID"),
                )
            })?;

            // Ontology constraint on `agent.spawned.idempotency_key` (v1,
            // additive 2026-07-17): non-empty, ≤ 256 bytes UTF-8 — enforced
            // before any effect so a refused spawn leaves the log untouched.
            if idempotency_key.is_empty() || idempotency_key.len() > IDEMPOTENCY_KEY_MAX_BYTES {
                return Err(ToolRefusal::new(
                    codes::ARGS_INVALID,
                    format!(
                        "idempotency_key must be non-empty and at most \
                         {IDEMPOTENCY_KEY_MAX_BYTES} bytes UTF-8"
                    ),
                ));
            }

            // The registry lock is held across the whole spawn: a concurrent
            // retry with the same key waits here and then hits the key map —
            // idempotency without a double spawn (§9).
            let mut workspaces = daemon.workspaces.lock().await;
            let entry = workspaces.get(&ws).ok_or_else(|| {
                ToolRefusal::new(
                    codes::WORKSPACE_UNKNOWN,
                    format!("workspace {workspace} is not open on this daemon"),
                )
            })?;
            if let Some(run) = entry.spawn_keys.get(&idempotency_key) {
                return Ok(run.to_string());
            }
            let agent_spec = entry
                .agents
                .iter()
                .find(|a| a.name == agent)
                .cloned()
                .ok_or_else(|| {
                    ToolRefusal::new(
                        codes::AGENT_UNKNOWN,
                        format!("workspace {workspace} defines no agent {agent:?}"),
                    )
                })?;
            let root = entry.root.clone();
            let gate_defs = entry.gates.clone();
            let egress_spec = entry.egress.clone();

            let root = tokio::fs::canonicalize(&root).await.map_err(|e| {
                ToolRefusal::new(
                    codes::SPAWN_FAILED,
                    format!("canonicalize workspace root {}: {e}", root.display()),
                )
            })?;
            // A standalone spawn starts its own causal chain: fresh
            // correlation, no causation fact.
            let run = launch_agent(
                &daemon,
                &agent_spec,
                &root,
                WorkspaceId::new(ws),
                Ulid::new(),
                None,
                &gate_defs,
                &egress_spec,
                // Recorded on the agent.spawned payload so the key→run map is
                // log-derivable across restart (I3; envelope workspace is set
                // by launch_agent, the ontology's keyed-spawn obligation).
                Some(&idempotency_key),
                // A standalone `spawn_agent` has no delegating lead: allocator
                // stays `"rezidnt"`, no delegation edge (DR-044 §Decision 3).
                None,
            )
            .await
            .map_err(|e| ToolRefusal::new(codes::SPAWN_FAILED, format!("{e:#}")))?;

            if let Some(entry) = workspaces.get_mut(&ws) {
                entry.spawn_keys.insert(idempotency_key, run.ulid());
            }
            Ok(run.ulid().to_string())
        })
    }

    /// Resolve the applied `[gates.permit]` verifier set for a run (SP-wire,
    /// DR-011 §1). The run→workspace link is log-derived (the `agent.spawned`
    /// fact's envelope `workspace`, I3); the permit gate config is then read from
    /// the opened-workspace registry (`gates["permit"]`, folded from
    /// `workspace.spec.applied`). BOTH native and exec verifiers dispatch on the
    /// permit axis (SP3, DR-015 §Decision 1): natives resolve by name, an exec
    /// entry carries its argv to `aggregate_async` for §8 dispatch. `None` when
    /// the run maps to no workspace or the workspace configures no permit gate —
    /// the PDP then degrades to escalate/deny (I6), never a synthesized allow.
    fn permit_config_for(&self, run: String) -> BoxFuture<Option<PermitConfig>> {
        let daemon = Arc::clone(&self.daemon);
        Box::pin(async move {
            // 1. run → workspace, log-derived from the run's `agent.spawned`
            //    envelope (I3: the honest source, not a side table).
            let ws = run_workspace(&daemon, &run).await?;

            // 2. workspace → applied `[gates.permit]` verifier set. The registry
            //    is derived state folded from `workspace.spec.applied` (I3). This
            //    is the DEV layer (SP4c-wire, DR-020 §Decision 1): the
            //    dev-editable source, stamped `PermitLayer::Dev`.
            let workspaces = daemon.workspaces.lock().await;
            let entry = workspaces.get(&ws)?;
            let gate = entry.gates.get("permit")?;
            let dev: Vec<PermitVerifierSpec> = gate
                .verifiers
                .iter()
                // SP3 (DR-015 §Decision 1/4): the permit axis dispatches BOTH
                // kinds. A native carries by name; an exec entry is no longer
                // dropped — it carries as an exec `PermitVerifierSpec` (argv from
                // its `exec` path, display name from `name` or the path), which
                // `aggregate_async` runs through `ExecVerifier` (§8 contract,
                // network-off + scrubbed env). rezidnt dispatches the operator's
                // argv; it never bundles a policy engine (I7).
                //
                // SP4c-wire (DR-020 §Decision 1): STAMPED `PermitLayer::Dev` — this
                // source is the workspace spec, the middle authority. `permit.rs`
                // constructors default to Session, so the layer is set explicitly.
                .filter_map(|v| {
                    if let Some(name) = &v.native {
                        Some(PermitVerifierSpec::native_in_layer(
                            PermitLayer::Dev,
                            name.clone(),
                            v.params.clone(),
                        ))
                    } else if let Some(exec) = &v.exec {
                        let name = v.name.clone().unwrap_or_else(|| exec.display().to_string());
                        Some(PermitVerifierSpec::exec_in_layer(
                            PermitLayer::Dev,
                            name,
                            vec![exec.display().to_string()],
                            v.params.clone(),
                        ))
                    } else {
                        None
                    }
                })
                .collect();

            // 3. Compose the THREE sourced layers (SP4c-wire, DR-020 §Decision
            //    1/2): ADMIN from the host source (outside the workspace spec,
            //    already Admin-stamped on the `Daemon`), DEV from the workspace
            //    spec above, SESSION from the run/agent scope (empty for now — the
            //    future per-run session layer). `compose_layers` concatenates them
            //    admin→dev→session; the flat aggregate consumer path is UNCHANGED
            //    (frozen by DR-019 Decision 1). Only the RESOLUTION merges three
            //    layers with provenance instead of reading one.
            let admin = daemon.admin_permit.clone();
            let session: Vec<PermitVerifierSpec> = Vec::new();
            Some(PermitConfig::from_specs(
                rezidnt_gate::permit::compose_layers(admin, dev, session),
            ))
        })
    }

    /// DR-032 §Decision 1: drive the EXISTING reaper to terminate the run's
    /// process. The pid is LOG-DERIVED — read from the run's `agent.spawned`
    /// fact (`runs.rs` records it on the spawn payload, I3: the log is truth,
    /// never a side table). The signal logic is REUSED, not reimplemented:
    /// `reaper::stop_with_escalation` performs TERM → grace → KILL. A run the log
    /// never spawned, or one whose spawn carried no pid (a run that never
    /// launched a process), is refused `RUN_UNKNOWN` — no fact (the operator door
    /// already passed at the core; a refusal here still emits nothing, I3).
    fn kill_run(&self, run: String) -> BoxFuture<Result<KillAck, ToolRefusal>> {
        let daemon = Arc::clone(&self.daemon);
        Box::pin(async move {
            let Some(pid) = run_pid(&daemon, &run).await else {
                return Err(ToolRefusal::new(
                    codes::RUN_UNKNOWN,
                    format!("run {run} has no live process on this daemon"),
                ));
            };
            // REUSE the reaper (reaper.rs:105) — do not reimplement signal logic.
            let description =
                rezidnt_run::reaper::stop_with_escalation(pid, rezidnt_run::reaper::TERM_GRACE)
                    .await
                    .map_err(|e| {
                        ToolRefusal::new(codes::SPAWN_FAILED, format!("stop run {run}: {e}"))
                    })?;
            // The reaper answered on SIGTERM within grace unless it escalated;
            // its description names the outcome. Report the terminal signal +
            // escalation stage for the fact's interrogable "how it stopped".
            let escalated = description.starts_with("escalated");
            Ok(KillAck {
                signal: if escalated { "SIGKILL" } else { "SIGTERM" }.to_string(),
                escalation: Some(if escalated { "kill" } else { "term" }.to_string()),
            })
        })
    }

    /// DR-044 §Decision 1/3 (as amended by DR-046 §Decision 4/5): the live
    /// lead→sub fan-out. Drives the EXISTING rails and adds no substrate (I4):
    /// the same `launch_agent` path `spawn_agent` uses, the same per-workspace
    /// `spawn_keys` dedup map, the same worktree allocation. What is new is the
    /// recorded PRINCIPAL (`allocator: "run:<lead ULID>"` on
    /// `worktree.allocated`) and the lead→sub edge `agent.spawned.lead_run`,
    /// both emitted inside `launch_agent`.
    ///
    /// The core has already run the DR-045 door and the DR-044 §Decision 4 width
    /// cap, so a refused call never reaches here (no allocation, no spawn, no
    /// fact — I3).
    ///
    /// Shape of the work, per task, IN CALL ORDER:
    /// - the task's `idempotency_key` resolves through the EXISTING per-workspace
    ///   `spawn_keys` map (log-derived from `agent.spawned.idempotency_key`, I3
    ///   — it survives a daemon restart because it is a fold, not memory). A hit
    ///   re-returns the SAME run and spawns nothing new, emitting no fact;
    /// - a per-task failure becomes that task's refused outcome and the remaining
    ///   tasks STAND. There is no all-or-nothing rollback: a mid-fan-out failure
    ///   does not un-spawn the subs that already started, and pretending
    ///   otherwise would be a lie (DR-044 §Decision 1);
    /// - a task that mints no run is REFUSED, never a failed sub — there is no
    ///   run to fail (I6, DR-044 §Decision 3).
    ///
    /// Tasks run SEQUENTIALLY under the registry lock, exactly as `spawn_agent`
    /// holds it across one spawn: a concurrent same-key retry waits and then hits
    /// the key map, so idempotency holds without a double spawn (§9). Sequential
    /// also declines to fire N concurrent allocations at the sole-allocator
    /// registry — the load risk DR-044 §Consequences names, against which the
    /// width cap is the only backpressure. There is NO in-daemon retry loop on a
    /// failed allocation: that is a livelock against a sole allocator, and
    /// re-issuing the call with the same keys is the honest retry.
    ///
    /// I2: nothing here reads or carries sub content. Outcomes are run ULIDs and
    /// refusal codes; sub work folds back through the existing per-run CAS-ref
    /// paths (DR-044 §Decision 5).
    fn fan_out(
        &self,
        workspace: String,
        lead_badge_id: String,
        tasks: Vec<FanOutTask>,
    ) -> BoxFuture<Result<Vec<FanOutOutcome>, ToolRefusal>> {
        let daemon = Arc::clone(&self.daemon);
        Box::pin(async move {
            let ws = Ulid::from_string(&workspace).map_err(|_| {
                ToolRefusal::new(
                    codes::WORKSPACE_UNKNOWN,
                    format!("workspace {workspace:?} is not a ULID"),
                )
            })?;

            // The LEAD's run is LOG-DERIVED from the badge id the §12 door
            // verified: fold `agent.spawned.badge_id == lead_badge_id` (I3). No
            // session object, no side table — DR-044 §Decision 5's whole point,
            // and the reason the tool has no caller-declared `lead_run` arg.
            // A badge that maps to no spawned run is a WHOLE-CALL refusal: there
            // is no lead to key the edges on, and an unparented sub is exactly
            // what DR-045 exists to make unreachable.
            let lead_run = badge_run(&daemon, &lead_badge_id).await.ok_or_else(|| {
                ToolRefusal::new(
                    codes::RUN_UNKNOWN,
                    format!(
                        "no run on this log was spawned under badge {lead_badge_id}; fan_out is \
                         lead-only and the lead must be a run this daemon spawned (DR-045 \
                         §Decision 1; the lead's run is what each sub records as \
                         agent.spawned.lead_run, DR-046 §Decision 5)"
                    ),
                )
            })?;

            // The registry lock is held across the WHOLE fan-out (see the doc
            // comment): same discipline as `spawn_agent`, one call wide.
            let mut workspaces = daemon.workspaces.lock().await;
            if !workspaces.contains_key(&ws) {
                return Err(ToolRefusal::new(
                    codes::WORKSPACE_UNKNOWN,
                    format!("workspace {workspace} is not open on this daemon"),
                ));
            }

            // ONE correlation for the whole fan-out: the N sub spawns are one
            // causal unit, and the envelope's `correlation` is the causal chain
            // (design §5). Mirrors the open chain, whose spawns already share the
            // open's correlation.
            let correlation = Ulid::new();
            // The lead's badge id is NOT carried forward: DR-046 §Decision 5
            // makes the lead→sub edge run-to-run, and the verified badge id is
            // already `agent_runs[lead_run].badge_id` off derived state. Its one
            // job here was folding the run above.
            let lead = LeadDelegation { lead_run };

            let mut outcomes = Vec::with_capacity(tasks.len());
            for task in tasks {
                let FanOutTask {
                    agent,
                    idempotency_key,
                } = task;

                // Ontology constraint on `agent.spawned.idempotency_key` (v1):
                // non-empty, ≤ 256 bytes UTF-8. Refused PER TASK, so one bad key
                // costs one sub and the rest of the fan-out stands.
                if idempotency_key.is_empty() || idempotency_key.len() > IDEMPOTENCY_KEY_MAX_BYTES {
                    outcomes.push(refused(
                        agent,
                        idempotency_key,
                        codes::ARGS_INVALID,
                        format!(
                            "idempotency_key must be non-empty and at most \
                             {IDEMPOTENCY_KEY_MAX_BYTES} bytes UTF-8"
                        ),
                    ));
                    continue;
                }

                let Some(entry) = workspaces.get(&ws) else {
                    // Unreachable in practice (checked above, lock held), but the
                    // registry is not this function's invariant to assert.
                    return Err(ToolRefusal::new(
                        codes::WORKSPACE_UNKNOWN,
                        format!("workspace {workspace} closed mid-fan-out"),
                    ));
                };

                // §9 idempotency, PER TASK, through the EXISTING map — no new
                // dedup mechanism (DR-044 §Decision 1). A hit spawns nothing and
                // emits nothing; the run it returns already carries its edge.
                if let Some(run) = entry.spawn_keys.get(&idempotency_key) {
                    outcomes.push(spawned(agent, idempotency_key, run.to_string()));
                    continue;
                }

                let Some(agent_spec) = entry.agents.iter().find(|a| a.name == agent).cloned()
                else {
                    outcomes.push(refused(
                        agent.clone(),
                        idempotency_key,
                        codes::AGENT_UNKNOWN,
                        format!("workspace {workspace} defines no agent {agent:?}"),
                    ));
                    continue;
                };
                let root = entry.root.clone();
                let gate_defs = entry.gates.clone();
                let egress_spec = entry.egress.clone();

                let root = match tokio::fs::canonicalize(&root).await {
                    Ok(root) => root,
                    Err(e) => {
                        outcomes.push(refused(
                            agent,
                            idempotency_key,
                            codes::SPAWN_FAILED,
                            format!("canonicalize workspace root {}: {e}", root.display()),
                        ));
                        continue;
                    }
                };

                match launch_agent(
                    &daemon,
                    &agent_spec,
                    &root,
                    WorkspaceId::new(ws),
                    correlation,
                    None,
                    &gate_defs,
                    &egress_spec,
                    Some(&idempotency_key),
                    // The delegating lead: this is what makes the allocation
                    // record `run:<lead ULID>` and puts `lead_run` on the sub's
                    // `agent.spawned` (DR-044 §Decision 3, DR-046 §Decision 5).
                    Some(&lead),
                )
                .await
                {
                    Ok(run) => {
                        if let Some(entry) = workspaces.get_mut(&ws) {
                            entry.spawn_keys.insert(idempotency_key.clone(), run.ulid());
                        }
                        outcomes.push(spawned(agent, idempotency_key, run.ulid().to_string()));
                    }
                    // A failed task mints NO run, so nothing can fold as `fail` —
                    // it is reported REFUSED (I6, DR-044 §Decision 3) and the
                    // remaining tasks continue. NOTE (honest scope): the failure
                    // that reaches here is whatever `launch_agent` reports,
                    // including a worktree it could not claim. This path allocates
                    // through the daemon's own git-CLI `allocate_worktree`
                    // (`runs.rs`), NOT the git adapter's registry, so it produces
                    // no `worktree.conflict` fact and cannot honestly claim that
                    // code — the refusal carries `spawn.failed` with the real
                    // message rather than a guessed conflict.
                    Err(e) => outcomes.push(refused(
                        agent,
                        idempotency_key,
                        codes::SPAWN_FAILED,
                        format!("{e:#}"),
                    )),
                }
            }
            Ok(outcomes)
        })
    }
}

/// One admitted task's outcome: a run, no refusal.
fn spawned(agent: String, idempotency_key: String, run: String) -> FanOutOutcome {
    FanOutOutcome {
        agent,
        idempotency_key,
        run: Some(run),
        code: None,
        message: None,
    }
}

/// One refused task's outcome: a machine-readable code, NO run — a refused sub
/// is never a failed sub, because no run was minted (I6, DR-044 §Decision 3).
fn refused(agent: String, idempotency_key: String, code: &str, message: String) -> FanOutOutcome {
    FanOutOutcome {
        agent,
        idempotency_key,
        run: None,
        code: Some(code.to_string()),
        message: Some(message),
    }
}

/// Fold the log to find the run spawned under a given badge id — the honest,
/// log-derived lead identity (I3), off the async threads (SQLite replay is
/// blocking). `agent.spawned.badge_id` is a REQUIRED v1 field, so this is a
/// direct read of an existing fact; `None` when no run on the log runs under
/// that badge.
async fn badge_run(daemon: &Arc<Daemon>, badge_id: &str) -> Option<RunId> {
    let fabric = Arc::clone(&daemon.fabric);
    let badge_id = badge_id.to_string();
    tokio::task::spawn_blocking(move || {
        let events = fabric.replay_since(None).ok()?;
        events.into_iter().find_map(|e| {
            if e.subject.as_str() == "agent.spawned"
                && e.payload()["badge_id"].as_str() == Some(&badge_id)
            {
                e.payload()["run"]
                    .as_str()
                    .and_then(|r| Ulid::from_string(r).ok())
                    .map(RunId::new)
            } else {
                None
            }
        })
    })
    .await
    .ok()
    .flatten()
}

/// Fold the log to find a run's pid (recorded on its `agent.spawned` fact by
/// `runs.rs`) — the honest, log-derived liveness key the reaper drives (I3),
/// off the async threads (SQLite replay is blocking). `None` for a run the log
/// never spawned or whose spawn carried no pid.
async fn run_pid(daemon: &Arc<Daemon>, run: &str) -> Option<u32> {
    let fabric = Arc::clone(&daemon.fabric);
    let run = run.to_string();
    tokio::task::spawn_blocking(move || {
        let events = fabric.replay_since(None).ok()?;
        events.into_iter().find_map(|e| {
            if e.subject.as_str() == "agent.spawned" && e.payload()["run"].as_str() == Some(&run) {
                e.payload()["pid"]
                    .as_u64()
                    .and_then(|p| u32::try_from(p).ok())
            } else {
                None
            }
        })
    })
    .await
    .ok()
    .flatten()
}

/// Fold the log to find a run's workspace (the envelope `workspace` on its
/// `agent.spawned` fact) — the honest run→workspace source (I3), off the async
/// threads (SQLite replay is blocking). `None` for a run the log never spawned.
async fn run_workspace(daemon: &Arc<Daemon>, run: &str) -> Option<Ulid> {
    let fabric = Arc::clone(&daemon.fabric);
    let run = run.to_string();
    tokio::task::spawn_blocking(move || {
        let events = fabric.replay_since(None).ok()?;
        events.into_iter().find_map(|e| {
            if e.subject.as_str() == "agent.spawned" && e.payload()["run"].as_str() == Some(&run) {
                e.workspace.map(|w| w.ulid())
            } else {
                None
            }
        })
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    //! SP3 daemon-side resolver coverage (DR-015 §Decision 1/4). The live
    //! `permit_exec_live.rs` tests drive exec dispatch via a STATIC
    //! `PermitConfig::from_specs`, which bypasses the daemon's real resolver —
    //! the `filter_map` in [`super::McpBridge::permit_config_for`] that turns an
    //! applied `[gates.permit]` `VerifierSpec.exec` into a
    //! `PermitVerifierSpec::exec`. That un-filter (the SP3 mechanism that stops
    //! exec entries being silently dropped) was covered only by construction +
    //! compile-green. This test walks the REAL seam: an applied spec's
    //! `[gates.permit]` block → `begin_open` populates the opened-workspace
    //! registry → `permit_config_for(run)` resolves it → assert BOTH kinds
    //! survive, in configured order, with the exec argv/name/params carried.

    use std::sync::Arc;

    use rezidnt_cas::Cas;
    use rezidnt_fabric::{EventLog, Fabric};
    use rezidnt_gate::permit::PermitVerifierKind;
    use rezidnt_types::{Event, SourceId, Subject, WorkspaceId};
    use serde_json::json;
    use ulid::Ulid;

    use rezidnt_mcp::McpSubstrate;

    use super::{McpBridge, run_workspace};
    use crate::runs::{Daemon, RunRegistry, begin_open};

    /// A bare `Daemon` over a temp log + CAS plus a REAL (empty) repo dir — no
    /// transports, just the state `begin_open` + `permit_config_for` touch. The
    /// repo dir exists so `begin_open`'s materialization canonicalizes it and
    /// publishes `workspace.opened` (opened = true), so the synchronously
    /// registered workspace is NEVER ghost-evicted — the resolver read is
    /// deterministic, not racing the detached materialize task. The spec carries
    /// ZERO agents, so materialization launches nothing (no harness binary
    /// needed) — this test isolates the permit RESOLVER, not the run substrate.
    fn test_daemon() -> (tempfile::TempDir, Arc<Daemon>, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
        let fabric = Arc::new(Fabric::new(log, 1024));
        let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
        let daemon = Arc::new(Daemon::new(fabric, cas, Arc::new(RunRegistry::default())));
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).expect("mkdir repo");
        (dir, daemon, repo)
    }

    /// Seed an `agent.spawned` fact whose ENVELOPE workspace is `ws` and whose
    /// payload names `run` — the honest run→workspace source `permit_config_for`
    /// folds (I3), mirroring the live test's `seed_run`. (The full
    /// launch_agent→spawned path is exercised by the socket e2e tests; here we
    /// isolate the resolver, not re-test the run substrate.)
    fn seed_spawned(daemon: &Arc<Daemon>, ws: WorkspaceId, run: &str) {
        let spawned = Event::new(
            SourceId::new("rezidnt-run"),
            Some(ws),
            Subject::new("agent.spawned"),
            Ulid::new(),
            None,
            1,
            json!({"run": run, "agent": "impl", "harness": "claude-code"}),
        )
        .expect("spawned envelope");
        daemon.fabric.publish(spawned).expect("publish spawned");
    }

    /// DR-015 §Decision 1/4 — the daemon resolver no longer drops an exec permit
    /// entry. An applied `[gates.permit]` block carrying a native FOLLOWED BY an
    /// exec entry, walked through the REAL `begin_open` → `permit_config_for`
    /// seam, resolves to a two-entry set IN ORDER: `[0]` a
    /// `PermitVerifierKind::Native`, `[1]` a `PermitVerifierKind::Exec` whose
    /// argv is the `exec` path, display name is the spec `name`, and params ride
    /// verbatim. Before SP3 the exec entry was filtered out (a one-entry native
    /// set); this pins that it survives, carried as an exec spec.
    #[tokio::test]
    async fn permit_config_for_resolves_native_and_exec_entries_in_order() {
        let (_dir, daemon, repo) = test_daemon();

        // An applied spec whose permit gate is [native, exec] in that order. The
        // registry `gates` is populated from THIS block by `begin_open` — the
        // real seam. Zero agents so materialization launches nothing.
        let exec_path = "policies/agent.policy";
        let spec_toml = format!(
            r#"[project]
name = "sp3-resolver"
repo = "{repo}"

[gates.permit]
verifiers = [
  {{ native = "tool-allowlist", params = {{ allow = ["Read"] }} }},
  {{ exec = "{exec_path}", name = "reference-policy", params = {{ knob = 1 }} }},
]
"#,
            repo = repo.display(),
        );

        let (workspace, _correlation) = begin_open(&daemon, &spec_toml, false)
            .await
            .expect("open the resolver spec");

        // Seed the run→workspace link the resolver folds (I3).
        const RUN: &str = "01SP3RESOLVERRUN000000000R";
        seed_spawned(&daemon, workspace, RUN);
        // Sanity: the resolver's run→workspace fold sees the seeded fact.
        assert_eq!(
            run_workspace(&daemon, RUN).await,
            Some(workspace.ulid()),
            "the seeded agent.spawned links the run to the opened workspace (I3)"
        );

        let bridge = McpBridge {
            daemon: Arc::clone(&daemon),
        };
        let config = bridge
            .permit_config_for(RUN.to_string())
            .await
            .expect("permit_config_for resolves the opened workspace's permit gate");

        let verifiers = config.verifiers();
        assert_eq!(
            verifiers.len(),
            2,
            "BOTH the native AND the exec entry survive the resolver — the exec \
             entry is no longer dropped (DR-015 §Decision 1): {verifiers:#?}"
        );

        // [0] native, in configured order.
        assert_eq!(verifiers[0].name, "tool-allowlist");
        assert_eq!(
            verifiers[0].kind(),
            &PermitVerifierKind::Native,
            "the first configured entry resolves as a native (order preserved)"
        );
        assert_eq!(verifiers[0].params, json!({ "allow": ["Read"] }));

        // [1] exec, carrying argv (the exec path) + display name + params.
        assert_eq!(
            verifiers[1].name, "reference-policy",
            "the exec entry's display name comes from the spec `name`"
        );
        assert_eq!(
            verifiers[1].kind(),
            &PermitVerifierKind::Exec {
                argv: vec![exec_path.to_string()],
            },
            "the exec entry resolves as an Exec kind whose argv is the `exec` path \
             (DR-015 §Decision 1/4 — the operator's argv, not a bundled engine): {:#?}",
            verifiers[1].kind()
        );
        assert_eq!(
            verifiers[1].params,
            json!({ "knob": 1 }),
            "the exec entry's params ride verbatim to the §8 dispatch"
        );
    }

    /// DR-015 §Decision 4 — when the spec `name` is omitted on an exec entry, the
    /// resolver falls back to the `exec` path as the display name (mirroring the
    /// S4 exec dispatch convention in `gates.rs`). Pins that an un-named exec
    /// entry still resolves (is not dropped) and stays interrogable by SOME name.
    #[tokio::test]
    async fn permit_config_for_exec_entry_without_name_falls_back_to_path() {
        let (_dir, daemon, repo) = test_daemon();
        let exec_path = "policies/unnamed.policy";
        let spec_toml = format!(
            r#"[project]
name = "sp3-resolver-unnamed"
repo = "{repo}"

[gates.permit]
verifiers = [
  {{ exec = "{exec_path}" }},
]
"#,
            repo = repo.display(),
        );
        let (workspace, _correlation) = begin_open(&daemon, &spec_toml, false)
            .await
            .expect("open the unnamed-exec spec");
        const RUN: &str = "01SP3RESOLVERUNNAMED0000R";
        seed_spawned(&daemon, workspace, RUN);

        let bridge = McpBridge {
            daemon: Arc::clone(&daemon),
        };
        let config = bridge
            .permit_config_for(RUN.to_string())
            .await
            .expect("permit_config_for resolves the unnamed-exec permit gate");
        let verifiers = config.verifiers();
        assert_eq!(
            verifiers.len(),
            1,
            "the un-named exec entry survives the resolver (not dropped): {verifiers:#?}"
        );
        assert_eq!(
            verifiers[0].name, exec_path,
            "an exec entry with no `name` falls back to its `exec` path for display"
        );
        assert!(
            matches!(verifiers[0].kind(), PermitVerifierKind::Exec { argv } if argv == &[exec_path.to_string()]),
            "still an Exec kind carrying its argv: {:#?}",
            verifiers[0].kind()
        );
    }
}
