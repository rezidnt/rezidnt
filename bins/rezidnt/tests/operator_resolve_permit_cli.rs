//! DR-033 slice-2 (operator-resolve-escalation) ORACLE — CRITERION 6: the
//! `rezidnt operator resolve-permit <run> <request_id> <allow|deny> [--reason ...]`
//! subcommand and its DR-004 exit-code classes. Drives the REAL binary
//! (`CARGO_BIN_EXE_rezidnt`) and asserts the stable exit code per class — the
//! deterministic judge that needs no live daemon. Mirrors the DR-032 slice-1
//! `operator_kill_run_cli.rs` pattern (incl. the REZIDNT_LOCKFILE override and
//! the stderr guard against clap-usage false-greens).
//!
//! ## DR-033 §Consequences — the subcommand
//! `rezidnt operator resolve-permit <run> <request_id> <allow|deny>` reads the
//! 0600 lockfile (port + operator badge) and POSTs a `resolve_permit`
//! `tools/call` over the loopback HTTP MCP surface — NOT the bare socket (DR-032
//! §Decision 2: the socket's UDS-identity would bypass the explicit operator
//! authorization DR-031 requires). The `<allow|deny>` maps to the tool's
//! `decision` arg; `--reason` is optional (rides the emitted fact when given).
//!
//! ## DR-004 exit-code classes (mirror bins/rezidnt/src/main.rs + kill-run)
//!   - 0 ok (the daemon accepted the resolution).
//!   - 2 LOCAL input/usage error — a MALFORMED/ABSENT run ULID, or a malformed
//!     `<decision>` (not `allow`/`deny`); clap usage errors are also 2.
//!   - 4 daemon-unreachable — no lockfile / a lockfile pointing at a dead port.
//!   - 5 tool-refused — the daemon refused the resolve.
//!
//! The IMPLEMENTER MUST MIRROR the existing per-verb failure-class table in
//! `main()` (the `(failure_code, result)` tuple) and the `OperatorCmd` enum,
//! NOT invent a new mapping.
//!
//! ## REVISION (2026-07-22, /debrief FAIL close) — the client fabricates NO target
//! DR-033 §Design ratified `resolve_permit { run, request_id, decision, reason? }`
//! — NO `action`/`target` operator inputs; the DAEMON derives them from the log.
//! The shipped CLI hardcoded `"action": "tool.invoke", "target": {}` into the
//! request body (`bins/rezidnt/src/main.rs:1110-1111`), so the emitted
//! `permit.resolved` carried an EMPTY `target.tool` and the PDP action-identity
//! match could NEVER fire. This board now PINS that the CLI request body carries
//! NO fabricated `action`/`target` — `request_body_carries_no_fabricated_target`
//! captures the actual POST over a loopback stub (deterministic, no live daemon
//! logic) and asserts the client sends only `{ badge, run, request_id, decision,
//! reason? }`. The subcommand shape already has NO --tool/--action/--target flag
//! (confirmed: the operator never types an action/target), so nothing to remove
//! there — the defect is the client BODY, which this pins.
//!
//! RED MODE: **no-such-subcommand-red** — `rezidnt operator resolve-permit` does
//! not exist yet (`OperatorCmd` has only `KillRun`), so clap exits with an
//! "unrecognized subcommand" USAGE error (also exit 2). To keep the
//! malformed-input tests honest-red, they ALSO assert the stderr is NOT clap's
//! unrecognized-subcommand/unexpected-argument message — i.e. the subcommand
//! exists and rejected the input itself. The daemon-unreachable and
//! well-formed-not-input tests are unambiguously red (a missing subcommand never
//! reaches the daemon dial and never validates a decision locally). The
//! body-capture test is ASSERT-RED against the CURRENT impl, which DOES POST a
//! body but fabricates `action`/`target` into it.
//!
//! Cross-platform note: the exit-code tests pin EXIT CODES only (no dial is
//! completed), so they are NOT `#![cfg(unix)]`-gated. When the implementer wires
//! the loopback-HTTP POST, the happy-path exit 0 requires a live `serve_http`
//! lockfile (the integration seam); its body must be linted on WSL per the
//! project's host-vs-WSL rule (memory: /vet is host-side).

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;

