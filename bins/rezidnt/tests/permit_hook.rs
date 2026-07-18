//! SP2 hook sub-slice oracle — the `rezidnt permit-hook` subcommand contract.
//! DR-014 §Decision 1: the PEP is the `rezidnt permit-hook` CLI subcommand (not
//! a new binary, I7). This board drives the REAL binary
//! (`CARGO_BIN_EXE_rezidnt`) with a stdin tool descriptor + `REZIDNT_SOCKET`,
//! captures stdout, and asserts the claude-code `PreToolUse` hook output.
//!
//! Covers:
//!   - CRITERION 4 (script leg, design §4): stdin tool descriptor →
//!     `hookSpecificOutput.permissionDecision` — the daemon `deny` maps to a
//!     `deny` hook output with a reason; `ask` maps to `ask`; NEVER coerced to
//!     proceed (I6). The pure word→class mapping is pinned in
//!     `rezidnt-proto/tests/permit_pep_enforcement.rs`; this pins the SCRIPT
//!     end-to-end (stdin → stdout contract).
//!   - CRITERION 5 (fail-posture, design §5; DR-014 §Decision 3): an
//!     unreachable/absent `REZIDNT_SOCKET` (or a past-timeout) resolves to
//!     `ask`, NEVER a silent proceed. The 250 ms default is
//!     `REZIDNT_PERMIT_TIMEOUT_MS`-overridable so a test forces the path fast.
//!
//! RED MODE: **assert-red / no-such-subcommand-red**. `rezidnt permit-hook` does
//! not exist yet (bins/rezidnt/src/main.rs has no `PermitHook` variant), so the
//! binary exits non-zero with a clap "unrecognized subcommand" error and writes
//! nothing on stdout — every stdout assertion here fails until the subcommand
//! lands. This is red for the RIGHT reason: the feature is absent.
//!
//! NOTE FOR THE IMPLEMENTER (output JSON shape follows claude-code's PreToolUse
//! contract, design §4): the load-bearing pins are (a) a daemon `deny` yields a
//! hook output whose `permissionDecision` is `deny` (+ a non-empty reason);
//! (b) an unreachable socket yields `ask`, never `allow`/proceed. If claude-
//! code's exact JSON key path differs from what design §4 records, align the
//! accessor `permission_decision(&stdout)` below — do NOT weaken the never-
//! coerce assertion.
//!
//! Unix-only: the PEP speaks the daemon UDS (design §3). Windows named-pipe
//! transport is out of scope for this slice (mirrors the proto's `#![cfg(unix)]`
//! socket tests).
#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

/// The stdin JSON claude-code passes a PreToolUse hook (design §4): `tool_name`,
/// `tool_input`, session/cwd context. The hook maps `tool_name` → `tool` and
/// extracts path args from `tool_input` → `paths`.
fn hook_stdin(tool: &str) -> String {
    serde_json::json!({
        "tool_name": tool,
        "tool_input": { "command": "echo hi" },
        "session_id": "sess-sp2",
        "cwd": "/tmp/worktree",
    })
    .to_string()
}

/// Run `rezidnt permit-hook`, feeding `stdin`, with the given env overrides.
/// Returns (exit_ok, stdout, stderr).
fn run_permit_hook(stdin: &str, env: &[(&str, &str)]) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("permit-hook")
        .env("REZIDNT_RUN", "01SP2HOOKCLIRUN0000000000R1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
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

/// CRITERION 5 (fail-posture, the daemon-independent judge, design §5) — an
/// UNREACHABLE socket resolves to `ask`, NEVER a silent proceed. This is the
/// honesty the whole permit axis rests on: when the PDP is not there, the PEP
/// fails CLOSED. A low `REZIDNT_PERMIT_TIMEOUT_MS` forces the path fast.
///
/// RED until `rezidnt permit-hook` exists AND fails closed to `ask`.
#[test]
fn hook_fails_closed_to_ask_when_socket_unreachable() {
    let (_ok, stdout, stderr) = run_permit_hook(
        &hook_stdin("Bash"),
        &[
            ("REZIDNT_SOCKET", "/nonexistent/rezidnt-sp2-dead.sock"),
            ("REZIDNT_PERMIT_TIMEOUT_MS", "50"),
        ],
    );
    let decision = permission_decision(&stdout).unwrap_or_else(|| {
        panic!("permit-hook must emit a PreToolUse decision on stdout; stdout={stdout:?} stderr={stderr:?}")
    });
    assert_ne!(
        decision, "allow",
        "an unreachable PDP is NEVER a silent proceed (I6, DR-014 §Decision 3): stdout={stdout}"
    );
    assert_eq!(
        decision, "ask",
        "an unreachable/absent socket fails CLOSED to ask (fail-posture, design §5): stdout={stdout}"
    );
}

/// CRITERION 5 (fail-posture, absent-socket leg) — with NO `REZIDNT_SOCKET` set
/// at all AND no daemon, the hook still resolves to `ask`, never proceed. Absence
/// of a reachable daemon is the same fail-closed posture as a dead socket.
///
/// RED until the subcommand exists and fails closed.
#[test]
fn hook_fails_closed_to_ask_when_socket_env_absent_and_no_daemon() {
    // Point at an unlikely path via the standard fallback: no daemon is
    // listening, so the connect fails and the posture must be `ask`. We still
    // set REZIDNT_SOCKET to a private dead path to avoid dialing a real dev
    // daemon on the host (test isolation).
    let (_ok, stdout, stderr) = run_permit_hook(
        &hook_stdin("Bash"),
        &[
            ("REZIDNT_SOCKET", "/nonexistent/rezidnt-sp2-absent.sock"),
            ("REZIDNT_PERMIT_TIMEOUT_MS", "50"),
        ],
    );
    let decision = permission_decision(&stdout).unwrap_or_else(|| {
        panic!("permit-hook must emit a PreToolUse decision; stdout={stdout:?} stderr={stderr:?}")
    });
    assert_eq!(
        decision, "ask",
        "no reachable daemon ⇒ fail closed to ask, never proceed (design §5): stdout={stdout}"
    );
}
