//! `rezidentd` — the daemon (S1 scope: fabric + log + broadcast + UDS
//! requests: tail / open / attach).
//!
//! Contract pinned by `tests/tail_socket.rs` (S0) and `tests/open_flow.rs` +
//! `tests/run_persistence.rs` (S1):
//! - env `REZIDNT_SOCKET` overrides the UDS path (else `rezidnt_proto::socket_path()`);
//!   env `REZIDNT_DB` overrides the event-log path; env `REZIDNT_CAS`
//!   overrides the CAS root (else `<db dir>/cas`);
//! - on startup: open the log, publish `daemon.started` onto the fabric;
//! - per connection: send the versioned hello line first, then read the
//!   client's request line — `tail` (replay from seq 0 + live, optional
//!   subject filter; also the back-compat default when the client sends
//!   nothing, the S0 behavior), `open` (materialize a §13 spec: workspace,
//!   worktree, agent spawn under capture — the run is daemon-owned and
//!   survives the client), or `attach` (replay the run's capture ring, then
//!   proxy live bytes). I1: this daemon renders nothing — every UI is a
//!   socket client.
//!
//! Platform: UDS is `#[cfg(unix)]`. On Windows this binary compiles clean and
//! exits with a runtime error naming the designed-but-unimplemented named
//! pipe (doc §9, S0 platform decision).

#[cfg(unix)]
mod gates;
#[cfg(unix)]
mod mcp;
#[cfg(unix)]
mod runs;

fn main() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        unix_daemon::run()
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!(
            "rezidentd S0 serves a Unix domain socket only; the Windows named pipe \
             (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
        )
    }
}

#[cfg(unix)]
mod unix_daemon {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::Context;
    use rezidnt_cas::Cas;
    use rezidnt_fabric::{EventLog, Fabric, RecvError, Subscriber};
    use rezidnt_gate::permit::{PermitLayer, PermitVerifierSpec};
    use rezidnt_proto::{
        Hello, PROTO_VERSION, Reply, Request, decode_request, encode_hello, encode_reply,
        socket_path,
    };
    use rezidnt_types::taxonomy::{ONTOLOGY_VERSION, SUBJECTS_V0};
    use rezidnt_types::{Event, SourceId, Subject};
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::unix::OwnedWriteHalf;
    use tokio::net::{UnixListener, UnixStream};
    use tracing::Instrument;

    use crate::runs::{
        Daemon, RunRegistry, begin_open, rebuild_workspaces, record_alarms, serve_attach,
    };

    /// Broadcast ring size (DEFAULT). Sized for control-plane volume (doc §5:
    /// facts and refs only); an overflowing subscriber takes the BINDING
    /// Lagged→resync path rather than back-pressuring the daemon.
    const BROADCAST_CAPACITY: usize = 1024;

    /// How long a connection may stay silent after the hello before it is
    /// served as a bare tail — the S0 back-compat default (S0 clients sent no
    /// request line). S1 clients send their request immediately.
    const REQUEST_WAIT: Duration = Duration::from_millis(500);

    /// DR-034 live-unblock default: with `REZIDNT_UNBLOCK_TIMEOUT_MS` UNSET (or
    /// unparseable), the bounded server-assisted long-poll is DISABLED — a held
    /// escalated `request_permission` returns `ask` immediately, exactly as it did
    /// before DR-034 (the pure DR-033 "honored on next ask" fallback, unchanged).
    /// This SEPARATE, opt-in knob is distinct from the 250ms hot-path
    /// `REZIDNT_PERMIT_TIMEOUT_MS` (DR-034 §Decision 2): a stalled-agent wait is a
    /// different budget than a hot decision, so it does NOT change today's
    /// behaviour for callers who never set it. `0` also disables the hold.
    const DEFAULT_UNBLOCK_TIMEOUT_MS: u64 = 0;