/// Run `rezidnt operator resolve-permit <run> <request_id> <decision>` with the
/// given env overrides. Returns (exit_code, stdout, stderr). `REZIDNT_LOCKFILE`
/// points at a private nonexistent path so no red test ever dials a real dev
/// daemon (test isolation).
fn run_resolve(
    run: &str,
    request_id: &str,
    decision: &str,
    env: &[(&str, &str)],
) -> (Option<i32>, String, String) {
    run_resolve_ext(run, request_id, decision, &[], env)
}

/// As [`run_resolve`], but appends `extra` flags after the positional args (e.g.
/// `--scope`, `--ttl-ms`). Keeps the base helper a thin call so the existing
/// exit-class tests are untouched.
fn run_resolve_ext(
    run: &str,
    request_id: &str,
    decision: &str,
    extra: &[&str],
    env: &[(&str, &str)],
) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("operator")
        .arg("resolve-permit")
        .arg(run)
        .arg(request_id)
        .arg(decision)
        .args(extra);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt operator resolve-permit");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// One-shot loopback stub: bind :0, accept ONE connection, capture the POSTed
/// JSON-RPC body, reply with a canned tool success so the CLI exits 0. Returns
/// (port, body-receiver, join-handle). Factored from
/// `request_body_carries_no_fabricated_target` so the DR-035 `--scope`/`--ttl-ms`
/// body-shape tests reuse the exact same capture path.
fn oneshot_capture_stub() -> (u16, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback stub");
    let port = listener.local_addr().expect("stub addr").port();
    let (tx, rx) = mpsc::channel::<String>();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        raw.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&raw);
                        if let Some((head, body)) = text.split_once("\r\n\r\n") {
                            let len = head
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("Content-Length:")
                                        .or_else(|| l.strip_prefix("content-length:"))
                                })
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            if body.len() >= len {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&raw).into_owned();
            let body = text
                .split_once("\r\n\r\n")
                .map(|(_, b)| b.to_string())
                .unwrap_or_default();
            let _ = tx.send(body);
            let resp_body = r#"{"jsonrpc":"2.0","id":1,"result":{"isError":false,"content":[{"type":"text","text":"{}"}]}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, rx, handle)
}

/// Write a stub lockfile at `path` pointing the CLI at the loopback `port` (the
/// shape `rezidnt_mcp::lockfile::read` parses: pid, port, url, badge).
fn write_stub_lockfile(path: &std::path::Path, port: u16) {
    let lock_json = format!(
        r#"{{"pid":1,"port":{port},"url":"http://127.0.0.1:{port}/mcp","badge":"deadbeefcafef00d"}}"#
    );
    std::fs::write(path, lock_json).expect("write stub lockfile");
}

/// A syntactically-invalid ULID — rejected as a LOCAL input error BEFORE any
/// daemon traffic.
const MALFORMED_RUN: &str = "not-a-ulid";
/// A well-formed run ULID (26 Crockford base32 chars) — passes local validation.
const WELLFORMED_RUN: &str = "01DR033RES0VECMD0000000CR1";
/// A well-formed request_id ULID (the escalation this resolution answers).
const WELLFORMED_REQ: &str = "01DR033RES0LVECLIREQ0000R1";
/// A lockfile path that does not exist, forcing daemon-unreachable without a dial.
const DEAD_LOCKFILE: &str = "/nonexistent/rezidnt-dr033-dead.lock";

