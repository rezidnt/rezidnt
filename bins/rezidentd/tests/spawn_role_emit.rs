//! SP4a oracle — CRITERION 2 (emit, I3): the `role` axis is recorded on
//! `agent.spawned` (DR-016 §Decision 2; ontology `agent.spawned.role?` line
//! 195). A launch whose spec declares `role = "reviewer"` emits `agent.spawned`
//! carrying `role: "reviewer"`; a roleless spec OMITS the field entirely —
//! never `role: ""`, never a synthesized default (DR-012 declared-vs-absent).
//!
//! This is pinned at the SAME level the existing `bare` / `allowed_tools` emit
//! is pinned (`tests/vet_gate.rs::vet_pass_is_ordered_before_spawn_and_records_
//! governed_fields`, lines 148-155): a real end-to-end daemon spawn of the gated
//! project, reading the `agent.spawned` payload off the live tail.
//!
//! API SHAPE THE IMPLEMENTER MUST MATCH: in `bins/rezidentd/src/runs.rs`
//! `launch_agent` (~line 729-764, alongside the `bare` / `allowed_tools` / `pep`
//! emit), insert `role` onto `spawned_payload` iff `agent.role` is `Some` —
//! `if let Some(role) = &agent.role { obj.insert("role", json!(role)); }`.
//! ABSENT `role` inserts NOTHING (mirror the `harness_version` / `pep` `if let
//! Some` gate — absence is honest, never `role: ""`).
//!
//! RED MODE — ASSERT-RED: today `launch_agent` emits no `role` key (it does not
//! read `agent.role`, which does not exist until CRITERION 1 lands), so
//! `spawned["payload"]["role"]` is JSON null and the positive assertion fails.
//! The omit-leg passes trivially today but becomes load-bearing once the field
//! is wired (it guards against a synthesized default). Both go honestly green
//! only when the emit reads a real `AgentSpec.role`.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{connect, make_gated_project, open_request, read_until, send_line, start_daemon};

/// Insert a `role = "<role>"` line into the gated spec's `[[agent]]` block,
/// right after the `worktree = "auto"` line (a stable anchor in
/// `make_gated_project`).
fn with_role(spec: &str, role: &str) -> String {
    let anchor = "worktree = \"auto\"\n";
    assert!(
        spec.contains(anchor),
        "test bug: gated spec lost its worktree anchor"
    );
    spec.replace(anchor, &format!("{anchor}role = \"{role}\"\n"))
}

/// CRITERION 2 (positive leg) — a launch whose spec declares `role = "reviewer"`
/// emits `agent.spawned` carrying `role: "reviewer"`, VERBATIM (ontology line
/// 195: taken verbatim from `AgentSpec.role`).
///
/// ASSERT-RED until `launch_agent` reads `agent.role` and emits it.
#[test]
fn spawn_records_declared_role_on_agent_spawned() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);
    let spec = with_role(&spec, "reviewer");

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
    });

    let spawned = lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .expect("read_until stopped on agent.spawned");
    assert_eq!(
        spawned["payload"]["role"], "reviewer",
        "the declared role rides agent.spawned verbatim (DR-016 §Decision 2; I3 \
         log-derivable) — got {spawned:#}"
    );
}

/// CRITERION 2 (the honesty leg — load-bearing) — a roleless gated spec (no
/// `role` line) emits `agent.spawned` with NO `role` key at all. Absence is the
/// honest "no role declared"; the emit must never synthesize `role: ""` or a
/// default (DR-012; ontology `agent.spawned.role?`). Mirrors the `bare` /
/// `allowed_tools` absent-is-honest discipline.
///
/// This spec is the untouched `make_gated_project` output — which carries
/// `bare` / `harness_version` / `allowed_tools` but NO `role` — so the omit is
/// exercised alongside the present governed fields.
#[test]
fn spawn_omits_role_when_absent_never_synthesized() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
    });

    let spawned = lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .expect("read_until stopped on agent.spawned");
    assert!(
        spawned["payload"].get("role").is_none(),
        "a roleless spec omits `role` on agent.spawned — absence is honest, never \
         `role: \"\"` or a synthesized default (DR-012; ontology role? line): {spawned:#}"
    );
    // Sanity: the OTHER governed fields ARE present on this same spawn, so the
    // omission is specific to `role`, not a dropped payload.
    assert_eq!(
        spawned["payload"]["bare"], true,
        "the governed spawn still records bare (the omit is role-specific)"
    );
}
