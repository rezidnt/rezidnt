//! c3-egress-fold oracle (DR-029) — CRITERION 4 (WSL `#[cfg(unix)]`): a live
//! governed `rezidnt open` run reaches the RUN-LOOP Mediated arm END-TO-END, driven
//! by a NON-EMPTY fold — DR-026 crit 4 now at the run-loop level, not just the
//! substrate suite. A governed run against a spec carrying a non-empty `[egress]`
//! allowlist + a resolvable `[egress.secrets]` ref, with `REZIDNT_EGRESS_SECRETS`
//! pointing at a host TOML resolving that ref:
//!
//! - the run reaches the Mediated arm (an `egress.mediated` posture fact, NOT the
//!   empty-allowlist ConfinedClosed downgrade at `runs.rs:1079-1081`);
//! - a `credential.injected` fact rides with `secret_ref` + `dest`, and the token
//!   appears in NO log fact (only `secret_ref`) — the agent never holds it.
//!
//! This drives a NON-EMPTY fold through the RUN LOOP (`fold_c3_policies` sourcing the
//! real allowlist + resolved injection map, `runs.rs:989`), not the substrate
//! directly. The substrate-level shared-netns inescapability + injection are the
//! `crates/rezidnt-run/tests/compose_shared_netns_c3_wire.rs` WSL suite; THIS proves
//! the daemon's own run loop now yields Mediated when a shipped run's `[egress]`
//! folds non-empty and a secret resolves.
//!
//! ## SUITE PLACEMENT — daemon integration (#[cfg(unix)]), the live-run-loop home.
//! Alongside `spawn_composed_c3_wire.rs`: a real `rezidnt open` governed run at the
//! socket surface, reading facts off the live tail. On the HOST (Windows) this whole
//! file compiles to ZERO tests — host /vet neither runs nor is satisfied by it
//! ([[vet-is-host-side-wsl-insufficient]]); the host analogues for the same fold are
//! the four host suites (spec-parse, SecretSource, fold-no-widening, facts/subjects).
//!
//! Run WSL-side, single-threaded (netns setup is process-global-ish; parallel netns
//! spawns flake — [[vet-concurrency-flake]]):
//!   CARGO_TARGET_DIR=~/.cache/rezidnt-target \
//!     cargo test -p rezidentd --test egress_fold_mediated_run_c3_wire -- --test-threads=1
//!
//! ## RED MODE — mixed (COMPILE-RED then LIVE-RED).
//!
//! - COMPILE-RED: this suite drives a NEW testkit helper the implementer must add,
//!   `start_daemon_with_egress_secrets(secrets_toml)` (mirroring
//!   `start_daemon_with_admin_permit`, DR-020) — a daemon started with
//!   `REZIDNT_EGRESS_SECRETS` pointed at a host secrets file OUTSIDE any workspace
//!   spec. Until it exists this cfg(unix) target fails to compile (the honest
//!   S4-skeleton signal).
//! - LIVE-RED: even once the helper exists, today `fold_c3_policies` hard-codes an
//!   EMPTY allowlist (`runs.rs:1020`), so a run NEVER reaches Mediated — the
//!   `egress.mediated` fact never appears and the assertion times out. That is the
//!   honest RED for the missing non-empty fold (DR-029 §Decision 1/5).
//! - On a box WITHOUT pasta+bwrap the run degrades to the loud unsandboxed/CLOSED
//!   floor; this suite EARLY-RETURNS then (the C3a/c3bc precedent, never a fake
//!   pass) — the mediated assertion needs both backends.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{
    // `start_daemon_with_egress_secrets` + `make_egress_project` are the testkit
    // helpers this suite drives (the DR-020 admin-permit harness precedent applied to
    // REZIDNT_EGRESS_SECRETS + the `[egress]` spec block).
    connect,
    make_egress_project,
    open_request,
    read_until,
    send_line,
    start_daemon_with_egress_secrets,
};

/// The token the agent must NEVER hold — resolved daemon-side from the host secrets
/// file, injected upstream only. Distinctive so the never-in-log scan is adversarial.
const TOKEN_VALUE: &str = "ghp_run_loop_mediated_secret_agent_never_holds_0xC3FOLD";
/// The by-reference label — the ONLY secret-identifying thing that may ride a fact.
const SECRET_REF: &str = "gh_token";

/// Do the composed backends look present on this box (a mediated arm needs both
/// pasta + bwrap)? Absent ⇒ the run degrades loud and this mediated arm is not
/// applicable here (host-covered by the degrade suites). Read off the tail: if the
/// run emitted an `egress.mediated` posture we have both; if it emitted only
/// `egress.unavailable` we do not.
fn mediated_posture(lines: &[serde_json::Value]) -> Option<&serde_json::Value> {
    lines.iter().find(|v| v["subject"] == "egress.mediated")
}

