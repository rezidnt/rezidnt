//! TRIALS-SLICE-B ENTRY ORACLE — criterion (b) of DR-050 §Decision 2, the
//! TOLERANCE arm: a harness emitting a garbage stream line must NOT kill the
//! run. `AdapterError::BadLine` stays tolerated (warn + continue) — that
//! behavior is deliberate and this test exists so the criterion's other arm
//! (surface `ContractViolated` as a fact-worthy failure, judged by
//! `dr050_contract_violated_surfacing.rs`) cannot be "fixed" by making the
//! stream loop fatal on every adapter error.
//!
//! ## RED MODE (stated plainly, house style)
//!
//! NEVER RED — a REMEDIATION GUARD, not an oracle test, and stated as such:
//! today's loop tolerates everything, so this passes now; it is load-bearing
//! the moment the implementer discriminates `ContractViolated`, when the
//! cheapest wrong fix (kill the run on any `map_line` error) would turn this
//! red. The RED oracle for the criterion lives in the structure test above.
#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{connect, open_request, read_until, send_line, start_daemon};

/// A temp project whose stub harness emits: a valid init line, then a
/// deliberately non-JSON garbage line (`BadLine` at the adapter), then a valid
/// success result line. If the loop stopped tolerating garbage, the result
/// line would never map and no success completion would fold.
fn make_garbage_line_project() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());

    let script = dir.path().join("harness.sh");
    std::fs::write(
        &script,
        r#"#!/bin/sh
echo '{"type":"system","subtype":"init","session_id":"fixture-session","claude_code_version":"2.1.191","tools":[]}'
echo 'this line is not JSON {{{ deliberate garbage (DR-050 tolerated-line arm)'
echo '{"type":"result","subtype":"success","is_error":false,"num_turns":1,"duration_ms":5,"total_cost_usd":0.001,"usage":{"input_tokens":1,"output_tokens":1},"session_id":"fixture-session"}'
"#,
    )
    .expect("write harness stub");
    set_executable(&script);

    let spec = format!(
        r#"[project]
name = "dr050-badline"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
bin_override = "{script}"
"#,
        repo = repo.display(),
        script = script.display(),
    );
    (dir, spec)
}

fn set_executable(script: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(script).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(script, perms).expect("chmod");
}

/// The run survives the garbage line: the stream continues past it and the
/// harness's own success result still folds as the run's completion — token
/// accounting from the RESULT line proves the loop kept mapping after the
/// garbage, rather than the completion having come from the daemon's
/// exit-status fallback.
#[test]
fn a_garbage_stream_line_does_not_kill_the_run() {
    let daemon = start_daemon();
    let (_project, spec) = make_garbage_line_project();

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.completed"
    });

    let completed = lines
        .iter()
        .find(|v| v["subject"] == "agent.completed")
        .expect("read_until stopped on agent.completed");
    assert_eq!(
        completed["payload"]["status"], "success",
        "a garbage line must not kill the run: the harness's own success result \
         is the completion — got {completed:#}"
    );
    assert_eq!(
        completed["payload"]["cost"]["input_tokens"], 1,
        "the completion carries the RESULT line's measured usage — proof the loop \
         kept mapping past the garbage line instead of dying into the exit-status \
         fallback: {completed:#}"
    );
}
