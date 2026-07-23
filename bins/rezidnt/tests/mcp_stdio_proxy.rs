//! Slice `mcp-stdio` ORACLE — the `rezidnt mcp` stdio↔loopback-HTTP JSON-RPC PROXY
//! (§16 S3 connection path; §9 "stdio (spawned by local clients like Claude Code)").
//! A Claude-spawned MCP client speaks line-delimited JSON-RPC over the proxy's
//! stdio; the proxy forwards each request to the resident daemon's loopback-HTTP MCP
//! (read from the 0600 lockfile) and relays the response back. Because I3 makes the
//! daemon the single writer, the stdio entrypoint CANNOT own a fabric — it is a thin
//! proxy to the daemon, not a second core.
//!
//! ## What this pins (the proxy's contract — no real daemon needed)
//! A FAKE loopback `/mcp` HTTP server stands in for the daemon and RECORDS every
//! request the proxy forwards, so the two load-bearing behaviors are machine-checked:
//!   1. **Badge injection for mutating tools.** A local client (Claude Code) does not
//!      know the operator badge (it lives in the 0600 lockfile). The proxy must inject
//!      `lock.badge` into the `arguments.badge` of a `tools/call` for a MUTATING tool
//!      (`open_project`/`spawn_agent`/`kill_run`/`resolve_permit`/`request_permission`;
//!      §12 — mutating calls need a badge), so the client never handles the token.
//!   2. **Read-class pass-through.** A `tools/call` for a read-class tool
//!      (`gate_explain`/`tail_events`, DR-005 unbadged) and non-tools/call methods
//!      (`initialize`) are forwarded UNCHANGED — no badge injected. The proxy's safety
//!      is that it scopes injection to the mutating set; the daemon would silently
//!      IGNORE a stray `badge` (it reads args off a JSON value, no `deny_unknown_fields`),
//!      so the proxy must simply never add one to a read-class call.
//!
//! And the framing: each stdin JSON-RPC line yields exactly one stdout JSON-RPC line,
//! ids preserved, in order.
//!
//! ## Startup fail-closed
//! At startup the proxy reads the lockfile; if it is absent/unreadable the daemon
//! cannot be located, so the proxy exits 4 (daemon-unreachable — the DR-004 class the
//! operator verbs use), rather than serving a surface that forwards nowhere.
//!
//! Cross-platform on purpose (loopback TCP + stdio, no UDS, no `#![cfg(unix)]`): the
//! proxy dials loopback and pipes stdio, so host `/vet` covers it. The live end-to-end
//! against a REAL daemon is the separate `s3-exit-demo` slice (`#[cfg(unix)]`/WSL).
//!
//! Authoring intent, past-tense-safe: written RED before the `mcp` verb existed — the
//! proxy is spawned as `rezidnt mcp`; with the verb absent clap exits "unrecognized
//! subcommand" (nonzero, no forwarding), and every assertion states the CONTRACT it
//! pins (stays true once the verb exists).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use rezidnt_mcp::lockfile::{self, Lockfile};

/// A fake loopback `/mcp` server: binds 127.0.0.1:0, accepts `expect` connections
/// (the proxy uses `Connection: close`, one request per connection), records each
/// forwarded JSON-RPC request body, and answers a minimal JSON-RPC result echoing the
/// id. Returns (bound_port, receiver of recorded request bodies).
fn fake_mcp_server(expect: usize) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake /mcp");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for conn in listener.incoming().take(expect) {
            let mut stream = match conn {
                Ok(s) => s,
                Err(_) => break,
            };
            // Read the HTTP head, then the body per Content-Length.
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut content_len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let l = line.trim_end();
                if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_len = v.trim().parse().unwrap_or(0);
                }
                if l.is_empty() {
                    break; // end of headers
                }
            }
            let mut body = vec![0u8; content_len];
            reader.read_exact(&mut body).expect("read body");
            let body = String::from_utf8(body).expect("utf8 body");
            // Model the daemon faithfully: a REQUEST (has `id`) draws a JSON-RPC
            // result; a NOTIFICATION (no `id`) draws an empty 202 (the real daemon
            // frames Content-Length: 0), so the proxy's no-response-line suppression
            // path is exercised, not merely reasoned about.
            let parsed = serde_json::from_str::<serde_json::Value>(&body).ok();
            let has_id = parsed.as_ref().is_some_and(|v| v.get("id").is_some());
            let http = if has_id {
                let id = parsed.and_then(|v| v.get("id").cloned()).unwrap();
                let resp =
                    serde_json::json!({"jsonrpc":"2.0","id":id,"result":{"ok":true}}).to_string();
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp.len(),
                    resp
                )
            } else {
                "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            };
            stream.write_all(http.as_bytes()).expect("write resp");
            stream.flush().ok();
            tx.send(body).expect("record request");
        }
    });
    (port, rx)
}

