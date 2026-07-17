//! `rezidnt` — the CLI.
//!
//! Verbs (doc §9):
//! - `rezidnt rebuild --db <path> --json` — refold from seq 0, print the
//!   `rezidnt_state::Graph` as JSON on stdout, exit 0. Pinned by
//!   `tests/rebuild_cli.rs`. Cross-platform.
//! - `rezidnt tail [--subject …]` — connect to the daemon socket, print the
//!   stream. Exercised by the S0 exit demo (two concurrent `rezidnt tail`
//!   clients), not by an automated oracle test — see the S0 work order.
//! - `rezidnt open <spec-path>` — materialize a workspace from a §13 spec
//!   file through the daemon; prints EXACTLY one stdout line
//!   `opened <workspace-name> run <run-ulid>` once `agent.spawned` is
//!   observed on the stream (the id is the fabric's, not decoration).
//!   Pinned by `bins/rezidentd/tests/cli_verbs.rs`.
//! - `rezidnt attach <run-ulid>` — replay the run's capture ring, then
//!   stream live bytes to stdout until EOF (dtach model). Pinned likewise.
//!
//! Stable exit codes (doc §9): 0 ok, 2 gate-fail, 3 substrate-fault,
//! 4 daemon-unreachable. Mapping: `rebuild` failures are substrate faults
//! (the log store misbehaved) → 3; `tail`/`attach` failures are daemon-side
//! (unreachable socket, bad hello, proto mismatch) → 4. `open`: a
//! missing/unreadable/unparseable spec file is a LOCAL input error → 2
//! (clap's usage-error convention, pinned by cli_verbs.rs; the §9 gate-fail
//! collision on the number 2 is flagged for /dr, not resolved here); a
//! daemon-side open-failed refusal → 3; connection failures → 4.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rezidnt",
    version,
    about = "rezidnt CLI: rebuild, tail, open, attach"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Refold the event log from seq 0 and print the graph as JSON.
    Rebuild {
        /// Path to the event log (SQLite).
        #[arg(long)]
        db: PathBuf,
        /// Print compact JSON (default prints human-indented JSON).
        #[arg(long)]
        json: bool,
    },
    /// Connect to the daemon socket and print the event stream (JSONL).
    Tail {
        /// Only print events with exactly this subject.
        #[arg(long)]
        subject: Option<String>,
    },
    /// Materialize a workspace from a project spec file (doc §13) and spawn
    /// its agents through the daemon.
    Open {
        /// Path to the project spec (rezidnt.toml shape).
        spec: PathBuf,
    },
    /// Replay a run's capture tail, then stream live bytes (dtach model).
    Attach {
        /// The run ULID, as printed by `rezidnt open`.
        run: String,
    },
}

fn main() {
    let cli = Cli::parse();
    // Per-verb stable failure class (doc §9); see module docs.
    let (failure_code, result) = match cli.cmd {
        Cmd::Rebuild { db, json } => (3, rebuild(&db, json)),
        Cmd::Tail { subject } => (4, tail(subject.as_deref())),
        Cmd::Open { spec } => {
            // Local input phase: exit 2 (see module docs for the /dr flag).
            let (name, spec_toml) = match read_spec(&spec) {
                Ok(parts) => parts,
                Err(e) => {
                    eprintln!("rezidnt: {e:#}");
                    std::process::exit(2);
                }
            };
            (4, open(&name, spec_toml))
        }
        Cmd::Attach { run } => {
            // A malformed run id is the same local-input class as a bad spec.
            let run = match run.parse::<ulid::Ulid>() {
                Ok(run) => run,
                Err(e) => {
                    eprintln!("rezidnt: run id {run:?} is not a ULID: {e}");
                    std::process::exit(2);
                }
            };
            (4, attach(run))
        }
    };
    if let Err(e) = result {
        eprintln!("rezidnt: {e:#}");
        std::process::exit(failure_code);
    }
}

/// Read and parse the spec file locally: the success line needs
/// `[project].name`, and a spec that cannot be read or parsed should fail
/// fast with the offending path on stderr, before any daemon traffic.
fn read_spec(path: &Path) -> anyhow::Result<(String, String)> {
    let spec_toml = std::fs::read_to_string(path)
        .with_context(|| format!("read spec file {}", path.display()))?;
    let spec = rezidnt_run::spec::ProjectSpec::from_toml_str(&spec_toml)
        .with_context(|| format!("parse spec file {}", path.display()))?;
    Ok((spec.name, spec_toml))
}

/// `rebuild` = fold(log from seq 0); the log is truth, the graph is derived (I3).
fn rebuild(db: &std::path::Path, compact: bool) -> anyhow::Result<()> {
    use anyhow::Context;

    let log = rezidnt_fabric::EventLog::open(db)
        .with_context(|| format!("open event log {}", db.display()))?;
    let rows = log.read_from(1).context("read log from seq 1")?;
    let events: Vec<rezidnt_types::Event> = rows.into_iter().map(|r| r.event).collect();
    let graph = rezidnt_state::fold(events.iter());
    let out = if compact {
        serde_json::to_string(&graph)?
    } else {
        serde_json::to_string_pretty(&graph)?
    };
    println!("{out}");
    Ok(())
}