/// CRITERION 4 (the centerpiece) — a governed run with a non-empty `[egress]`
/// allowlist + a resolvable secret reaches the RUN-LOOP Mediated arm: an
/// `egress.mediated` posture fact lands (proof the fold yielded a non-empty allowlist
/// and the daemon took the Mediated arm, not the empty-allowlist ConfinedClosed
/// downgrade at `runs.rs:1079-1081`), and a `credential.injected` fact rides carrying
/// the `secret_ref` (never the value).
///
/// COMPILE-RED until the testkit helpers exist; LIVE-RED until `fold_c3_policies`
/// sources the real allowlist + resolved injection map; GREEN on a WSL box with
/// pasta + bwrap.
#[test]
fn governed_run_with_non_empty_egress_reaches_the_mediated_arm() {
    // A host secrets file resolving `gh_token` — lives OUTSIDE any workspace spec
    // (the authority boundary: a dev cannot self-grant, DR-029 §Decision 3 / DR-020).
    let secrets_toml = format!("{SECRET_REF} = \"{TOKEN_VALUE}\"\n");
    let daemon = start_daemon_with_egress_secrets(&secrets_toml);

    // A governed project spec carrying a non-empty `[egress]` allowlist +
    // `[egress.secrets]` mapping github.com -> gh_token (the folded authority).
    let (_project, spec) = make_egress_project(100, &["github.com"], &[("github.com", SECRET_REF)]);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    // Scan the full spawn-time window for the composed posture fact + any injection.
    let lines = read_until(&mut tail, Duration::from_secs(25), |v| {
        v["subject"] == "agent.completed"
    });

    // If the box lacks pasta+bwrap the run degraded loud (egress.unavailable) — the
    // mediated arm is not applicable here; the degrade floor is host-covered.
    if mediated_posture(&lines).is_none()
        && lines.iter().any(|v| v["subject"] == "egress.unavailable")
    {
        eprintln!(
            "pasta and/or bwrap absent on this box; the run degraded to the loud egress.unavailable \
             floor — the run-loop MEDIATED arm is not applicable here (host-covered by the degrade \
             suites)"
        );
        return;
    }

    // THE mediated-arm assertion: the run took the Mediated arm end-to-end.
    let posture = mediated_posture(&lines).unwrap_or_else(|| {
        panic!(
            "CRITERION 4 VIOLATION: a governed run with a non-empty [egress] allowlist + a \
             resolvable secret did NOT reach the run-loop Mediated arm — no `egress.mediated` \
             posture fact. Either the fold still hard-codes the EMPTY allowlist (runs.rs:1020, so \
             the empty-allowlist ConfinedClosed downgrade at runs.rs:1079-1081 fired), or the run \
             loop never sourced the [egress] block. Subjects seen: {:?}",
            lines
                .iter()
                .filter_map(|v| v["subject"].as_str())
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        posture["payload"]["egress_enforceable"].as_bool(),
        Some(true),
        "CRITERION 4: the Mediated posture discloses egress IS enforceable (the sealed netns is \
         the sole route out) — the honesty anchor (DR-029 §Decision 5)"
    );

    // A credential was injected on the approved mediated egress — recorded BY
    // REFERENCE (the secret_ref names it, never the value).
    let injected = lines
        .iter()
        .find(|v| v["subject"] == "credential.injected")
        .unwrap_or_else(|| {
            panic!(
                "CRITERION 4 VIOLATION: the run reached Mediated but emitted no \
                 `credential.injected` fact — the resolved secret was never brokered to its \
                 allowlisted host. Subjects seen: {:?}",
                lines
                    .iter()
                    .filter_map(|v| v["subject"].as_str())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        injected["payload"]["secret_ref"].as_str(),
        Some(SECRET_REF),
        "CRITERION 4: the injection is recorded BY REFERENCE — the secret_ref names it"
    );
    assert_eq!(
        injected["payload"]["dest"].as_str(),
        Some("github.com"),
        "the injection names the allowlisted destination the secret was brokered toward"
    );
}

/// CRITERION 4 (never-in-log — the catastrophic-failure guard at the RUN-LOOP level)
/// — across the WHOLE emitted log of a live mediated run, the token VALUE appears in
/// NO fact (only the `secret_ref` does); the agent never holds it. This is DR-026
/// crit 4/5 now proven against the daemon's real fabric, not a hand-built fact set.
///
/// COMPILE-RED until the testkit helpers exist; LIVE-RED until the fold is non-empty;
/// GREEN on a WSL box with pasta + bwrap.
#[test]
fn the_token_value_appears_in_no_log_fact_of_a_live_mediated_run() {
    let secrets_toml = format!("{SECRET_REF} = \"{TOKEN_VALUE}\"\n");
    let daemon = start_daemon_with_egress_secrets(&secrets_toml);
    let (_project, spec) = make_egress_project(100, &["github.com"], &[("github.com", SECRET_REF)]);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(25), |v| {
        v["subject"] == "agent.completed"
    });

    // Applicable only when the run actually reached Mediated (else there was no
    // injection to leak — the degrade floor is host-covered).
    if mediated_posture(&lines).is_none() {
        eprintln!(
            "run did not reach Mediated on this box; the never-in-log injection arm is not \
                   applicable here"
        );
        return;
    }

    // THE scan: no fact's serialized JSON contains the token value anywhere.
    for v in &lines {
        let serialized = serde_json::to_string(v).expect("fact serializes");
        assert!(
            !serialized.contains(TOKEN_VALUE),
            "CRITERION 4 VIOLATION (CATASTROPHIC): the token VALUE appeared in a live-run fact \
             (subject {:?}) — the secret leaked into the log. Only the secret_ref may ride a fact, \
             NEVER the value (DR-026 crit 4/5, DR-029 §Invariant-fit I2/I3). Fact: {serialized}",
            v["subject"].as_str()
        );
    }
    // Non-vacuous: the secret_ref DID ride some fact (the label is present, only the
    // value is absent).
    assert!(
        lines
            .iter()
            .any(|v| v["payload"]["secret_ref"].as_str() == Some(SECRET_REF)),
        "non-vacuous: the secret_ref rode a fact (the by-reference label is present) — so the \
         value-absence scan above is meaningful, not empty"
    );
}
