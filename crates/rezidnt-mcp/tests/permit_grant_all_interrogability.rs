//! DR-035 sub-slice 2 (`escalation-grant-all`) ORACLE — CRITERION 6, I6
//! INTERROGABILITY of a BROAD grant. The fold oracle (`permit_grant_all_fold.rs`)
//! pins the broadened MATCH; THIS board pins DR-035 §Invariants I6 / §Decision 2:
//! a broad grant is a RECORDED, ATTRIBUTABLE, EXPLAINABLE fact, never a silent
//! widening. When the PDP applies a broad resolution to a SIBLING action the
//! operator never individually saw, `gate why`/`debrief`/`gate_explain` must
//! RENDER the matched predicate verbatim — "granted by broad resolution X
//! matching `any action on (run, tool)`" / the `scope` value — and cite the
//! deciding resolution (`resolved_from`) so a reader chains to WHO (operator
//! badge) and WHY (reason). A broad grant that widened silently — with no
//! predicate on its explain — would be indistinguishable from a request-scoped
//! grant, defeating the blast-radius accountability the DR requires.
//!
//! This MIRRORS `resolve_permit_interrogability.rs` (an APPLIED request-scoped
//! grant surfaces `resolved_from`) and `permit_ttl_interrogability.rs` (an
//! EXPIRED resolution surfaces its expiry note): the broad-grant case is the
//! third honest outcome of a resolution, and it too must be interrogable.
//!
//! ## API surface this board PINS (minimum the implementer must wire)
//!   - When the PDP applies a BROAD resolution (`scope="run_tool"`) to an incoming
//!     action, `gate_explain`'s permit branch surfaces the matched predicate. The
//!     MINIMUM: the explain carries the `scope` value AND `resolved_from` (the
//!     broad resolution's request_id), so a reader tells a broad grant from a
//!     request-scoped one and chains to the operator. The implementer MAY inline
//!     the operator attribution; the floor is that the predicate is NAMED and the
//!     resolution is CITED.
//!   - a REQUEST-SCOPED grant (absent scope) carries NO broad-predicate marker on
//!     its explain — the negative control. A phantom predicate would misreport a
//!     narrow grant as broad, the inverse of a silent widening.
//!
//! ## Why this is RED today (the absent interrogation surface, named by failure)
//! Two gaps, either sufficient: (1) the fold discards `scope`
//! (`crates/rezidnt-state/src/lib.rs:977`) and `resolution_for` matches exactly,
//! so a SIBLING action never applies the broad resolution — the run RE-ESCALATES
//! (verdict `ask`) instead of granting, so there is no broad grant to interrogate;
//! (2) even for the resolution's OWN action, `apply_folded_resolution`
//! (`crates/rezidnt-mcp/src/lib.rs:1312-1316`) emits only `resolved_from`, never
//! the matched `scope`, so `gate_explain` surfaces no predicate. The positive
//! test asserts the predicate is present and gets an escalation with none.
//!
//! ## Harness level — HOST-side, NOT `#[cfg(unix)]`
//! The EMPTY permit config re-escalates a fresh ask through the PDP (no exec, no
//! `/bin/sh`); the broad resolution is seeded on the log. So this runs host-side
//! on the /vet gauntlet, exactly like the sibling interrogability boards.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR035GABINTERR0GAB0000R1";
const ESCALATED_REQ: &str = "01DR035GABINTERR0GESCR00R1";
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

/// Seed an escalation → BROAD resolution history for `tool.invoke` on
/// `(RUN, Bash)`. The resolution is `scope="run_tool"` + a generous `ttl_ms` (the
/// coupling: broad is bounded). Because it is broad, the next ask for a DIFFERENT
/// action on the same tool (`tool.exec`) must APPLY it — and `gate_explain` must
/// name the broad predicate. `ttl_ms` is large so expiry is not in play (the axis
/// under test is the broadening + its interrogation).
fn seed_broad_resolution(core: &McpCore) {
    for e in [
        ev(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
        ),
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "action": "tool.invoke", "target": {"tool": "Bash"}}),
        ),
        ev(
            "permit.escalated",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "reason": "routed to a human"}),
        ),
        ev(
            "permit.resolved",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "action": "tool.invoke", "target": {"tool": "Bash"},
                "decision": "allow", "operator_badge_id": OPERATOR_ID,
                "reason": "operator approved any action on this tool, time-boxed",
                "ttl_ms": 3_600_000, "scope": "run_tool"
            }),
        ),
    ] {
        core.fabric().publish(e).expect("publish fixture event");
    }
}