/// A fake `/mcp` server that serves `serve_ok` requests normally, then simulates a
/// daemon that DIES mid-session: on the next connection it reads the request (so the
/// proxy's write completes) and closes without a valid response, so the proxy's
/// `loopback_post` fails and it must answer that request with a JSON-RPC error
/// (fail-closed) and keep serving. Returns the bound port.
fn fake_mcp_server_dropping(serve_ok: usize) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake /mcp");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        let mut served = 0usize;
        for conn in listener.incoming() {
            let mut stream = match conn {
                Ok(s) => s,
                Err(_) => break,
            };
            // Drain the request head + body so the client's write completes cleanly.
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut content_len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let l = line.trim_end();
                if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_len = v.trim().parse().unwrap_or(0);
                }
                if l.is_empty() {
                    break;
                }
            }
            let mut body = vec![0u8; content_len];
            let _ = reader.read_exact(&mut body);
            if served < serve_ok {
                let id = serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(&body))
                    .ok()
                    .and_then(|v| v.get("id").cloned())
                    .unwrap_or(serde_json::Value::Null);
                let resp =
                    serde_json::json!({"jsonrpc":"2.0","id":id,"result":{"ok":true}}).to_string();
                let http = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp.len(),
                    resp
                );
                let _ = stream.write_all(http.as_bytes());
                let _ = stream.flush();
                served += 1;
            } else {
                // The daemon "dies": drop the connection with no response.
                drop(stream);
                break;
            }
        }
    });
    port
}

/// Write a lockfile (via the real `write_atomic`) pointing the proxy at `port` with a
/// known operator `badge`, and return its path inside `dir`.
fn write_lockfile(dir: &std::path::Path, port: u16, badge: &str) -> std::path::PathBuf {
    let path = dir.join("mcp.lock");
    let lf = Lockfile {
        pid: 0,
        port,
        url: format!("http://127.0.0.1:{port}/mcp"),
        badge: badge.to_string(),
    };
    lockfile::write_atomic(&path, &lf).expect("write lockfile");
    path
}

/// Run `rezidnt mcp` with `REZIDNT_LOCKFILE` set and the given stdin lines; return
/// (exit_code, stdout_lines).
fn run_proxy(lockfile_path: &std::path::Path, stdin_lines: &[&str]) -> (Option<i32>, Vec<String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .arg("mcp")
        .env("REZIDNT_LOCKFILE", lockfile_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rezidnt mcp");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for line in stdin_lines {
            // Tolerate a broken pipe: in the fail-closed (exit-4) case the proxy
            // exits at startup before consuming stdin, so this write may fail.
            let _ = writeln!(stdin, "{line}");
        }
        // drop stdin → EOF, proxy should finish and exit.
    }
    let out = child.wait_with_output().expect("wait proxy");
    let stdout = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    (out.status.code(), stdout)
}

const BADGE: &str = "operator-badge-token-deadbeef";

/// The proxy forwards each stdin JSON-RPC line to the daemon and relays the response,
/// injecting the operator badge ONLY into mutating tool calls and leaving read-class
/// calls and non-tool methods untouched. Ids/order preserved 1:1.
#[test]
fn proxy_forwards_and_injects_badge_only_for_mutating_tools() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (port, rx) = fake_mcp_server(3);
    let lock = write_lockfile(dir.path(), port, BADGE);

    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"open_project","arguments":{"spec":"name=x\n"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"gate_explain","arguments":{"run":"01JQ"}}}"#,
    ];
    let (code, stdout) = run_proxy(&lock, &requests);

    assert_eq!(code, Some(0), "proxy must exit 0 on clean stdin EOF");
    assert_eq!(
        stdout.len(),
        3,
        "proxy must relay exactly one JSON-RPC response line per request; got: {stdout:?}"
    );
    // Ids preserved and in order.
    for (i, line) in stdout.iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).expect("stdout line is JSON-RPC");
        assert_eq!(
            v["id"],
            serde_json::json!(i as i64 + 1),
            "response {i} must carry the matching id"
        );
    }

    // Collect the three forwarded request bodies the fake daemon recorded.
    let mut forwarded = Vec::new();
    for _ in 0..3 {
        forwarded.push(
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("fake daemon recorded a forwarded request"),
        );
    }
    let parsed: Vec<serde_json::Value> = forwarded
        .iter()
        .map(|b| serde_json::from_str(b).expect("forwarded body is JSON"))
        .collect();

    // 1) initialize forwarded unchanged (no badge injected into params).
    assert!(
        parsed[0]["params"].get("badge").is_none()
            && parsed[0]["params"]["arguments"].get("badge").is_none(),
        "initialize must be forwarded unchanged (no badge): {}",
        parsed[0]
    );
    // 2) open_project (MUTATING) got the operator badge injected into arguments.
    assert_eq!(
        parsed[1]["params"]["arguments"]["badge"],
        serde_json::json!(BADGE),
        "open_project (mutating) must have the operator badge injected from the lockfile: {}",
        parsed[1]
    );
    // 3) gate_explain (READ-CLASS) left untouched — no badge injected. (The daemon
    // would ignore a stray badge, so the guarantee is that the proxy never adds one.)
    assert!(
        parsed[2]["params"]["arguments"].get("badge").is_none(),
        "gate_explain (read-class) must be forwarded WITHOUT a badge — the proxy scopes \
         injection to the mutating set: {}",
        parsed[2]
    );
}

