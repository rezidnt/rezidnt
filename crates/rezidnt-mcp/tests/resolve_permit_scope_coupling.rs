//! DR-035 sub-slice 2 (`escalation-grant-all`) ORACLE — CRITERION 5, the
//! SECURITY-CRITICAL structural coupling (DR-035 §Decision 3 / §Design). A broad
//! (`scope="run_tool"`) resolution MUST carry a bounded `ttl_ms`: you can be broad
//! OR permanent, never BOTH. The coupling is enforced at the `resolve_permit`
//! tool boundary — the operator badge is verified FIRST, then the
//! scope-implies-ttl argument validation, BEFORE any fact is emitted. This makes
//! the dangerous quadrant (broad AND permanent) STRUCTURALLY UNREACHABLE on the
//! log, not merely discouraged: a broad-and-permanent `permit.resolved` can never
//! be minted, so the reducer/PDP need no guard against it.
//!
//! ## What this board PINS (the tool-boundary contract, DR-035 §Design)
//!   - `resolve_permit { badge, run, request_id, decision, reason?, ttl_ms?,
//!     scope? }` — `scope` + `ttl_ms` are additive-optional (already on
//!     `ResolvePermitArgs`). When `scope == Some("run_tool")` and `ttl_ms ==
//!     None`, the call is REFUSED, and NO `permit.resolved` fact lands (badge-
//!     first, validate-before-emit — no partial state, I3).
//!   - `scope == Some("run_tool")` WITH a `ttl_ms` SUCCEEDS: exactly one
//!     `permit.resolved` fact lands, carrying BOTH the `scope` and the `ttl_ms`
//!     (the broadening + its bound ride the fact so the reducer folds both).
//!   - `scope == None` with NO `ttl_ms` STILL SUCCEEDS: permanent, request-scoped
//!     — today's DR-033 behavior, unchanged by the coupling (the coupling binds
//!     ONLY the broad case).
//!
//! ## Why this is RED today (the absent coupling guard, named by the failure)
//! `call_resolve_permit` (`crates/rezidnt-mcp/src/lib.rs:814-882`) parses `scope`
//! (it is on `ResolvePermitArgs`) but NEVER validates the scope-implies-ttl
//! coupling AND never threads `scope` onto the emitted payload (only `ttl_ms`,
//! line 872). So a broad-WITHOUT-ttl call today PASSES the door, derives the
//! action/target, and EMITS a `permit.resolved` fact — the exact broad-and-
//! permanent shape the coupling forbids. The refusal test's "NO fact emitted"
//! assertion FAILS (a fact IS emitted), naming the missing guard. The broad-WITH-
//! ttl success test's "fact carries scope" assertion FAILS (scope is dropped on
//! emit), naming the missing scope-thread.
//!
//! ## Harness level (answering the oracle brief's explicit question)
//! This runs at the MCP CORE level (`McpCore` + in-process `Fabric`), NOT the
//! `#[cfg(unix)]` daemon/socket harness: a resolve is a door + a fold-derive + a
//! fact emit — no exec, no `/bin/sh`, no PTY. So it runs HOST-side on the /vet
//! gauntlet (no WSL needed), exactly like `resolve_permit_door.rs`.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR035COUPLING0RES0LVE00R1";
const ESCALATED_REQ: &str = "01DR035COUPLINGESCREQ0000R1";

fn root() -> rezidnt_run::badge::RootKey {
    rezidnt_run::badge::RootKey::from_bytes([9u8; 32])
}

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// Seed the escalation the daemon DERIVES `(action, target)` from — the request
/// carrying the real `action`/`target`, then the escalation routed to a human. A
/// `resolve_permit` for `ESCALATED_REQ` folds this and stamps the derived
/// `(action, target)` on the emitted fact (DR-033 §Design). Same shape as
/// `resolve_permit_door.rs::seed_escalation` so the coupling is tested on a
/// KNOWN, resolvable escalation — the refusal is on the SCOPE coupling, not an
/// unknown-request lookup.
fn seed_escalation(core: &McpCore) {
    for e in [
        ev(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
        ),
        ev(
            "permit.requested",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "action": "tool.invoke", "target": {"tool": "Bash"}
            }),
        ),
        ev(
            "permit.escalated",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "reason": "no policy configured — routed to a human"
            }),
        ),
    ] {
        core.fabric().publish(e).expect("publish fixture event");
    }
}

