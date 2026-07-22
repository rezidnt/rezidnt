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
//! Stable exit codes (doc §9, ratified by DR-004): 0 ok, 1 unexpected
//! internal error, 2 local input/usage error, 3 substrate-fault (incl.
//! daemon-side refusals), 4 daemon-unreachable, 5 gate-fail (S4+; an
//! `inconclusive` verdict is 3, never coerced — I6). Mapping: `rebuild`
//! failures are substrate faults (the log store misbehaved) → 3;
//! `tail`/`attach` failures are daemon-side (unreachable socket, bad hello,
//! proto mismatch) → 4. `open`: a missing/unreadable/unparseable spec file
//! is a LOCAL input error → 2 (clap's usage-error convention, pinned by
//! cli_verbs.rs); a daemon-side open-failed refusal → 3; connection
//! failures → 4.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};

mod permit_hook;

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
    /// Read-only fleet board (ratatui). Connects to the daemon, tails the
    /// event stream (replay-from-seq-0 then live via the existing `tail` op),
    /// folds it into a `rezidnt_state::Graph`, and renders the fleet. Pure
    /// client — no daemon change, consumes only a watch channel (I1). `q` or
    /// Ctrl-C quits (read-only navigation).
    Board,
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
    /// Run the `vet` gate over a project spec's governed agents (pre-spawn
    /// policy: bare-mode / pinned-version / allowed-tools). Exit 0 pass, 5
    /// fail, 3 inconclusive (DR-004).
    Vet {
        /// Path to the project spec (rezidnt.toml shape).
        spec: PathBuf,
        /// Emit a machine-readable verdict on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Replay a run's recorded verdicts from log + CAS and report the result
    /// (the compliance sentence, doc §8). Exit 0 all-pass, 5 gate-fail, 3
    /// inconclusive or integrity-alarm (DR-004; never coerced, I6).
    Debrief {
        /// The run ULID.
        run: String,
        /// Emit the machine-readable replay report on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Interrogate a run's gate verdicts (§9 interrogability).
    Gate {
        #[command(subcommand)]
        cmd: GateCmd,
    },
    /// Operator-only actions (DR-031/DR-032): explicit operator authorization
    /// over the loopback-HTTP MCP surface, carrying the operator badge from the
    /// 0600 lockfile.
    Operator {
        #[command(subcommand)]
        cmd: OperatorCmd,
    },
    /// The permit Policy Enforcement Point (DR-014 §Decision 1). claude-code's
    /// `PreToolUse` hook config invokes this: it reads the tool descriptor on
    /// stdin, asks the daemon PDP over `REZIDNT_SOCKET`, and writes the
    /// `hookSpecificOutput.permissionDecision` (`allow`/`deny`/`ask`) on stdout.
    /// Fails CLOSED to `ask` when the daemon is unreachable (never a silent
    /// proceed, I6). Not a separate binary (I7) — a subcommand of `rezidnt`.
    #[command(name = "permit-hook")]
    PermitHook,
}

#[derive(Subcommand)]
enum OperatorCmd {
    /// Terminate a run (DR-032 §Decision 1): POST a `kill_run` `tools/call` over
    /// the loopback-HTTP MCP surface, carrying the operator badge from the 0600
    /// lockfile. DR-004 exits: 0 ok, 2 malformed run ULID (local input), 4
    /// daemon-unreachable, 5 tool-refused (the daemon refused the kill).
    #[command(name = "kill-run")]
    KillRun {
        /// The run ULID, as printed by `rezidnt open`.
        run: String,
    },
}

#[derive(Subcommand)]
enum GateCmd {
    /// Return the failing verifier, evidence refs, and exact recorded inputs
    /// for a run's blocking gate. Exit 0 (the interrogation succeeded; the
    /// verdict rides the output, not the exit code).
    Why {
        /// The run ULID.
        run: String,
        /// Emit the machine-readable answer on stdout.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    // Per-verb stable failure class (doc §9); see module docs.
    let (failure_code, result) = match cli.cmd {
        Cmd::Rebuild { db, json } => (3, rebuild(&db, json)),
        Cmd::Tail { subject } => (4, tail(subject.as_deref())),
        Cmd::Board => (4, board()),
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
        // The gate verbs own their own DR-004 exit codes (0/3/5) — they
        // std::process::exit internally rather than folding into the
        // failure-class table above.
        Cmd::Vet { spec, json } => (1, vet(&spec, json)),
        Cmd::Debrief { run, json } => (1, debrief(&run, json)),
        Cmd::Gate {
            cmd: GateCmd::Why { run, json },
        } => (1, gate_why(&run, json)),
        Cmd::Operator {
            cmd: OperatorCmd::KillRun { run },
        } => {
            // Local input phase (DR-004): a malformed/absent run ULID is exit 2,
            // the same class `attach` gives a bad run id — rejected BEFORE any
            // daemon traffic. The subcommand then owns its own 0/4/5 mapping
            // (operator_kill_run exits internally), so a placeholder class here.
            let run = match run.parse::<ulid::Ulid>() {
                Ok(run) => run,
                Err(e) => {
                    eprintln!("rezidnt: run id {run:?} is not a ULID: {e}");
                    std::process::exit(2);
                }
            };
            (1, operator_kill_run(run))
        }
        // The PEP emits its decision on stdout and fails closed to `ask`
        // internally; a hard error here (unreadable stdin / stdout write) is an
        // unexpected internal fault → 1.
        Cmd::PermitHook => (1, permit_hook::run()),
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

/// Unix socket client plumbing shared by `tail`/`open`/`attach`/`debrief`:
/// connect, consume + check the hello, send the request line, hand back the
/// reader. Relocated to `rezidnt-client` (DR-023) — this thin wrapper preserves
/// the CLI's `anyhow` edge (the shared client returns its own `ClientError`,
/// which `?` folds into `anyhow` unchanged behavior).
#[cfg(unix)]
fn connect_and_request(
    request: &rezidnt_proto::Request,
) -> anyhow::Result<std::io::BufReader<std::os::unix::net::UnixStream>> {
    Ok(rezidnt_client::connect_and_request(request)?)
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

/// `board`: the read-only fleet board (S5, I1). A PURE socket client — it
/// rides the EXISTING `Request::Tail { subject: None }` op (replay-from-seq-0
/// then live), folds each event into a `rezidnt_state::Graph`, publishes each
/// snapshot on a `tokio::sync::watch<Graph>`, and renders the fleet from the
/// watch channel ONLY (never a raw Event — that is the I1 render-side proof).
/// No daemon change, no new proto op.
///
/// Two adapter tasks (each spanned): an INGEST task owns the blocking socket
/// reader and does the fold+publish; a RENDER task drives crossterm's raw-mode
/// terminal from the watch receiver. `q`/Esc/Ctrl-C quit (read-only
/// navigation, not a control-plane action). The terminal is restored on every
/// normal `Ok`/`Err` return of `render_loop`; a panic inside the draw/poll
/// closure would unwind past teardown (no Drop guard), but the process is
/// exiting at that point.
#[cfg(unix)]
fn board() -> anyhow::Result<()> {
    use rezidnt_state::Graph;
    use tokio::sync::watch;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build board runtime")?;

    runtime.block_on(async {
        // Connect on a blocking thread: `connect_and_request` returns a
        // blocking std socket reader (no async socket work in this pure
        // client — the daemon is untouched).
        let reader = tokio::task::spawn_blocking(|| {
            connect_and_request(&rezidnt_proto::Request::Tail { subject: None })
        })
        .await
        .context("join board connect task")??;

        let (tx, rx) = watch::channel(Graph::default());

        // INGEST adapter task: own the blocking reader, fold every event into
        // a running Graph, publish each snapshot on the watch sender. Runs on
        // the blocking pool (the socket read is blocking I/O).
        let ingest = tokio::task::spawn_blocking(move || {
            let span = tracing::info_span!("adapter", kind = "board-ingest");
            let _enter = span.enter();
            ingest_loop(reader, &tx)
        });

        // RENDER adapter task: drive the terminal from the watch receiver
        // ONLY. Also blocking (terminal I/O + crossterm event poll).
        let render = tokio::task::spawn_blocking(move || {
            let span = tracing::info_span!("adapter", kind = "board-render");
            let _enter = span.enter();
            render_loop(rx)
        });

        // The render loop owns the exit signal (user quit). When it returns,
        // drop its handle and abort the ingest task; the terminal is already
        // restored inside `render_loop`.
        let render_result = render.await.context("join board render task")?;
        ingest.abort();
        // Surface an ingest error only if the render side did not already fail
        // (a clean quit makes ingest's aborted/closed-channel exit expected).
        render_result
    })
}

/// The ingest side: read JSONL event frames off the blocking socket reader,
/// fold each into the watch-published Graph. Returns when the daemon closes
/// the stream or the watch receiver is gone (the board quit).
#[cfg(unix)]
fn ingest_loop(
    mut reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    tx: &tokio::sync::watch::Sender<rezidnt_state::Graph>,
) -> anyhow::Result<()> {
    use std::io::BufRead;

    use rezidnt_types::Event;

    let mut graph = rezidnt_state::Graph::default();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).context("read event frame")?;
        if n == 0 {
            return Ok(()); // daemon closed the stream
        }
        let frame = line.trim_end();
        if frame.is_empty() {
            continue;
        }
        // Validating decode (I2 applies on the wire too), then fold into the
        // derived Graph and publish the snapshot. The render loop reads state,
        // never this Event.
        let event = Event::from_json_line(frame).context("decode event frame")?;
        rezidnt_state::apply(&mut graph, &event);
        if tx.send(graph.clone()).is_err() {
            return Ok(()); // render side gone: board quit
        }
    }
}

/// The render side: crossterm raw-mode terminal, redraw from the watch
/// receiver on every change, poll for a quit key. ALWAYS restores the terminal
/// before returning (clean teardown on quit, EOF, or error).
#[cfg(unix)]
fn render_loop(mut rx: tokio::sync::watch::Receiver<rezidnt_state::Graph>) -> anyhow::Result<()> {
    use std::time::Duration;

    use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers};
    use rezidnt_tui::{draw, project};

    let mut terminal = setup_terminal().context("enter raw-mode terminal")?;

    // Run the loop and guarantee teardown regardless of how it ends.
    let outcome = (|| -> anyhow::Result<()> {
        loop {
            // Draw the current fleet state (the watch snapshot, projected —
            // never a raw Event).
            let view = project(&rx.borrow());
            terminal
                .draw(|frame| draw(frame, &view))
                .context("draw board frame")?;

            // Interleave: wait briefly for a quit key; if none, check whether
            // the watch published a fresh snapshot and redraw.
            if event::poll(Duration::from_millis(100)).context("poll terminal input")?
                && let CtEvent::Key(key) = event::read().context("read terminal input")?
                && key.kind == KeyEventKind::Press
            {
                let quit = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL));
                if quit {
                    return Ok(());
                }
            }

            // Non-blocking check for a new snapshot; also detects the ingest
            // side closing (daemon stream ended).
            match rx.has_changed() {
                Ok(true) => {
                    rx.borrow_and_update();
                }
                Ok(false) => {}
                Err(_) => return Ok(()), // sender gone: stream ended
            }
        }
    })();

    // Teardown is unconditional — never leave the terminal raw.
    restore_terminal(&mut terminal);
    outcome
}

