//! c3-op-secrets oracle (DR-030) — CRITERION 5 (WSL `#[cfg(unix)]`, OWNER-GATED,
//! LIVE `op`): a live governed `rezidnt open` run whose `[egress.secrets]` maps an
//! allowlisted host to an `op://vault/item/field` REFERENCE reaches the run-loop
//! Mediated arm with the OP-RESOLVED token injected UPSTREAM — the agent never holds
//! it (only `secret_ref = "op://…"` rides a fact). This is DR-029's crit-4 mediated
//! run (`egress_fold_mediated_run_c3_wire.rs`) sourcing an `op://` ref instead of a
//! host-file label — the op backend proven END-TO-END through the daemon's real fold
//! + run loop.
//!
//! ## OWNER-GATED — needs the OWNER's LIVE `op` setup. This arm requires:
//!   - the real 1Password `op` CLI installed (`op --version` succeeds), AND
//!   - a real, vault-scoped `OP_SERVICE_ACCOUNT_TOKEN` in the daemon's env whose
//!     vault holds the item the `op://` ref names.
//! Absent EITHER, the test EARLY-RETURNS (an honest SKIP) — exactly the pasta/bwrap
//! availability-gate precedent (DR-025), never a fake pass. Marked clearly: a CI box
//! without the owner's live op/token SKIPS; only the owner's box proves it green.
//!
//! ## SUITE PLACEMENT — daemon integration (`#[cfg(unix)]`), the live-run-loop home,
//! alongside `egress_fold_mediated_run_c3_wire.rs` (DR-029). On the HOST (Windows)
//! this whole file compiles to ZERO tests ([[vet-is-host-side-wsl-insufficient]]);
//! the op backend's host analogues are the four host suites (op resolve, composite
//! dispatch, degrade taxonomy, leak discipline).
//!
//! Run WSL-side, single-threaded (netns setup is process-global-ish; parallel netns
//! spawns flake — [[vet-concurrency-flake]]), on the owner's box with op + a token:
//!   OP_SERVICE_ACCOUNT_TOKEN=<owner token> \
//!   CARGO_TARGET_DIR=~/.cache/rezidnt-target \
//!     cargo test -p rezidentd --test egress_fold_op_mediated_run_c3_wire -- --test-threads=1
//!
//! ## RED MODE — mixed (COMPILE-RED then LIVE-RED/SKIP).
//!   - COMPILE-RED: drives a NEW testkit helper the implementer must add,
//!     `start_daemon_with_op_secrets()` — a daemon started WITHOUT
//!     `REZIDNT_EGRESS_SECRETS` but WITH the real `OP_SERVICE_ACCOUNT_TOKEN`
//!     inherited from the test's env (so the op backend auths) and the daemon's op
//!     binary resolvable on PATH. Until it exists this cfg(unix) target fails to
//!     compile (the honest S4-skeleton signal). Plus `op_ref_available()` gating
//!     helper.
//!   - LIVE-RED: even with the helper, until the daemon's fold DISPATCHES an `op://`
//!     ref to the `OpSecretSource` (the CompositeSecretSource in `fold_c3_policies`),
//!     the op:// ref is treated as a plain host-file label, resolves to Ok(None), and
//!     the mapping DROPS — no `credential.injected` for the op host, the mediated-arm
//!     assertion fails. That is the honest RED for the missing op dispatch.
//!   - SKIP: on a box without op+token, or without pasta+bwrap, the run cannot reach
//!     a live op-injected Mediated arm; the test early-returns (never a fake pass).
// The oracle module-doc's nested bullet lists trip clippy's `doc_lazy_continuation`
// under host `-D warnings` ([[clippy-doc-lazy-continuation-trap]] — the crate doc is
// linted even where cfg(unix) excludes the body, and a false `#![cfg(unix)]` would
// short-circuit a later inner attr, so this allow MUST precede it). Lint-only
// accommodation; no assertion or prose is changed (flagged for /debrief).
#![allow(clippy::doc_lazy_continuation)]
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{
    connect,
    make_egress_project,
    // `start_daemon_with_op_secrets` + `op_ref_available` are the NEW testkit gates
    // this suite drives (the DR-029 egress-secrets harness applied to op auth).
    op_ref_available,
    open_request,
    read_until,
    send_line,
    start_daemon_with_op_secrets,
};

