//! S3 oracle — the two §9 transports over the SAME core: stdio (line-delimited
//! JSON-RPC over a byte stream) and loopback HTTP on 127.0.0.1 with port 0
//! announced via the lockfile — never a fixed port.

mod util;

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn initialize_line(id: u64) -> String {
    util::rpc(
        id,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "oracle", "version": "0"}
        }),
    )
    .to_string()
}

/// The stdio transport shape: one JSON-RPC request line in, one response
/// line out, id-correlated. Exercised in-process over a duplex pipe — the
/// same byte discipline a spawning client (Claude Code) speaks.
#[tokio::test]
async fn stdio_transport_answers_line_delimited_jsonrpc() {
    let (_dir, core) = util::core();
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server);
    tokio::spawn(rezidnt_mcp::serve_stdio(
        Arc::clone(&core),
        server_read,
        server_write,
    ));

    let (client_read, mut client_write) = tokio::io::split(client);
    client_write
        .write_all(format!("{}\n", initialize_line(11)).as_bytes())
        .await
        .expect("write initialize line");

    let mut lines = BufReader::new(client_read).lines();
    let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .expect("stdio transport must answer within 5 s, not hang")
        .expect("read response line")
        .expect("stream must not close before answering");
    let response: serde_json::Value = serde_json::from_str(&line)
        .unwrap_or_else(|e| panic!("response line must be JSON ({e}): {line}"));
    assert_eq!(response["id"], json!(11), "response correlated to request");
    assert!(
        response["result"]["protocolVersion"].as_str().is_some(),
        "initialize over stdio answers like initialize anywhere: {response:#}"
    );
}

/// Exit machinery + triage item 6: HTTP on 127.0.0.1 requested at port 0;
/// the ACTUAL port is discovered via the lockfile (pid, port, url, operator
/// badge) — and the announced endpoint really answers JSON-RPC.
#[tokio::test]
async fn http_port_zero_is_announced_via_lockfile_and_answers() {
    let (_dir, core) = util::core();
    let lock_dir = tempfile::tempdir().expect("tempdir");
    let lock_path = lock_dir.path().join("mcp.lock");

    let handle = rezidnt_mcp::serve_http(Arc::clone(&core), &lock_path)
        .await
        .expect("serve_http binds and announces");
    assert_ne!(
        handle.port, 0,
        "the BOUND port is real, never the 0 we asked with"
    );

    let lockfile = rezidnt_mcp::lockfile::read(&lock_path).expect("lockfile parses");
    assert_eq!(
        lockfile.port, handle.port,
        "lockfile announces the bound port"
    );
    assert_eq!(
        lockfile.pid,
        std::process::id(),
        "lockfile names this process"
    );
    assert_eq!(lockfile.url, handle.url);
    assert!(
        lockfile.url.contains(&format!("127.0.0.1:{}", handle.port)),
        "loopback only, discovered port: {}",
        lockfile.url
    );
    assert_eq!(
        lockfile.badge.len(),
        64,
        "operator badge token: 256 bits, hex"
    );

    // The announced endpoint answers JSON-RPC (raw HTTP/1.1, no client dep).
    let response = http_post_json(&lockfile.url, &initialize_line(21));
    assert_eq!(response["id"], json!(21));
    assert!(
        response["result"]["protocolVersion"].as_str().is_some(),
        "initialize over HTTP answers like initialize anywhere: {response:#}"
    );
}

/// The lockfile write itself: atomic, parseable back, mode 0600 on unix
/// (doc §12 — possession of the operator badge is scoped to the local user).
#[tokio::test]
async fn lockfile_roundtrips_and_is_private() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mcp.lock");
    let written = rezidnt_mcp::lockfile::Lockfile {
        pid: std::process::id(),
        port: 43210,
        url: "http://127.0.0.1:43210/mcp".to_string(),
        badge: "ab".repeat(32),
    };
    rezidnt_mcp::lockfile::write_atomic(&path, &written).expect("write");
    let read = rezidnt_mcp::lockfile::read(&path).expect("read back");
    assert_eq!(read, written);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "lockfile carries the operator badge: 0600");
    }
}

/// Minimal HTTP/1.1 POST returning the JSON-RPC response body. Tolerates
/// plain JSON bodies and SSE (`data:` line) — both legal for streamable
/// HTTP servers. Blocking IO is fine in a test.
fn http_post_json(url: &str, body: &str) -> serde_json::Value {
    let rest = url
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("loopback http url expected, got {url}"));
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = std::net::TcpStream::connect(host).expect("connect announced endpoint");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
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
        // Tolerate chunked encoding by slicing the outermost JSON object.
        let start = payload.find('{').expect("a JSON body");
        let end = payload.rfind('}').expect("a JSON body");
        payload[start..=end].to_string()
    };
    serde_json::from_str(&json_text)
        .unwrap_or_else(|e| panic!("response body must be JSON-RPC ({e}): {json_text}"))
}
