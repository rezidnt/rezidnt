//! DR-055 ORACLE — `agent.spawned.model?` reaches the FACT (DR-055 §Decision
//! 3; ontology `agent.spawned.model?` bullet, S1 baseline). The real gap this
//! judges, verified against the tree at cut time: `AgentSpec.model` (DR-048
//! slice A) reaches the spawn argv via `push_model_flag`
//! (`crates/rezidnt-run/src/spawner.rs`) and rides the spec's vet preimage,
//! but `launch_agent`'s `spawned_payload` build (`bins/rezidentd/src/runs.rs`)
//! never writes it to `agent.spawned` — so the model axis is not log-derivable
//! from the run's own facts, which is fatal to a feature whose point is
//! comparing models.
//!
//! Pinned at the SAME level the `role?` emit was pinned
//! (`tests/spawn_role_emit.rs`, the DR-016 SP4a precedent this file mirrors
//! line for line): a real end-to-end daemon spawn of the gated project,
//! reading the `agent.spawned` payload off the live tail.
//!
//! API SHAPE THE IMPLEMENTER MUST MATCH: in `launch_agent`, alongside the
//! `role` emit, insert `model` onto `spawned_payload` iff `agent.model` is
//! `Some` — verbatim from `AgentSpec.model` (ontology: "the flag the harness
//! was handed", REQUESTED, never any vendor acknowledgment). ABSENT inserts
//! NOTHING (the `role`/`pep` `if let Some` gate — absence is honest, never
//! `model: ""` and never a synthesized vendor-default name).
//!
//! RED MODE (against the tree at cut time — session 33, post-`bcd0db9`):
//! ASSERT-RED on the positive leg — `launch_agent` emits no `model` key today,
//! so `spawned["payload"]["model"]` is JSON null and the assertion fails. The
//! two omit-legs pass trivially today (GREEN-BY-ABSENCE, flagged here for the
//! auditor per the dr006/dr050 oracle-honesty precedent) and become
//! load-bearing the day the emit lands: they are what forbids a synthesized
//! default model and a synthesized `trial` on ordinary spawns.
//!
//! WSL/unix-only (`#![cfg(unix)]`): the host gauntlet compiles this to
//! nothing; run it WSL-side.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{connect, make_gated_project, open_request, read_until, send_line, start_daemon};

/// Insert a `model = "<model>"` line into the gated spec's `[[agent]]` block,
/// right after the `worktree = "auto"` line (the same stable anchor
/// `spawn_role_emit.rs` uses). `AgentSpec.model` is the DR-048 slice-A field;
/// the stub harness ignores the `--model` argv this adds.
fn with_model(spec: &str, model: &str) -> String {
    let anchor = "worktree = \"auto\"\n";
    assert!(
        spec.contains(anchor),
        "test bug: gated spec lost its worktree anchor"
    );
    spec.replace(anchor, &format!("{anchor}model = \"{model}\"\n"))
}

/// POSITIVE LEG (ASSERT-RED today) — a launch whose spec declares
/// `model = "model-oracle-a"` emits `agent.spawned` carrying it VERBATIM, and
/// the same fact carries `agent` and `harness` — so all THREE variant axes
/// (agent, harness, model) ride one spawn fact, which is the ground on which
/// DR-055 REFUSED `variant?` as a synonym (DR-006). If any axis went missing,
/// the refusal's premise would be false.
#[test]
fn spawn_records_declared_model_on_agent_spawned_with_all_three_axes() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);
    let spec = with_model(&spec, "model-oracle-a");

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
        spawned["payload"]["model"], "model-oracle-a",
        "the declared model rides agent.spawned VERBATIM (DR-055 §Decision 3; \
         the model as REQUESTED — the flag the harness was handed) — got {spawned:#}"
    );
    assert_eq!(
        spawned["payload"]["agent"], "impl",
        "axis 1 of the variant triple rides the same fact"
    );
    assert_eq!(
        spawned["payload"]["harness"], "claude-code",
        "axis 2 of the variant triple rides the same fact — with model?, \
         agent.spawned carries all three axes, the variant? refusal's ground"
    );
}

/// HONESTY LEG (GREEN-BY-ABSENCE today; load-bearing after the emit lands) —
/// a model-less spec emits `agent.spawned` with NO `model` key at all. Absence
/// is the honest "no model declared => the harness's own default ran"; naming
/// the vendor's default would be a present claim of knowledge rezidnt does not
/// hold (ontology `model?`; DR-012, the exact mirror of `role?` never
/// synthesizing "contributor").
#[test]
fn spawn_omits_model_when_absent_never_synthesized() {
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
        spawned["payload"].get("model").is_none(),
        "a model-less spec omits `model` on agent.spawned — absence is honest, \
         never `model: \"\"` or a synthesized vendor default (DR-012): {spawned:#}"
    );
    // Sanity: the governed fields ARE present on this same spawn, so the
    // omission is model-specific, not a dropped payload (the role board's leg).
    assert_eq!(
        spawned["payload"]["bare"], true,
        "the governed spawn still records bare (the omit is model-specific)"
    );
}

/// TRIAL HONESTY LEG (GREEN-BY-ABSENCE today; load-bearing once `open_trial`
/// lands) — an ORDINARY spawn (this whole project opens outside any trial)
/// carries NO `trial` key: a non-trial run is not a sample of anything, and
/// `trial?` is present IFF the spawn came through `open_trial`'s server-side
/// matrix expansion (ontology `trial?`; DR-012 — never synthesized, so every
/// non-trial `agent.spawned` payload stays byte-identical to the pre-DR-055
/// shape).
#[test]
fn ordinary_spawns_never_carry_a_trial_key() {
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
        spawned["payload"].get("trial").is_none(),
        "an ordinary spawn is not a trial sample — `trial` must be ABSENT, \
         never synthesized (DR-012; ontology trial? bullet): {spawned:#}"
    );
}
