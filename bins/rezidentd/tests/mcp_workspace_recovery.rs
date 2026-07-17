//! Pre-S4 remediation oracle — written against the auditor's findings BEFORE
//! the fix. Two I3 findings on the daemon's MCP surface are encoded:
//!
//! - **S3-T1 — process-lifetime derived state:** the workspaces map and the
//!   `spawn_agent` idempotency keys live only in memory (`runs.rs`
//!   `Daemon::workspaces`, `OpenedWorkspace::spawn_keys`). After a daemon
//!   restart a previously-acked workspace answers `workspace.unknown` even
//!   though `workspace.opened` is on the log, and a retried spawn key mints a
//!   SECOND run. I3: anything that cannot be rebuilt from log + CAS is
//!   misdesigned — the map must be rebuilt from the log on start (the S2
//!   restart-exactly-once precedent).
//! - **S3-T2 — ghost-workspace window:** `begin_open` registers the
//!   workspace entry BEFORE detached materialization; when materialization
//!   fails post-ack (only `daemon.warning {what: "open-failed"}` reaches the
//!   log, never `workspace.opened`), the entry persists forever and
//!   `spawn_agent` can mint `agent.spawned` facts for a workspace that was
//!   never opened on the log. The remediation direction (either shape
//!   satisfies these pins): gate `spawn_agent` on the `workspace.opened`
//!   fact, or evict the entry on materialization failure — both answer
//!   `workspace.unknown`.
//!
//! Restart is real: SIGKILL the daemon, respawn on the SAME db (see
//! `common::restart_daemon_with_mcp`). Everything here goes through MCP only
//! (I5), and every pin ties behavior to the log, not to memory.
//!
//! Deliberately UNPINNED (implementer latitude, noted in the work order):
//! HOW the spawn key becomes log-derivable — today's `agent.spawned` v1
//! payload does not carry the idempotency key, so the obvious route is an
//! additive payload field, which goes through `/subject` (warden), not
//! through this test file.
#![cfg(unix)]

mod common;

use std::process::Command;
use std::time::{Duration, Instant};