/// CRITERION 6 (input class — malformed run) — a MALFORMED run ULID maps to the
/// LOCAL input-error class (exit 2), the same class `attach`/`kill-run` give a
/// bad run id. It must NOT reach the daemon.
///
/// HONEST-RED GUARD: clap's usage errors are ALSO exit 2, so the bare code check
/// would pass for the wrong reason while the subcommand is absent. We therefore
/// ALSO require the stderr NOT be clap's unrecognized-subcommand/unexpected-arg
/// message. RED until the subcommand lands and validates the run locally.
#[test]
fn malformed_run_is_input_error() {
    let (code, _out, stderr) = run_resolve(
        MALFORMED_RUN,
        WELLFORMED_REQ,
        "allow",
        &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)],
    );
    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand") && !lc.contains("unexpected argument"),
        "RED-HONESTY: the subcommand must EXIST and reject the run id itself, not clap \
         rejecting an unknown subcommand — stderr was: {stderr}"
    );
    assert_eq!(
        code,
        Some(2),
        "a malformed run ULID is a LOCAL input error (exit 2, DR-004) — stderr: {stderr}"
    );
}

/// CRITERION 6 (input class — malformed decision) — a decision that is NOT
/// `allow`/`deny` is a LOCAL input/usage error (exit 2), rejected before any
/// daemon traffic. This pins the closed two-value decision enum on the CLI edge
/// (matching `ResolvePermitArgs.decision: "allow"|"deny"`).
///
/// HONEST-RED GUARD: same as above — a bare exit-2 could be clap rejecting the
/// unknown subcommand, so require the stderr is NOT that message.
#[test]
fn malformed_decision_is_input_error() {
    let (code, _out, stderr) = run_resolve(
        WELLFORMED_RUN,
        WELLFORMED_REQ,
        "maybe",
        &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)],
    );
    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand"),
        "RED-HONESTY: the subcommand must EXIST and reject the DECISION value itself, \
         not clap rejecting an unknown subcommand — stderr was: {stderr}"
    );
    assert_eq!(
        code,
        Some(2),
        "a decision that is not allow|deny is a LOCAL input error (exit 2, DR-004; the \
         closed two-value enum) — stderr: {stderr}"
    );
}

/// CRITERION 6 (daemon-unreachable class) — a WELL-FORMED run + request_id +
/// valid decision with NO reachable daemon (a lockfile that does not exist) maps
/// to the daemon-unreachable class (exit 4, DR-004). The inputs are valid, so it
/// is NEVER an input error — the failure is that the daemon cannot be reached.
///
/// RED until the subcommand exists, validates locally, and dials the daemon.
#[test]
fn wellformed_no_daemon_is_unreachable() {
    let (code, _out, stderr) = run_resolve(
        WELLFORMED_RUN,
        WELLFORMED_REQ,
        "allow",
        &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)],
    );
    assert_eq!(
        code,
        Some(4),
        "a well-formed resolve with no reachable daemon is daemon-unreachable (exit 4, \
         DR-004) — NOT an input error and NOT a silent success; stderr: {stderr}"
    );
}

/// CRITERION 6 (happy-path boundary — the deterministic slice) — well-formed
/// inputs (`allow` and `deny` both) must PASS local validation, i.e. are NEVER
/// misclassified as input errors (exit 2). Combined with
/// `wellformed_no_daemon_is_unreachable` (exit 4 with no daemon), this fences the
/// happy path from BOTH the input and unreachable classes. The full exit-0
/// happy path needs a live daemon (the implementer's integration seam).
///
/// RED until the subcommand exists and accepts well-formed inputs locally.
#[test]
fn wellformed_decisions_are_not_input_errors() {
    for decision in ["allow", "deny"] {
        let (code, _out, stderr) = run_resolve(
            WELLFORMED_RUN,
            WELLFORMED_REQ,
            decision,
            &[("REZIDNT_LOCKFILE", DEAD_LOCKFILE)],
        );
        assert_ne!(
            code,
            Some(2),
            "a well-formed resolve ({decision}) must PASS local validation — it is NOT a \
             local input error (exit 2 is reserved for a malformed run/decision, DR-004); \
             stderr: {stderr}"
        );
    }
}

