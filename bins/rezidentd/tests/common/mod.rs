//! Shared harness for daemon-level S1 integration tests (unix only).
#![cfg(unix)]
#![allow(dead_code)] // each integration test uses a subset

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Kills the daemon on drop so a failing test never leaks a process.
pub struct DaemonGuard {
    pub child: Child,
    pub socket: PathBuf,
    pub db: PathBuf,
    _dir: tempfile::TempDir,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start rezidentd with a temp socket + db; wait for the socket to appear.
pub fn start_daemon() -> DaemonGuard {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let child = Command::new(env!("CARGO_BIN_EXE_rezidentd"))
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    DaemonGuard {
        child,
        socket,
        db,
        _dir: dir,
    }
}

/// Connect, consume the hello line, leave the stream positioned at frame 2.
pub fn connect(socket: &Path) -> BufReader<UnixStream> {
    let stream = UnixStream::connect(socket).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");
    let mut reader = BufReader::new(stream);
    let mut hello = String::new();
    reader.read_line(&mut hello).expect("hello line");
    assert!(
        hello.contains("\"proto\""),
        "first frame must be the hello, got {hello:?}"
    );
    reader
}

pub fn send_line(reader: &mut BufReader<UnixStream>, line: &str) {
    let stream = reader.get_mut();
    stream.write_all(line.as_bytes()).expect("write request");
    stream.write_all(b"\n").expect("write newline");
}

/// Read envelope lines until `stop` returns true or the deadline passes;
/// returns every parsed line. Panics on deadline — the caller's assertion
/// message is the failure.
pub fn read_until(
    reader: &mut BufReader<UnixStream>,
    deadline: Duration,
    mut stop: impl FnMut(&serde_json::Value) -> bool,
) -> Vec<serde_json::Value> {
    let until = Instant::now() + deadline;
    let mut seen = Vec::new();
    let mut line = String::new();
    while Instant::now() < until {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // daemon closed the stream
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    let done = stop(&v);
                    seen.push(v);
                    if done {
                        return seen;
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => panic!("tail read failed: {e}"),
        }
    }
    panic!(
        "deadline: stop condition never met; saw {} lines: {seen:?}",
        seen.len()
    );
}

/// A temp project: git repo + stub harness script + §13 spec pointing at both.
/// The script emits fake stream-json with `gap_ms` between lines, so tests can
/// hold a run open long enough to kill clients around it.
pub fn make_project(gap_ms: u64) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());

    let gap_s = gap_ms as f64 / 1000.0;
    let script = dir.path().join("harness.sh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo '{{"type":"system","subtype":"init","session_id":"fixture-session","claude_code_version":"2.1.191","tools":[]}}'
sleep {gap_s}
echo '{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"working"}}]}}}}'
sleep {gap_s}
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"duration_ms":5,"total_cost_usd":0.001,"usage":{{"input_tokens":1,"output_tokens":1}},"session_id":"fixture-session"}}'
"#
        ),
    )
    .expect("write harness stub");
    let mut perms = std::fs::metadata(&script).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");

    let spec = format!(
        r#"[project]
name = "s1-exit"
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

/// JSON-escape a spec into an `open` request line.
pub fn open_request(spec_toml: &str) -> String {
    serde_json::to_string(&serde_json::json!({"op": "open", "spec_toml": spec_toml}))
        .expect("open request json")
}