/// Enter the alternate screen in raw mode and hand back a ratatui terminal.
#[cfg(unix)]
fn setup_terminal()
-> anyhow::Result<ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>> {
    use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
    use ratatui::Terminal;
    use ratatui::backend::CrosstermBackend;

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("construct terminal")
}

/// Best-effort terminal restore: leave raw mode and the alternate screen. Runs
/// on every exit path; failures are logged, never propagated (a teardown error
/// must not mask the real outcome, and we are exiting anyway).
#[cfg(unix)]
fn restore_terminal(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) {
    use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};

    if let Err(e) = disable_raw_mode() {
        tracing::warn!(error = %e, "board: failed to disable raw mode on teardown");
    }
    if let Err(e) = crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen) {
        tracing::warn!(error = %e, "board: failed to leave alternate screen on teardown");
    }
    if let Err(e) = terminal.show_cursor() {
        tracing::warn!(error = %e, "board: failed to restore cursor on teardown");
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
            other => anyhow::bail!("unexpected reply to open: {other:?}"),
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

/// Route the computed replay-divergence alarms to the daemon's single writer
/// (DR-006, I3): connect, send `record_alarms`, and BLOCK on the ack. The
/// daemon appends each new `integrity.alarm` fact through its Fabric (dedup by
/// (run, gate, verifier) off the log) and acks only once the append is
/// durable — so the caller's report/exit stay correct and the fact is on the
/// log by the time this returns. A daemon-side failure surfaces as a
/// machine-readable error frame.
#[cfg(unix)]
fn record_alarms(alarms: &[rezidnt_gate::IntegrityAlarm]) -> anyhow::Result<()> {
    use std::io::BufRead;

    let records: Vec<rezidnt_proto::AlarmRecord> = alarms
        .iter()
        .map(|a| rezidnt_proto::AlarmRecord {
            run: a.run.clone(),
            gate: a.gate.clone(),
            verifier: a.verifier.clone(),
            recorded: verdict_str(a.recorded).to_string(),
            replayed: verdict_str(a.replayed).to_string(),
        })
        .collect();

    let mut reader =
        connect_and_request(&rezidnt_proto::Request::RecordAlarms { alarms: records })?;

    // The daemon's FIRST frame after the hello answers this request: the
    // AlarmsRecorded ack (append durable) or a machine-readable error.
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .context("read record-alarms ack")?;
        if n == 0 {
            anyhow::bail!("daemon closed the stream before acking record_alarms");
        }
        if line.trim().is_empty() {
            continue;
        }
        return match rezidnt_proto::decode_reply(line.trim_end())
            .context("decode record-alarms ack")?
        {
            rezidnt_proto::Reply::AlarmsRecorded { .. } => Ok(()),
            rezidnt_proto::Reply::Error { code, message, .. } => {
                anyhow::bail!("daemon refused record_alarms ({code}): {message}")
            }
            other => anyhow::bail!("unexpected reply to record_alarms: {other:?}"),
        };
    }
}

