//! The daemon's implementation of the MCP substrate seam (I4): the
//! `rezidnt-mcp` core stays transport- and substrate-agnostic; this bridge
//! maps its two mutating tools onto the S1 run substrate in `runs.rs`.
//!
//! Refusal discipline (§12, S3 board): every failure is a machine-readable
//! `ToolRefusal` code — the MCP surface never answers with prose alone, and a
//! refused mutation leaves the log untouched.
#![cfg(unix)]

use std::sync::Arc;

use rezidnt_gate::permit::PermitVerifierSpec;
use rezidnt_mcp::{BoxFuture, McpSubstrate, OpenAck, PermitConfig, ToolRefusal, codes};
use rezidnt_types::WorkspaceId;
use ulid::Ulid;

use crate::runs::{Daemon, begin_open, launch_agent};

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
                // Recorded on the agent.spawned payload so the key→run map is
                // log-derivable across restart (I3; envelope workspace is set
                // by launch_agent, the ontology's keyed-spawn obligation).
                Some(&idempotency_key),
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
    /// `workspace.spec.applied`). Only NATIVE verifiers dispatch on the permit
    /// axis (the SP-wire aggregator resolves natives by name); an exec entry in
    /// a permit gate is skipped. `None` when the run maps to no workspace or the
    /// workspace configures no permit gate — the PDP then degrades to
    /// escalate/deny (I6), never a synthesized allow.
    fn permit_config_for(&self, run: String) -> BoxFuture<Option<PermitConfig>> {
        let daemon = Arc::clone(&self.daemon);
        Box::pin(async move {
            // 1. run → workspace, log-derived from the run's `agent.spawned`
            //    envelope (I3: the honest source, not a side table).
            let ws = run_workspace(&daemon, &run).await?;

            // 2. workspace → applied `[gates.permit]` verifier set. The registry
            //    is derived state folded from `workspace.spec.applied` (I3).
            let workspaces = daemon.workspaces.lock().await;
            let entry = workspaces.get(&ws)?;
            let gate = entry.gates.get("permit")?;
            let specs: Vec<PermitVerifierSpec> = gate
                .verifiers
                .iter()
                // Permit dispatch is native-only (SP-wire resolves by name); an
                // exec entry on the permit axis is skipped, never silently run.
                .filter_map(|v| {
                    v.native.as_ref().map(|name| PermitVerifierSpec {
                        name: name.clone(),
                        params: v.params.clone(),
                    })
                })
                .collect();
            Some(PermitConfig::from_specs(specs))
        })
    }
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
