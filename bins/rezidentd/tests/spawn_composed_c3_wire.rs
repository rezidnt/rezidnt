//! c3-wire oracle (DR-028) — the RUN-LOOP-level arms of criterion 1 (the spawn
//! goes THROUGH the composed confinement path, the raw `Command::new(&plan.bin)`
//! bypass at `runs.rs:785` is gone) and criterion 5 (the daemon OWNS + REAPS the
//! composed child and consumes its stdout — no detached orphan waiter).
//!
//! ## SUITE PLACEMENT — daemon integration (#[cfg(unix)]), the live-spawn home.
//! Placed alongside `open_flow.rs`/`spawn_role_emit.rs` (the run-loop integration
//! home): a real `rezidnt open` governed run at the socket surface, reading facts
//! off the live tail. The pure composed-argv shape + the three degrade DECISIONS
//! are the host-runnable analogues (`crates/rezidnt-run/tests/compose_wire_c3.rs`);
//! the live shared-netns inescapability is the WSL substrate suite
//! (`crates/rezidnt-run/tests/compose_shared_netns_c3_wire.rs`). THIS file pins the
//! run-loop OBSERVABLE: a governed spawn emits a composed-spawn/degrade fact (proof
//! the composed decision path ran, not the raw Command) AND the run reaches
//! `agent.completed` (proof the daemon reaped the composed child + drained stdout).
//!
//! ## RED MODE — LIVE-RED (the run-loop wiring is missing). Today `launch_agent`
//! spawns the child DIRECTLY via `tokio::process::Command::new(&plan.bin)` and
//! emits NO composed-spawn/degrade fact — so `read_until` for the composed fact
//! times out and the assertion fails. This is the honest RED for the missing
//! c3-wire run-loop composition (DR-028 §Decision 2/4), NOT a compile break.
//!
//! ## The observable this pins (the implementer's run-loop work order)
//! A governed `rezidnt open` run must emit a DURABLE composed-spawn/degrade fact
//! naming which of the three composed states it took (mediated / confined-CLOSED /
//! unsandboxed — DR-028 §Decision 4). Its subject is warden-gated (a placeholder
//! until the `sandbox.*`/`egress.*` family is minted, DR-028 §Consequences); this
//! test keys off the payload shape the c3bc/c3a fold suites already pin (a
//! `network`/`sandbox` posture field), not a ratified subject string.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{connect, make_gated_project, open_request, read_until, send_line, start_daemon};

/// Does this fact look like the c3-wire composed-spawn/degrade fact? Keyed off the
/// posture fields the composition records (DR-028 §Decision 4) rather than a
/// warden-unratified subject string: a `network` posture (mediated | sealed) OR an
/// explicit `sandbox`/`egress_enforceable` degrade marker on a spawn-scoped fact.
fn is_composed_spawn_fact(v: &serde_json::Value) -> bool {
    let p = &v["payload"];
    p.get("network").is_some()
        || p.get("egress_enforceable").is_some()
        || (p.get("sandbox").is_some()
            && v["subject"].as_str().is_some_and(|s| s.contains("sandbox")))
}