    /// The DR-034 live-unblock deadline: `REZIDNT_UNBLOCK_TIMEOUT_MS` when set to a
    /// parseable value, else [`DEFAULT_UNBLOCK_TIMEOUT_MS`] (0 = disabled). Parsed
    /// like `REZIDNT_PERMIT_TIMEOUT_MS` in the PEP (env → `parse::<u64>` → default).
    fn unblock_timeout_ms() -> u64 {
        std::env::var("REZIDNT_UNBLOCK_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_UNBLOCK_TIMEOUT_MS)
    }

    /// The hello's `schema` field: identity hash of the compiled-in subject
    /// taxonomy (version string + subjects, newline-delimited). Derived from
    /// the same constants the drift-guard test pins against the canonical
    /// `spec/ontology.md`.
    fn ontology_hash() -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(ONTOLOGY_VERSION.as_bytes());
        for subject in SUBJECTS_V0 {
            hasher.update(b"\n");
            hasher.update(subject.as_bytes());
        }
        format!("blake3:{}", hasher.finalize().to_hex())
    }

    /// `REZIDNT_DB` override, else `~/.local/state/rezidnt/events.db`
    /// (the doc §9 fallback directory).
    fn db_path() -> PathBuf {
        if let Some(explicit) = std::env::var_os("REZIDNT_DB") {
            return PathBuf::from(explicit);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".local")
            .join("state")
            .join("rezidnt")
            .join("events.db")
    }

    /// `REZIDNT_CAS` override, else `cas/` next to the event log — log + CAS
    /// together are the whole persistent truth (I3).
    fn cas_path(db: &std::path::Path) -> PathBuf {
        if let Some(explicit) = std::env::var_os("REZIDNT_CAS") {
            return PathBuf::from(explicit);
        }
        db.parent()
            .map(|dir| dir.join("cas"))
            .unwrap_or_else(|| PathBuf::from("cas"))
    }