/// CRITERION 5 (client fabricates NO target — the /debrief FAIL close) — the CLI
/// request body carries ONLY `{ badge, run, request_id, decision, reason? }` and
/// NO fabricated `action`/`target`. This is the DETERMINISTIC pin (no live
/// daemon logic): a one-shot loopback TCP stub captures the actual POSTed body
/// and returns a canned JSON-RPC success. Asserts the client does NOT hardcode
/// `"action": "tool.invoke"` / `"target": {}` — the exact bytes the shipped
/// `operator_resolve_permit` fabricated (`bins/rezidnt/src/main.rs:1110-1111`),
/// which made the emitted `permit.resolved` carry an empty target and broke the
/// PDP match. The daemon DERIVES action/target from the log; the client must not
/// pre-supply them.
///
/// A `permit.resolved` carrying a real target is the daemon-derive job proven in
/// `crates/rezidnt-mcp/tests/resolve_permit_door.rs`; here we pin the client end:
/// it sends no action/target at all.
///
/// ASSERT-RED against the CURRENT impl: it POSTs a body that DOES contain
/// `action` + `target` keys, so the "no action/target key" assertion fails. (And
/// no-such-subcommand-red while `OperatorCmd::ResolvePermit` is absent — clap
/// exits before any POST, so the stub sees no connection and the test fails on
/// the missing captured body, which is the correct red.)
///
/// Cross-platform: uses only `std::net` loopback + a lockfile the CLI reads via
/// `REZIDNT_LOCKFILE`; no UDS. NOT `#![cfg(unix)]`-gated.
#[test]
fn request_body_carries_no_fabricated_target() {
    // One-shot loopback stub + stub lockfile (shared with the DR-035 body-shape
    // tests): capture the POSTed body, reply with a canned JSON-RPC success.
    let (port, rx, handle) = oneshot_capture_stub();
    let dir = tempfile::tempdir().expect("tempdir");
    let lock_path = dir.path().join("mcp.lock");
    write_stub_lockfile(&lock_path, port);

    let (code, _out, stderr) = run_resolve(
        WELLFORMED_RUN,
        WELLFORMED_REQ,
        "allow",
        &[("REZIDNT_LOCKFILE", lock_path.to_str().unwrap())],
    );

    let body = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| {
            panic!(
                "the CLI must POST a resolve_permit body to the loopback daemon — captured \
                 none (subcommand absent or never dialed?); exit {code:?}, stderr: {stderr}"
            )
        });
    let _ = handle.join();

    let req: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("POST body must be JSON-RPC ({e}): {body}"));
    let args = &req["params"]["arguments"];
    assert!(
        args.is_object(),
        "the resolve_permit tools/call must carry an arguments object — got {req:#}"
    );
    // The DERIVE contract: the client sends NO action and NO target. The daemon
    // derives them from the log by request_id. A fabricated (empty) target here
    // is the exact /debrief defect.
    assert!(
        args.get("action").is_none(),
        "the CLI must NOT fabricate an `action` — the daemon DERIVES it from the log by \
         request_id (DR-033 §Design). A hardcoded action was the /debrief FAIL. body: {body}"
    );
    assert!(
        args.get("target").is_none(),
        "the CLI must NOT fabricate a `target` — the shipped client hardcoded `target: {{}}` \
         which broke the PDP action-identity match. The daemon DERIVES the target from the \
         log (DR-033 §Design). body: {body}"
    );
    // Positive: the trimmed shape the DR ratified rides the body verbatim.
    assert_eq!(
        args["request_id"],
        serde_json::json!(WELLFORMED_REQ),
        "the request_id (the audit correlation the daemon derives from) rides the body"
    );
    assert_eq!(
        args["decision"],
        serde_json::json!("allow"),
        "the human decision rides the body verbatim (the input verb, not coerced)"
    );
}

