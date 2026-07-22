//! DR-034 live-unblock — the REAL-PEP leg (the client half the daemon-only
//! implementation was missing). `permit_live_unblock.rs` proves the DAEMON holds
//! and wakes a held socket request, but it hand-rolls a raw `UnixStream` with a
//! multi-second client budget — it stands in for the PEP, it is NOT the PEP. The
//! shipped client that stalls in production is `rezidnt permit-hook`'s
//! `ask_daemon()`, which read the reply under the 250ms hot-path budget. Against
//! a daemon now HOLDING an escalated reply up to `REZIDNT_UNBLOCK_TIMEOUT_MS`
//! (8s), a real PEP hit its own 250ms read timeout, `?`-propagated, and
//! fail-closed to `ask` — so nothing proved a real agent ever waited long enough
//! to see the wake. DR-034 §Design says the change lands "in the daemon handler
//! AND in the `#[cfg(unix)]` body of `ask_daemon()`"; this suite is the judge for
//! that client half.
//!
//! RED-before / GREEN-after: against the pre-fix PEP (read bounded by the 250ms
//! hot-path) `hook_resumes_allow_when_unblock_engaged` is RED — the hook cuts the
//! read off at 250ms and emits `ask` before the 8s hold can wake it. After the
//! PEP lifts its reply-read budget to the unblock window it is GREEN (the hook
//! collects the daemon's woken `allow`). The paired
//! `hook_fails_closed_to_ask_without_unblock_knob` guards the UNCHANGED path: with
//! the knob unset, an escalate-without-resolve still fails closed to `ask` inside
//! the hot-path window — proving the extended read budget is opt-in and the fast
//! fail-closed posture is intact.
//!
//! The wake is driven through the REAL operator door (a `resolve_permit`
//! `tools/call` over loopback-HTTP MCP, badge from the 0600 lockfile) — one
//! daemon, both doors — so a green run proves the whole DR-034 mechanism end to
//! end through the SHIPPED PEP, not a raw-socket stand-in.

#![cfg(unix)]

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::{
    cli_bin, connect, mcp_tool_call, read_until, send_line, start_daemon_with_mcp_and_unblock,
    stub_harness, wait_for_lockfile,
};
use serde_json::json;

const LOCK_DEADLINE: Duration = Duration::from_secs(10);
const TAIL_DEADLINE: Duration = Duration::from_secs(20);

/// A LONG unblock budget for the resume test: the operator resolution lands while
/// the held reply is outstanding. 8s comfortably outlasts open→escalate→resolve
/// on CI without dragging the suite (mirrors `permit_live_unblock.rs`).
const UNBLOCK_LONG_MS: u64 = 8_000;

/// An empty-permit-gate project (`gates = ["permit"]`, verifier set EMPTY →
/// escalate, DR-011 §3): the natural "held" starting state. Mirrors
/// `permit_live_unblock.rs::make_empty_permit_project`.
fn make_empty_permit_project(dir: &Path, gap_ms: u64) -> String {
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());
    let script = stub_harness(dir, gap_ms);
    format!(
        r#"[project]
name = "dr034-pep-empty-permit"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["permit"]
bin_override = "{script}"

[gates.permit]
verifiers = []
"#,
        repo = repo.display(),
        script = script.display(),
    )
}

/// Open the spec over the bare socket and tail until `agent.spawned`; return the
/// spawned run's ulid — the run the held permit request targets.
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

/// Spawn the REAL `rezidnt permit-hook` PEP in a thread, feeding it the stdin
/// descriptor claude-code passes for `tool`. `REZIDNT_RUN` carries the run
/// (deterministic discovery); `unblock_ms` sets the PEP's `REZIDNT_UNBLOCK_TIMEOUT_MS`
/// so its reply-read budget matches the daemon's hold (the DR-034 PEP half). The
/// thread returns the hook's `(success, stdout, stderr)` once it emits its one
/// PreToolUse frame. Because the hook mints its OWN `request_id` internally, the
/// test learns it from the `permit.escalated` fact the hook lands (below).
fn spawn_permit_hook(
    socket: &Path,
    run: &str,
    tool: &str,
    unblock_ms: Option<u64>,
) -> thread::JoinHandle<(bool, String, String)> {
    let socket = socket.to_path_buf();
    let run = run.to_string();
    let tool = tool.to_string();
    thread::spawn(move || {
        let stdin = json!({
            "tool_name": tool,
            "tool_input": { "command": "echo hi" },
            "session_id": "sess-dr034-pep",
            "cwd": "/tmp/worktree",
        })
        .to_string();
        let mut cmd = Command::new(cli_bin());
        cmd.arg("permit-hook")
            .env("REZIDNT_SOCKET", &socket)
            .env("REZIDNT_RUN", &run)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match unblock_ms {
            Some(ms) => {
                cmd.env("REZIDNT_UNBLOCK_TIMEOUT_MS", ms.to_string());
            }
            // Unset: the knob is genuinely absent, so the PEP reads under the
            // hot-path budget (the UNCHANGED fast fail-closed path).
            None => {
                cmd.env_remove("REZIDNT_UNBLOCK_TIMEOUT_MS");
            }
        }
        let mut child = cmd.spawn().expect("spawn rezidnt permit-hook");
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
    })
}

/// The claude-code PreToolUse decision word from the hook stdout JSON
/// (`hookSpecificOutput.permissionDecision`, design §4).
fn permission_decision(stdout: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    v.get("hookSpecificOutput")?
        .get("permissionDecision")?
        .as_str()
        .map(String::from)
}