/// The `op://` reference the spec declares — a NAME (vault/item/field), never a
/// value; it is the ONLY secret-identifying thing that may ride a fact. On the
/// owner's box this must name a real item the service-account's vault holds.
/// Overridable via `REZIDNT_TEST_OP_REF` so the owner points it at a live item
/// without editing the suite.
fn op_ref() -> String {
    std::env::var("REZIDNT_TEST_OP_REF")
        .unwrap_or_else(|_| "op://rezidnt-test/github-token/credential".to_string())
}

/// Do the composed backends look present (a mediated arm needs pasta + bwrap)? Read
/// off the tail exactly like the DR-029 sibling suite.
fn mediated_posture(lines: &[serde_json::Value]) -> Option<&serde_json::Value> {
    lines.iter().find(|v| v["subject"] == "egress.mediated")
}

/// CRITERION 5 (the centerpiece) — a governed run with a non-empty `[egress]`
/// allowlist + an `op://` `[egress.secrets]` ref reaches the RUN-LOOP Mediated arm
/// with the OP-RESOLVED token injected: an `egress.mediated` posture fact lands, and
/// a `credential.injected` fact rides carrying the `op://` REF as its `secret_ref`
/// (never the value). Skipped honestly when op/token or pasta/bwrap are absent.
///
/// COMPILE-RED until the testkit helpers exist; LIVE-RED until the fold dispatches
/// op:// to OpSecretSource; SKIP without the owner's live op + token; GREEN on the
/// owner's WSL box with op + a token + pasta + bwrap.
#[test]
fn governed_run_with_an_op_secret_reaches_the_mediated_arm_injected() {
    // OWNER GATE: skip honestly unless a live op + a service-account token are
    // present (the pasta/bwrap availability-gate precedent, never a fake pass).
    if !op_ref_available() {
        eprintln!(
            "SKIP: the live `op` CLI and/or OP_SERVICE_ACCOUNT_TOKEN are absent — the op-injected \
             mediated arm needs the OWNER's live 1Password setup (op installed + a vault-scoped \
             service-account token whose vault holds the item the op:// ref names). Host-covered by \
             the four host suites (op resolve, dispatch, degrade taxonomy, leak discipline)."
        );
        return;
    }

    // A daemon whose env carries the real OP_SERVICE_ACCOUNT_TOKEN (inherited from
    // this test's env) and whose op binary is on PATH — NO REZIDNT_EGRESS_SECRETS
    // (this is the op backend, not the host-file one).
    let daemon = start_daemon_with_op_secrets();

    // A governed spec: github.com allowlisted, mapped to an op:// REFERENCE (the
    // folded authority; the value resolves daemon-side via `op read`, pre-seal).
    let op = op_ref();
    let (_project, spec) = make_egress_project(100, &["github.com"], &[("github.com", &op)]);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(30), |v| {
        v["subject"] == "agent.completed"
    });

    // If the box lacks pasta+bwrap the run degraded loud — the mediated arm is not
    // applicable here (host-covered by the degrade suites).
    if mediated_posture(&lines).is_none()
        && lines.iter().any(|v| v["subject"] == "egress.unavailable")
    {
        eprintln!(
            "SKIP: pasta and/or bwrap absent — the run degraded to the loud egress.unavailable \
             floor; the run-loop MEDIATED arm is not applicable here."
        );
        return;
    }

    // THE mediated-arm assertion: the run took the Mediated arm end-to-end with the
    // op-resolved secret.
    let posture = mediated_posture(&lines).unwrap_or_else(|| {
        panic!(
            "CRITERION 5 VIOLATION: a governed run with an op:// [egress.secrets] ref did NOT reach \
             the run-loop Mediated arm — no `egress.mediated` posture. Either the fold does not \
             DISPATCH op:// to OpSecretSource (so the ref dropped as an unresolvable host-file \
             label), or the [egress] block never folded. Subjects seen: {:?}",
            lines
                .iter()
                .filter_map(|v| v["subject"].as_str())
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        posture["payload"]["egress_enforceable"].as_bool(),
        Some(true),
        "CRITERION 5: the Mediated posture discloses egress IS enforceable — the honesty anchor"
    );

    // A credential was injected on the approved mediated egress — recorded BY
    // REFERENCE, and the reference is the op:// REF (proof the op backend resolved
    // it, not the host-file one).
    let injected = lines
        .iter()
        .find(|v| v["subject"] == "credential.injected")
        .unwrap_or_else(|| {
            panic!(
                "CRITERION 5 VIOLATION: the run reached Mediated but emitted no \
                 `credential.injected` fact — the op-resolved secret was never brokered to its \
                 allowlisted host (the op:// ref likely DROPPED — check op auth/resolution). \
                 Subjects seen: {:?}",
                lines
                    .iter()
                    .filter_map(|v| v["subject"].as_str())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        injected["payload"]["secret_ref"].as_str(),
        Some(op.as_str()),
        "CRITERION 5: the injection is recorded BY REFERENCE and the reference is the op:// REF — \
         proof the op backend resolved it and the ref (a NAME) rides the fact, never the value"
    );
    assert_eq!(
        injected["payload"]["dest"].as_str(),
        Some("github.com"),
        "the injection names the allowlisted destination the op-resolved secret was brokered toward"
    );
}

/// CRITERION 5 (never-in-log — the agent never holds the op-resolved token) — across
/// the WHOLE emitted log of a live op-injected mediated run, NO fact carries a token
/// VALUE; only the `op://` `secret_ref` rides. The token resolves daemon-side and is
/// injected upstream on the plaintext the agent never sees.
///
/// Because the token value is only known to the owner's live 1Password (this suite
/// never learns it), the never-in-log guard is expressed as: NO fact carries an
/// `Authorization`-shaped bearer value AND the `credential.injected` fact carries the
/// op:// ref (not a resolved value). The substrate-level two-sided capture proving
/// upstream-received-vs-agent-absent is the `egress_mediation_c3bc.rs` WSL suite; THIS
/// proves the daemon's run loop rode only the ref onto the fabric.
///
/// COMPILE-RED until the helpers exist; SKIP without the owner's live op + token.
#[test]
fn no_fact_of_a_live_op_mediated_run_carries_a_resolved_token_value() {
    if !op_ref_available() {
        eprintln!(
            "SKIP: the live op / OP_SERVICE_ACCOUNT_TOKEN are absent — needs the owner's live setup."
        );
        return;
    }

    let daemon = start_daemon_with_op_secrets();
    let op = op_ref();
    let (_project, spec) = make_egress_project(100, &["github.com"], &[("github.com", &op)]);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(30), |v| {
        v["subject"] == "agent.completed"
    });

    if mediated_posture(&lines).is_none() {
        eprintln!(
            "SKIP: run did not reach Mediated on this box; the never-in-log arm is n/a here."
        );
        return;
    }

    // The op:// ref (a NAME) rides — but the ref is NOT a secret value. Assert no
    // fact leaks a token by shape: no fact payload carries a bearer/authorization
    // VALUE, and no fact carries the service-account token from the daemon env.
    let sa_token = std::env::var("OP_SERVICE_ACCOUNT_TOKEN").unwrap_or_default();
    for v in &lines {
        let serialized = serde_json::to_string(v).expect("fact serializes");
        if !sa_token.is_empty() {
            assert!(
                !serialized.contains(&sa_token),
                "CRITERION 5 VIOLATION (CATASTROPHIC): the SERVICE-ACCOUNT TOKEN appeared in a \
                 live-run fact (subject {:?}) — the daemon-side auth token must ride NO fact \
                 (DR-030 §Decision 3). Fact: {serialized}",
                v["subject"].as_str()
            );
        }
        // The op-resolved token value is unknown to this suite, but an Authorization
        // header value would be a leak regardless — no fact should carry one inline.
        assert!(
            !serialized.to_ascii_lowercase().contains("authorization"),
            "CRITERION 5 VIOLATION: a live-run fact (subject {:?}) carried an `authorization` field \
             — the injected credential rides upstream only, never onto the fabric (DR-026 crit 4/5). \
             Fact: {serialized}",
            v["subject"].as_str()
        );
    }

    // Non-vacuous: the op:// ref DID ride a fact as the by-reference secret_ref.
    assert!(
        lines
            .iter()
            .any(|v| v["payload"]["secret_ref"].as_str() == Some(op.as_str())),
        "non-vacuous: the op:// ref rode a fact as the by-reference secret_ref — so the \
         value-absence scan above is meaningful, not empty"
    );
}