/// CRITERION 1 (run-loop arm) — a governed `rezidnt open` run spawns THROUGH the
/// composed confinement path: the run emits a durable composed-spawn/degrade fact
/// recording which composed state it took. That fact EXISTS only if the spawn went
/// through the composition decision (DR-028 §Decision 4) — the raw
/// `Command::new(&plan.bin)` bypass emits no such fact. On a box WITH bwrap+pasta
/// the state is mediated/confined; WITHOUT, it is the loud unsandboxed fact — but
/// in EVERY case the composed decision runs and records itself (never a silent raw
/// spawn).
///
/// LIVE-RED until the run loop routes the spawn through the composed path and emits
/// the fact.
#[test]
fn governed_run_emits_a_composed_spawn_degrade_fact_not_a_silent_raw_spawn() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    // Read until the run completes (or the composed fact appears) so we scan the
    // full spawn-time window for the composed decision fact.
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.completed" || is_composed_spawn_fact(v)
    });

    let composed = lines.iter().find(|v| is_composed_spawn_fact(v));
    assert!(
        composed.is_some(),
        "CRITERION 1 (run-loop): a governed run emitted NO composed-spawn/degrade fact — the \
         spawn did NOT go through the composed confinement path (it took the raw \
         `Command::new(&plan.bin)` bypass at runs.rs:785 that c3-wire removes, DR-028 §Decision \
         2). The composed decision MUST run and record its state (mediated / confined-CLOSED / \
         unsandboxed) on every governed spawn — never a silent raw spawn. Subjects seen: {:?}",
        lines
            .iter()
            .filter_map(|v| v["subject"].as_str())
            .collect::<Vec<_>>()
    );

    // The fact names a composed posture, not a bare legacy spawn: it declares
    // either a network posture or an explicit un-enforceable marker — the honesty
    // guard that no state silently claims (or fails to disclaim) enforcement.
    let fact = composed.expect("composed fact present");
    let p = &fact["payload"];
    let discloses_posture = p.get("network").is_some() || p.get("egress_enforceable").is_some();
    assert!(
        discloses_posture,
        "CRITERION 1/4 (run-loop): the composed-spawn fact must DISCLOSE its posture (a \
         `network` = mediated|sealed, or `egress_enforceable` marker) so no run silently claims \
         enforcement it lacks (DR-028 §Decision 4). Got: {fact:#}"
    );
}

/// CRITERION 5 (run-loop arm) — the daemon OWNS + REAPS the composed child and
/// DRAINS its stdout: the governed run reaches `agent.completed` with the
/// adapter-mapped telemetry (`agent.message`) in between. `agent.message` on the
/// fabric proves the run loop consumed the composed child's PIPED stdout (the
/// adapter maps stream-json off it); `agent.completed` proves the daemon reaped
/// the composed child (a detached orphan waiter could not surface completion to
/// the run loop). This is the S1 "daemon owns the process" contract threaded
/// through the composed spawn (DR-028 §Decision 2) — the composed child is a
/// daemon-owned `tokio::process::Child`, not a pid + detached waiter.
///
/// LIVE-RED until the composed spawn returns the daemon-owned child the run loop
/// drains + the reaper adopts (today the raw spawn already completes, so this arm
/// becomes load-bearing once the spawn is composed: it guards that the RESHAPE does
/// not regress into a detached-orphan reap that never surfaces completion).
#[test]
fn daemon_reaps_the_composed_child_and_drains_its_stdout() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(25), |v| {
        v["subject"] == "agent.completed"
    });

    let subjects: Vec<String> = lines
        .iter()
        .filter_map(|v| v["subject"].as_str().map(String::from))
        .collect();
    let pos = |s: &str| subjects.iter().position(|x| x == s);

    // stdout was DRAINED off the composed child: the adapter mapped a message from
    // the piped stream (proof the run loop owns + reads the child's stdout).
    assert!(
        pos("agent.message").is_some(),
        "CRITERION 5 (run-loop): no `agent.message` — the run loop did NOT drain the composed \
         child's piped stdout (the adapter maps stream-json off it). The composed spawn must \
         return a daemon-owned child the run loop reads, not a pid + detached waiter (DR-028 \
         §Decision 2). Subjects: {subjects:?}"
    );
    // The composed child was REAPED: completion surfaced to the run loop. A
    // detached orphan waiter (the shape `sandbox.rs:328` deferred) could not.
    assert!(
        pos("agent.completed").is_some(),
        "CRITERION 5 (run-loop): the run never reached `agent.completed` — the daemon did not \
         reap the composed child + surface completion. The composed child is daemon-owned and \
         the daemon reaper adopts it (S1, DR-028 §Decision 2), never a detached orphan. \
         Subjects: {subjects:?}"
    );
    // Ordering: stdout drained (message) before completion — the reap follows the
    // drain, the normal S1 lifecycle preserved through the composed spawn.
    assert!(
        matches!((pos("agent.message"), pos("agent.completed")), (Some(m), Some(c)) if m < c),
        "CRITERION 5 (run-loop): `agent.message` must precede `agent.completed` — the composed \
         child's stdout is drained before the daemon reaps it (the S1 lifecycle preserved through \
         composition). Subjects: {subjects:?}"
    );
}
