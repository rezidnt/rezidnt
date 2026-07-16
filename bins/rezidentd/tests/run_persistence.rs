//! S1 oracle: persistence — "kill the client mid-run and the run survives
//! (daemon owns the process); `attach` replays the tail" (S1 exit criteria,
//! DR-001 dtach model).
#![cfg(unix)]

mod common;

use common::{connect, make_project, open_request, read_until, send_line, start_daemon};
use std::io::Read;
use std::time::Duration;

/// The opening client disconnects immediately; the run must still complete —
/// observed by a later, unrelated tail subscriber.
#[test]
fn client_disconnect_does_not_kill_the_run() {
    let daemon = start_daemon();
    let (_project, spec) = make_project(800);

    {
        let mut opener = connect(&daemon.socket);
        send_line(&mut opener, &open_request(&spec));
        // Wait only for proof the run started, then die like a real client.
        read_until(&mut opener, Duration::from_secs(20), |v| {
            v["subject"] == "agent.spawned"
        });
    } // opener dropped: socket closed mid-run

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.completed"
    });
    // Reaching here IS the assertion: the run outlived its client.
}

/// `attach` replays the capture ring from the start of the stream before
/// proxying live bytes — a late subscriber still sees the beginning.
#[test]
fn attach_replays_the_capture_tail() {
    let daemon = start_daemon();
    let (_project, spec) = make_project(800);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));
    let lines = read_until(&mut opener, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
    });
    let run = lines
        .last()
        .and_then(|v| v["payload"]["run"].as_str())
        .expect("agent.spawned payload must carry the run id")
        .to_string();

    // Attach AFTER the first stream bytes were produced.
    let mut attach = connect(&daemon.socket);
    send_line(&mut attach, &format!(r#"{{"op":"attach","run":"{run}"}}"#));
    let mut replay = Vec::new();
    let stream = attach.get_mut();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let mut chunk = [0u8; 4096];
    while replay.len() < 64 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => replay.extend_from_slice(&chunk[..n]),
            Err(e) => panic!("attach read failed before any replay bytes: {e}"),
        }
    }
    let text = String::from_utf8_lossy(&replay);
    assert!(
        text.contains(r#""type":"system""#),
        "attach must replay from the ring's start (the init line), got: {text:?}"
    );
}
