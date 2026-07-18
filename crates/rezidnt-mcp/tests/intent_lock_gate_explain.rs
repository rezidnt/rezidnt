//! SP-intent oracle — DR-010 §8 criterion 3: `gate_explain` surfaces the
//! `intent-lock` escalation / denial (reason + policy_ref + evidence_ref) so a
//! run blocked (escalated) for an off-task tool is INTERROGABLE — a human reads
//! WHY the request was surfaced (I6; design `docs/design/intent-lock.md` §4).
//!
//! MODE: **green-locking**. `gate_explain`'s permit leg (landed SP1) already
//! resolves ANY `permit.escalated` / `permit.denied` fact GENERICALLY by
//! subject (crates/rezidnt-mcp/src/lib.rs `call_gate_explain`), not by verifier
//! name — so an intent-lock escalation is already interrogable the moment the
//! `intent-lock` native produces one and the daemon logs `permit.escalated`.
//! There is NO gap at the MCP surface (DR-010 §8 note: "`gate_explain` already
//! resolves permit.escalated/permit.denied (landed in SP1)"). This test PINS
//! that interrogability against the committed intent-lock escalation fixture, so
//! a later edit that narrows the permit leg (e.g. keying on verifier name, or
//! dropping the intent-lock escalation) turns it red. It is honest as a
//! green-lock: the RED SP-intent work is the `intent-lock` NATIVE
//! (crates/rezidnt-gate/tests/intent_lock_native.rs), which is what PRODUCES the
//! escalation this test then confirms is surfaced.
//!
//! If the auditor finds the permit leg does NOT in fact surface an intent-lock
//! escalation's reason/policy_ref/evidence_ref, this test flips to the assert-red
//! MCP gap DR-010 §8 permits — and the implementer's work order gains an MCP item.

mod util;

use serde_json::json;

const ESCALATED_RUN: &str = "01SPNTENTESCRN000000000000";

/// DR-010 §8 crit 3 — an off-task request escalated by `intent-lock` is
/// interrogable: `gate_explain` on that run resolves the escalation as an `ask`
/// verdict (NEVER coerced to allow, I6) and surfaces the deciding `policy_ref`,
/// the `evidence_ref`, and the human-readable `reason` naming the off-task tool
/// and the declared intent. This is the "blocked agent always reads WHY" leg
/// (design §4), on the intent axis.
#[tokio::test]
async fn gate_explain_surfaces_intent_lock_escalation() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "sp_intent_off_task_escalation.jsonl");

    let result = util::tool_call(&core, 1, "gate_explain", json!({"run": ESCALATED_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "an escalated run is interrogable — gate_explain must not answer gate.no_verdict: {result:#}"
    );
    let payload = util::tool_payload(&result);

    assert_eq!(
        payload["verdict"],
        json!("ask"),
        "an intent-lock escalation surfaces as `ask` (route-to-a-human), NEVER coerced to allow (I6, DR-010 §3)"
    );
    assert_eq!(
        payload["reason"],
        json!("off-task tool Bash not in declared intent [Read, Grep, Glob]"),
        "the escalation reason names the off-task tool AND the declared intent so a human reads WHY (I6, DR-010 §8)"
    );
    assert_eq!(
        payload["policy_ref"]["hash"],
        json!("n7en7l0ckp0l1cy00000000000000000000000000000000000000000d3m0p1"),
        "the deciding policy_ref is resolvable (I6)"
    );
    assert_eq!(
        payload["evidence_ref"]["hash"],
        json!("0fftaskbashev1dence000000000000000000000000000000000000000d3m01"),
        "the escalation evidence ref is resolvable (I6, I2 — a ref, never inline bytes)"
    );
}