    /// The HOST-LEVEL admin permit layer (SP4c-wire, DR-020 §Decision 1): read
    /// `REZIDNT_ADMIN_PERMIT` → a host TOML file with a top-level `[gates.permit]`
    /// block (the SAME `verifiers = [{ native, params }]` shape a workspace uses),
    /// living OUTSIDE any workspace spec — a dev physically cannot edit or reorder
    /// it (the authority boundary). Parse each verifier into a
    /// [`PermitVerifierSpec`] STAMPED [`PermitLayer::Admin`]. ABSENT env var ⇒ the
    /// empty admin layer (no regression to the pre-SP4c single-source path). A set
    /// env var pointing at a missing/malformed file is an honest startup error —
    /// never a silently-empty admin layer that would drop the boundary.
    fn admin_permit_layer() -> anyhow::Result<Vec<PermitVerifierSpec>> {
        let Some(path) = std::env::var_os("REZIDNT_ADMIN_PERMIT") else {
            return Ok(Vec::new());
        };
        let path = PathBuf::from(path);
        let text = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "read REZIDNT_ADMIN_PERMIT host admin permit file {}",
                path.display()
            )
        })?;
        let gate = rezidnt_run::spec::permit_gate_from_host_toml(&text).with_context(|| {
            format!(
                "parse [gates.permit] from host admin permit {}",
                path.display()
            )
        })?;
        // A file with no `[gates.permit]` block contributes zero admin verifiers
        // (an admin surface that grants/denies nothing) — honest, not an error.
        let Some(gate) = gate else {
            return Ok(Vec::new());
        };
        let specs = gate
            .verifiers
            .iter()
            // Same native/exec fork as the dev source (mcp.rs::permit_config_for),
            // but STAMPED `PermitLayer::Admin` — the whole point of the boundary.
            .filter_map(|v| {
                if let Some(name) = &v.native {
                    Some(PermitVerifierSpec::native_in_layer(
                        PermitLayer::Admin,
                        name.clone(),
                        v.params.clone(),
                    ))
                } else if let Some(exec) = &v.exec {
                    let name = v.name.clone().unwrap_or_else(|| exec.display().to_string());
                    Some(PermitVerifierSpec::exec_in_layer(
                        PermitLayer::Admin,
                        name,
                        vec![exec.display().to_string()],
                        v.params.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        Ok(specs)
    }

    pub fn run() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("build tokio runtime")?;
        runtime.block_on(serve())
    }

    async fn serve() -> anyhow::Result<()> {
        let db = db_path();
        let sock = socket_path();

        if let Some(parent) = db.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create db dir {}", parent.display()))?;
        }
        if let Some(parent) = sock.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create socket dir {}", parent.display()))?;
        }
        // A stale socket file from a previous run would make bind fail.
        match tokio::fs::remove_file(&sock).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("unlink stale {}", sock.display())),
        }

        // Open the log and publish the startup fact off the async threads
        // (SQLite is blocking; rust-conventions: no blocking in async).
        let fabric = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<Arc<Fabric>> {
                let log = EventLog::open(&db)
                    .with_context(|| format!("open event log {}", db.display()))?;
                let fabric = Arc::new(Fabric::new(log, BROADCAST_CAPACITY));
                let started = Event::new(
                    SourceId::new("daemon"),
                    None,
                    Subject::new("daemon.started"),
                    ulid::Ulid::new(),
                    None,
                    1,
                    json!({"pid": std::process::id()}),
                )
                .context("construct daemon.started")?;
                fabric.publish(started).context("publish daemon.started")?;
                Ok(fabric)
            })
            .await
            .context("startup task panicked")??
        };

        // CAS root next to the log (blocking fs — off the async threads).
        let cas = {
            let root = cas_path(&db);
            tokio::task::spawn_blocking(move || -> anyhow::Result<Arc<Cas>> {
                Ok(Arc::new(Cas::open(&root).with_context(|| {
                    format!("open cas root {}", root.display())
                })?))
            })
            .await
            .context("cas open task panicked")??
        };
        // SP4c-wire (DR-020 §Decision 1): source the host-level admin permit
        // layer from `REZIDNT_ADMIN_PERMIT` (a config file OUTSIDE any workspace
        // spec) BEFORE serving, so `permit_config_for` merges it FIRST. Absent
        // env var ⇒ empty admin layer ⇒ unchanged single-source behavior.
        let admin_permit = admin_permit_layer().context("source host admin permit layer")?;
        let daemon = Arc::new(
            Daemon::new(fabric, cas, Arc::new(RunRegistry::default()))
                .with_admin_permit(admin_permit),
        );

        // S3-T1 remediation (I3): the open-workspace map is derived state —
        // rebuild it from log + CAS BEFORE any transport can serve a
        // `spawn_agent`, so a restart on the same log answers for every
        // workspace the log says is open (and for every recorded spawn key).
        rebuild_workspaces(&daemon)
            .await
            .context("rebuild open-workspace map from log")?;

        // SP2 (DR-013 decision 1): construct the PDP core UNCONDITIONALLY — not
        // gated on REZIDNT_MCP_LOCKFILE — so the socket-side permit path does not
        // depend on the HTTP transport being enabled. The socket handler and the
        // optional HTTP transport share this ONE Arc: one decision path, MCP and
        // socket facts are byte-identical (I3, no fork). The `McpBridge`
        // substrate resolves each run's `[gates.permit]` config (DR-011); the
        // CAS is the daemon's own (native verifiers pin evidence, I2).
        let bridge: Arc<dyn rezidnt_mcp::McpSubstrate> = Arc::new(crate::mcp::McpBridge {
            daemon: Arc::clone(&daemon),
        });
        let pdp = Arc::new(
            rezidnt_mcp::McpCore::new_shared(
                Arc::clone(&daemon.fabric),
                rezidnt_mcp::BadgeBook::new(),
            )
            .with_cas(Arc::clone(&daemon.cas))
            .with_substrate(bridge)
            // SP4b (DR-017 §Decision 6): verify agent macaroons against the SAME
            // process-lifetime root key the daemon MINTS them with. A clone shares
            // the 32-byte secret — the daemon (`launch_agent`) mints, this core
            // (`check_badge` Path 2) verifies; both anchor to one key. Without this
            // the production core was keyless — every agent macaroon on a mutating
            // call would be `badge.invalid` (the open seam the auditor flagged).
            // The key is NEVER on the fabric (I2/design §4).
            .with_root_key(daemon.root_key.clone()),
        );

        // S3 (doc §9, I5): the loopback-HTTP MCP transport, requested via
        // REZIDNT_MCP_LOCKFILE. Bound at 127.0.0.1:0; the REAL port plus the
        // daemon-lifetime operator badge are announced in the 0600 lockfile.
        // The handle must live as long as the daemon: dropping it stops the
        // listener. It serves over the SAME core as the socket PDP.
        let _mcp_transport = match std::env::var_os("REZIDNT_MCP_LOCKFILE") {
            Some(lockfile) => {
                let handle =
                    rezidnt_mcp::serve_http(Arc::clone(&pdp), std::path::Path::new(&lockfile))
                        .await
                        .context("start mcp loopback-http transport")?;
                tracing::info!(url = %handle.url, "mcp http transport announced");
                Some(handle)
            }
            None => None,
        };

        let listener =
            UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
        // §12: control socket at mode 0600 — owner-only.
        tokio::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))
            .await
            .with_context(|| format!("chmod 0600 {}", sock.display()))?;
        tracing::info!(socket = %sock.display(), db = %db.display(), "rezidentd listening");

        loop {
            let (stream, _addr) = listener.accept().await.context("accept")?;
            let daemon = Arc::clone(&daemon);
            let pdp = Arc::clone(&pdp);
            let span = tracing::info_span!("adapter", kind = "uds-conn");
            tokio::spawn(
                async move {
                    if let Err(e) = handle_conn(stream, daemon, pdp).await {
                        // Client disconnects surface here; not daemon faults.
                        tracing::debug!(error = %e, "connection ended");
                    }
                }
                .instrument(span),
            );
        }
    }

    /// Resync on a blocking thread (SQLite read); the subscriber travels
    /// through the closure and back.
    async fn resync_blocking(
        fabric: Arc<Fabric>,
        mut sub: Subscriber,
    ) -> anyhow::Result<(Vec<Event>, Subscriber)> {
        tokio::task::spawn_blocking(move || {
            let missed = sub.resync(&fabric)?;
            Ok::<_, rezidnt_fabric::FabricError>((missed, sub))
        })
        .await
        .context("resync task panicked")?
        .context("resync from log")
    }

    /// Per-connection protocol: hello line → request line (or S0 silence) →
    /// the requested stream.
    async fn handle_conn(
        stream: UnixStream,
        daemon: Arc<Daemon>,
        pdp: Arc<rezidnt_mcp::McpCore>,
    ) -> anyhow::Result<()> {
        let (read_half, mut write_half) = stream.into_split();

        let hello = Hello {
            proto: PROTO_VERSION,
            schema: ontology_hash(),
            daemon: env!("CARGO_PKG_VERSION").to_string(),
        };
        let mut frame = encode_hello(&hello)?;
        frame.push('\n');
        write_half.write_all(frame.as_bytes()).await?;

        // First client line = the request. Silence for REQUEST_WAIT (or an
        // immediate EOF on the read side) is the S0 back-compat bare tail.
        let mut reader = BufReader::new(read_half);
        let mut first = String::new();
        let request = match tokio::time::timeout(REQUEST_WAIT, reader.read_line(&mut first)).await {
            Err(_silent) => Request::Tail { subject: None },
            Ok(Ok(0)) => Request::Tail { subject: None },
            Ok(Ok(_)) => decode_request(first.trim_end()).context("decode request line")?,
            Ok(Err(e)) => return Err(e).context("read request line"),
        };

        match request {
            Request::Tail { subject } => {
                serve_tail(&daemon, &mut write_half, subject.as_deref()).await
            }
            Request::Open { spec_toml } => {
                // S3 request-scoped ack (rezidnt_proto::Reply): the FIRST
                // frame after the hello answers THIS request — open_ok with
                // the workspace + correlation the materialization facts
                // carry, or a machine-readable spec.invalid error frame.
                // The materialization itself is daemon-owned (detached inside
                // begin_open), so the run survives the client (S1 exit
                // criterion); the opening client is then served the plain
                // tail so every step is visible on its own socket.
                match begin_open(&daemon, &spec_toml, true).await {
                    Ok((workspace, correlation)) => {
                        let ack = Reply::OpenOk {
                            workspace: workspace.ulid(),
                            correlation,
                        };
                        write_reply(&mut write_half, &ack).await?;
                        serve_tail(&daemon, &mut write_half, None).await
                    }
                    Err(refusal) => {
                        let error = Reply::Error {
                            op: "open".to_string(),
                            code: rezidnt_proto::codes::SPEC_INVALID.to_string(),
                            message: refusal.message,
                            run: None,
                        };
                        write_reply(&mut write_half, &error).await?;
                        Ok(()) // orderly close after the error frame
                    }
                }
            }
            Request::Attach { run } => {
                // S3: an unknown run answers ONE machine-readable error frame
                // and then closes — never a hang, never a silent EOF.
                if daemon.registry.get(&run).is_none() {
                    let error = Reply::Error {
                        op: "attach".to_string(),
                        code: rezidnt_proto::codes::RUN_UNKNOWN.to_string(),
                        message: format!("no run {run} on this daemon"),
                        run: Some(run),
                    };
                    write_reply(&mut write_half, &error).await?;
                    return Ok(());
                }
                serve_attach(&daemon, run, &mut write_half).await
            }
            Request::RecordAlarms { alarms } => {
                // DR-006: the CLI computed the divergence(s) with its direct
                // read; the daemon (single writer, I3) dedups against the log
                // and appends each new integrity.alarm through its Fabric.
                // Ack AFTER the append lands so the CLI's subsequent log read
                // is race-free.
                match record_alarms(&daemon, &alarms).await {
                    Ok(appended) => {
                        write_reply(&mut write_half, &Reply::AlarmsRecorded { appended }).await?;
                        Ok(()) // orderly close after the ack
                    }
                    Err(e) => {
                        let error = Reply::Error {
                            op: "record_alarms".to_string(),
                            code: rezidnt_proto::codes::INTERNAL.to_string(),
                            message: format!("{e:#}"),
                            run: None,
                        };
                        write_reply(&mut write_half, &error).await?;
                        Ok(())
                    }
                }
            }
            Request::RequestPermission {
                run,
                request_id,
                action,
                tool,
                context_ref,
                paths,
                // DR-013 decision 3: the socket transport skips the §12 badge
                // door — the 0600 owner-only UDS IS the identity — so `badge` is
                // ignored here (it stays optional on the wire for forward-compat).
                badge: _,
            } => {
                // SP2 (DR-013 decision 1): service the permit decision over the
                // SAME transport-neutral PDP the MCP surface uses. The socket
                // carries the PEP's `request_id` token; `decide_permit` echoes it
                // so the ask and the on-log decision fact share one id. The two
                // facts (`permit.requested` + one decision fact) are emitted by
                // `decide_permit` — the socket handler never re-emits (I3).
                // DR-034 live-unblock: keep the request's action identity so a
                // held escalation can be RE-DECIDED against a landing
                // `permit.resolved` (the wake keys on `(run, tool)`, DR-034
                // §Decision 3). Cloned BEFORE `req` moves into `decide_permit`.
                let held_run = run.clone();
                let held_action = action.clone();
                let held_tool = tool.clone();
                let req = rezidnt_mcp::PermitRequest {
                    run,
                    request_id: Some(request_id),
                    action,
                    tool,
                    // The socket skips the badge door (DR-013 decision 3).
                    badge: None,
                    context_ref,
                    // DR-014 §Decision 4: thread the wire `paths` axis through to
                    // the PDP so `path-scope` decides identically over socket and
                    // MCP (closes the `bb7afe3` asymmetry). `None` when the sender
                    // omits it — the native then sees no paths → escalate.
                    paths,
                };
                match pdp.decide_permit(req).await {
                    Ok(outcome) => {
                        // DR-034: on a DECISIVE verdict (allow/deny) the hot path is
                        // untouched — return at once, never hold. Only an
                        // `ask`/escalate MAY enter the bounded long-poll below.
                        let outcome = if outcome.decision == rezidnt_mcp::Decision::Ask {
                            await_unblock(
                                &daemon,
                                &pdp,
                                &held_run,
                                &held_action,
                                &held_tool,
                                outcome,
                            )
                            .await?
                        } else {
                            outcome
                        };
                        let reply = Reply::PermitDecision {
                            request_id: outcome.request_id,
                            decision: outcome.decision.as_word().to_string(),
                            reason: outcome.reason,
                        };
                        write_reply(&mut write_half, &reply).await?;
                        Ok(()) // orderly close after the decision frame
                    }
                    Err(e) => {
                        // A daemon-side PDP fault is honest INTERNAL — never a
                        // coerced decision (I6).
                        let error = Reply::Error {
                            op: "request_permission".to_string(),
                            code: rezidnt_proto::codes::INTERNAL.to_string(),
                            message: format!("{e}"),
                            run: None,
                        };
                        write_reply(&mut write_half, &error).await?;
                        Ok(())
                    }
                }
            }
        }
    }

    /// Write one `Reply` JSONL frame.
    async fn write_reply(out: &mut OwnedWriteHalf, reply: &Reply) -> anyhow::Result<()> {
        let mut frame = encode_reply(reply).context("encode reply frame")?;
        frame.push('\n');
        out.write_all(frame.as_bytes()).await?;
        Ok(())
    }

    /// DR-034 live-unblock — a bounded server-assisted long-poll for a HELD
    /// escalated `request_permission`. The first `decide_permit` already escalated
    /// (`permit.requested` + `permit.escalated` are on the log, so `escalated` is
    /// the outcome passed in). While `REZIDNT_UNBLOCK_TIMEOUT_MS > 0` and until the
    /// deadline: each time a `permit.resolved` for THIS run lands, RE-DECIDE the
    /// same held request via `recheck_resolution` — which applies a matching
    /// resolution as the on-log `permit.granted`/`permit.denied` WITHOUT re-logging
    /// a second requested/escalated pair (I3). A wake carries the ORIGINAL held
    /// `request_id` (it flows through the re-decide, never re-minted — DR-034
    /// §Decision 3). On deadline expiry with no matching resolution, return the
    /// ORIGINAL escalation unchanged — fail-closed to `ask` (DR-034 §Decision 2),
    /// never `allow`, never a hang past the deadline. A FOREIGN resolution (a
    /// different action/run) never flips the re-decide, so the loop simply waits
    /// out the deadline and returns `ask` — the ledger-check's action-identity
    /// match is the only gate (no separate id equality needed).
    async fn await_unblock(
        daemon: &Arc<Daemon>,
        pdp: &Arc<rezidnt_mcp::McpCore>,
        run: &str,
        action: &str,
        tool: &str,
        escalated: rezidnt_mcp::PermitOutcome,
    ) -> anyhow::Result<rezidnt_mcp::PermitOutcome> {
        let budget_ms = unblock_timeout_ms();
        if budget_ms == 0 {
            // Hold disabled: pure DR-033 fallback (immediate `ask`), unchanged.
            return Ok(escalated);
        }

        let span = tracing::info_span!(
            "adapter",
            kind = "permit-unblock",
            run = run,
            request_id = %escalated.request_id
        );
        let deadline = Duration::from_millis(budget_ms);
        let held_id = escalated.request_id.clone();
        // DR-035: the HELD request's ORIGINAL `permit.requested` envelope-ULID
        // timestamp — the anchor the TTL filter measures a landing resolution's
        // deadline against (the request happened once, on the first pass). A
        // resolution that lands DURING the hold is necessarily newer than this, so
        // its deadline is always past the anchor and it always applies; TTL only
        // bites the honored-on-a-later-next-ask path (a fresh `decide_permit`).
        let held_requested_ms = escalated.requested_ms;

        let waited = tokio::time::timeout(deadline, async {
            // Subscribe FIRST, then do an immediate re-decide: a resolution that
            // landed between the escalate emit and this subscribe is caught by the
            // fold (`recheck_resolution` re-folds the whole log), not missed.
            let mut sub = daemon.fabric.subscribe();
            if let Some(outcome) = pdp
                .recheck_resolution(run, action, tool, &held_id, held_requested_ms)
                .await
                .context("permit live-unblock recheck")?
            {
                return Ok::<Option<rezidnt_mcp::PermitOutcome>, anyhow::Error>(Some(outcome));
            }
            loop {
                match sub.recv().await {
                    Ok(event) => {
                        // Only a resolution for THIS run can flip the re-decide; a
                        // `permit.resolved` for the run triggers a fresh
                        // ledger-check. Any other fact (or a foreign run's resolve)
                        // is ignored — cheap filter before the fold.
                        if event.subject.as_str() != "permit.resolved"
                            || event.payload()["run"] != json!(run)
                        {
                            continue;
                        }
                        if let Some(outcome) = pdp
                            .recheck_resolution(run, action, tool, &held_id, held_requested_ms)
                            .await
                            .context("permit live-unblock recheck")?
                        {
                            return Ok(Some(outcome));
                        }
                    }
                    // A lagged subscriber may have skipped the resolution: re-fold
                    // the full log (the fold sees it regardless) and keep waiting if
                    // it still does not match.
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(dropped = n, "permit-unblock subscriber lagged; re-folding");
                        if let Some(outcome) = pdp
                            .recheck_resolution(run, action, tool, &held_id, held_requested_ms)
                            .await
                            .context("permit live-unblock recheck")?
                        {
                            return Ok(Some(outcome));
                        }
                    }
                    Err(RecvError::Closed) => return Ok(None),
                }
            }
        })
        .instrument(span)
        .await;

        match waited {
            // A matching resolution woke the held request within the deadline.
            Ok(Ok(Some(outcome))) => Ok(outcome),
            // Fabric closed with no match — fail closed to the original `ask`.
            Ok(Ok(None)) => Ok(escalated),
            // A recheck fault: never coerce — surface it (the caller answers Error).
            Ok(Err(e)) => Err(e),
            // Deadline expiry, no matching resolution: fail closed to `ask`
            // (DR-034 §Decision 2) — the ORIGINAL escalation, never `allow`.
            Err(_elapsed) => Ok(escalated),
        }
    }

    /// Tail stream: replay from seq 0, then live, optionally filtered by
    /// subject (the filter drops non-matching envelopes server-side; seq/id
    /// bookkeeping still covers the full stream).
    async fn serve_tail(
        daemon: &Arc<Daemon>,
        out: &mut OwnedWriteHalf,
        subject: Option<&str>,
    ) -> anyhow::Result<()> {
        let wanted = |event: &Event| subject.is_none_or(|want| event.subject.as_str() == want);

        // Subscribe FIRST, then replay: anything published between the
        // subscribe and the log read arrives twice (log + ring) and the
        // subscriber's append-position (seq) tracking discards the ring copy
        // — the same discipline as the BINDING Lagged→resync rule.
        let sub = daemon.fabric.subscribe();
        let (replayed, mut sub) = resync_blocking(Arc::clone(&daemon.fabric), sub).await?;
        let mut backlog = String::new();
        for event in replayed.iter().filter(|e| wanted(e)) {
            backlog.push_str(&event.to_json_line()?);
            backlog.push('\n');
        }
        out.write_all(backlog.as_bytes()).await?;

        loop {
            match sub.recv().await {
                Ok(event) => {
                    if !wanted(&event) {
                        continue;
                    }
                    let mut line = event.to_json_line()?;
                    line.push('\n');
                    out.write_all(line.as_bytes()).await?;
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(dropped = n, "tail subscriber lagged; resyncing from log");
                    let (missed, resynced) =
                        resync_blocking(Arc::clone(&daemon.fabric), sub).await?;
                    sub = resynced;
                    let mut lines = String::new();
                    for event in missed.iter().filter(|e| wanted(e)) {
                        lines.push_str(&event.to_json_line()?);
                        lines.push('\n');
                    }
                    out.write_all(lines.as_bytes()).await?;
                }
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }
}