/// DR-035 §Decision 2 (CLI parity — `--scope` rides the body VERBATIM) — the MCP
/// `resolve_permit` tool accepts an optional broad `scope`, but the shipped
/// subcommand only wired `--ttl-ms`, so a broad grant was reachable ONLY via raw
/// MCP. This pins that `--scope run_tool` (paired with `--ttl-ms`, per the
/// coupling guard) rides the POSTed body as `scope` verbatim — the client passes
/// it through, NOT interpreting it. The daemon owns the semantics: fail-closed on
/// unknown values and the `SCOPE_REQUIRES_TTL` coupling
/// (`crates/rezidnt-mcp/src/lib.rs`), so the CLI stays a thin conduit (no local
/// scope validation, preserving I5 forward-compat for a future second axis).
#[test]
fn scope_flag_rides_body_verbatim() {
    let (port, rx, handle) = oneshot_capture_stub();
    let dir = tempfile::tempdir().expect("tempdir");
    let lock_path = dir.path().join("mcp.lock");
    write_stub_lockfile(&lock_path, port);

    // A broad grant MUST be time-boxed (the coupling guard), so pair --scope with
    // --ttl-ms — the daemon would refuse broad-and-permanent (SCOPE_REQUIRES_TTL).
    let (code, _out, stderr) = run_resolve_ext(
        WELLFORMED_RUN,
        WELLFORMED_REQ,
        "allow",
        &["--scope", "run_tool", "--ttl-ms", "60000"],
        &[("REZIDNT_LOCKFILE", lock_path.to_str().unwrap())],
    );

    let body = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| {
            panic!(
                "the CLI must POST a resolve_permit body carrying --scope — captured none \
                 (flag absent or never dialed?); exit {code:?}, stderr: {stderr}"
            )
        });
    let _ = handle.join();

    let req: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("POST body must be JSON-RPC ({e}): {body}"));
    let args = &req["params"]["arguments"];
    assert_eq!(
        args["scope"],
        serde_json::json!("run_tool"),
        "`--scope run_tool` must ride the resolve_permit body as `scope` VERBATIM (DR-035 \
         §Decision 2) — the client passes it through, not interpreting it. body: {body}"
    );
    // The paired TTL still rides too (the coupling the daemon enforces).
    assert_eq!(
        args["ttl_ms"],
        serde_json::json!(60000),
        "`--ttl-ms` rides alongside `--scope` (the broad grant is time-boxed). body: {body}"
    );
}

/// DR-035 §Decision 2 (negative control — absent `--scope` OMITS the key) — with
/// no `--scope` flag the POSTed body carries NO `scope` key (OMITTED, never null),
/// so the daemon keeps the DR-033 exact request-scoped match. Mirrors the
/// `permit.resolved` emit rule (`scope` present ONLY on a broad grant) so a narrow
/// grant is never misreported as broad.
#[test]
fn absent_scope_flag_omits_key() {
    let (port, rx, handle) = oneshot_capture_stub();
    let dir = tempfile::tempdir().expect("tempdir");
    let lock_path = dir.path().join("mcp.lock");
    write_stub_lockfile(&lock_path, port);

    let (code, _out, stderr) = run_resolve_ext(
        WELLFORMED_RUN,
        WELLFORMED_REQ,
        "allow",
        &[],
        &[("REZIDNT_LOCKFILE", lock_path.to_str().unwrap())],
    );

    let body = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| {
            panic!("the CLI must POST a body; captured none. exit {code:?}, stderr: {stderr}")
        });
    let _ = handle.join();

    let req: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("POST body must be JSON-RPC ({e}): {body}"));
    let args = &req["params"]["arguments"];
    assert!(
        args.get("scope").is_none(),
        "with no --scope flag the body must OMIT `scope` (never null) — the DR-033 exact \
         request-scoped match is unchanged (the negative control). body: {body}"
    );
}
