//! SP4b producer-side wiring proof (DR-017 §Decision 2): when a lead agent's
//! badge is NARROWED for a role-scoped spawn, the daemon records the capability
//! edge as a durable `permit.delegated` fact BEFORE `agent.spawned` — the
//! replayable capability-chain fact the reducer folds (I3). This is the
//! end-to-end assertion the SP4 exit demo needs: a real daemon spawn emits the
//! delegation edge on the live tail, not just the spawn fact.
//!
//! Producer scope (this file's job): the EMIT path is genuinely exercised and
//! asserted. The FOLD is pinned by `crates/rezidnt-state/tests/permit_delegation.rs`;
//! the crypto by `crates/rezidnt-run/tests/badge_macaroon.rs`. Here we prove the
//! daemon actually PUTS the fact on the log with the right edge + caveat shape.
//!
//! Delegation trigger: the spec's `role` (SP4a — the sub-agent narrowing signal,
//! DR-016 §Decision 3). A role-declared spawn ATTENUATES the run's base (lead)
//! badge with a `Role` caveat (DR-018 §(a): a real `base_badge.attenuate(role)`
//! at the daemon boundary — NOT a root-key re-mint of a `<run>:role:<role>`
//! identifier) and emits the edge; a roleless spawn emits NO delegation (the
//! honesty leg — no consumer-less noise).
//!
//! ## DR-018 §(a) mechanism note (what this black-box test can and cannot pin)
//! DR-018 flips `Macaroon::badge_id()` to `hex(blake3(sig)[..8])` (sig-derived)
//! and replaces the producer's root-key re-mint with a true offline `attenuate`.
//! The CRYPTO proof that `attenuate` under a SHARED identifier yields distinct
//! sig-derived ids (parent ≠ child WHILE identifier is preserved) is pinned in
//! `crates/rezidnt-run/tests/badge_macaroon.rs`
//! (`attenuate_yields_distinct_badge_ids_under_a_shared_identifier`) — that board
//! holds the root key, so it can reconstruct the sigs. Here the daemon mints its
//! root key internally (`RootKey::mint`) and the run identifier is a per-run
//! ULID, so this test canNOT recompute the exact ids; it pins the fact-shape the
//! producer must emit (distinct, non-empty, sig-derived-SHAPE ids + verbatim role
//! caveat + no token leak). It must NOT assert around the sig mechanism it cannot
//! observe (test honesty: no theater). The `assert_ne!(parent, child)` below is
//! REQUIRED under both schemes and stays; the sig-derivation discriminator is the
//! crypto board's job.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{connect, make_gated_project, open_request, read_until, send_line, start_daemon};

/// Insert a `role = "<role>"` line right after the `worktree = "auto"` anchor
/// (mirrors `spawn_role_emit::with_role`).
fn with_role(spec: &str, role: &str) -> String {
    let anchor = "worktree = \"auto\"\n";
    assert!(
        spec.contains(anchor),
        "test bug: gated spec lost its worktree anchor"
    );
    spec.replace(anchor, &format!("{anchor}role = \"{role}\"\n"))
}

/// A role-declared spawn emits `permit.delegated` capturing the capability edge:
/// a `parent_badge_id` (the run's base/lead badge) DISTINCT from the
/// `child_badge_id` (the role-narrowed badge the sub-agent runs under), and
/// `added_caveats` carrying the tagged `Role` caveat verbatim (I3).
#[test]
fn role_spawn_emits_permit_delegated_edge() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);
    let spec = with_role(&spec, "reviewer");

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "permit.delegated"
    });

    let delegated = lines
        .iter()
        .find(|v| v["subject"] == "permit.delegated")
        .expect("read_until stopped on permit.delegated");
    let payload = &delegated["payload"];

    assert!(
        payload["run"].as_str().is_some(),
        "the delegation is keyed on the run so it folds onto the dossier (I3): {delegated:#}"
    );
    let parent = payload["parent_badge_id"]
        .as_str()
        .expect("parent_badge_id present");
    let child = payload["child_badge_id"]
        .as_str()
        .expect("child_badge_id present");
    assert!(
        !parent.is_empty() && !child.is_empty(),
        "both ends of the capability edge are loggable badge ids: {delegated:#}"
    );
    // DR-018 §(a): badge_id is `hex(blake3(sig)[..8])` — an 8-byte prefix, so
    // exactly 16 lowercase-hex chars. Shape is unchanged from DR-005; only the
    // pre-image moved (the sig, hashed, not the identifier). The token itself is
    // NEVER on the fabric (I2/§12).
    for (which, id) in [("parent", parent), ("child", child)] {
        assert_eq!(
            id.len(),
            16,
            "{which}_badge_id is an 8-byte hex prefix (DR-018 sig-derived shape): {delegated:#}"
        );
        assert!(
            id.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{which}_badge_id is lowercase hex: {delegated:#}"
        );
    }
    assert_ne!(
        parent, child,
        "parent (base/lead badge) and child (role-attenuated badge) are DISTINCT \
         ends of the edge — a real delegation, not a self-loop. Under DR-018 §(a) \
         the child is `base_badge.attenuate(role)` (shared identifier, re-keyed \
         sig), so the ids differ by their sig chains, not their identifiers: {delegated:#}"
    );
    assert_eq!(
        payload["added_caveats"],
        serde_json::json!([{ "kind": "role", "role": "reviewer" }]),
        "the narrowing Role caveat folds verbatim — the tagged first-party shape \
         the reducer + crypto share (DR-017; never re-derived, I3): {delegated:#}"
    );
    // The badge token is NEVER on the fabric — only the loggable ids (I2/§12).
    assert!(
        !delegated.to_string().contains("REZIDNT_BADGE"),
        "no badge token leaks onto the fabric (I2)"
    );
}

/// The honesty leg (I3, no consumer-less noise): a ROLELESS spawn emits NO
/// `permit.delegated` fact — there was no narrowing, so there is no delegation
/// edge to record. We reach `agent.spawned` and assert no delegation preceded it.
#[test]
fn roleless_spawn_emits_no_delegation() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    // Read through to agent.spawned; a delegation, if wrongly emitted, would have
    // landed BEFORE it (the emit is ordered before agent.spawned).
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
    });
    assert!(
        !lines.iter().any(|v| v["subject"] == "permit.delegated"),
        "a roleless spawn narrows nothing — no permit.delegated edge is recorded \
         (I3: no consumer-less noise); saw {lines:#?}"
    );
}
