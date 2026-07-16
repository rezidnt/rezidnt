//! `rezidnt` — the CLI.
//!
//! S0 verbs (doc §9):
//! - `rezidnt rebuild --db <path> --json` — refold from seq 0, print the
//!   `rezidnt_state::Graph` as JSON on stdout, exit 0. Pinned by
//!   `tests/rebuild_cli.rs`. Cross-platform.
//! - `rezidnt tail [--subject …]` — connect to the daemon socket, print the
//!   stream. Exercised by the S0 exit demo (two concurrent `rezidnt tail`
//!   clients), not by an automated oracle test — see the S0 work order.
//!
//! Stable exit codes (doc §9): 0 ok, 2 gate-fail, 3 substrate-fault,
//! 4 daemon-unreachable. S0 mapping: `rebuild` failures are substrate faults
//! (the log store misbehaved) → 3; `tail` failures are daemon-side
//! (unreachable socket, bad hello, proto mismatch) → 4.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rezidnt", version, about = "rezidnt CLI (S0: rebuild, tail)")]
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
}

fn main() {
    let cli = Cli::parse();
    // Per-verb stable failure class (doc §9); see module docs.
    let (failure_code, result) = match cli.cmd {
        Cmd::Rebuild { db, json } => (3, rebuild(&db, json)),
        Cmd::Tail { subject } => (4, tail(subject.as_deref())),
    };
    if let Err(e) = result {
        eprintln!("rezidnt: {e:#}");
        std::process::exit(failure_code);
    }
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

#[cfg(unix)]
fn tail(subject: Option<&str>) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    use anyhow::Context;
    use rezidnt_proto::{check_hello, decode_hello, socket_path};
    use rezidnt_types::Event;

    let sock = socket_path();
    let stream = UnixStream::connect(&sock)
        .with_context(|| format!("connect to daemon at {}", sock.display()))?;
    let mut reader = BufReader::new(stream);

    let mut hello_line = String::new();
    reader.read_line(&mut hello_line).context("read hello")?;
    let hello = decode_hello(hello_line.trim_end()).context("decode hello")?;
    check_hello(&hello).context("proto check")?;

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

#[cfg(not(unix))]
fn tail(_subject: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt tail S0 speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}