#[cfg(not(unix))]
fn record_alarms(_alarms: &[rezidnt_gate::IntegrityAlarm]) -> anyhow::Result<()> {
    anyhow::bail!(
        "recording integrity alarms speaks a Unix domain socket only; the Windows \
         named pipe (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

/// `REZIDNT_DB` override, else `~/.local/state/rezidnt/events.db` (mirrors the
/// daemon's `db_path`). The gate verbs read the log directly (the CLI is a
/// client, I1/I5).
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

/// The default CAS root (used by the `permit-hook` PEP to pin a bulky
/// `tool_input` as a `context_ref`, I2): `REZIDNT_CAS` override, else `cas/`
/// next to the default log path. Mirrors [`cas_path`] over [`db_path`].
/// Unix-only: only the UDS `permit_hook::ask_daemon` pins bulk context.
#[cfg(unix)]
pub(crate) fn cas_dir() -> PathBuf {
    cas_path(&db_path())
}

/// `REZIDNT_CAS` override, else `cas/` next to the log (mirrors the daemon's
/// `cas_path`). Replay is log + CAS, nothing else (I3).
fn cas_path(db: &Path) -> PathBuf {
    if let Some(explicit) = std::env::var_os("REZIDNT_CAS") {
        return PathBuf::from(explicit);
    }
    db.parent()
        .map(|dir| dir.join("cas"))
        .unwrap_or_else(|| PathBuf::from("cas"))
}

/// Read every event from the log (seq 0). The gate verbs fold/scan this.
fn read_log_events(db: &Path) -> anyhow::Result<Vec<rezidnt_types::Event>> {
    let log = rezidnt_fabric::EventLog::open(db)
        .with_context(|| format!("open event log {}", db.display()))?;
    let rows = log.read_from(1).context("read log from seq 1")?;
    Ok(rows.into_iter().map(|r| r.event).collect())
}

/// `rezidnt vet <spec>`: run the three vet natives over each governed agent's
/// pinned spec blob. DR-004 exits: 5 fail, 3 inconclusive, 0 pass. The verdict
/// rides `--json` stdout verbatim (I6).
fn vet(spec_path: &Path, as_json: bool) -> anyhow::Result<()> {
    use rezidnt_gate::{
        AllowedTools, BareMode, NativeVerifier, PinnedVersion, Verdict, VerifierInput,
    };

    let spec_toml = std::fs::read_to_string(spec_path)
        .with_context(|| format!("read spec file {}", spec_path.display()))?;
    let spec = rezidnt_run::spec::ProjectSpec::from_toml_str(&spec_toml)
        .with_context(|| format!("parse spec file {}", spec_path.display()))?;

    let db = db_path();
    let cas_root = cas_path(&db);
    let cas = rezidnt_cas::Cas::open(&cas_root)
        .with_context(|| format!("open cas {}", cas_root.display()))?;

    let natives: Vec<(&str, Box<dyn NativeVerifier>)> = vec![
        ("bare-mode", Box::new(BareMode)),
        ("pinned-version", Box::new(PinnedVersion)),
        ("allowed-tools", Box::new(AllowedTools)),
    ];

    // Vet every agent whose gates name `vet` (a bare spec vets all agents).
    let governed: Vec<&rezidnt_run::spec::AgentSpec> = spec
        .agents
        .iter()
        .filter(|a| a.gates.is_empty() || a.gates.iter().any(|g| g == "vet"))
        .collect();

    for agent in governed {
        let blob = rezidnt_run::spec::agent_spec_toml(agent);
        let cas_ref = cas
            .put(blob.as_bytes(), "application/toml")
            .context("pin spec")?;
        let input = VerifierInput {
            gate: "vet".to_string(),
            workspace: None,
            refs: std::collections::BTreeMap::from([(
                "spec".to_string(),
                format!("cas:blake3:{}", cas_ref.hash),
            )]),
            params: serde_json::json!({}),
            timeout_ms: rezidnt_gate::DEFAULT_TIMEOUT_MS,
        };
        for (name, native) in &natives {
            let out = native.verify(&input, &cas).context("vet native")?;
            match out.verdict {
                Verdict::Pass => {}
                // `emit_verdict` diverges (`-> !`): it prints and exits.
                Verdict::Fail => emit_verdict(as_json, "fail", Some(name), 5),
                Verdict::Inconclusive => emit_verdict(as_json, "inconclusive", Some(name), 3),
            }
        }
    }
    emit_verdict(as_json, "pass", None, 0)
}

/// Print a `{verdict, verifier?}` object (or a plain line) and exit with the
/// DR-004 code — the verdict rides the output, the code rides the class.
fn emit_verdict(as_json: bool, verdict: &str, verifier: Option<&str>, code: i32) -> ! {
    if as_json {
        let mut obj = serde_json::json!({"verdict": verdict});
        if let Some(v) = verifier {
            obj["verifier"] = serde_json::json!(v);
        }
        println!("{obj}");
    } else {
        match verifier {
            Some(v) => println!("{verdict} ({v})"),
            None => println!("{verdict}"),
        }
    }
    std::process::exit(code);
}

/// `rezidnt debrief <run>`: replay the run's recorded verdicts from log + CAS
/// (rezidnt-gate::replay), then report `{alarms, gates, cost}` and exit per
/// DR-004: 3 if any integrity alarm or any inconclusive verdict (never
/// coerced), else 5 if any fail, else 0.
fn debrief(run: &str, as_json: bool) -> anyhow::Result<()> {
    let db = db_path();
    let cas_root = cas_path(&db);
    let cas = rezidnt_cas::Cas::open(&cas_root)
        .with_context(|| format!("open cas {}", cas_root.display()))?;
    let events = read_log_events(&db)?;

    // Replay is over the whole log; scope to this run's gate facts.
    let run_events: Vec<rezidnt_types::Event> = events
        .iter()
        .filter(|e| e.payload()["run"].as_str() == Some(run))
        .cloned()
        .collect();
    let report = rezidnt_gate::replay(&run_events, &cas).context("replay run verdicts")?;

    // The gate verdicts as recorded on the log (folded state), for the report.
    let graph = rezidnt_state::fold(events.iter());
    let gates = graph
        .agent_runs
        .get(run)
        .map(|r| &r.gates)
        .cloned()
        .unwrap_or_default();
    let cost = graph
        .agent_runs
        .get(run)
        .map(|r| {
            serde_json::json!({
                "total_usd": r.total_usd,
                "input_tokens": r.input_tokens,
                "output_tokens": r.output_tokens,
            })
        })
        .unwrap_or_else(|| serde_json::json!({}));

    // DR-004 exit class: an integrity alarm or any inconclusive is 3 (neither
    // trusted nor coerced, I6); else any recorded fail is 5; else 0.
    let has_inconclusive = gates.values().any(|g| g.verdict == "inconclusive");
    let has_fail = gates.values().any(|g| g.verdict == "fail");
    let code = if !report.alarms.is_empty() || has_inconclusive {
        3
    } else if has_fail {
        5
    } else {
        0
    };

    let alarms: Vec<serde_json::Value> = report
        .alarms
        .iter()
        .map(|a| {
            serde_json::json!({
                "verifier": a.verifier,
                "recorded": verdict_str(a.recorded),
                "replayed": verdict_str(a.replayed),
            })
        })
        .collect();

    // The divergence VERDICT is computed above from the CLI's own local log +
    // CAS read; it is the primary signal and is printed FIRST, before any
    // durable-append attempt. Printing before appending is what keeps the
    // finding alive when the daemon is unreachable (DR-006 daemon-down
    // complement): the additive audit improvement must never destroy the
    // finding it decorates.
    if as_json {
        let out = serde_json::json!({
            "run": run,
            "alarms": alarms,
            "gates": gates,
            "cost": cost,
        });
        println!("{out}");
    } else {
        println!("debrief {run}: {} alarm(s)", alarms.len());
    }

    // DR-006: a divergence must land a DURABLE `integrity.alarm` fact on the
    // log. The CLI keeps its direct READ (the report above) but routes the
    // APPEND through the daemon's single writer (I3) — never a second writer
    // racing the append-only log. The daemon dedups by (run, gate, verifier)
    // off the log, so re-running debrief appends nothing new.
    //
    // The append is BEST-EFFORT (DR-006 daemon-down complement, auditor FAIL
    // remediation): it decorates the already-printed finding with durability,
    // so its failure degrades LOUDLY on stderr but does NOT propagate — the
    // exit class stays the DR-004 divergence code (3, computed above) whether
    // or not the daemon was reachable. A hard `?` here would misclassify a real
    // divergence as a catch-all crash (main()'s Debrief failure class is 1) and
    // suppress the report. When the daemon IS up the append lands durably and
    // the fact is on the log before this returns; only the warning differs.
    if !report.alarms.is_empty()
        && let Err(e) = record_alarms(&report.alarms)
    {
        eprintln!(
            "rezidnt: WARNING: integrity alarm(s) found but NOT durably recorded — \
             the daemon was unreachable (append via the single log writer failed): {e:#}"
        );
    }

    std::process::exit(code);
}

fn verdict_str(v: rezidnt_gate::Verdict) -> &'static str {
    match v {
        rezidnt_gate::Verdict::Pass => "pass",
        rezidnt_gate::Verdict::Fail => "fail",
        rezidnt_gate::Verdict::Inconclusive => "inconclusive",
    }
}

/// `rezidnt gate why <run>`: return the failing verifier, evidence refs, and
/// EXACT recorded inputs from the run's blocking gate fact (§9). Exit 0 — the
/// interrogation succeeded; the recorded verdict rides the output verbatim.
fn gate_why(run: &str, as_json: bool) -> anyhow::Result<()> {
    let db = db_path();
    let events = read_log_events(&db)?;

    // The blocking gate fact: the most recent gate.failed / gate.inconclusive
    // for this run (the verdict that blocked it). gate.passed does not block.
    let blocker = events.iter().rfind(|e| {
        e.payload()["run"].as_str() == Some(run)
            && matches!(e.subject.as_str(), "gate.failed" | "gate.inconclusive")
    });

    let Some(event) = blocker else {
        anyhow::bail!("no blocking gate fact recorded for run {run}");
    };
    let verdict = match event.subject.as_str() {
        "gate.failed" => "fail",
        _ => "inconclusive",
    };
    let payload = event.payload();
    if as_json {
        let out = serde_json::json!({
            "run": run,
            "gate": payload["gate"],
            "verdict": verdict,
            "verifier": payload["verifier"],
            "evidence": payload["evidence"],
            "inputs": payload["inputs"],
        });
        println!("{out}");
    } else {
        println!(
            "run {run} blocked at gate {} by {} ({verdict})",
            payload["gate"], payload["verifier"]
        );
    }
    std::process::exit(0);
}

/// The operator lockfile path: `REZIDNT_LOCKFILE` override (the test uses it),
/// else the XDG default `~/.local/state/rezidnt/mcp.lock` (next to the log,
/// mirroring [`db_path`]'s state dir). The daemon announces the loopback-HTTP
/// port + operator badge here (the same 0600 lockfile `serve_http` writes).
fn lockfile_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("REZIDNT_LOCKFILE") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local")
        .join("state")
        .join("rezidnt")
        .join("mcp.lock")
}

