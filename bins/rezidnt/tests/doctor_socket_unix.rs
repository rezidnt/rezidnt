//! DR-036 sub-slice `onboarding-doctor` ORACLE (UNIX-gated slice) — the
//! socket/lockfile-writable check's PASS/NON-PASS pair, exercised through the
//! `REZIDNT_SOCKET` / `REZIDNT_LOCKFILE` seams against REAL filesystem paths. The
//! §11 golden-path assumes the daemon's socket/lockfile transport path is writable
//! (§11 line 240 / line 252); on unix that path is a UDS whose *writability* is a
//! filesystem property of its parent directory. These two tests pin that the check
//! DISCRIMINATES — a writable parent → `pass`, a non-writable / non-existent parent
//! → non-pass — which is the socket half of criterion 1's never-coerce honesty.
//!
//! `#![cfg(unix)]`-gated (runs on WSL, not host Windows): the "non-writable parent"
//! leg uses unix directory permissions (mode 0o555, no write bit) as the
//! deterministic lever, and the socket path is the unix UDS the daemon actually
//! binds. The cross-platform `doctor_cli.rs` covers the shape/exit/daemon-free
//! contract on host `/vet`; THIS file adds the unix-only writable-probe
//! discrimination. Per the project's host-vs-WSL rule (memory: /vet is host-side),
//! this file's lints run on WSL — the implementer must green it there.
//!
//! Seam pinned for the implementer: the socket/lockfile-writable check MUST read
//! `REZIDNT_SOCKET` (and/or `REZIDNT_LOCKFILE`) — the EXISTING env vars
//! (`bins/rezidnt/src/main.rs` `lockfile_path()` honors `REZIDNT_LOCKFILE`; the
//! daemon honors `REZIDNT_SOCKET`) — and probe the WRITABILITY of that path's
//! PARENT directory (can the daemon create the socket/lockfile there?). It must NOT
//! bind or connect the socket (criterion 3: opens no socket) and must NOT probe a
//! hardcoded XDG path (or these seams could not force the two legs).
//!
//! Authoring intent, past-tense-safe: written RED before `doctor` existed. With no
//! `Doctor` verb, clap exited "unrecognized subcommand" (exit 2) and printed no
//! JSON, so `parse_checks` panicked on the empty stdout — the correct red at
//! authoring time; the stderr guard confirms the cause was the absent subcommand.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::Value;

/// Run `rezidnt doctor --json` in a controlled env; return (exit, stdout, stderr).
fn run_doctor_json(env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("doctor").arg("--json");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt doctor --json");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// HONEST-RED guard: stderr must not be clap's absent-subcommand usage error.
fn assert_not_clap_usage_error(stderr: &str) {
    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand") && !lc.contains("unexpected argument"),
        "RED-HONESTY: `rezidnt doctor` must EXIST and run its socket-writable check itself, \
         NOT be clap rejecting an unknown subcommand — stderr: {stderr}"
    );
}

/// Parse `doctor --json` into the `checks` array (panics loudly while unbuilt).
fn parse_checks(stdout: &str, stderr: &str, code: Option<i32>) -> Vec<Value> {
    let doc: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`doctor --json` must print a JSON findings object (doctor absent / no --json \
             yet?): {e}; exit {code:?}; stderr: {stderr}; stdout: {stdout:?}"
        )
    });
    doc["checks"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("the doctor --json object must carry a `checks` array: {doc:#}"))
}

/// The socket/lockfile-writable check element (name contains `socket` or
/// `lockfile`), or a loud panic — the check must be reported in both legs.
fn writable_check(checks: &[Value]) -> Value {
    checks
        .iter()
        .find(|c| {
            let n = c["name"].as_str().unwrap_or_default().to_lowercase();
            n.contains("socket") || n.contains("lockfile")
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!("a socket/lockfile-writable check must be reported (§11 line 240): {checks:#?}")
        })
}

fn status_of(check: &Value) -> String {
    check["status"]
        .as_str()
        .unwrap_or_else(|| panic!("the writable check must carry a string status: {check:#}"))
        .to_lowercase()
}

/// PASS leg — `REZIDNT_SOCKET`/`REZIDNT_LOCKFILE` point INTO a writable temp dir
/// (its parent exists and is writable), so the daemon COULD create the socket/
/// lockfile there. The socket/lockfile-writable check must report `pass`.
///
/// Written RED: `doctor` absent → no JSON → panic. Once built, the satisfiable leg
/// of the writable-probe discrimination.
#[test]
fn writable_socket_path_passes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let (code, stdout, stderr) = run_doctor_json(&[
        ("REZIDNT_SOCKET", sock.to_str().unwrap()),
        ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
    ]);
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    let check = writable_check(&checks);
    assert_eq!(
        status_of(&check),
        "pass",
        "with REZIDNT_SOCKET/REZIDNT_LOCKFILE under a WRITABLE parent dir, the \
         socket/lockfile-writable check must be `pass` (the daemon could create it there). \
         check: {check:#}"
    );
}

