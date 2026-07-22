//! DR-032 slice-1 (operator-kill-run) ORACLE — CRITERION 5: the `rezidnt
//! operator kill-run <run>` subcommand and its DR-004 exit-code classes. This
//! board drives the REAL binary (`CARGO_BIN_EXE_rezidnt`) and asserts the stable
//! exit code per class — the deterministic judge that needs no live daemon.
//!
//! ## DR-032 §Decision 3 — the subcommand
//! `rezidnt operator kill-run <run>` reads the 0600 lockfile (port + operator
//! badge) and POSTs a `tools/call` for `kill_run` over the loopback HTTP MCP
//! surface — NOT the bare socket (DR-032 §Decision 2: the socket's UDS-identity
//! would bypass the explicit operator authorization DR-031 requires).
//!
//! ## DR-004 exit-code classes (mirror bins/rezidnt/src/main.rs:18-27)
//!
//! - 0 ok (happy path — the daemon accepted the kill).
//! - 2 LOCAL input/usage error — a MALFORMED/ABSENT run ULID (the exact class
//!   `attach` uses for a bad run id, main.rs:148-157; clap usage errors are also 2).
//! - 4 daemon-unreachable — no lockfile / a lockfile pointing at a dead port
//!   (mirrors the `tail`/`attach`/`board` failure class, main.rs:135-157).
//! - 5 gate/tool-refused (as applicable — e.g. the daemon refused the kill).
//!
//! The IMPLEMENTER MUST MIRROR the existing per-verb failure-class table in
//! `main()` (the `(failure_code, result)` tuple), NOT invent a new mapping.
//!
//! RED MODE: **no-such-subcommand-red** — `rezidnt operator kill-run` does not
//! exist yet (main.rs has no `Operator` variant / `OperatorCmd::KillRun`), so
//! clap exits with a "unrecognized subcommand" USAGE error. clap's usage-error
//! exit code is 2, which HAPPENS to match the malformed-input class — so the
//! `malformed_run_is_input_error` test could pass for the WRONG reason today.
//! To keep it honest-red, that test ALSO asserts the stderr is the daemon/kill
//! path's message, NOT clap's "unrecognized subcommand" — see its body. The
//! daemon-unreachable and happy-path tests are unambiguously red (a missing
//! subcommand never reaches the daemon dial and never exits 0/4).
//!
//! Cross-platform note: this pins EXIT CODES only (no socket/HTTP dial is
//! completed in the red tests), so unlike the UDS-only `permit_hook.rs` it is
//! NOT `#![cfg(unix)]`-gated. When the implementer wires the loopback-HTTP POST,
//! the happy-path test's daemon dependency (a live `serve_http` lockfile) is the
//! integration seam; its BODY must be linted on WSL per the project's
//! host-vs-WSL rule (memory: /vet is host-side).

use std::process::Command;

/// Run `rezidnt operator kill-run <run>` with the given env overrides. Returns
/// (exit_code, stdout, stderr). We point `REZIDNT_LOCKFILE` at a private,
/// nonexistent path so no red test ever dials a real dev daemon (test
/// isolation) — the implementer reads the operator lockfile from this env
/// override (else the default XDG path).
fn run_kill(run: &str, env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("operator").arg("kill-run").arg(run);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt operator kill-run");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A syntactically-invalid ULID (contains the excluded letters I/L/O/U and is the
/// wrong length) — must be rejected as a LOCAL input error BEFORE any daemon
/// traffic.
const MALFORMED_RUN: &str = "not-a-ulid";
/// A well-formed run ULID (26 Crockford base32 chars) — must PASS local
/// validation and proceed to the daemon dial (so it is NEVER exit 2).
const WELLFORMED_RUN: &str = "01DR032RVNK111C11000000000";
/// A lockfile path that does not exist, forcing the daemon-unreachable class
/// without dialing a real daemon.
const DEAD_LOCKFILE: &str = "/nonexistent/rezidnt-dr032-dead.lock";

/// CRITERION 5 (input class) — a MALFORMED run ULID maps to the LOCAL
/// input-error class (exit 2), the same class `attach` gives a bad run id
/// (main.rs:148-157). It must NOT reach the daemon.
///
/// HONEST-RED GUARD: clap's "unrecognized subcommand" usage error is ALSO exit
/// 2, so a bare code check would pass for the wrong reason while `operator
/// kill-run` is absent. We therefore ALSO require the stderr NOT be clap's
/// unrecognized-subcommand message — i.e. the subcommand exists and rejected the
/// run id itself. RED until the subcommand lands and validates the run locally.
#[test]
fn malformed_run_is_input_error() {
    let (code, _stdout, stderr) = run_kill(MALFORMED_RUN, &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)]);
    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand") && !lc.contains("unexpected argument"),
        "RED-HONESTY: the subcommand must EXIST and reject the run id itself, not clap \
         rejecting an unknown subcommand — stderr was: {stderr}"
    );
    assert_eq!(
        code,
        Some(2),
        "a malformed run ULID is a LOCAL input error (exit 2, DR-004; the class attach \
         gives a bad run id) — stderr: {stderr}"
    );
}

/// CRITERION 5 (daemon-unreachable class) — a WELL-FORMED run with NO reachable
/// daemon (a lockfile that does not exist) maps to the daemon-unreachable class
/// (exit 4, DR-004; the class `tail`/`attach` use). The run id is valid, so it
/// is NEVER an input error — the failure is that the daemon cannot be reached.
///
/// RED until the subcommand exists, validates the run, and dials the daemon
/// (exiting 4 when the lockfile/port is dead).
#[test]
fn wellformed_run_no_daemon_is_unreachable() {
    let (code, _stdout, stderr) = run_kill(WELLFORMED_RUN, &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)]);
    assert_eq!(
        code,
        Some(4),
        "a well-formed run with no reachable daemon is daemon-unreachable (exit 4, DR-004) \
         — NOT an input error and NOT a silent success; stderr: {stderr}"
    );
}

/// CRITERION 5 (happy-path boundary — the deterministic slice of it) — a
/// WELL-FORMED run must PASS local validation, i.e. it is NEVER rejected as an
/// input error (exit 2). The full happy-path exit 0 requires a live daemon
/// (a `serve_http` lockfile), which is the implementer's integration seam; the
/// UNIT-deterministic pin here is that a good run id is not misclassified as bad
/// input. Combined with `wellformed_run_no_daemon_is_unreachable` (which pins
/// exit 4 with no daemon), this fences the happy path from BOTH the input and
/// unreachable classes.
///
/// RED until the subcommand exists and accepts a well-formed run locally.
#[test]
fn wellformed_run_is_not_an_input_error() {
    let (code, _stdout, stderr) = run_kill(WELLFORMED_RUN, &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)]);
    assert_ne!(
        code,
        Some(2),
        "a well-formed run ULID must PASS local validation — it is NOT a local input error \
         (exit 2 is reserved for a malformed/absent run, DR-004); stderr: {stderr}"
    );
}
