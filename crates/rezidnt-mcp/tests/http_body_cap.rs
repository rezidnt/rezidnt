//! Oracle (S3-T5 LOW): the hand-rolled loopback HTTP transport caps the
//! request HEAD at 64 KiB but reads the BODY unbounded — a `Content-Length`
//! (or actual body) of any size is read straight into memory. Loopback-only
//! and lockfile-gated, so this is a LOCAL-DoS surface, not remote; but a
//! transport with no body cap mirrors none of the I2 payload discipline.
//!
//! PIN: a request whose body exceeds a defined cap is REJECTED with a
//! machine-readable answer (a 413-class HTTP status) and is NOT accumulated
//! unbounded; a request at/under the cap still answers JSON-RPC normally.
//!
//! DEFAULT (flagged in the oracle report): the request-body cap is 64 KiB —
//! mirroring the existing 64 KiB HEAD cap already in `serve_http_conn`, and
//! leaving headroom over the I2 32 KiB *payload* rule (the JSON-RPC envelope
//! wraps the payload) while still bounding the memory a single loopback
//! connection can force the daemon to allocate. Cheap to revisit.
//!
//! RED MODE: assert-red. Today the oversized request is read fully into memory
//! and answered 200 (or parse-errors on a huge value), never 413 — the cap
//! does not exist, so the 413 assertion fails.

mod util;

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

/// The body cap this oracle pins (DEFAULT — see module docs). Kept local to
/// the test on purpose: when the implementer lands the real constant, this
/// value is what /vet holds them to; a change to the cap is a change to this
/// test (the DEFAULT's single source of truth for the pin).
const BODY_CAP_BYTES: usize = 64 * 1024;

/// Serve the HTTP transport; the returned handle keeps the listener alive
/// (dropping `HttpHandle` stops it), so the caller must hold it for the test.
async fn serve(
    core: Arc<rezidnt_mcp::McpCore>,
    lock_path: &std::path::Path,
) -> rezidnt_mcp::HttpHandle {
    rezidnt_mcp::serve_http(core, lock_path)
        .await
        .expect("serve_http binds and announces")
}

/// Raw HTTP/1.1 POST with an EXPLICIT body and Content-Length; returns the
/// status line (first line of the response). Blocking IO is fine in a test.
fn http_post_raw(url: &str, body: &[u8]) -> String {
    let rest = url
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("loopback http url expected, got {url}"));
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = std::net::TcpStream::connect(host).expect("connect announced endpoint");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let head = format!(
        "POST /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).expect("send head");
    stream.write_all(body).expect("send body");
    stream.flush().expect("flush");
    let mut raw = Vec::new();
    // A cap-rejecting server may close after the status line; read what comes.
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    text.lines()
        .next()
        .unwrap_or_else(|| panic!("no status line in response: {text}"))
        .to_string()
}

/// THE PIN: a body larger than the cap is rejected 413-class and never read
/// unbounded. We declare an honest oversized Content-Length and send that many
/// bytes; the server must refuse rather than allocate them all.
#[tokio::test]
async fn oversized_request_body_is_rejected_not_read_unbounded() {
    let (_dir, core) = util::core();
    let lock_dir = tempfile::tempdir().expect("tempdir");
    let handle = serve(Arc::clone(&core), &lock_dir.path().join("mcp.lock")).await;
    let url = handle.url.clone();

    // One byte over the cap. Valid JSON in shape (a huge string), so the ONLY
    // reason to reject it is the size cap — not a parse error.
    let filler = "x".repeat(BODY_CAP_BYTES + 1);
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"pad":"{filler}"}}}}"#
    );
    assert!(
        body.len() > BODY_CAP_BYTES,
        "sanity: the crafted body exceeds the cap"
    );

    let status = http_post_raw(&url, body.as_bytes());
    assert!(
        status.contains(" 413"),
        "an over-cap body must be rejected 413-class (payload too large), not \
         read unbounded into memory; got status: {status:?}"
    );
}

/// A body AT the cap boundary still works — the cap rejects the excess, never
/// the legitimate request. A normal `initialize` is well under 64 KiB.
#[tokio::test]
async fn body_within_cap_still_answers() {
    let (_dir, core) = util::core();
    let lock_dir = tempfile::tempdir().expect("tempdir");
    let handle = serve(Arc::clone(&core), &lock_dir.path().join("mcp.lock")).await;
    let url = handle.url.clone();

    let body = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "oracle", "version": "0"}
        }
    })
    .to_string();
    assert!(
        body.len() <= BODY_CAP_BYTES,
        "sanity: a normal initialize is under the cap"
    );

    let status = http_post_raw(&url, body.as_bytes());
    assert!(
        status.contains(" 200"),
        "a request under the cap must answer normally (200); got: {status:?}"
    );
}
