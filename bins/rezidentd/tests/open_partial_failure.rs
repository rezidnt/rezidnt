//! S4 board item (from the S3-week debrief, medium finding): EVICTION
//! OVER-REACH on partial open failure.
//!
//! `materialize_open` (bins/rezidentd/src/runs.rs:374-457) evicts the
//! workspace entry on ANY materialization failure — but `workspace.opened`
//! publishes at step 1, so a POST-FACT failure (agent 2 of 2 fails to
//! launch) evicts a workspace the log OPENED. A restarted daemon refolds the
//! log and answers `spawn_agent` happily; the live daemon answers
//! `workspace.unknown`: fold(log) != live map, which is exactly the I3
//! defect class S3-T1 fixed for restarts.
//!
//! The honest direction (pinned here): `workspace.opened` is ON THE LOG, so
//! the workspace IS open — live and restarted daemons must give the SAME
//! answer. The failed agent is the failed agent; the workspace stays
//! spawnable. Eviction is only for the GHOST case (opened_id never
//! published — the S3-T2 pin, unchanged by this board).
//!
//! RED MODE: assert-red — today the live `spawn_agent` after the partial
//! failure is refused `workspace.unknown` (fast failure on the isError
//! assertion).
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::{
    make_project, mcp_post, mcp_tool_call, restart_daemon_with_mcp, rpc, start_daemon_with_mcp,
    tool_payload, wait_for_lockfile,
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
                "clientInfo": {"name": "oracle-s4", "version": "0"}
            }),
        ),
    );
    assert!(
        response["result"]["protocolVersion"].as_str().is_some(),
        "initialize must answer over HTTP: {response:#}"
    );
}

/// Poll `tail_events` until `pred` matches; returns the log as served.
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

/// The pin: after a POST-FACT materialization failure (second agent's
/// harness binary does not exist), the workspace whose `workspace.opened`
/// IS on the log stays spawnable on the LIVE daemon — and a restarted daemon
/// agrees, because both answers derive from the log (I3).
#[test]
fn partially_failed_open_keeps_logged_workspace_spawnable_live() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let badge = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    // Agent 1 ("impl") is the working S1 stub; agent 2 ("broken") names a
    // nonexistent harness binary, so its launch fails AFTER workspace.opened
    // and workspace.spec.applied are published.
    let (_project, spec) = make_project(100);
    let spec = format!(
        "{spec}\n[[agent]]\nname = \"broken\"\nharness = \"claude-code\"\nworktree = \"auto\"\nbin_override = \"/nonexistent/rezidnt-s4-oracle-harness\"\n"
    );

    let opened = mcp_tool_call(
        &url,
        2,
        "open_project",
        json!({"badge": badge, "spec_toml": spec}),
    );
    assert_ne!(
        opened["isError"],
        json!(true),
        "open is acked (the failure is post-fact, detached): {opened:#}"
    );
    let workspace = tool_payload(&opened)["workspace"]
        .as_str()
        .expect("open ack names the workspace")
        .to_string();

    // Both facts: the workspace WAS opened on the log, and the launch chain
    // failed afterwards (visible, not silent).
    tail_until(&url, Duration::from_secs(20), |e| {
        e["subject"] == "workspace.opened" && e["workspace"] == json!(workspace)
    });
    tail_until(&url, Duration::from_secs(20), |e| {
        e["subject"] == "daemon.warning" && e["payload"]["what"] == "open-failed"
    });

    // THE PIN (red today): the LIVE daemon must answer from the log —
    // workspace.opened is there, so "impl" is spawnable. Eviction of a
    // logged workspace is the over-reach.
    let live = mcp_tool_call(
        &url,
        3,
        "spawn_agent",
        json!({
            "badge": badge,
            "workspace": workspace,
            "agent": "impl",
            "idempotency_key": "s4-partial-failure-live"
        }),
    );
    assert_ne!(
        live["isError"],
        json!(true),
        "workspace.opened is ON THE LOG: the live daemon must not evict it \
         after a post-fact launch failure (fold(log) == live map, I3): {live:#}"
    );
    tail_until(&url, Duration::from_secs(20), |e| {
        e["subject"] == "agent.spawned"
            && e["payload"]["idempotency_key"] == "s4-partial-failure-live"
    });

    // Restart-equality (the direction-setter; green once the pin holds): a
    // restarted daemon folds the same log and must give the SAME answer.
    restart_daemon_with_mcp(&mut daemon, &lock_path);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url after restart").to_string();
    let badge = lock["badge"]
        .as_str()
        .expect("badge after restart")
        .to_string();
    initialize(&url);

    let restarted = mcp_tool_call(
        &url,
        4,
        "spawn_agent",
        json!({
            "badge": badge,
            "workspace": workspace,
            "agent": "impl",
            "idempotency_key": "s4-partial-failure-restarted"
        }),
    );
    assert_ne!(
        restarted["isError"],
        json!(true),
        "live and restarted daemons must agree — both fold the log: {restarted:#}"
    );
}
