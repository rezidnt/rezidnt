//! S3 oracle — the exit demo's machinery, MCP ONLY, over the loopback-HTTP
//! transport discovered via the lockfile (doc §9; I5: every capability is an
//! MCP tool/resource first).
//!
//! Board pins (flagged as DEFAULTs in the oracle work order):
//! - `REZIDNT_MCP_LOCKFILE=<path>` asks the daemon to serve MCP over HTTP on
//!   `127.0.0.1:0` and announce `{pid, port, url, badge}` there (0600);
//! - the lockfile `badge` is the OPERATOR badge token local clients present
//!   on mutating tools (doc §12: badges on EVERY mutating MCP call — Claude
//!   Code included, not just spawned agents).
//!
//! Pending-ratification note (S2 pattern): attribution-shape assertions
//! (`badge_id` on mutation facts) are a warden item; these tests tie calls
//! to log facts through the acked `correlation` instead, which no
//! ratification can change.
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::{
    make_project, mcp_post, mcp_tool_call, rpc, start_daemon_with_mcp, tool_payload,
    wait_for_lockfile,
};
use serde_json::json;

const LOCK_DEADLINE: Duration = Duration::from_secs(10);

fn initialize(url: &str) {
    let response = mcp_post(
        url,
        &rpc(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "oracle", "version": "0"}
            }),
        ),
    );
    assert!(
        response["result"]["protocolVersion"].as_str().is_some(),
        "initialize must answer over HTTP: {response:#}"
    );
}

/// Poll `tail_events` (MCP only — no socket peeking) until an event matching
/// `pred` shows up; returns the whole log as served.
fn tail_until(
    url: &str,
    deadline: Duration,
    mut pred: impl FnMut(&serde_json::Value) -> bool,
) -> Vec<serde_json::Value> {
    let until = Instant::now() + deadline;
    loop {
        let result = mcp_tool_call(url, 40, "tail_events", json!({}));
        let events = tool_payload(&result)["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if events.iter().any(&mut pred) {
            return events;
        }
        assert!(
            Instant::now() < until,
            "deadline: tail_events never showed the expected event; last saw {} events",
            events.len()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Triage item 6: the HTTP transport binds port 0 and announces the REAL
/// port via the lockfile — plus the operator badge, private to the user.
#[test]
fn lockfile_announces_bound_port_and_operator_badge() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);

    let port = lock["port"].as_u64().expect("lockfile carries port");
    assert!(port > 0, "the announced port is the BOUND one, never 0");
    let url = lock["url"].as_str().expect("lockfile carries url");
    assert!(
        url.contains(&format!("127.0.0.1:{port}")),
        "loopback only, announced port: {url}"
    );
    assert!(
        lock["pid"].as_u64().is_some(),
        "pid for staleness detection"
    );
    let badge = lock["badge"].as_str().expect("operator badge token");
    assert_eq!(badge.len(), 64, "256-bit token, hex (doc §12)");
    assert!(badge.chars().all(|c| c.is_ascii_hexdigit()));

    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&lock_path)
        .expect("stat lockfile")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the lockfile carries a capability: 0600");

    initialize(url);
}

/// Exit (a): open a project via MCP only. The result is REQUEST-SCOPED —
/// it names the workspace and the correlation id, and the log's
/// materialization facts carry exactly that correlation (the ack is tied to
/// the log, not vibes).
#[test]
fn open_project_via_mcp_returns_correlated_workspace() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    let badge = lock["badge"].as_str().expect("badge");
    initialize(url);

    let (_project, spec) = make_project(100);
    let result = mcp_tool_call(
        url,
        2,
        "open_project",
        json!({"badge": badge, "spec_toml": spec}),
    );
    assert_ne!(
        result["isError"],
        json!(true),
        "open must succeed: {result:#}"
    );
    let payload = tool_payload(&result);
    let workspace = payload["workspace"]
        .as_str()
        .expect("result names the workspace ulid");
    let correlation = payload["correlation"]
        .as_str()
        .expect("result names the correlation");

    let events = tail_until(url, Duration::from_secs(20), |e| {
        e["subject"] == "workspace.opened" && e["workspace"] == json!(workspace)
    });
    let opened = events
        .iter()
        .find(|e| e["subject"] == "workspace.opened" && e["workspace"] == json!(workspace))
        .expect("workspace.opened for the acked workspace");
    assert_eq!(
        opened["correlation"],
        json!(correlation),
        "the acked correlation IS the materialization chain's correlation"
    );
}

