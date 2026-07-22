//! DR-033 slice-2 (operator-resolve-escalation) ORACLE — CRITERION 5
//! (interrogability, I6): a HUMAN-resolved grant is DISTINGUISHABLE from a policy
//! grant. After the PDP applies a resolution, `gate_explain` on the run must
//! surface `resolved_from` and let a reader chain it to the resolution's
//! `operator_badge_id` / `reason` — so `debrief` / `gate why` can show
//! "escalated → human-resolved(allow) by operator badge_id → granted" (I6,
//! DR-033 §Invariants / §Design "interrogation").
//!
//! ## Why this is a distinct board from the PDP crux
//! The PDP crux (`permit_resolved_pdp.rs`) proves the DECISION and that the
//! emitted fact carries `resolved_from`. THIS board proves the INTERROGATION
//! surface: a reader hitting `gate_explain` (the I6 "why" tool) can tell a
//! human-resolved grant from a verifier-decided one, and reach WHO overrode and
//! WHY. A grant with no `resolved_from` on its explain is a policy grant; a grant
//! WITH it, chained to the resolution, is a recorded human override — never a
//! silent coercion.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!   - `gate_explain`'s permit branch surfaces `resolved_from` when the latest
//!     decision fact carries it (the applied grant/denial). The current permit
//!     branch (`crates/rezidnt-mcp/src/lib.rs:1199-1218`) surfaces
//!     `request_id`/`policy_ref`/`evidence_ref`/`reason` — `resolved_from` joins
//!     that set so the applied-from-resolution authority is interrogable.
//!   - the chain resolves: `explain.resolved_from` == the `permit.resolved`'s
//!     `request_id`; a reader following it finds the resolution's
//!     `operator_badge_id` + `reason` on the log (the dossier / tail carries the
//!     resolution fact). The implementer MAY additionally inline the operator
//!     attribution on the explain; the MINIMUM is that `resolved_from` is present
//!     and correct so the chain is followable.
//!
//! NOTE FOR THE IMPLEMENTER (do NOT weaken): if `gate_explain`'s shape has no
//! home for `resolved_from`, adding it to the permit branch IS the honest
//! surface — assert against whichever the implementer wires, but a human-resolved
//! grant MUST be distinguishable from a policy grant, and the operator
//! id + reason MUST be reachable.
//!
//! RED MODE: ASSERT-RED — with no PDP ledger-check the run escalates (no grant to
//! interrogate), and `gate_explain` surfaces no `resolved_from`. Deterministic:
//! the fixture log is the whole input; `gate_explain` folds it.
//!
//! NOT `#![cfg(unix)]`-gated: the PDP path uses only the EMPTY verifier set and a
//! pure `tool-allowlist` NATIVE (the negative control) — no exec/`/bin/sh` — so
//! it runs host-side, and the assert-red is observable on the host /vet gauntlet.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR033INTERR0GAB0000000R1";
const ESCALATED_REQ: &str = "01DR033INTERR0GESCREQ000R1";
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

/// Seed the escalation → resolution history for `Bash` on `RUN`, so the NEXT ask
/// applies the resolution. Same fixture shape as the PDP crux board.
fn seed_escalation_resolved(core: &McpCore) {
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
            json!({"run": RUN, "request_id": ESCALATED_REQ, "policy_ref": {"hash": "e5ca1a7e", "bytes": 8, "mime": "application/json"}, "reason": "routed to a human"}),
        ),
        ev(
            "permit.resolved",
            json!({"run": RUN, "request_id": ESCALATED_REQ, "action": "tool.invoke", "target": {"tool": "Bash"}, "decision": "allow", "operator_badge_id": OPERATOR_ID, "reason": "operator approved after review"}),
        ),
    ] {
        core.fabric().publish(e).expect("publish fixture event");
    }
}

/// CRITERION 5 — after the PDP applies the resolution (fresh ask, new
/// request_id), `gate_explain` surfaces the grant as HUMAN-RESOLVED: its
/// `resolved_from` equals the resolution's `request_id`, and following that id
/// reaches the resolution's `operator_badge_id` + `reason` on the log — a human
/// override, distinct from a policy grant. ASSERT-RED (no ledger-check → the run
/// escalates and no resolved_from surfaces).
#[tokio::test]
async fn gate_explain_chains_resolved_from_to_the_operator() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    seed_escalation_resolved(&core);

    // Fresh ask for the SAME action, DIFFERENT request_id (daemon mints one).
    let _ = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;

    // Interrogate the run: gate_explain answers "why" on the latest decision.
    let explain =
        util::tool_payload(&util::tool_call(&core, 2, "gate_explain", json!({ "run": RUN })).await);
    assert_eq!(
        explain["verdict"],
        json!("allow"),
        "the interrogated verdict is the applied grant (allow) — the resolution took"
    );
    let resolved_from = explain
        .get("resolved_from")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "gate_explain must surface `resolved_from` on a human-resolved grant, so it is \
                 DISTINCT from a policy grant (I6, DR-033 §Design interrogation) — got {explain:#}"
            )
        });
    assert_eq!(
        resolved_from, ESCALATED_REQ,
        "explain.resolved_from chains to the permit.resolved's request_id"
    );

    // Follow the chain: the resolution fact on the log carries WHO and WHY.
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
        "the chain reaches WHO overrode — the operator badge id (I6)"
    );
    assert_eq!(
        resolution.payload()["reason"],
        json!("operator approved after review"),
        "the chain reaches WHY — the operator's reason (I6)"
    );
}

/// CRITERION 5 (the distinction is load-bearing) — a POLICY grant (no resolution
/// in play) carries NO `resolved_from` on its explain, so a reader can tell it
/// apart from a human override. This is the negative control that makes the
/// positive test meaningful. A grant-all policy decides here; the explain must
/// NOT claim a phantom resolution.
#[tokio::test]
async fn a_policy_grant_carries_no_resolved_from() {
    use rezidnt_gate::permit::PermitVerifierSpec;

    let badge = Badge::mint().expect("mint badge");
    // A trivial always-allow native so the run GRANTS by POLICY, not by
    // resolution — the `tool-allowlist` native admitting the requested tool.
    let allow_all = PermitVerifierSpec::native("tool-allowlist", json!({ "allow": ["Bash"] }));
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(&badge);
    let core = Arc::new(
        McpCore::new(fabric, book).with_permit_config(PermitConfig::from_specs(vec![allow_all])),
    );
    core.fabric()
        .publish(ev(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
        ))
        .expect("publish spawned");

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
        "the policy granted the Bash request"
    );
    assert!(
        explain.get("resolved_from").is_none() || explain["resolved_from"].is_null(),
        "a POLICY grant carries NO resolved_from — a human override is DISTINCT from \
         a policy grant (I6, DR-033); a phantom resolved_from here would misreport \
         the deciding authority — got {explain:#}"
    );
}