/// A caller-supplied badge is NOT overwritten: if the client already put a badge on a
/// mutating call (e.g. an agent presenting its own macaroon), the proxy forwards it
/// as-is rather than clobbering it with the operator token.
#[test]
fn proxy_does_not_overwrite_a_caller_supplied_badge() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (port, rx) = fake_mcp_server(1);
    let lock = write_lockfile(dir.path(), port, BADGE);

    let reqs = [
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"badge":"agent-macaroon-xyz","workspace":"w","idempotency_key":"k"}}}"#,
    ];
    let (code, _stdout) = run_proxy(&lock, &reqs);
    assert_eq!(code, Some(0));

    let body = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("recorded request");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["params"]["arguments"]["badge"],
        serde_json::json!("agent-macaroon-xyz"),
        "a caller-supplied badge must be preserved, not overwritten by the operator token"
    );
}

/// Fail-closed at startup: with no readable lockfile the daemon cannot be located, so
/// the proxy exits 4 (daemon-unreachable, the DR-004 class the operator verbs use)
/// rather than serving a surface that forwards nowhere.
#[test]
fn proxy_exits_4_when_daemon_lockfile_is_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("nope.lock");
    let (code, _stdout) = run_proxy(
        &missing,
        &[r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#],
    );
    assert_eq!(
        code,
        Some(4),
        "proxy must exit 4 (daemon-unreachable) when the lockfile is absent/unreadable"
    );
}

/// A JSON-RPC NOTIFICATION (no `id`) draws no response from the daemon, so the proxy
/// must write NOTHING to stdout for it — while still forwarding it. Proves the
/// suppression path against a faithful daemon (empty 202), not merely reasoning.
#[test]
fn proxy_suppresses_output_for_a_notification() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (port, rx) = fake_mcp_server(2);
    let lock = write_lockfile(dir.path(), port, BADGE);

    let reqs = [
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, // notification: no id
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"gate_explain","arguments":{"run":"01JQ"}}}"#,
    ];
    let (code, stdout) = run_proxy(&lock, &reqs);
    assert_eq!(code, Some(0));

    // Both were forwarded to the daemon...
    let f1 = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("fwd 1");
    let f2 = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("fwd 2");
    assert!(
        f1.contains("notifications/initialized") || f2.contains("notifications/initialized"),
        "the notification must still be forwarded to the daemon"
    );
    // ...but only the id-bearing request drew a stdout line.
    assert_eq!(
        stdout.len(),
        1,
        "a notification draws no daemon response, so the proxy writes no line for it; got: {stdout:?}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout[0]).unwrap();
    assert_eq!(
        v["id"],
        serde_json::json!(9),
        "the only stdout line is the id-bearing request's response"
    );
}

/// Mid-session daemon loss is FAIL-CLOSED: when the daemon dies, the proxy answers the
/// in-flight request with a JSON-RPC error (never a silent hang, I6) carrying that
/// request's id, and KEEPS SERVING — it reaches stdin EOF and exits 0.
#[test]
fn proxy_errors_and_keeps_serving_on_midsession_daemon_loss() {
    let dir = tempfile::tempdir().expect("tempdir");
    let port = fake_mcp_server_dropping(1); // serves req 1, drops req 2's connection
    let lock = write_lockfile(dir.path(), port, BADGE);

    let reqs = [
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gate_explain","arguments":{"run":"a"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"gate_explain","arguments":{"run":"b"}}}"#,
    ];
    let (code, stdout) = run_proxy(&lock, &reqs);

    assert_eq!(
        code,
        Some(0),
        "the proxy must reach stdin EOF and exit 0 even after a mid-session daemon loss"
    );
    assert_eq!(
        stdout.len(),
        2,
        "both requests draw a stdout line (a result, then an error): {stdout:?}"
    );
    let r1: serde_json::Value = serde_json::from_str(&stdout[0]).unwrap();
    assert_eq!(r1["id"], serde_json::json!(1));
    assert!(
        r1.get("result").is_some(),
        "req 1 got a normal result: {}",
        stdout[0]
    );
    let r2: serde_json::Value = serde_json::from_str(&stdout[1]).unwrap();
    assert_eq!(
        r2["id"],
        serde_json::json!(2),
        "the error must carry the failed request's id (not null)"
    );
    assert!(
        r2.get("error").is_some(),
        "req 2 (daemon dead) must get a JSON-RPC error, not a hang: {}",
        stdout[1]
    );
}