/// A mutating call with the lockfile badge absent is refused over HTTP too —
/// the transport does not soften the door (doc §12).
#[test]
fn open_project_without_badge_is_refused_over_http() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    initialize(url);

    let (_project, spec) = make_project(100);
    let result = mcp_tool_call(url, 3, "open_project", json!({"spec_toml": spec}));
    assert_eq!(result["isError"], json!(true));
    assert_eq!(
        tool_payload(&result)["code"],
        json!("badge.required"),
        "machine-readable refusal: {result:#}"
    );
}

/// Exit (b) + §9 idempotency: `spawn_agent` twice with the SAME key returns
/// the SAME run and spawns exactly once.
#[test]
fn spawn_agent_is_idempotent_by_key() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    let badge = lock["badge"].as_str().expect("badge");
    initialize(url);

    let (_project, spec) = make_project(100);
    let opened = mcp_tool_call(
        url,
        4,
        "open_project",
        json!({"badge": badge, "spec_toml": spec}),
    );
    let workspace = tool_payload(&opened)["workspace"]
        .as_str()
        .expect("workspace ulid")
        .to_string();

    let spawn = |id: u64| {
        let result = mcp_tool_call(
            url,
            id,
            "spawn_agent",
            json!({
                "badge": badge,
                "workspace": workspace,
                "agent": "impl",
                "idempotency_key": "oracle-key-1"
            }),
        );
        assert_ne!(
            result["isError"],
            json!(true),
            "spawn must succeed: {result:#}"
        );
        tool_payload(&result)["run"]
            .as_str()
            .expect("spawn result names the run ulid")
            .to_string()
    };
    let first = spawn(5);
    let second = spawn(6);
    assert_eq!(
        first, second,
        "same idempotency key, same run — no double spawn"
    );

    let events = tail_until(url, Duration::from_secs(20), |e| {
        e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(first)
    });
    let spawned = events
        .iter()
        .filter(|e| e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(first))
        .count();
    assert_eq!(spawned, 1, "exactly one agent.spawned for the keyed run");
}

/// Exit (c): the spawned run's dossier is readable via MCP resources and
/// carries the folded accounting once the run completes.
#[test]
fn dossier_readable_via_mcp_after_completion() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    let badge = lock["badge"].as_str().expect("badge");
    initialize(url);

    let (_project, spec) = make_project(100);
    let opened = mcp_tool_call(
        url,
        7,
        "open_project",
        json!({"badge": badge, "spec_toml": spec}),
    );
    assert_ne!(
        opened["isError"],
        json!(true),
        "open must succeed: {opened:#}"
    );

    // The spec's agent runs to completion (stub harness); find its run via
    // MCP only.
    let events = tail_until(url, Duration::from_secs(30), |e| {
        e["subject"] == "agent.completed"
    });
    let run = events
        .iter()
        .find(|e| e["subject"] == "agent.completed")
        .and_then(|e| e["payload"]["run"].as_str())
        .expect("agent.completed names its run")
        .to_string();

    let result = mcp_post(
        url,
        &rpc(
            8,
            "resources/read",
            json!({"uri": format!("rezidnt://run/{run}/dossier")}),
        ),
    );
    let text = result["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("dossier contents[0].text: {result:#}"));
    let dossier: serde_json::Value = serde_json::from_str(text).expect("dossier is JSON");
    assert_eq!(dossier["status"], json!("completed"));
    assert!(
        dossier["total_usd"].as_f64().is_some(),
        "dossier accounting is folded from the log (I3): {dossier:#}"
    );
}

/// Exit (d): `gate_explain` for a FORCED failure — a stub verdict seeded on
/// the log (I3) — answers over HTTP with the failing verifier, evidence
/// refs, and exact inputs (doc §8).
#[test]
fn gate_explain_forced_failure_over_http() {
    let (_daemon, lock_path) = start_daemon_with_mcp(Some("s3_gate_forced_failure.jsonl"));
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    initialize(url);

    let result = mcp_tool_call(
        url,
        9,
        "gate_explain",
        json!({"run": "01S3GATEFA1DED000000000R01"}),
    );
    assert_ne!(
        result["isError"],
        json!(true),
        "explainable run: {result:#}"
    );
    let explain = tool_payload(&result);
    assert_eq!(explain["verdict"], json!("fail"));
    assert_eq!(explain["verifier"], json!("tests-pass"));
    assert_eq!(
        explain["evidence"][0]["hash"],
        json!("a0fda6ff40cb5f91bd2d09cbfb839ae91b9b4c9aa0ccfc0981986c10d4d08246"),
        "evidence refs verbatim from the seeded log"
    );
    assert_eq!(
        explain["inputs"]["timeout_ms"],
        json!(120000),
        "the exact recorded inputs come back: {explain:#}"
    );
}