/// `rezidnt operator kill-run <run>` (DR-032 §Decision 3). Reads the 0600
/// lockfile (port + operator badge), then POSTs a `kill_run` `tools/call` over
/// the loopback-HTTP MCP surface — NOT the bare socket (DR-032 §Decision 2: the
/// socket's UDS-identity would bypass the explicit operator authorization
/// DR-031 requires). Carries the operator badge from the lockfile.
///
/// This function OWNS its DR-004 exit codes (0/4/5) and exits internally
/// (mirroring the gate verbs): 0 ok, 4 daemon-unreachable (no lockfile / a dead
/// port), 5 tool-refused (the daemon refused the kill). The malformed-run input
/// class (exit 2) is handled by the caller before this runs. The run ULID is
/// already validated; it is passed as text on the wire.
///
/// I7: the loopback POST is a minimal hand-rolled HTTP/1.1 exchange over
/// `std::net::TcpStream` — no HTTP crate is pulled in (one static binary, no new
/// attack surface). The transport is loopback-only and lockfile-gated.
fn operator_kill_run(run: ulid::Ulid) -> anyhow::Result<()> {
    let path = lockfile_path();
    // No lockfile / unreadable / unparseable ⇒ daemon-unreachable (exit 4). A
    // client cannot reach a daemon it cannot locate (the class `tail`/`attach`
    // use for an unreachable socket).
    let lock = match rezidnt_mcp::lockfile::read(&path) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: cannot read operator lockfile {}: {e}",
                path.display()
            );
            std::process::exit(4);
        }
    };

    // The JSON-RPC tools/call for kill_run, carrying the operator badge (§12)
    // and the run. The badge TOKEN rides only the request body to the loopback
    // daemon — never printed, never logged (§12/I2).
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "kill_run",
            "arguments": {
                "badge": lock.badge,
                "run": run.to_string(),
            },
        },
    });

    // Dial the loopback port and POST. Any connect/IO failure is
    // daemon-unreachable (exit 4): a lockfile pointing at a dead port is exactly
    // the unreachable class.
    let response = match loopback_post(lock.port, &request.to_string()) {
        Ok(body) => body,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: kill_run POST to loopback:{} failed: {e:#}",
                lock.port
            );
            std::process::exit(4);
        }
    };

    // A JSON-RPC error object (protocol misuse) or an unparseable body is a
    // daemon-side fault surfacing on the kill path — treat as tool-refused (the
    // kill did not happen). A tool result with `isError: true` is the daemon
    // REFUSING the kill (badge rejected, run not live) ⇒ exit 5.
    let parsed: serde_json::Value = match serde_json::from_str(&response) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rezidnt: kill_run refused: unparseable daemon response ({e})");
            std::process::exit(5);
        }
    };
    if let Some(err) = parsed.get("error") {
        eprintln!("rezidnt: kill_run refused: {err}");
        std::process::exit(5);
    }
    let result = &parsed["result"];
    if result["isError"] == serde_json::json!(true) {
        let detail = result["content"][0]["text"]
            .as_str()
            .unwrap_or("(no detail)");
        eprintln!("rezidnt: kill_run refused: {detail}");
        std::process::exit(5);
    }
    println!("killed run {run}");
    std::process::exit(0);
}

/// Minimal hand-rolled loopback HTTP/1.1 POST to `127.0.0.1:<port>/mcp` (I7 — no
/// HTTP crate). Writes the request, reads the full response, and returns the
/// body (the bytes after the `\r\n\r\n` head/body split). Loopback-only; the
/// port comes from the 0600 lockfile.
fn loopback_post(port: u16, body: &str) -> anyhow::Result<String> {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("connect loopback:{port}"))?;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .context("write kill_run request")?;
    stream.flush().context("flush kill_run request")?;

    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .context("read kill_run response")?;
    // Split off the HTTP head; the body is the JSON-RPC response. A response
    // with no head/body split is a malformed daemon reply (surfaced by the
    // caller as a refusal).
    match raw.split_once("\r\n\r\n") {
        Some((_head, body)) => Ok(body.to_string()),
        None => anyhow::bail!("daemon response had no HTTP head/body split"),
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
fn board() -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt board speaks a Unix domain socket only; the Windows named pipe \
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
