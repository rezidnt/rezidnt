//! S0 oracle — the exit criterion at the real surface: two concurrent socket
//! subscribers observe the stream from a live `rezidentd`.
//!
//! Pinned daemon contract (see `src/main.rs`): env `REZIDNT_SOCKET` /
//! `REZIDNT_DB` overrides; per connection the daemon sends the versioned
//! hello line, then replays the log from seq 0 as event JSONL, then streams
//! live. Unix-only (UDS); runs in WSL2, compiles to empty on Windows.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn wait_for_socket(sock: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if sock.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut err = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            panic!(
                "rezidentd exited (status {status:?}) before binding its socket — \
                 daemon unimplemented? stderr:\n{err}"
            );
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("rezidentd did not bind {} within 5s", sock.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn connect(sock: &Path) -> BufReader<UnixStream> {
    let stream = UnixStream::connect(sock).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    BufReader::new(stream)
}

fn read_two_lines(reader: &mut BufReader<UnixStream>) -> (serde_json::Value, serde_json::Value) {
    let mut hello = String::new();
    reader.read_line(&mut hello).expect("hello line");
    let mut event = String::new();
    reader.read_line(&mut event).expect("first event line");
    (
        serde_json::from_str(&hello).expect("hello frame must be JSON"),
        serde_json::from_str(&event).expect("event frame must be JSON"),
    )
}

#[test]
fn two_concurrent_socket_subscribers_observe_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_rezidentd"))
        .env("REZIDNT_SOCKET", &sock)
        .env("REZIDNT_DB", &db)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rezidentd");
    wait_for_socket(&sock, &mut daemon);

    // Two concurrent subscribers: both connections open before either reads.
    let mut sub_1 = connect(&sock);
    let mut sub_2 = connect(&sock);
    let (hello_1, event_1) = read_two_lines(&mut sub_1);
    let (hello_2, event_2) = read_two_lines(&mut sub_2);

    for hello in [&hello_1, &hello_2] {
        assert_eq!(hello["proto"], 1, "hello.proto must be 1 (doc §9)");
        assert!(
            hello["schema"].as_str().is_some_and(|s| !s.is_empty()),
            "hello.schema must carry the ontology hash, got {hello}"
        );
        assert!(
            hello["daemon"].as_str().is_some_and(|s| s.contains('.')),
            "hello.daemon must be a semver, got {hello}"
        );
    }
    for event in [&event_1, &event_2] {
        assert_eq!(
            event["subject"], "daemon.started",
            "first replayed event must be the daemon's own startup fact"
        );
    }
    assert_eq!(
        event_1["id"], event_2["id"],
        "concurrent subscribers must observe the same stream"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
}