/// CRITERION 6 (the positive) — after the PDP applies a BROAD resolution to a
/// SIBLING action (a DIFFERENT action on the same tool, one the operator never
/// individually saw), `gate_explain` RENDERS the matched predicate: the explain
/// carries the `scope` value ("run_tool") AND `resolved_from` (the broad
/// resolution's request_id), and following that id reaches the operator badge +
/// reason on the log. A broad grant is recorded, attributable, explainable —
/// never a silent widening (I6, DR-035 §Decision 2 / §Invariants).
///
/// RED today: the broadening is not wired, so the sibling ask RE-ESCALATES
/// (verdict `ask`) instead of granting — there is no broad grant to interrogate,
/// and no `scope` surfaces. The predicate assertion fails, naming the absent
/// broad-grant interrogation surface.
#[tokio::test]
async fn gate_explain_names_the_broad_predicate_on_a_sibling_grant() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    seed_broad_resolution(&core);

    // Fresh ask for a DIFFERENT action on the SAME tool — the broad resolution
    // must apply it (grant), even though the operator resolved `tool.invoke`.
    let _ = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.exec", "tool": "Bash"}),
    )
    .await;

    let explain =
        util::tool_payload(&util::tool_call(&core, 2, "gate_explain", json!({ "run": RUN })).await);

    assert_eq!(
        explain["verdict"],
        json!("allow"),
        "the broad resolution GRANTED the sibling action (`tool.exec`) — a broad grant took, \
         not a re-escalation (RED today: broadening not wired, so the sibling re-escalates \
         to `ask`)"
    );

    // The matched predicate is NAMED on the explain — the broadening is visible,
    // not silent. `scope` is the machine-readable predicate token; `gate why`
    // renders it as "any action on (run, tool)".
    assert_eq!(
        explain["scope"],
        json!("run_tool"),
        "gate_explain RENDERS the matched broad predicate (`scope=\"run_tool\"` = any action \
         on (run, tool)) — a broad grant is EXPLAINABLE, never a silent widening (I6, DR-035 \
         §Decision 2 / §Invariants); got {explain:#}"
    );

    // And it cites the deciding resolution so a reader chains to WHO/WHY.
    let resolved_from = explain
        .get("resolved_from")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "a broad grant cites its deciding resolution (`resolved_from`) so the broadening \
                 is attributable — got {explain:#}"
            )
        });
    assert_eq!(
        resolved_from, ESCALATED_REQ,
        "resolved_from chains to the broad permit.resolved's request_id (the audit correlation)"
    );

    // Follow the chain: the broad resolution fact carries WHO and WHY.
    let resolution = util::log_events(&core)
        .into_iter()
        .find(|e| {
            e.subject.as_str() == "permit.resolved"
                && e.payload()["request_id"] == json!(resolved_from)
        })
        .expect("resolved_from resolves to the permit.resolved fact on the log (I3)");
    assert_eq!(
        resolution.payload()["operator_badge_id"],
        json!(OPERATOR_ID),
        "the chain reaches WHO widened the grant — the operator badge id (I6, blast-radius \
         accountability)"
    );
    assert_eq!(
        resolution.payload()["scope"],
        json!("run_tool"),
        "the cited resolution fact itself records the broadening (a recorded, attributable \
         fact, DR-035 §Decision 2)"
    );
}

/// CRITERION 6 (the negative control) — a REQUEST-SCOPED grant (absent scope)
/// carries NO broad-predicate marker on its explain, so a reader tells a broad
/// grant from a narrow one. A phantom `scope` here would misreport a narrow grant
/// as broad — the inverse silent-widening failure. This makes the positive test
/// meaningful.
///
/// Seeds an EXACT (absent-scope) resolution and asks for its OWN action, which
/// grants by the DR-033 request-scoped match (this path already works). The
/// explain must NOT claim a broad predicate.
#[tokio::test]
async fn a_request_scoped_grant_carries_no_broad_predicate() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    for e in [
        ev(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
        ),
        ev(
            "permit.requested",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "action": "tool.invoke", "target": {"tool": "Bash"}}),
        ),
        ev(
            "permit.escalated",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "reason": "routed to a human"}),
        ),
        // Absent scope, absent ttl — DR-033 permanent, request-scoped.
        ev(
            "permit.resolved",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "action": "tool.invoke", "target": {"tool": "Bash"},
                "decision": "allow", "operator_badge_id": OPERATOR_ID,
                "reason": "operator approved this exact invocation"
            }),
        ),
    ] {
        core.fabric().publish(e).expect("publish fixture event");
    }

    // Ask for the resolution's OWN action — grants by the request-scoped match.
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
        json!("allow"),
        "the request-scoped resolution granted its own action (DR-033 path, already works)"
    );
    assert!(
        explain.get("scope").is_none() || explain["scope"].is_null(),
        "a REQUEST-SCOPED grant carries NO broad predicate on its explain — a phantom `scope` \
         would misreport a narrow grant as broad (the inverse silent-widening failure); a broad \
         grant is DISTINCT from a request-scoped one (I6, DR-035 §Decision 2) — got {explain:#}"
    );
}
