//! DR-035 sub-slice 1 (`escalation-ttl`) — I6 INTERROGABILITY of EXPIRY. The
//! oracle's `permit_ttl_fold.rs` pins the pure filter (an expired resolution is
//! not applied); THIS board pins DR-035 §Invariants I6: an expired resolution
//! that no longer applies must be EXPLAINABLE, never a silent vanish. When a
//! resolution's TTL has lapsed and the next ask re-escalates, `gate_explain`
//! (the I6 "why" tool) must surface "not applied: resolution X expired at
//! deadline T → re-escalated" so a reader tells an expiry-driven re-escalation
//! from an ordinary first-time escalate, and can chain to the operator who set
//! the (now-lapsed) resolution.
//!
//! This MIRRORS `resolve_permit_interrogability.rs` (which proves an APPLIED
//! resolution surfaces `resolved_from`): the applied and expired cases are the
//! two honest outcomes of a resolution, and BOTH are interrogable — never a
//! silent coercion (I6, DR-033 posture preserved and extended).
//!
//! NOT `#![cfg(unix)]`-gated: the EMPTY permit config escalates the fresh ask
//! (no exec/`/bin/sh`), so this runs host-side on the /vet gauntlet.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, EventParts, SourceId, Subject};
use serde_json::{Value, json};
use time::OffsetDateTime;
use ulid::Ulid;

const RUN: &str = "01DR035TTLINTERR0GAB000R1";
const ESCALATED_REQ: &str = "01DR035TTLINTERR0GESCR0R1";
const OPERATOR_ID: &str = "0badc0de";

fn core_empty_permit(badge: &Badge) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_permit_config(PermitConfig::from_specs(vec![]));
    (dir, Arc::new(core))
}

/// Mint an event whose ENVELOPE ULID timestamp is exactly `at` — so the
/// resolution's `resolved_at_ms` (T0) is a CHOSEN past instant, well before the
/// fresh ask's `now()` deadline. Same controlled-clock technique the fold oracle
/// uses (`Ulid::from_datetime`), with no `sleep`.
fn ev_at(subject: &str, payload: Value, at: OffsetDateTime) -> Event {
    let id = Ulid::from_datetime(at.into());
    Event::from_parts(EventParts {
        id,
        ts: at,
        v: 1,
        source: SourceId::new("rezidnt-run"),
        workspace: None,
        subject: Subject::new(subject),
        correlation: id,
        causation: None,
        payload,
    })
    .expect("test event under 32KiB")
}

/// Seed the escalation → time-boxed resolution history for `Bash` on `RUN`,
/// anchored FAR in the past (2020) with a SMALL ttl, so by the time the test's
/// fresh ask lands at `now()` the deadline has long lapsed and the resolution is
/// EXPIRED.
fn seed_expired_resolution(core: &McpCore) {
    // 2020-01-01T00:00:00Z — years before any real test run.
    let t0 = OffsetDateTime::from_unix_timestamp(1_577_836_800).expect("valid past instant");
    for e in [
        ev_at(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
            t0,
        ),
        ev_at(
            "permit.requested",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "action": "tool.invoke", "target": {"tool": "Bash"}}),
            t0,
        ),
        ev_at(
            "permit.escalated",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "policy_ref": {"hash": "e5ca1a7e", "bytes": 8, "mime": "application/json"}, "reason": "routed to a human"}),
            t0,
        ),
        ev_at(
            "permit.resolved",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "action": "tool.invoke", "target": {"tool": "Bash"}, "decision": "allow", "operator_badge_id": OPERATOR_ID, "reason": "operator approved, time-boxed", "ttl_ms": 60_000}),
            t0,
        ),
    ] {
        core.fabric().publish(e).expect("publish fixture event");
    }
}

/// I6 — after a TTL lapses, the fresh ask re-escalates (empty config → ask), and
/// `gate_explain` surfaces the EXPIRY: the re-escalation is not a silent vanish.
/// The `expired_resolution` note names WHICH resolution lapsed
/// (`resolved_from` == the resolution's request_id), its deadline, and chains to
/// the operator (WHO) + reason (WHY). This is the DR-035 §I6 obligation the fold
/// oracle does not encode.
#[tokio::test]
async fn gate_explain_surfaces_the_expired_resolution_on_re_escalation() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    seed_expired_resolution(&core);

    // Fresh ask for the SAME action at `now()` — the resolution has long expired,
    // so the ledger-check skips it and the empty config re-escalates.
    let _ = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;

    let explain =
        util::tool_payload(&util::tool_call(&core, 2, "gate_explain", json!({ "run": RUN })).await);

    // The re-escalation is honest: verdict is `ask`, never a coerced allow.
    assert_eq!(
        explain["verdict"],
        json!("ask"),
        "an expired resolution re-escalates — never a silent grant (I6, DR-035 §Decision 1)"
    );

    // And the expiry is EXPLAINABLE, not a silent vanish (the I6 crux).
    let note = explain.get("expired_resolution").unwrap_or_else(|| {
        panic!(
            "gate_explain must EXPLAIN the expiry on a re-escalation — a lapsed override is \
             interrogable, never a silent vanish (I6, DR-035 §Invariants) — got {explain:#}"
        )
    });
    assert_eq!(
        note["resolved_from"],
        json!(ESCALATED_REQ),
        "the expiry note names WHICH resolution lapsed (chains to its request_id, DR-035 §I6)"
    );
    assert_eq!(
        note["ttl_ms"],
        json!(60_000),
        "the note carries the resolution's own TTL"
    );
    assert_eq!(
        note["operator_badge_id"],
        json!(OPERATOR_ID),
        "the expiry note chains to WHO set the (now-lapsed) resolution (I6)"
    );
    assert_eq!(
        note["reason"],
        json!("operator approved, time-boxed"),
        "the expiry note chains to WHY the operator resolved (I6)"
    );
    // The deadline is the anchor + ttl; it is genuinely before the fresh-ask time.
    let deadline = note["deadline_ms"]
        .as_u64()
        .expect("deadline_ms is a number");
    let request_ms = note["request_ms"].as_u64().expect("request_ms is a number");
    assert!(
        request_ms > deadline,
        "the fresh ask landed PAST the deadline — that is why it re-escalated \
         (deadline {deadline}, request {request_ms})"
    );
}

/// The distinction is load-bearing: an ordinary FIRST-TIME escalate (no
/// resolution in play at all) carries NO `expired_resolution` note, so a reader
/// tells an expiry-driven re-escalation from a plain one. The negative control
/// that makes the positive test meaningful — a phantom note would misreport why.
#[tokio::test]
async fn a_plain_escalate_carries_no_expiry_note() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    core.fabric()
        .publish(
            Event::new(
                SourceId::new("rezidnt-run"),
                None,
                Subject::new("agent.spawned"),
                Ulid::new(),
                None,
                1,
                json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
            )
            .expect("event"),
        )
        .expect("publish spawned");

    // First-ever ask for this action — no resolution exists, empty config → ask.
    let _ = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;

    let explain =
        util::tool_payload(&util::tool_call(&core, 2, "gate_explain", json!({ "run": RUN })).await);
    assert_eq!(
        explain["verdict"],
        json!("ask"),
        "a plain first-time escalate"
    );
    assert!(
        explain.get("expired_resolution").is_none() || explain["expired_resolution"].is_null(),
        "a plain escalate carries NO expiry note — a phantom note would misreport why the \
         request escalated (I6) — got {explain:#}"
    );
}
