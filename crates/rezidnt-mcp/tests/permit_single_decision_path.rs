//! SP2 oracle — §5 criterion 7: the SINGLE decision path (I3, DR-013 decision
//! 1). MCP and socket permission requests with identical inputs must produce
//! BYTE-IDENTICAL decision facts — the proof that the socket handler does NOT
//! reimplement the PDP but shares one transport-neutral `decide_permit`
//! entrypoint (design §2). Two on-log facts per permission come from exactly
//! one code path, or the transports drift and one lies.
//!
//! RED MODE: **compile-red** against `rezidnt_mcp::{PermitRequest, PermitOutcome,
//! Decision}` and `McpCore::decide_permit(...)` — the transport-neutral
//! entrypoint DR-013 ratifies but that does NOT exist yet. The crate fails to
//! compile until the extraction lands. Then it is **assert-red** until both the
//! MCP JSON-RPC adapter and (by construction) the socket handler route through
//! the one method, so the emitted facts match.
//!
//! The `sp_wire_aggregate_deny` golden fixture (handoff residual — a thin
//! green-lock in the generic fixture-replay) is FOLDED IN here (criterion 7's
//! explicit ask): its committed `permit.denied` payload is the golden shape the
//! live single decision path must reproduce, tying the fixture to the SP2 path
//! rather than leaving it an untethered replay.

mod util;

use std::path::PathBuf;

use rezidnt_mcp::{
    // NB: the transport-neutral PDP entrypoint + its request/outcome types.
    // These are the SP2 extraction (design §2, DR-013 decision 1) and do NOT
    // exist yet — this `use` is the compile-red anchor.
    Decision,
    McpCore,
    PermitConfig,
    PermitRequest,
};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

/// The permit config that reproduces the `sp_wire_aggregate_deny` fixture's
/// decision axis: a `path-scope` native that denies an out-of-scope path. The
/// fixture denied `/etc/shadow` with reason "path /etc/shadow outside allowed
/// scope" — the same native + params reproduce that verdict deterministically.
fn path_scope_deny_config() -> PermitConfig {
    PermitConfig::natives(&[("path-scope", json!({"allow": ["src/checkout/**"]}))])
}

/// Seed a run onto the core's log so `decide_permit` can fold it (I3).
fn seed_run(core: &McpCore, run: &str) {
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        json!({"run": run, "agent": "impl", "harness": "claude-code"}),
    )
    .expect("spawned envelope");
    core.fabric().publish(spawned).expect("publish spawned");
}

/// The last `permit.denied` payload on the core's log, with the volatile
/// envelope fields stripped so the PAYLOAD (the decision fact proper) is what is
/// compared. `id`/`ts`/`correlation`/`causation` are per-emit and per-clock, so
/// they are not part of "the same decision fact" — the payload is.
fn last_denied_payload(core: &McpCore) -> Value {
    util::log_events(core)
        .into_iter()
        .rfind(|e| e.subject.as_str() == "permit.denied")
        .map(|e| e.payload().clone())
        .expect("a permit.denied fact on the log")
}

/// The last `permit.requested` payload on the core's log. Unlike the decision
/// fact, this one is NOT claimed byte-identical across transports: it carries
/// transport-local caller identity (`badge_id`), present on MCP and absent on
/// the socket by §3. Envelope fields are stripped for the same reason as above.
fn last_requested_payload(core: &McpCore) -> Value {
    util::log_events(core)
        .into_iter()
        .rfind(|e| e.subject.as_str() == "permit.requested")
        .map(|e| e.payload().clone())
        .expect("a permit.requested fact on the log")
}

