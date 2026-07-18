//! SP2 hook sub-slice oracle — CRITERION 7 (headline) + the live-daemon leg of
//! CRITERION 4 (script → deny/ask output). DR-014 §Decision 1/2, design §9
//! criterion 1: "a permit-gated run's live tool call outside the allowlist is
//! blocked — the hook emits `deny` with reason; the log carries `permit.requested`
//! + `permit.denied`. One take."
//!
//! HONEST-JUDGE NOTE (stated plainly, per the work order): a fully-live
//! claude-code interception cannot be driven in-process — claude-code is an
//! external CLI. The strongest honest judge available is to drive the
//! `rezidnt permit-hook` subcommand (the PEP itself, DR-014 §Decision 1)
//! against a REAL test daemon over the real socket: feed the hook the stdin tool
//! descriptor claude-code would pass, and assert the hook's PreToolUse output
//! AND the two on-log permit facts. This exercises the exact code path a live
//! claude-code PreToolUse hook invokes — everything except claude-code's own
//! process, which is not part of the daemon under test.
//!
//! RED MODE: **no-such-subcommand-red then assert-red**. `rezidnt permit-hook`
//! does not exist (bins/rezidnt/src/main.rs), so the hook process exits non-zero
//! with clap's "unrecognized subcommand" and writes nothing on stdout; every
//! assertion fails for the right reason (feature absent). Once the subcommand
//! lands it becomes assert-red until the deny path + facts are wired end to end.
//!
//! The daemon socket already services `Request::RequestPermission` via the
//! shared `decide_permit` PDP (SP-wire landed) and a `[gates.permit]`
//! `tool-allowlist` gate denies an off-allowlist tool — so the ONLY missing leg
//! this board pins is the hook subcommand that carries the decision out as
//! claude-code PreToolUse output. That is exactly the SP2 hook sub-slice.

#![cfg(unix)]

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::{cli_bin, connect, read_until, send_line, start_daemon, stub_harness};
use serde_json::json;

const TAIL_DEADLINE: Duration = Duration::from_secs(20);

/// A permit-gated project: the agent declares `gates = ["permit"]` with a
/// `tool-allowlist` native scoped to `allow`. The harness holds the run open
/// long enough for the hook to ask mid-run.
fn make_permit_project(gap_ms: u64, allow: &[&str]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());
    let script = stub_harness(dir.path(), gap_ms);
    let allow_list = allow
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let spec = format!(
        r#"[project]
name = "sp2-hook-live"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["permit"]
bin_override = "{script}"

[gates.permit]
verifiers = [
  {{ native = "tool-allowlist", params = {{ allow = [{allow_list}] }} }},
]
"#,
        repo = repo.display(),
        script = script.display(),
        allow_list = allow_list,
    );
    (dir, spec)
}

/// Open the spec, tail until `agent.spawned`, return the spawned run's ulid.
fn open_and_get_run(socket: &Path, spec: &str) -> String {
    let mut opener = connect(socket);
    send_line(
        &mut opener,
        &serde_json::to_string(&json!({"op": "open", "spec_toml": spec})).unwrap(),
    );
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "agent.spawned"
    });
    lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .and_then(|v| v["payload"]["run"].as_str())
        .expect("agent.spawned carries the run ulid")
        .to_string()
}

/// Drive `rezidnt permit-hook` against the live daemon socket, feeding the stdin
/// tool descriptor claude-code would pass for `tool`. `run` rides `REZIDNT_RUN`
/// (deterministic run discovery, design §3). Returns the hook's stdout.
fn run_permit_hook(socket: &Path, run: &str, tool: &str) -> (bool, String, String) {
    let stdin = json!({
        "tool_name": tool,
        "tool_input": { "command": "echo hi" },
        "session_id": "sess-sp2-live",
        "cwd": "/tmp/worktree",
    })
    .to_string();
    let mut child = Command::new(cli_bin())
        .arg("permit-hook")
        .env("REZIDNT_SOCKET", socket)
        .env("REZIDNT_RUN", run)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rezidnt permit-hook");
    child
        .stdin
        .take()
        .expect("hook stdin")
        .write_all(stdin.as_bytes())
        .expect("write hook stdin");
    let out = child.wait_with_output().expect("wait permit-hook");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The claude-code PreToolUse decision word from the hook's stdout JSON
/// (`hookSpecificOutput.permissionDecision`, design §4).
fn permission_decision(stdout: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    v.get("hookSpecificOutput")?
        .get("permissionDecision")?
        .as_str()
        .map(String::from)
}

/// CRITERION 7 (headline, one take) — a permit-gated run's tool call OUTSIDE the
/// allowlist, asked through the `rezidnt permit-hook` PEP against the live
/// daemon, yields a `deny` PreToolUse output (+ reason) AND lands
/// `permit.requested` + `permit.denied` on the log (I3).
///
/// RED until the `permit-hook` subcommand exists and blocks the off-allowlist
/// tool end to end.
#[test]
fn hook_denies_off_allowlist_tool_and_lands_permit_facts() {
    let daemon = start_daemon();
    // allow only Read; the hook asks for Bash → deny.
    let (_project, spec) = make_permit_project(800, &["Read"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let (_ok, stdout, stderr) = run_permit_hook(&daemon.socket, &run, "Bash");
    let decision = permission_decision(&stdout).unwrap_or_else(|| {
        panic!(
            "permit-hook must emit a PreToolUse decision on stdout; stdout={stdout:?} stderr={stderr:?}"
        )
    });
    assert_eq!(
        decision, "deny",
        "an off-allowlist tool is DENIED at the hook, one take (criterion 1/headline): stdout={stdout}"
    );
    // The blocked agent reads WHY (I6): the hook surfaces the daemon's reason.
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("hook stdout is JSON");
    let reason = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or("");
    assert!(
        !reason.is_empty(),
        "a deny hook output carries a non-empty reason (I6): stdout={stdout}"
    );

    // The two facts land on the log (I3) — the permission stream is first-class.
    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "permit.denied" && v["payload"]["run"] == json!(run)
    });
    assert!(
        lines
            .iter()
            .any(|v| v["subject"] == "permit.requested" && v["payload"]["run"] == json!(run)),
        "permit.requested lands on the log (I3); saw {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|v| v["subject"] == "permit.denied" && v["payload"]["run"] == json!(run)),
        "permit.denied lands on the log (I3); saw {lines:#?}"
    );
}

/// CRITERION 4 (live script leg) — an ALLOWLISTED tool asked through the hook
/// yields an `allow` PreToolUse output (the tool proceeds). Paired with the deny
/// test, this pins the total mapping end to end over the real socket: allow →
/// allow, deny → deny (the never-coerce corner is pinned in the proto crate).
///
/// RED until the subcommand maps a live `allow` decision to hook `allow`.
#[test]
fn hook_allows_an_allowlisted_tool() {
    let daemon = start_daemon();
    let (_project, spec) = make_permit_project(600, &["Read", "Grep", "Bash"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let (_ok, stdout, stderr) = run_permit_hook(&daemon.socket, &run, "Bash");
    let decision = permission_decision(&stdout).unwrap_or_else(|| {
        panic!("permit-hook must emit a PreToolUse decision; stdout={stdout:?} stderr={stderr:?}")
    });
    assert_eq!(
        decision, "allow",
        "an allowlisted tool is ALLOWED through the hook (criterion 4, allow leg): stdout={stdout}"
    );
}