use common::{
    make_project, mcp_post, mcp_tool_call, restart_daemon_with_mcp, rpc, start_daemon_with_mcp,
    stub_harness, tool_payload, wait_for_lockfile,
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

/// Poll `tail_events` until an event matching `pred` shows up; returns the
/// whole log as served.
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

/// One `tail_events` snapshot of the log as served right now.
fn tail_snapshot(url: &str) -> Vec<serde_json::Value> {
    let result = mcp_tool_call(url, 41, "tail_events", json!({}));
    tool_payload(&result)["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// `spawn_agent` sugar; returns the raw tool result (may carry `isError`).
fn spawn_agent(url: &str, id: u64, badge: &str, workspace: &str, key: &str) -> serde_json::Value {
    mcp_tool_call(
        url,
        id,
        "spawn_agent",
        json!({
            "badge": badge,
            "workspace": workspace,
            "agent": "impl",
            "idempotency_key": key
        }),
    )
}

/// Open a project via MCP and wait until its `workspace.opened` fact is ON
/// THE LOG (not just acked) — the precondition both restart pins build on.
fn open_and_materialize(url: &str, badge: &str, spec: &str) -> String {
    let opened = mcp_tool_call(
        url,
        2,
        "open_project",
        json!({"badge": badge, "spec_toml": spec}),
    );
    assert_ne!(
        opened["isError"],
        json!(true),
        "open must succeed: {opened:#}"
    );
    let workspace = tool_payload(&opened)["workspace"]
        .as_str()
        .expect("open ack names the workspace ulid")
        .to_string();
    tail_until(url, Duration::from_secs(20), |e| {
        e["subject"] == "workspace.opened" && e["workspace"] == json!(workspace)
    });
    workspace
}

/// S3-T1 pin (a): `workspace.opened` is on the log, so the workspace exists —
/// restart notwithstanding. Today the workspaces map is process memory and a
/// restarted daemon answers `workspace.unknown` for a workspace the log says
/// is open: derived state that cannot be rebuilt from the log (I3).
#[test]
fn spawn_agent_succeeds_after_daemon_restart_on_same_log() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    let badge = lock["badge"].as_str().expect("badge");
    initialize(url);

    let (_project, spec) = make_project(50);
    let workspace = open_and_materialize(url, badge, &spec);

    // The restart: process memory gone, log kept.
    restart_daemon_with_mcp(&mut daemon, &lock_path);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url after restart");
    let badge = lock["badge"].as_str().expect("badge after restart");
    initialize(url);

    let result = spawn_agent(url, 3, badge, &workspace, "oracle-restart-spawn");
    assert_ne!(
        result["isError"],
        json!(true),
        "workspace.opened is on the log, so the workspace IS open — a restarted \
         daemon answering workspace.unknown is exactly the I3 violation (S3-T1): {result:#}"
    );
    let run = tool_payload(&result)["run"]
        .as_str()
        .expect("spawn result names the run ulid")
        .to_string();
    tail_until(url, Duration::from_secs(20), |e| {
        e["subject"] == "agent.spawned"
            && e["payload"]["run"] == json!(run)
            && e["workspace"] == json!(workspace)
    });
}

/// S3-T1 pin (b): the §9 idempotency contract has no process-lifetime
/// footnote. A key that minted run R before the restart returns THE SAME run
/// R after it, and the log carries exactly one `agent.spawned` for R —
/// log-derived idempotency, the S2 restart-exactly-once precedent.
#[test]
fn spawn_key_idempotency_survives_daemon_restart() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    let badge = lock["badge"].as_str().expect("badge");
    initialize(url);

    let (_project, spec) = make_project(50);
    let workspace = open_and_materialize(url, badge, &spec);

    let first = spawn_agent(url, 4, badge, &workspace, "oracle-durable-key");
    assert_ne!(
        first["isError"],
        json!(true),
        "precondition: the keyed spawn succeeds pre-restart: {first:#}"
    );
    let run = tool_payload(&first)["run"]
        .as_str()
        .expect("spawn result names the run ulid")
        .to_string();
    // The keyed run's spawned fact must be ON THE LOG before the kill.
    tail_until(url, Duration::from_secs(20), |e| {
        e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(run)
    });

    restart_daemon_with_mcp(&mut daemon, &lock_path);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url after restart");
    let badge = lock["badge"].as_str().expect("badge after restart");
    initialize(url);

    let retry = spawn_agent(url, 5, badge, &workspace, "oracle-durable-key");
    assert_ne!(
        retry["isError"],
        json!(true),
        "a keyed retry after restart is the §9 contract, not an error: {retry:#}"
    );
    let retried_run = tool_payload(&retry)["run"]
        .as_str()
        .expect("retry result names the run ulid")
        .to_string();
    assert_eq!(
        retried_run, run,
        "same idempotency key, same run — across restarts; a second ULID means \
         the key map was process memory, not log-derived state (I3, S3-T1)"
    );

    let spawned = tail_snapshot(url)
        .iter()
        .filter(|e| e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(run))
        .count();
    assert_eq!(
        spawned, 1,
        "exactly one agent.spawned for the keyed run, restart notwithstanding"
    );
}

/// S3-T2 pin: an open acked but never materialized (no `workspace.opened` on
/// the log — only `daemon.warning {what: "open-failed"}`) is a GHOST. Its
/// registered entry must not let `spawn_agent` mint `agent.spawned` facts for
/// a workspace the log never opened: machine-readable refusal
/// (`workspace.unknown`), and zero `agent.spawned` for it, ever.
///
/// The post-ack failure is real: the spec's repo path does not exist when the
/// detached materialization canonicalizes it (`runs.rs`
/// `try_materialize_open`, before fact 1) — and by spawn time the path exists
/// again, so nothing downstream refuses by accident.
#[test]
fn ghost_workspace_never_mints_agent_spawned() {
    let (_daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url");
    let badge = lock["badge"].as_str().expect("badge");
    initialize(url);

    // A spec that parses and names a known harness — it passes every pre-ack
    // check — but whose repo path is not there when materialization runs.
    let project = tempfile::tempdir().expect("tempdir");
    let script = stub_harness(project.path(), 50);
    let repo = project.path().join("ghost-repo");
    let spec = format!(
        r#"[project]
name = "ghost"
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

    let opened = mcp_tool_call(
        url,
        6,
        "open_project",
        json!({"badge": badge, "spec_toml": spec}),
    );
    assert_ne!(
        opened["isError"],
        json!(true),
        "test premise: the ack happens before materialization can fail — a \
         pre-ack refusal here means the ghost window moved and this pin must \
         be revisited: {opened:#}"
    );
    let workspace = tool_payload(&opened)["workspace"]
        .as_str()
        .expect("open ack names the workspace ulid")
        .to_string();

    // The post-ack failure lands on the log; workspace.opened never does.
    let events = tail_until(url, Duration::from_secs(20), |e| {
        e["subject"] == "daemon.warning" && e["payload"]["what"] == json!("open-failed")
    });
    assert!(
        !events
            .iter()
            .any(|e| e["subject"] == "workspace.opened" && e["workspace"] == json!(workspace)),
        "ghost precondition: no workspace.opened ever reached the log for the \
         acked workspace"
    );

    // The path comes back AFTER materialization already failed — too late to
    // open the workspace, but enough for a naive spawn to limp through.
    std::fs::create_dir(&repo).expect("mkdir reborn repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());

    let result = spawn_agent(url, 7, badge, &workspace, "oracle-ghost-key");
    assert_eq!(
        result["isError"],
        json!(true),
        "spawn_agent in a workspace with no workspace.opened on the log must \
         refuse — minting agent.spawned for a never-opened workspace is the \
         ghost-workspace window (S3-T2, I3): {result:#}"
    );
    assert_eq!(
        tool_payload(&result)["code"],
        json!("workspace.unknown"),
        "machine-readable refusal: the log never opened this workspace, so it \
         is unknown — whichever remediation shape (opened-fact gate or \
         evict-on-failure) answers"
    );

    // And the log stays clean: no agent.spawned for the ghost, ever.
    let spawned = tail_snapshot(url)
        .iter()
        .filter(|e| e["subject"] == "agent.spawned" && e["workspace"] == json!(workspace))
        .count();
    assert_eq!(
        spawned, 0,
        "zero agent.spawned facts for a workspace the log never opened"
    );
}