/// §5 criterion 7 (the headline): identical inputs through the MCP adapter and
/// through the transport-neutral `decide_permit` produce BYTE-IDENTICAL decision
/// facts. `policy_ref`/`evidence_ref` are content-addressed (blake3 of the same
/// policy/evidence bytes), so a shared code path yields identical CAS hashes;
/// `reason`/`request_id`/`run` are identical by input. A DIVERGENCE here is the
/// fork DR-013 decision 1 forbids.
///
/// COMPILE-RED on `PermitRequest`/`decide_permit`; then ASSERT-RED until both
/// callers share the one entrypoint.
#[tokio::test]
async fn mcp_and_socket_entrypoints_emit_byte_identical_decision_facts() {
    const RUN: &str = "01SP2ONEPATHRUN0000000R001";
    const REQ: &str = "01SP2ONEPATHREQ0000000Q001";
    let badge = Badge::mint().expect("mint badge");

    // Leg 1 — the MCP JSON-RPC adapter (`call_request_permission`), which DR-013
    // decision 1 makes a thin wrapper over `decide_permit`.
    let (_dir_a, core_a) = {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = rezidnt_fabric::EventLog::open(&dir.path().join("events.db")).expect("open log");
        let fabric = rezidnt_fabric::Fabric::new(log, 1024);
        let mut book = rezidnt_mcp::BadgeBook::new();
        book.admit(&badge);
        let core = std::sync::Arc::new(
            McpCore::new(fabric, book).with_permit_config(path_scope_deny_config()),
        );
        (dir, core)
    };
    seed_run(&core_a, RUN);
    let _ = util::tool_call(
        &core_a,
        1,
        "request_permission",
        json!({
            "badge": badge.token_hex(),
            "run": RUN,
            "request_id": REQ,   // the socket carries a token; MCP echoes it when supplied
            "action": "tool.invoke",
            "tool": "Edit",
            "paths": ["/etc/shadow"],
        }),
    )
    .await;
    let mcp_fact = last_denied_payload(&core_a);
    let mcp_requested = last_requested_payload(&core_a);

    // Leg 2 — the transport-neutral entrypoint the socket handler calls directly
    // (no JSON-RPC envelope), with the SAME PermitRequest inputs.
    let (_dir_b, core_b) = {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = rezidnt_fabric::EventLog::open(&dir.path().join("events.db")).expect("open log");
        let fabric = rezidnt_fabric::Fabric::new(log, 1024);
        let core = std::sync::Arc::new(
            McpCore::new(fabric, rezidnt_mcp::BadgeBook::new())
                .with_permit_config(path_scope_deny_config()),
        );
        (dir, core)
    };
    seed_run(&core_b, RUN);
    let outcome = core_b
        .decide_permit(PermitRequest {
            run: RUN.to_string(),
            request_id: Some(REQ.to_string()),
            action: "tool.invoke".to_string(),
            tool: "Edit".to_string(),
            badge: None, // DR-013 decision 3: socket transport skips the badge door
            context_ref: None,
            paths: Some(json!(["/etc/shadow"])),
        })
        .await
        .expect("decide_permit succeeds");
    assert_eq!(
        outcome.decision,
        Decision::Deny,
        "the socket entrypoint denies the out-of-scope path (criterion 7 precondition)"
    );
    assert_eq!(
        outcome.request_id, REQ,
        "the outcome carries the supplied request_id, not a minted one (criterion 3/7)"
    );
    let socket_fact = last_denied_payload(&core_b);
    let socket_requested = last_requested_payload(&core_b);

    assert_eq!(
        mcp_fact, socket_fact,
        "MCP and socket decision facts must be BYTE-IDENTICAL — one decision path, no fork (I3, DR-013 decision 1)"
    );

    // The paired `permit.requested` fact is NOT byte-identical: it legitimately
    // diverges on `badge_id`, the transport-local caller identity. The MCP leg
    // supplied a badge admitted to the book, so `decide_permit` stamps a
    // `badge_id`; the socket leg passed `badge: None` (§3 — the socket skips the
    // badge door), so it never can. Pin exactly that: present on MCP, absent on
    // socket, and otherwise identical.
    assert!(
        mcp_requested.get("badge_id").is_some(),
        "the MCP requested fact carries a resolved badge_id (present by design): {mcp_requested:#}"
    );
    assert!(
        socket_requested.get("badge_id").is_none(),
        "the socket requested fact carries NO badge_id (absent by design, §3): {socket_requested:#}"
    );
    let strip_badge = |mut v: Value| -> Value {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("badge_id");
        }
        v
    };
    assert_eq!(
        strip_badge(mcp_requested.clone()),
        strip_badge(socket_requested.clone()),
        "the two permit.requested facts must be identical EXCEPT badge_id — the only field that diverges by design (§3), not a fork (I3)"
    );
}