/// NON-PASS leg — `REZIDNT_SOCKET`/`REZIDNT_LOCKFILE` point under a parent dir with
/// NO write bit (mode 0o555): the daemon could NOT create the socket/lockfile there.
/// The check must report a NON-pass status (fail or inconclusive) — NEVER `pass`
/// for an unwritable path (I6 never-coerce). Paired with `writable_socket_path_passes`
/// so the check is proven to DISCRIMINATE, not always-inconclusive.
///
/// Written RED: `doctor` absent → no JSON → panic. Once built, the unsatisfiable leg
/// of the writable-probe discrimination.
#[test]
fn unwritable_socket_path_is_not_pass() {
    let parent = tempfile::tempdir().expect("tempdir");
    // Strip the write bit from the parent so nothing can be created inside it.
    let mut perms = std::fs::metadata(parent.path())
        .expect("stat parent")
        .permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(parent.path(), perms).expect("chmod parent read-only");

    // Guard: if the test runner is root (e.g. some CI/WSL setups), 0o555 does NOT
    // block writes and the leg is not exercisable — skip rather than false-green.
    let probe = parent.path().join(".doctor-writable-probe");
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        eprintln!(
            "skipping unwritable_socket_path_is_not_pass: parent is writable despite 0o555 \
             (running as root?) — the unwritable leg is not exercisable here"
        );
        // Best-effort: restore perms so tempdir cleanup succeeds.
        if let Ok(md) = std::fs::metadata(parent.path()) {
            let mut p = md.permissions();
            p.set_mode(0o755);
            let _ = std::fs::set_permissions(parent.path(), p);
        }
        return;
    }

    let sock = parent.path().join("rezidnt.sock");
    let lock = parent.path().join("mcp.lock");
    let (code, stdout, stderr) = run_doctor_json(&[
        ("REZIDNT_SOCKET", sock.to_str().unwrap()),
        ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
    ]);
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    let check = writable_check(&checks);
    let s = status_of(&check);

    // Restore perms BEFORE asserting so a panic never leaks an undeletable tempdir.
    if let Ok(md) = std::fs::metadata(parent.path()) {
        let mut p = md.permissions();
        p.set_mode(0o755);
        let _ = std::fs::set_permissions(parent.path(), p);
    }

    assert_ne!(
        s, "pass",
        "with REZIDNT_SOCKET/REZIDNT_LOCKFILE under a NON-writable parent (0o555), the \
         socket/lockfile-writable check must NOT be `pass` (I6: an unsatisfiable check is \
         never coerced to pass) — it must be `fail` or `inconclusive`. check status was: {s}"
    );
}

/// PRECEDENCE leg — the two seams DIVERGE: `REZIDNT_LOCKFILE`'s parent is WRITABLE
/// while `REZIDNT_SOCKET`'s parent is NOT (0o555). The check prefers the LOCKFILE
/// (the path this CLI's `lockfile_path()` is authoritative about; a dead socket is
/// `open`'s exit-4 concern, not a doctor gate), so it must probe the writable
/// lockfile parent and report `pass` — a SOCKET-first probe would fail on the
/// unwritable socket parent. Asserting `pass` pins the `REZIDNT_LOCKFILE`-first
/// precedence the retired socket-first probe left untested.
///
/// (The `pass` verdict is the correct answer for a lockfile-first probe regardless
/// of the runner's uid; under root the 0o555 socket parent stays writable, so this
/// leg no longer DISCRIMINATES against a hypothetical socket-first regression there —
/// but it never false-greens, matching this file's root-tolerance philosophy.)
#[test]
fn divergent_parents_probe_lockfile_not_socket() {
    // Socket parent: stripped of its write bit — a socket-first probe would fail here.
    let sock_parent = tempfile::tempdir().expect("socket-parent tempdir");
    let mut perms = std::fs::metadata(sock_parent.path())
        .expect("stat socket parent")
        .permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(sock_parent.path(), perms).expect("chmod socket parent read-only");

    // Lockfile parent: a distinct, writable dir — the path a lockfile-first probe uses.
    let lock_parent = tempfile::tempdir().expect("lockfile-parent tempdir");

    let sock = sock_parent.path().join("rezidnt.sock");
    let lock = lock_parent.path().join("mcp.lock");
    let (code, stdout, stderr) = run_doctor_json(&[
        ("REZIDNT_SOCKET", sock.to_str().unwrap()),
        ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
    ]);
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    let check = writable_check(&checks);
    let s = status_of(&check);

    // Restore perms BEFORE asserting so a panic never leaks an undeletable tempdir.
    if let Ok(md) = std::fs::metadata(sock_parent.path()) {
        let mut p = md.permissions();
        p.set_mode(0o755);
        let _ = std::fs::set_permissions(sock_parent.path(), p);
    }

    assert_eq!(
        s, "pass",
        "with REZIDNT_LOCKFILE under a WRITABLE parent and REZIDNT_SOCKET under a NON-writable \
         one (0o555), the check must PROBE THE LOCKFILE parent (REZIDNT_LOCKFILE-first \
         precedence) and report `pass` — a socket-first probe would have failed on the \
         unwritable socket parent. check: {check:#}"
    );
}