/// A core with the operator badge admitted and the daemon root key wired (so the
/// door is exercised exactly as `resolve_permit_door.rs` does). No substrate: a
/// resolve is a door + a fold-derive + a fact emit.
fn operator_core(operator: &Badge) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(operator);
    let core = McpCore::new(fabric, book).with_root_key(root());
    (dir, Arc::new(core))
}

fn resolved_facts(core: &McpCore) -> Vec<Event> {
    util::log_events(core)
        .into_iter()
        .filter(|e| e.subject.as_str() == "permit.resolved")
        .collect()
}

/// CRITERION 5 (the security-critical refusal) — a `resolve_permit` with
/// `scope="run_tool"` and NO `ttl_ms` is REFUSED, and CRUCIALLY NO
/// `permit.resolved` fact lands on the log. Broad-and-permanent is structurally
/// unreachable: the coupling is validated BEFORE any emit (badge-first,
/// validate-before-emit — no partial state, I3). The operator badge is valid and
/// the escalation is KNOWN, so the refusal is on the SCOPE COUPLING specifically,
/// not a badge or unknown-request failure.
///
/// RED today: `call_resolve_permit` has NO coupling guard — it admits this,
/// derives the action/target, and EMITS a fact. The "no fact" assertion fails,
/// naming the absent guard.
#[tokio::test]
async fn broad_scope_without_ttl_is_refused_no_fact() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);

    let result = util::tool_call(
        &core,
        1,
        "resolve_permit",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "request_id": ESCALATED_REQ,
            "decision": "allow",
            "scope": "run_tool",
            // NO ttl_ms — the forbidden broad-and-permanent shape.
        }),
    )
    .await;

    assert_eq!(
        result["isError"],
        json!(true),
        "a broad (scope=\"run_tool\") resolution WITHOUT a ttl_ms is REFUSED — broad OR \
         permanent, never both (DR-035 §Decision 3); got {result:#}"
    );
    // The refusal must NOT be a badge failure (the badge is valid) nor an
    // unknown-request failure (the escalation is seeded) — it is the SCOPE
    // coupling. Pin that the refusal is not a mis-attributed door/lookup refusal.
    let payload = util::tool_payload(&result);
    assert_ne!(
        payload["code"],
        json!(rezidnt_mcp::codes::BADGE_REQUIRED),
        "the refusal is the SCOPE coupling, not a missing-badge refusal (the badge is valid)"
    );
    assert_ne!(
        payload["code"],
        json!(rezidnt_mcp::codes::BADGE_INVALID),
        "the refusal is the SCOPE coupling, not a badge-invalid refusal (the badge is valid)"
    );
    assert_ne!(
        payload["code"],
        json!(rezidnt_mcp::codes::RUN_UNKNOWN),
        "the refusal is the SCOPE coupling, not an unknown-escalation refusal (it is seeded)"
    );

    // THE security-critical assertion — validate-before-emit: NO fact lands, so a
    // broad-and-permanent resolution can never reach the log (I3, DR-035 §Design).
    assert!(
        resolved_facts(&core).is_empty(),
        "a REFUSED broad-without-ttl resolve emits NO permit.resolved fact — the coupling is \
         validated BEFORE any emit (no partial state); a broad-and-permanent fact must be \
         STRUCTURALLY UNREACHABLE on the log (DR-035 §Decision 3, §Design)"
    );
}

