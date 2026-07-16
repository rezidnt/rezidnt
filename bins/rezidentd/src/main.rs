//! `rezidentd` — the daemon (S0 scope: fabric + log + broadcast + UDS tail).
//!
//! S0 contract pinned by `tests/tail_socket.rs`:
//! - env `REZIDNT_SOCKET` overrides the UDS path (else `rezidnt_proto::socket_path()`);
//!   env `REZIDNT_DB` overrides the event-log path;
//! - on startup: open the log, publish `daemon.started` onto the fabric;
//! - per connection: send the versioned hello line first, then replay the log
//!   from seq 0 as event-envelope JSONL, then continue with live events
//!   (S0 pin: replay-then-live makes "two concurrent subscribers observe the
//!   stream" deterministic; I1: this daemon renders nothing — every UI is a
//!   socket client).
//!
//! Platform: UDS is `#[cfg(unix)]`. On Windows this binary compiles clean and
//! exits with a runtime error naming the designed-but-unimplemented named
//! pipe (doc §9, S0 platform decision).

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

    use anyhow::Context;
    use rezidnt_fabric::{EventLog, Fabric, RecvError, Subscriber};
    use rezidnt_proto::{Hello, PROTO_VERSION, encode_hello, socket_path};
    use rezidnt_types::taxonomy::{ONTOLOGY_VERSION, SUBJECTS_V0};
    use rezidnt_types::{Event, SourceId, Subject};
    use serde_json::json;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{UnixListener, UnixStream};
    use tracing::Instrument;

    /// Broadcast ring size (DEFAULT). Sized for control-plane volume (doc §5:
    /// facts and refs only); an overflowing subscriber takes the BINDING
    /// Lagged→resync path rather than back-pressuring the daemon.
    const BROADCAST_CAPACITY: usize = 1024;

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

        let listener =
            UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
        // §12: control socket at mode 0600 — owner-only.
        tokio::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))
            .await
            .with_context(|| format!("chmod 0600 {}", sock.display()))?;
        tracing::info!(socket = %sock.display(), db = %db.display(), "rezidentd listening");

        loop {
            let (stream, _addr) = listener.accept().await.context("accept")?;
            let fabric = Arc::clone(&fabric);
            let span = tracing::info_span!("adapter", kind = "uds-tail");
            tokio::spawn(
                async move {
                    if let Err(e) = handle_conn(stream, fabric).await {
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

    /// Per-connection stream: hello line → replay from seq 0 → live events.
    async fn handle_conn(mut stream: UnixStream, fabric: Arc<Fabric>) -> anyhow::Result<()> {
        let hello = Hello {
            proto: PROTO_VERSION,
            schema: ontology_hash(),
            daemon: env!("CARGO_PKG_VERSION").to_string(),
        };
        let mut frame = encode_hello(&hello)?;
        frame.push('\n');
        stream.write_all(frame.as_bytes()).await?;

        // Subscribe FIRST, then replay: anything published between the
        // subscribe and the log read arrives twice (log + ring) and the
        // subscriber's append-position (seq) tracking discards the ring copy
        // — the same discipline as the BINDING Lagged→resync rule.
        let sub = fabric.subscribe();
        let (replayed, mut sub) = resync_blocking(Arc::clone(&fabric), sub).await?;
        let mut backlog = String::new();
        for event in &replayed {
            backlog.push_str(&event.to_json_line()?);
            backlog.push('\n');
        }
        stream.write_all(backlog.as_bytes()).await?;

        loop {
            match sub.recv().await {
                Ok(event) => {
                    let mut line = event.to_json_line()?;
                    line.push('\n');
                    stream.write_all(line.as_bytes()).await?;
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(dropped = n, "tail subscriber lagged; resyncing from log");
                    let (missed, resynced) = resync_blocking(Arc::clone(&fabric), sub).await?;
                    sub = resynced;
                    let mut lines = String::new();
                    for event in &missed {
                        lines.push_str(&event.to_json_line()?);
                        lines.push('\n');
                    }
                    stream.write_all(lines.as_bytes()).await?;
                }
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }
}
