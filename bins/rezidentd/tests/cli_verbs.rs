//! S1 remediation oracle: the `rezidnt open` / `rezidnt attach` CLI verbs
//! (the S1 debrief's failed driver) and refusal-at-open for unknown
//! harnesses (the I4 finding, pinned daemon-side).
//!
//! ## Why the CLI binary is tested from the daemon's crate (documented seam)
//!
//! These tests spawn BOTH workspace binaries. Cargo exposes
//! `CARGO_BIN_EXE_<name>` only for bin targets of the crate under test, so
//! `bins/rezidnt/tests` cannot see `CARGO_BIN_EXE_rezidentd` and vice versa,
//! and artifact-dependencies (the clean mechanism) are still unstable. The
//! pragmatic resolution: the tests live here, next to the daemon they need,
//! and locate the `rezidnt` CLI as a SIBLING of `CARGO_BIN_EXE_rezidentd`
//! in the same `target/<profile>/` directory. Under `cargo test --workspace`
//! (which is what the vet gauntlet runs) every workspace bin is built before
//! integration tests execute, so the sibling exists. A bare
//! `cargo test -p rezidentd` may not have built the CLI — [`cli_bin`] panics
//! with instructions instead of mis-testing. No `.exe` handling: this file
//! is `#![cfg(unix)]` like the rest of the S1 suite.
//!
//! ## Pinned `rezidnt open` output shape (the contract, not a suggestion)
//!
//! On success, stdout is EXACTLY one line:
//!
//! ```text
//! opened <workspace-name> run <run-ulid>
//! ```
//!
//! where `<workspace-name>` is the spec's `[project].name` and `<run-ulid>`
//! is the 26-char Crockford ULID of the spawned run — the SAME id carried in
//! `agent.spawned` `payload.run`, so `rezidnt attach <run-ulid>` works
//! verbatim from a copy-paste. Exit 0.
//!
//! ## Exit-code note (flagged for /dr, not resolved here)
//!
//! Doc §9 (ratified by DR-004) pins 0 ok / 1 unexpected / 2 local
//! input-usage / 3 substrate-fault / 4 daemon-unreachable / 5 gate-fail.
//! These tests pin clap's conventional exit 2 for local usage/input errors,
//! which DR-004 aligned the table with; gate-fail moved to the previously
//! unclaimed 5, so the historical collision on the number 2 is resolved.
#![cfg(unix)]

mod common;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{
    DaemonGuard, connect, make_project, open_request, read_until, send_line, start_daemon,
};

/// Locate the `rezidnt` CLI as a sibling of the daemon binary (see module
/// docs for why this seam exists and when it holds).
fn cli_bin() -> PathBuf {
    let path = Path::new(env!("CARGO_BIN_EXE_rezidentd")).with_file_name("rezidnt");
    assert!(
        path.exists(),
        "rezidnt CLI not found at {} — these tests need every workspace bin \
         built; run `cargo test --workspace` (the vet gauntlet) or \
         `cargo build -p rezidnt` first",
        path.display()
    );
    path
}

/// Run a CLI verb against the test daemon's socket/db env; capture output.
fn run_cli(daemon: &DaemonGuard, args: &[&str]) -> Output {
    Command::new(cli_bin())
        .args(args)
        .env("REZIDNT_SOCKET", &daemon.socket)
        .env("REZIDNT_DB", &daemon.db)
        .output()
        .expect("run rezidnt CLI")
}

/// Crockford base32, 26 chars — the ULID wire shape.
fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

/// Enforce the pinned `open` stdout shape; returns (workspace-name, run-ulid).
fn parse_opened(stdout: &str) -> (String, String) {
    let trimmed = stdout.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "pinned shape: `rezidnt open` prints exactly one stdout line \
         `opened <workspace-name> run <run-ulid>`; got {stdout:?}"
    );
    let words: Vec<&str> = lines[0].split_whitespace().collect();
    match words.as_slice() {
        ["opened", name, "run", run] if is_ulid(run) => ((*name).to_string(), (*run).to_string()),
        _ => panic!(
            "pinned shape: `opened <workspace-name> run <run-ulid>` \
             (run = 26-char Crockford ULID); got {:?}",
            lines[0]
        ),
    }
}

/// Pre-fix failure mode at 1ac0cff: `open` is an unknown subcommand — clap
/// exits 2 with empty stdout, so the exit-code assertion fails first.
///
/// Pins: `rezidnt open <spec-path>` reads the spec FROM A FILE, materializes
/// the workspace through the daemon, reports the pinned one-line identity on
/// stdout (the run id being the real `agent.spawned` run, not decoration),
/// exits 0 — and every materialization step is visible in `tail`.
#[test]
fn cli_open_materializes_and_reports() {
    let daemon = start_daemon();
    let (project, spec) = make_project(200);
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, &spec).expect("write spec file");

    let out = run_cli(&daemon, &["open", spec_path.to_str().expect("utf8 path")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "rezidnt open <spec> must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (name, run_id) = parse_opened(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(
        name, "s1-exit",
        "workspace name must be the spec's [project].name"
    );

    // The identity on stdout must be the identity on the fabric.
    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.completed"
    });
    let subjects: Vec<String> = lines
        .iter()
        .filter_map(|v| v["subject"].as_str().map(String::from))
        .collect();
    let pos = |s: &str| {
        subjects
            .iter()
            .position(|x| x == s)
            .unwrap_or_else(|| panic!("{s} never appeared in tail; saw {subjects:?}"))
    };
    assert!(pos("workspace.opened") < pos("agent.spawned"));
    assert!(pos("agent.spawned") < pos("agent.completed"));

    let spawned = lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .expect("agent.spawned line");
    assert_eq!(
        spawned["payload"]["run"].as_str(),
        Some(run_id.as_str()),
        "the run id printed by `open` must be the run id in agent.spawned"
    );
}

