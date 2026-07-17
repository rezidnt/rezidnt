//! S3 oracle — the two parked-from-S2 socket-protocol items, now due:
//! request-scoped `open` acks and a machine-readable `attach` unknown-run
//! error frame (never a hang, never a bare disconnect).
//!
//! Wire shapes are `rezidnt_proto::Reply` (S3 type-layer pin); these tests
//! parse raw JSON so the daemon's actual bytes are what is judged.
#![cfg(unix)]

mod common;

use std::io::BufRead;
use std::time::Duration;

use common::{
    connect, make_project, open_request, read_reply_line, read_until, send_line, start_daemon,
};
use serde_json::json;

const REPLY_DEADLINE: Duration = Duration::from_secs(5);

/// Triage item 2: attaching to a nonexistent run yields ONE machine-readable
/// error frame — `{"reply":"error","op":"attach","code":"run.unknown"}` with
/// the run echoed — then an orderly close. Not a hang, not a silent EOF.
#[test]
fn attach_unknown_run_answers_an_error_frame_then_closes() {
    let daemon = start_daemon();
    let ghost = "01ARZ3NDEKTSV4RRFFQ69G5ZZZ";

    let mut conn = connect(&daemon.socket);
    send_line(
        &mut conn,
        &json!({"op": "attach", "run": ghost}).to_string(),
    );

    let frame = read_reply_line(&mut conn, REPLY_DEADLINE);
    assert_eq!(
        frame["reply"],
        json!("error"),
        "an error FRAME, got {frame:#}"
    );
    assert_eq!(frame["op"], json!("attach"));
    assert_eq!(
        frame["code"],
        json!("run.unknown"),
        "machine-readable code (rezidnt_proto::codes::RUN_UNKNOWN)"
    );
    assert_eq!(
        frame["run"],
        json!(ghost),
        "the run is echoed for the client"
    );

    // Orderly close after the error frame: the next read hits EOF within the
    // deadline instead of hanging.
    let mut rest = String::new();
    let closed = loop {
        rest.clear();
        match conn.read_line(&mut rest) {
            Ok(0) => break true,
            Ok(_) if rest.trim().is_empty() => continue,
            Ok(_) => panic!("no frames after the error frame, got: {rest}"),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break false;
            }
            Err(e) => panic!("read after error frame: {e}"),
        }
    };
    assert!(
        closed,
        "the daemon must close the connection after the error frame, not hang it"
    );
}

/// Triage item 1: an `open` request is ACKED on its own connection —
/// `{"reply":"open_ok","workspace":…,"correlation":…}` as the first frame —
/// and the acked correlation is the one every materialization fact carries.
#[test]
fn open_request_is_acked_with_workspace_and_correlation() {
    let daemon = start_daemon();
    let (_project, spec) = make_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));
    let ack = read_reply_line(&mut opener, Duration::from_secs(10));
    assert_eq!(
        ack["reply"],
        json!("open_ok"),
        "request-scoped ack: {ack:#}"
    );
    let workspace = ack["workspace"]
        .as_str()
        .expect("ack names the workspace ulid");
    let correlation = ack["correlation"]
        .as_str()
        .expect("ack names the correlation");

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "workspace.opened"
    });
    let opened = lines
        .iter()
        .find(|v| v["subject"] == "workspace.opened")
        .expect("workspace.opened on the fabric");
    assert_eq!(
        opened["workspace"],
        json!(workspace),
        "the ack and the log name the same workspace"
    );
    assert_eq!(
        opened["correlation"],
        json!(correlation),
        "the ack's correlation IS the materialization chain's correlation — the ack is tied to the log"
    );
}

/// An `open` whose spec does not parse gets `{"reply":"error","op":"open",
/// "code":"spec.invalid"}` — a frame, not a dropped connection.
#[test]
fn open_invalid_spec_answers_a_spec_invalid_error_frame() {
    let daemon = start_daemon();

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request("this is not toml ["));
    let frame = read_reply_line(&mut opener, REPLY_DEADLINE);
    assert_eq!(frame["reply"], json!("error"), "got {frame:#}");
    assert_eq!(frame["op"], json!("open"));
    assert_eq!(
        frame["code"],
        json!("spec.invalid"),
        "machine-readable code (rezidnt_proto::codes::SPEC_INVALID)"
    );
}