/// Tail (fresh) until a `permit.escalated` for `run` lands; return the PEP-minted
/// `request_id` it carries. The hook mints its own id inside `build_request`, so
/// this is how the operator learns WHICH escalation to resolve.
fn escalated_request_id(socket: &Path, run: &str) -> String {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "permit.escalated" && v["payload"]["run"] == json!(run)
    });
    lines
        .iter()
        .find(|v| v["subject"] == "permit.escalated" && v["payload"]["run"] == json!(run))
        .and_then(|v| v["payload"]["request_id"].as_str())
        .expect("permit.escalated carries the PEP's request_id")
        .to_string()
}

/// Drive a REAL operator `resolve_permit` over loopback-HTTP MCP (the operator
/// door, badge from the 0600 lockfile); `request_id` is the escalated ask's id.
fn operator_resolve(url: &str, badge: &str, run: &str, request_id: &str, decision: &str) {
    let result = mcp_tool_call(
        url,
        60,
        "resolve_permit",
        json!({
            "badge": badge,
            "run": run,
            "request_id": request_id,
            "decision": decision,
            "reason": "operator approved after review (DR-034 real-PEP oracle)",
        }),
    );
    assert_ne!(
        result["isError"],
        json!(true),
        "the operator resolve_permit must succeed so a real permit.resolved lands: {result:#}"
    );
}

/// CRITERION (real-PEP resume) — a `rezidnt permit-hook` invocation whose request
/// escalates, WITH `REZIDNT_UNBLOCK_TIMEOUT_MS` engaged, waits out the daemon's
/// hold and emits `allow` when the operator resolves within the budget — the
/// SHIPPED agent resumes without a re-prompt.
///
/// RED against the pre-fix PEP: `ask_daemon` read the reply under the 250ms
/// hot-path budget, so the hook self-escalated to `ask` long before the 8s hold
/// woke — `permissionDecision` was `ask`, never `allow`. GREEN after the PEP
/// lifts its reply-read budget to the unblock window.
#[test]
fn hook_resumes_allow_when_unblock_engaged() {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_LONG_MS);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile url").to_string();
    let badge = lock["badge"].as_str().expect("operator badge").to_string();

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    // The REAL PEP, its reply-read budget matching the daemon hold.
    let hook = spawn_permit_hook(&daemon.socket, &run, "Bash", Some(UNBLOCK_LONG_MS));

    // Learn the PEP's minted request_id from its escalation.
    let req_id = escalated_request_id(&daemon.socket, &run);
    // DELIBERATELY hold PAST the 250ms hot-path budget before resolving, so the
    // daemon is genuinely holding beyond the OLD read cutoff when the resolve
    // lands. This is what makes the test RED against a 250ms-read PEP (it would
    // self-escalate to `ask` here) and GREEN only for a PEP that extended its
    // reply-read budget — modelling a real operator who resolves seconds later,
    // not sub-250ms. Well under the 8s hold + 2s client margin.
    thread::sleep(Duration::from_millis(1_500));
    operator_resolve(&url, &badge, &run, &req_id, "allow");

    let (_ok, stdout, stderr) = hook.join().expect("permit-hook thread");
    let decision = permission_decision(&stdout).unwrap_or_else(|| {
        panic!("permit-hook must emit a PreToolUse decision; stdout={stdout:?} stderr={stderr:?}")
    });
    assert_eq!(
        decision, "allow",
        "the REAL PEP waited out the daemon's hold and RESUMED with the operator's allow — a \
         250ms-cutoff PEP would have emitted `ask` here (the gap this test pins): stdout={stdout}"
    );
}

/// CRITERION (unchanged fast fail-closed) — with `REZIDNT_UNBLOCK_TIMEOUT_MS`
/// UNSET, a hook whose request escalates and gets NO resolution fails closed to
/// `ask` WITHIN the hot-path window — never a hang, never `allow`. Proves the
/// extended read budget is strictly opt-in: the shipped default posture (250ms
/// fail-closed) is untouched.
#[test]
fn hook_fails_closed_to_ask_without_unblock_knob() {
    // A short unblock budget on the DAEMON is irrelevant here — the PEP has the
    // knob UNSET, so it must read under the hot-path budget regardless of what the
    // daemon might hold. (The daemon side is a don't-care; no resolve is driven.)
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_LONG_MS);
    let _ = wait_for_lockfile(&lock_path, LOCK_DEADLINE);

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    let started = Instant::now();
    // Knob UNSET on the PEP → hot-path read budget → fast fail-closed.
    let hook = spawn_permit_hook(&daemon.socket, &run, "Bash", None);
    let (_ok, stdout, stderr) = hook.join().expect("permit-hook thread");
    let elapsed = started.elapsed();

    let decision = permission_decision(&stdout).unwrap_or_else(|| {
        panic!("permit-hook must emit a PreToolUse decision; stdout={stdout:?} stderr={stderr:?}")
    });
    assert_eq!(
        decision, "ask",
        "with the unblock knob UNSET the escalated hook fails CLOSED to `ask` (never allow) — the \
         unchanged hot-path posture: stdout={stdout}"
    );
    // The whole invocation resolves well within the daemon's 8s hold — the PEP did
    // NOT wait the long budget (it never opted in). Open→spawn→escalate + process
    // spawn dominate; a generous 6s ceiling still proves it did not sit on an 8s
    // hold + 2s margin (10s).
    assert!(
        elapsed < Duration::from_secs(6),
        "the unset-knob hook fail-closes FAST (hot-path window), not on the long unblock budget — \
         took {elapsed:?}"
    );
    let _ = stderr;
}