/// §5 criterion 7 (fixture fold-in): the `sp_wire_aggregate_deny` golden — a
/// path-scope deny of `/etc/shadow` — is the shape the live single decision path
/// reproduces. The committed fixture's `permit.denied` payload is loaded and its
/// STABLE decision fields (subject-decision, reason, the path-scope semantics)
/// are asserted equal to what the live `decide_permit` emits. This retires the
/// fixture's green-lock by binding it to the SP2 path: if the live deny reason
/// ever drifts from the committed golden, THIS fails.
///
/// COMPILE-RED on `decide_permit`; then ASSERT-RED on the live emit.
#[tokio::test]
async fn live_deny_matches_the_committed_aggregate_deny_golden() {
    // Load the committed golden decision line.
    let fixture = util::fixtures_dir().join("sp_wire_aggregate_deny.jsonl");
    let text = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|e| panic!("golden fixture must exist: {e}"));
    let golden_denied: Value = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["subject"] == json!("permit.denied"))
        .expect("the golden carries a permit.denied line")["payload"]
        .clone();
    assert_eq!(
        golden_denied["reason"],
        json!("path /etc/shadow outside allowed scope"),
        "golden precondition: the committed deny reason is path-scope's"
    );

    // Drive the LIVE single decision path with the same policy + request.
    const RUN: &str = "01SP2GOLDENRUN00000000R001";
    const REQ: &str = "01SP2GOLDENREQ00000000Q001";
    let (_dir, core) = {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = rezidnt_fabric::EventLog::open(&dir.path().join("events.db")).expect("open log");
        let fabric = rezidnt_fabric::Fabric::new(log, 1024);
        let core = std::sync::Arc::new(
            McpCore::new(fabric, rezidnt_mcp::BadgeBook::new())
                .with_permit_config(path_scope_deny_config()),
        );
        (dir, core)
    };
    seed_run(&core, RUN);
    let outcome = core
        .decide_permit(PermitRequest {
            run: RUN.to_string(),
            request_id: Some(REQ.to_string()),
            action: "tool.invoke".to_string(),
            tool: "Edit".to_string(),
            badge: None,
            context_ref: None,
            paths: Some(json!(["/etc/shadow"])),
        })
        .await
        .expect("decide_permit succeeds");
    assert_eq!(outcome.decision, Decision::Deny);

    let live_denied = last_denied_payload(&core);
    // The live emit reproduces the golden's STABLE decision fields. `policy_ref`/
    // `evidence_ref` hashes and `request_id`/`run` are per-emit/per-input, so the
    // load-bearing golden match is the deciding verifier's REASON — the policy
    // semantics the fixture pins.
    assert_eq!(
        live_denied["reason"], golden_denied["reason"],
        "the live single decision path reproduces the committed golden's deciding reason (criterion 7 fixture fold-in)"
    );
    assert!(
        live_denied["policy_ref"]["hash"].is_string()
            && live_denied["evidence_ref"]["hash"].is_string(),
        "the live deny carries resolvable policy/evidence refs like the golden (I6, I2): {live_denied:#}"
    );
}

// Keep PathBuf honestly used even if a future edit trims a path helper.
#[allow(dead_code)]
fn _touch(_p: PathBuf) {}
