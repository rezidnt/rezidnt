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

    use crate::runs::{Daemon, RunRegistry, begin_open, rebuild_workspaces, serve_attach};

    /// Broadcast ring size (DEFAULT). Sized for control-plane volume (doc §5:
    /// facts and refs only); an overflowing subscriber takes the BINDING
    /// Lagged→resync path rather than back-pressuring the daemon.
    const BROADCAST_CAPACITY: usize = 1024;

    /// How long a connection may stay silent after the hello before it is
    /// served as a bare tail — the S0 back-compat default (S0 clients sent no
    /// request line). S1 clients send their request immediately.
    const REQUEST_WAIT: Duration = Duration::from_millis(500);

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
        let daemon = Arc::new(Daemon::new(fabric, cas, Arc::new(RunRegistry::default())));

        // S3-T1 remediation (I3): the open-workspace map is derived state —
        // rebuild it from log + CAS BEFORE any transport can serve a
        // `spawn_agent`, so a restart on the same log answers for every
        // workspace the log says is open (and for every recorded spawn key).
        rebuild_workspaces(&daemon)
            .await
            .context("rebuild open-workspace map from log")?;

        // S3 (doc §9, I5): the loopback-HTTP MCP transport, requested via
        // REZIDNT_MCP_LOCKFILE. Bound at 127.0.0.1:0; the REAL port plus the
        // daemon-lifetime operator badge are announced in the 0600 lockfile.
        // The handle must live as long as the daemon: dropping it stops the
        // listener.
        let _mcp_transport = match std::env::var_os("REZIDNT_MCP_LOCKFILE") {
            Some(lockfile) => {
                let bridge: Arc<dyn rezidnt_mcp::McpSubstrate> = Arc::new(crate::mcp::McpBridge {
                    daemon: Arc::clone(&daemon),
                });
                let core = Arc::new(
                    rezidnt_mcp::McpCore::new_shared(
                        Arc::clone(&daemon.fabric),
                        rezidnt_mcp::BadgeBook::new(),
                    )
                    .with_substrate(bridge),
                );
                let handle = rezidnt_mcp::serve_http(core, std::path::Path::new(&lockfile))
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
            let span = tracing::info_span!("adapter", kind = "uds-conn");
            tokio::spawn(
                async move {
                    if let Err(e) = handle_conn(stream, daemon).await {
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
    async fn handle_conn(stream: UnixStream, daemon: Arc<Daemon>) -> anyhow::Result<()> {
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
        }
    }

    /// Write one `Reply` JSONL frame.
    async fn write_reply(out: &mut OwnedWriteHalf, reply: &Reply) -> anyhow::Result<()> {
        let mut frame = encode_reply(reply).context("encode reply frame")?;
        frame.push('\n');
        out.write_all(frame.as_bytes()).await?;
        Ok(())
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