/// Pre-fix failure mode at 1ac0cff: `open` is an unknown subcommand, so its
/// stdout is empty and the pinned-shape parse in the setup panics.
///
/// Pins: `rezidnt attach <run-id>` (the id copy-pasted from `open`'s pinned
/// stdout) replays the run's capture ring — the stub harness's init line
/// (`"type":"system"`) appears on attach stdout whether the run is still
/// live or already finished (DR-001 dtach model).
#[test]
fn cli_attach_replays_tail() {
    let daemon = start_daemon();
    let (project, spec) = make_project(700);
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, &spec).expect("write spec file");

    let out = run_cli(&daemon, &["open", spec_path.to_str().expect("utf8 path")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "rezidnt open <spec> must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (_name, run_id) = parse_opened(&String::from_utf8_lossy(&out.stdout));

    // attach may stream live indefinitely: read via a drain thread under a
    // deadline, then kill the process once the marker is seen.
    let mut child = Command::new(cli_bin())
        .args(["attach", &run_id])
        .env("REZIDNT_SOCKET", &daemon.socket)
        .env("REZIDNT_DB", &daemon.db)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rezidnt attach");
    let mut stdout = child.stdout.take().expect("attach stdout piped");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    const MARKER: &[u8] = br#""type":"system""#;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut captured: Vec<u8> = Vec::new();
    let mut found = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                captured.extend_from_slice(&chunk);
                if captured.windows(MARKER.len()).any(|w| w == MARKER) {
                    found = true;
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        found,
        "attach must replay the capture ring: stub init marker {:?} not in \
         {} captured bytes: {:?}",
        String::from_utf8_lossy(MARKER),
        captured.len(),
        String::from_utf8_lossy(&captured)
    );
}

/// Pre-fix failure mode at 1ac0cff: unknown subcommand ALSO exits 2, so the
/// exit-code assertion is deliberately not the teeth here — clap's
/// unknown-subcommand stderr never names the spec path, and that assertion
/// is what fails today. Both must hold after implementation.
///
/// Pins: a nonexistent spec path is a LOCAL input error — exit 2 (clap's
/// usage-error convention, ratified by DR-004), stderr names the offending
/// path, and no success line reaches stdout.
#[test]
fn cli_open_missing_spec_is_exit_2_family() {
    let daemon = start_daemon();
    let missing = tempfile::tempdir()
        .expect("tempdir")
        .path()
        .join("nonexistent")
        .join("rezidnt.toml");
    let missing_str = missing.to_str().expect("utf8 path");

    let out = run_cli(&daemon, &["open", missing_str]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(missing_str),
        "stderr must name the missing spec path {missing_str}; got: {stderr}"
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing spec file is a local input error: exit 2; stderr: {stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("opened "),
        "no success line on stdout for a failed open"
    );
}

/// Pre-fix failure mode at 1ac0cff: `launch_agent` never consults
/// `agent.harness` — the stub spawns fine via `bin_override`, so
/// `agent.spawned` hits the fabric and the no-spawn assertion fails.
///
/// Pins (the I4 finding, daemon-side): an `open` whose spec names an unknown
/// harness is REFUSED AT OPEN — no `agent.spawned` ever appears, and the
/// refusal is visible in `tail` as `daemon.warning` with
/// `payload.what == "open-failed"`. `bin_override` is deliberately left
/// pointing at a working stub: refusal must key on the harness NAME, not on
/// a spawn failure. (The AgentSubstrate trait seam itself is S2+
/// architecture — not pinned here.)
#[test]
fn open_refuses_unknown_harness() {
    let daemon = start_daemon();
    let (_project, spec) = make_project(100);
    let spec = spec.replace(
        r#"harness = "claude-code""#,
        r#"harness = "not-a-real-harness""#,
    );
    assert!(
        spec.contains("not-a-real-harness"),
        "test bug: harness line substitution failed; spec: {spec}"
    );

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
            || (v["subject"] == "daemon.warning" && v["payload"]["what"] == "open-failed")
    });

    let spawned: Vec<_> = lines
        .iter()
        .filter(|v| v["subject"] == "agent.spawned")
        .collect();
    assert!(
        spawned.is_empty(),
        "unknown harness must be refused at open — no agent.spawned; \
         observed: {spawned:?}"
    );
    let last = lines.last().expect("read_until returned lines");
    assert_eq!(last["subject"], "daemon.warning");
    assert_eq!(
        last["payload"]["what"], "open-failed",
        "the refusal must be visible in tail as an open-failed warning; \
         got: {last:?}"
    );
}