/// Unix socket client plumbing shared by `tail`/`open`/`attach`: connect,
/// consume + check the hello, send the request line, hand back the reader.
#[cfg(unix)]
fn connect_and_request(
    request: &rezidnt_proto::Request,
) -> anyhow::Result<std::io::BufReader<std::os::unix::net::UnixStream>> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    use rezidnt_proto::{check_hello, decode_hello, encode_request, socket_path};

    let sock = socket_path();
    let stream = UnixStream::connect(&sock)
        .with_context(|| format!("connect to daemon at {}", sock.display()))?;
    let mut reader = BufReader::new(stream);

    let mut hello_line = String::new();
    reader.read_line(&mut hello_line).context("read hello")?;
    let hello = decode_hello(hello_line.trim_end()).context("decode hello")?;
    check_hello(&hello).context("proto check")?;

    let frame = encode_request(request).context("encode request")?;
    let stream = reader.get_mut();
    stream.write_all(frame.as_bytes()).context("send request")?;
    stream.write_all(b"\n").context("send request newline")?;
    Ok(reader)
}

#[cfg(unix)]
fn tail(subject: Option<&str>) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};

    use rezidnt_types::Event;

    // Explicit tail request (server-side subject filter) skips the daemon's
    // S0 back-compat silence window.
    let mut reader = connect_and_request(&rezidnt_proto::Request::Tail {
        subject: subject.map(String::from),
    })?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).context("read event")?;
        if n == 0 {
            return Ok(()); // daemon closed the stream
        }
        let frame = line.trim_end();
        if frame.is_empty() {
            continue;
        }
        // Validating decode (I2 applies on the wire too), then print verbatim.
        let event = Event::from_json_line(frame).context("decode event frame")?;
        if subject.is_some_and(|want| event.subject.as_str() != want) {
            continue;
        }
        writeln!(out, "{frame}")?;
    }
}

/// `open`: send the spec, read the S3 request-scoped ack (`open_ok` with the
/// workspace + correlation, or a machine-readable error frame), then watch
/// the stream for THIS open's facts and print the pinned one-line identity
/// once `agent.spawned` lands on the acked correlation.
///
/// The marker ULID (minted client-side before the request) still guards the
/// `daemon.warning` arm against replayed history: warnings carry their own
/// correlation, so time-ordering (id > marker) is what scopes them to this
/// open.
#[cfg(unix)]
fn open(name: &str, spec_toml: String) -> anyhow::Result<()> {
    use std::io::BufRead;

    use rezidnt_types::Event;

    let marker = ulid::Ulid::new();
    let mut reader = connect_and_request(&rezidnt_proto::Request::Open { spec_toml })?;

    // S3: the daemon acks the request FIRST (rezidnt_proto::Reply) — the
    // acked correlation is the one every materialization fact carries, so
    // the marker/name inference of the S1 client is gone.
    let mut ack_line = String::new();
    loop {
        ack_line.clear();
        let n = reader.read_line(&mut ack_line).context("read open ack")?;
        if n == 0 {
            anyhow::bail!("daemon closed the stream before acking the open");
        }
        if !ack_line.trim().is_empty() {
            break;
        }
    }
    let correlation =
        match rezidnt_proto::decode_reply(ack_line.trim_end()).context("decode open ack frame")? {
            rezidnt_proto::Reply::OpenOk { correlation, .. } => correlation,
            rezidnt_proto::Reply::Error { code, message, .. } => {
                // Daemon-side refusal: substrate fault class (§9 → 3).
                eprintln!("rezidnt: open refused ({code}): {message}");
                std::process::exit(3);
            }
        };

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).context("read event")?;
        if n == 0 {
            anyhow::bail!("daemon closed the stream before the open completed");
        }
        let frame = line.trim_end();
        if frame.is_empty() {
            continue;
        }
        let event = Event::from_json_line(frame).context("decode event frame")?;
        if event.id <= marker {
            continue; // replayed history from before this open
        }
        match event.subject.as_str() {
            "agent.spawned" if event.correlation == correlation => {
                let run = event.payload()["run"]
                    .as_str()
                    .context("agent.spawned payload carries no run id")?;
                // The pinned output shape (cli_verbs.rs): exactly one line.
                println!("opened {name} run {run}");
                return Ok(());
            }
            "daemon.warning" if event.payload()["what"] == "open-failed" => {
                // Daemon-side refusal: substrate fault class (§9 → 3).
                eprintln!(
                    "rezidnt: open failed: {}",
                    event.payload()["error"].as_str().unwrap_or("(no detail)")
                );
                std::process::exit(3);
            }
            _ => {}
        }
    }
}

/// `attach`: raw byte copy of the daemon's replay-then-live capture stream
/// to stdout until EOF (dtach model — no TTY work, no decoding).
#[cfg(unix)]
fn attach(run: ulid::Ulid) -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let mut reader = connect_and_request(&rezidnt_proto::Request::Attach { run })?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).context("read capture bytes")?;
        if n == 0 {
            return Ok(()); // run finished (or daemon closed): EOF
        }
        out.write_all(&buf[..n]).context("write capture bytes")?;
        out.flush().context("flush capture bytes")?;
    }
}

#[cfg(not(unix))]
fn tail(_subject: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt tail speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

#[cfg(not(unix))]
fn open(_name: &str, _spec_toml: String) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt open speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

#[cfg(not(unix))]
fn attach(_run: ulid::Ulid) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt attach speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}
