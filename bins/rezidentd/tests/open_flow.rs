//! S1 oracle: `rezidnt open` materialization — every step visible in `tail`
//! (the S1 exit criterion, exercised at the real socket surface with a stub
//! harness, zero network).
#![cfg(unix)]

mod common;

use common::{connect, make_project, open_request, read_until, send_line, start_daemon};
use std::time::Duration;

/// After an `open`, a tail from seq 0 shows the materialization facts in
/// causal order, through to the run's completion — with the adapter-mapped
/// telemetry (`agent.message`) on the fabric between spawn and completion.
#[test]
fn open_materialization_facts_visible_in_tail_in_order() {
    let daemon = start_daemon();
    let (_project, spec) = make_project(300);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.completed"
    });

    let subjects: Vec<String> = lines
        .iter()
        .filter_map(|v| v["subject"].as_str().map(String::from))
        .collect();
    let pos = |s: &str| {
        subjects
            .iter()
            .position(|x| x == s)
            .unwrap_or_else(|| panic!("{s} never appeared in tail; saw {subjects:?}"))
    };

    // The S1 exit demo's ordering contract.
    assert!(pos("workspace.opened") < pos("workspace.spec.applied"));
    assert!(pos("workspace.spec.applied") < pos("worktree.allocated"));
    assert!(pos("worktree.allocated") < pos("agent.spawned"));
    assert!(pos("agent.spawned") < pos("agent.message"));
    assert!(pos("agent.message") < pos("agent.completed"));
}

/// The worktree fact records rezidnt as allocator (sole-allocator model,
/// DR-001) and a path that actually exists on disk.
#[test]
fn worktree_allocated_fact_names_rezidnt_as_allocator() {
    let daemon = start_daemon();
    let (_project, spec) = make_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "worktree.allocated"
    });
    let wt = lines.last().expect("worktree.allocated line");
    assert_eq!(wt["payload"]["allocator"], "rezidnt");
    let path = wt["payload"]["path"].as_str().expect("payload.path");
    assert!(
        std::path::Path::new(path).exists(),
        "allocated worktree path must exist: {path}"
    );
}