/// CRITERION 5 (the admitted broad case) — a `resolve_permit` with
/// `scope="run_tool"` AND a `ttl_ms` SUCCEEDS: exactly one `permit.resolved` fact
/// lands carrying BOTH the scope and the ttl_ms, so the reducer folds a bounded
/// broad resolution. This is the ALLOWED quadrant (broad + bounded).
///
/// RED today: `call_resolve_permit` threads `ttl_ms` (line 872) but NOT `scope`
/// onto the payload — so the emitted fact is missing `scope`. The "fact carries
/// scope" assertion fails, naming the absent scope-thread.
#[tokio::test]
async fn broad_scope_with_ttl_succeeds_and_carries_both() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);

    let result = util::tool_call(
        &core,
        2,
        "resolve_permit",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "request_id": ESCALATED_REQ,
            "decision": "allow",
            "reason": "operator approved any action on this tool, time-boxed",
            "scope": "run_tool",
            "ttl_ms": 60_000,
        }),
    )
    .await;

    assert_eq!(
        result["isError"],
        json!(false),
        "a broad resolution WITH a ttl_ms is admitted (broad + bounded is the allowed \
         quadrant, DR-035 §Decision 3); got {result:#}"
    );

    let facts = resolved_facts(&core);
    assert_eq!(
        facts.len(),
        1,
        "an admitted broad+ttl resolve emits EXACTLY ONE permit.resolved fact (single writer, I3)"
    );
    let fact = &facts[0];
    assert_eq!(
        fact.payload()["scope"],
        json!("run_tool"),
        "the emitted fact carries `scope=\"run_tool\"` VERBATIM so the reducer folds the \
         broadening (RED today: call_resolve_permit threads ttl_ms but NOT scope onto the \
         payload — the broadening is dropped on emit)"
    );
    assert_eq!(
        fact.payload()["ttl_ms"],
        json!(60_000),
        "the emitted fact carries the bounding ttl_ms alongside the scope (broad + bounded)"
    );
    assert_eq!(
        fact.payload()["decision"],
        json!("allow"),
        "the human decision rides as the input verb, never coerced (I6)"
    );
}

/// CRITERION 5 (the unchanged default) — a `resolve_permit` with NO `scope` and
/// NO `ttl_ms` STILL SUCCEEDS: permanent, request-scoped — today's DR-033
/// behavior. The coupling binds ONLY the broad case; it must not tighten the
/// request-scoped default. This is the regression guard that proves the coupling
/// did not over-reach into the permanent-request-scoped path.
///
/// GREEN today AND after implementation (the door already admits this shape) — a
/// guard, not a red-before-impl assertion. It fails ONLY if a coupling
/// implementation wrongly demands a ttl on EVERY resolve (over-reach).
#[tokio::test]
async fn absent_scope_absent_ttl_still_succeeds_permanent_request_scoped() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);

    let result = util::tool_call(
        &core,
        3,
        "resolve_permit",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "request_id": ESCALATED_REQ,
            "decision": "allow",
            // No scope, no ttl_ms — DR-033 permanent, request-scoped.
        }),
    )
    .await;

    assert_eq!(
        result["isError"],
        json!(false),
        "an absent-scope, absent-ttl resolve still succeeds — permanent, request-scoped \
         (DR-033 behavior, unchanged); the coupling binds ONLY the broad case, it must NOT \
         demand a ttl on every resolve; got {result:#}"
    );
    let facts = resolved_facts(&core);
    assert_eq!(
        facts.len(),
        1,
        "the permanent request-scoped resolve emits its one fact (unchanged from DR-033)"
    );
    assert!(
        facts[0].payload().get("scope").is_none() || facts[0].payload()["scope"].is_null(),
        "a request-scoped resolve carries NO scope on its fact — absent = exact match (never \
         synthesized to a phantom broadening, I3/DR-033 §Decision 3)"
    );
}
