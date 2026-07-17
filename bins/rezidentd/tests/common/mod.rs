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

// ---------------------------------------------------------------------------
// S3 additions: MCP loopback-HTTP transport (lockfile-announced) and log
// pre-seeding for the stubbed-verdict gate_explain board.
// ---------------------------------------------------------------------------

/// Start rezidentd with the MCP HTTP transport requested via
/// `REZIDNT_MCP_LOCKFILE` (S3 board pin: env-overridable lockfile path,
/// mirroring `REZIDNT_SOCKET`). Optionally pre-seed the event db from a
/// committed golden fixture BEFORE the daemon starts — the log is truth
/// (I3), so a stub gate verdict is seeded there and nowhere else.
///
/// Returns the guard plus the lockfile path (the file itself appears when
/// the transport is up — waiting for it is the caller's assertion).
pub fn start_daemon_with_mcp(seed_fixture: Option<&str>) -> (DaemonGuard, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let lockfile = dir.path().join("mcp.lock");

    if let Some(name) = seed_fixture {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures");
        let text = std::fs::read_to_string(fixtures.join(name)).expect("fixture exists");
        let mut log = rezidnt_fabric::EventLog::open(&db).expect("open seed db");
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let event = rezidnt_types::Event::from_json_line(line).expect("fixture line parses");
            log.append(&event).expect("seed append");
        }
    }

    let child = Command::new(env!("CARGO_BIN_EXE_rezidentd"))
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .env("REZIDNT_MCP_LOCKFILE", &lockfile)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    (
        DaemonGuard {
            child,
            socket,
            db,
            _dir: dir,
        },
        lockfile,
    )
}

/// Wait for the MCP lockfile to appear and parse (S3: port 0 is announced
/// HERE, never fixed). Panicking on the deadline IS the red assertion.
pub fn wait_for_lockfile(path: &Path, deadline: Duration) -> serde_json::Value {
    let until = Instant::now() + deadline;
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
        {
            return v;
        }
        assert!(
            Instant::now() < until,
            "MCP lockfile never appeared/parsed at {} — the HTTP transport was not announced",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Minimal HTTP/1.1 POST of one JSON-RPC message to the announced endpoint.
/// Tolerates plain JSON and SSE (`data:` line) response bodies.
pub fn mcp_post(url: &str, body: &str) -> serde_json::Value {
    use std::io::Read;
    let rest = url
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("loopback http url expected, got {url}"));
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = std::net::TcpStream::connect(host).expect("connect announced endpoint");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let request = format!(
        "POST /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("send request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let text = String::from_utf8_lossy(&raw);
    let (head, payload) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no header/body split in: {text}"));
    assert!(
        head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"),
        "endpoint must answer 200, got: {head}"
    );
    let json_text = if payload.contains("data:") {
        payload
            .lines()
            .find_map(|l| l.strip_prefix("data:"))
            .expect("an SSE body carries a data: line")
            .trim()
            .to_string()
    } else {
        let start = payload.find('{').expect("a JSON body");
        let end = payload.rfind('}').expect("a JSON body");
        payload[start..=end].to_string()
    };
    serde_json::from_str(&json_text)
        .unwrap_or_else(|e| panic!("response body must be JSON-RPC ({e}): {json_text}"))
}

/// JSON-RPC request builder + tools/call sugar over [`mcp_post`].
pub fn rpc(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// tools/call over HTTP; returns the MCP tool result object.
pub fn mcp_tool_call(url: &str, id: u64, tool: &str, args: serde_json::Value) -> serde_json::Value {
    let response = mcp_post(
        url,
        &rpc(
            id,
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        ),
    );
    assert!(
        response.get("error").is_none(),
        "tools/call must not be a protocol error: {response:#}"
    );
    response["result"].clone()
}

/// `content[0].text` parsed as JSON — the machine-readable tool payload.
pub fn tool_payload(result: &serde_json::Value) -> serde_json::Value {
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool result must carry content[0].text: {result:#}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("content[0].text must be JSON ({e}): {text}"))
}

/// Read one reply line from a socket connection with a deadline; panics on
/// timeout (the caller's message is the failure).
pub fn read_reply_line(
    reader: &mut BufReader<UnixStream>,
    deadline: Duration,
) -> serde_json::Value {
    let until = Instant::now() + deadline;
    let mut line = String::new();
    while Instant::now() < until {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => panic!("connection closed before a reply frame arrived"),
            Ok(_) if line.trim().is_empty() => continue,
            Ok(_) => {
                return serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("reply frame must be JSON ({e}): {line}"));
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => panic!("reply read failed: {e}"),
        }
    }
    panic!("deadline: no reply frame arrived on the connection");
}
