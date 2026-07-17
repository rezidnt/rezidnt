//! Oracle (S3-T6 LOW): `daemon.warning {what: "open-failed"}` mints a FRESH
//! correlation (`Ulid::new()`) instead of the failed open's own correlation.
//! A client can then only scope the warning by TIME-MARKER (id > marker), so a
//! CONCURRENT client's failed open that lands after our marker is
//! indistinguishable from ours — it can falsely satisfy our "did my open
//! fail?" check. `begin_open` has the correlation in hand post-S3, so the
//! warning can and must carry it.
//!
//! PIN: the `daemon.warning` emitted for a failed open carries the SAME
//! correlation as that open's request chain — i.e. equal to the
//! `workspace.opened` correlation for a post-materialization failure. Two
//! concurrent failed opens then carry two different correlations and are
//! distinguishable.
//!
//! RED MODE: assert-red. `warn_open_failed` currently constructs the event
//! with `Ulid::new()`, so `warning.correlation != opened.correlation` — the
//! equality assertion fails today. Daemon integration test, WSL.
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
                "clientInfo": {"name": "oracle-s3t6", "version": "0"}
            }),
        ),
    );
    assert!(
        response["result"]["protocolVersion"].as_str().is_some(),
        "initialize must answer over HTTP: {response:#}"
    );
}

/// Poll `tail_events` until `pred` matches; returns the full event log served.
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

/// THE PIN: a post-materialization open failure emits `workspace.opened` and
/// `daemon.warning{open-failed}` that share ONE correlation — the open's. The
/// warning is scoped by causal chain, not by a time marker, so a concurrent
/// failed open is distinguishable.
#[test]
fn open_failed_warning_carries_the_opens_correlation() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let badge = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    // A valid spec (agent 1 is the working S1 stub) plus a second agent whose
    // harness binary does not exist: its launch fails AFTER workspace.opened,
    // so both workspace.opened and daemon.warning{open-failed} reach the log
    // on the SAME open chain.
    let (_project, spec) = make_project(100);
    let spec = format!(
        "{spec}\n[[agent]]\nname = \"broken\"\nharness = \"claude-code\"\nworktree = \"auto\"\nbin_override = \"/nonexistent/rezidnt-s3t6-oracle-harness\"\n"
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

    // Both facts land on the log; capture the whole log once both are present.
    let log = tail_until(&url, Duration::from_secs(20), |e| {
        e["subject"] == "daemon.warning" && e["payload"]["what"] == "open-failed"
    });

    let opened_evt = log
        .iter()
        .find(|e| e["subject"] == "workspace.opened" && e["workspace"] == json!(workspace))
        .unwrap_or_else(|| panic!("workspace.opened for {workspace} must be on the log: {log:#?}"));
    let warning = log
        .iter()
        .find(|e| e["subject"] == "daemon.warning" && e["payload"]["what"] == "open-failed")
        .unwrap_or_else(|| panic!("daemon.warning{{open-failed}} must be on the log: {log:#?}"));

    let opened_correlation = opened_evt["correlation"]
        .as_str()
        .expect("workspace.opened carries a correlation");
    let warning_correlation = warning["correlation"]
        .as_str()
        .expect("daemon.warning carries a correlation");

    assert_eq!(
        warning_correlation, opened_correlation,
        "the open-failed warning must carry the OPEN's correlation, not a fresh \
         Ulid — otherwise two concurrent failed opens are indistinguishable \
         (S3-T6). warning={warning:#}, opened={opened_evt:#}"
    );
}
