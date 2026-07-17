//! Shared harness for the S3 MCP oracle board.
#![allow(dead_code)] // each integration test uses a subset

use std::path::PathBuf;
use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore};
use rezidnt_run::badge::Badge;
use rezidnt_types::Event;
use serde_json::{Value, json};

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// A core over a fresh temp log, with `badges` pre-admitted.
/// Returns the tempdir guard alongside so the SQLite file outlives the test.
pub fn core_with_badges(badges: &[&Badge]) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    for badge in badges {
        book.admit(badge);
    }
    (dir, Arc::new(McpCore::new(fabric, book)))
}

pub fn core() -> (tempfile::TempDir, Arc<McpCore>) {
    core_with_badges(&[])
}

/// Publish every envelope of a committed golden fixture onto the core's
/// fabric (the fixture IS the log — I3).
pub fn seed_fixture(core: &McpCore, name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()));
    let events: Vec<Event> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Event::from_json_line(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect();
    for event in &events {
        core.fabric()
            .publish(event.clone())
            .unwrap_or_else(|e| panic!("{name}: publish failed: {e}"));
    }
    events
}

/// All events currently on the core's log (asserting side effects and their
/// absence).
pub fn log_events(core: &McpCore) -> Vec<Event> {
    core.fabric().replay_since(None).expect("replay log")
}

/// One JSON-RPC 2.0 request value.
pub fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// Dispatch a request and unwrap the JSON-RPC `result`, panicking (with the
/// full response) on a protocol-level error object.
pub async fn call_ok(core: &McpCore, id: u64, method: &str, params: Value) -> Value {
    let response = core
        .handle(rpc(id, method, params))
        .await
        .expect("a request (with id) must get a response");
    assert_eq!(
        response["id"],
        json!(id),
        "response id echoes the request id"
    );
    assert!(
        response.get("error").is_none(),
        "expected a result, got a JSON-RPC error: {response:#}"
    );
    response["result"].clone()
}

/// `tools/call` and return the MCP tool result object (`content`,
/// `isError`, ...).
pub async fn tool_call(core: &McpCore, id: u64, tool: &str, args: Value) -> Value {
    call_ok(
        core,
        id,
        "tools/call",
        json!({"name": tool, "arguments": args}),
    )
    .await
}

/// The machine-readable payload of a tool result: `content[0].text` parsed
/// as JSON. Tool successes AND tool-level refusals both ride this shape.
pub fn tool_payload(result: &Value) -> Value {
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool result must carry content[0].text: {result:#}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("content[0].text must be machine-readable JSON ({e}): {text}"))
}

/// Assert a tool result is a REFUSAL with the given machine-readable code.
pub fn assert_tool_refusal(result: &Value, code: &str) {
    assert_eq!(
        result["isError"],
        json!(true),
        "a refused tool call carries isError: true — got {result:#}"
    );
    let payload = tool_payload(result);
    assert_eq!(
        payload["code"],
        json!(code),
        "refusal code must be machine-readable, exactly {code:?} — got {payload:#}"
    );
}

/// `tools/list` → the tool array.
pub async fn list_tools(core: &McpCore) -> Vec<Value> {
    let result = call_ok(core, 90, "tools/list", json!({})).await;
    result["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list result carries a tools array: {result:#}"))
        .clone()
}

pub fn find_tool<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|t| t["name"] == json!(name))
        .unwrap_or_else(|| panic!("tool {name} must be served; got {tools:#?}"))
}
