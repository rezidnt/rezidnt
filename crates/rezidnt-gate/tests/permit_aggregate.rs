//! SP-wire oracle — the permit AGGREGATION seam (the residual the SP-intent
//! `/debrief` flagged: the live PDP hardcodes a SINGLE `ToolAllowlist.verify()`,
//! so `PathScope`/`SpendCap`/`IntentLock` are registered but NEVER run on the
//! live path). SP-wire gives `request_permission` a verifier-SELECTION seam that
//! dispatches the CONFIGURED `[gates.permit]` verifier set and aggregates their
//! verdicts into one permit decision.
//!
//! Design: permit-engine `docs/design/permit-engine.md` §6 (the
//! `[gates.permit].verifiers` TOML shape) + DR-008; the three-valued mapping and
//! the natives landed in SP1/SP-intent. This file pins the AGGREGATION at the
//! `rezidnt-gate` layer — the layer where the contract is FULLY ratified (the
//! natives already consume pinned params, `decision_for` already maps the
//! verdict). The MCP-live-path proof + the config/state-injection SEAM live in
//! `crates/rezidnt-mcp/tests/permit_wire_dispatch.rs` (that is where the design
//! FORK is — see the SP-wire work order).
//!
//! RED MODE: **compile-red** — every test references `permit::aggregate` and
//! `permit::PermitVerifierSpec` (the ordered-set dispatch API), neither of which
//! exists yet, so the crate fails to compile until the implementer adds them.
//! Then the assertions pin:
//!   - CRITERION 1: the configured set runs IN ORDER (not a hardcoded single
//!     verifier) — until short-circuit.
//!   - CRITERION 2: first-`Fail` short-circuits → Deny; else any `Inconclusive`
//!     → Escalate; else all `Pass` → Grant. The aggregate verdict maps via
//!     `permit::decision_for` (I6 — inconclusive NEVER coerced to allow).
//!   - CRITERION 4: the aggregate result names the DECIDING verifier and carries
//!     its evidence, so the decision fact's `policy_ref`/`evidence_ref` pin the
//!     REAL reason (not a hardcoded `tool-allowlist`).
//!
//! ## The aggregation control flow (mirrors `run_gate`, mapped to a decision)
//! The S4 `bins/rezidentd/src/gates.rs::run_gate` rule, applied to the permit
//! axis: in-order execution, first-failure short-circuits, three-valued
//! precedence — but the terminal maps to a `PermitDecision` (Grant/Deny/
//! Escalate) via `decision_for`, not to a `gate.*` fact. The engine IS the
//! policy engine (design §4): there is no second aggregator to build.

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::permit::{self, PermitDecision, PermitVerifierSpec};
use rezidnt_gate::{Verdict, VerifierInput};
use serde_json::{Value, json};

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

/// A permit `VerifierInput`: the request's `tool` plus the per-verifier params
/// ride `params`; there is no CAS blob (the descriptor is inline, ontology
/// `permit.requested.target`). The aggregator injects each verifier's own
/// `PermitVerifierSpec.params` merged with the request axis.
fn permit_input(params: Value) -> VerifierInput {
    VerifierInput {
        gate: permit::LIFECYCLE_POINT.to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params,
        timeout_ms: rezidnt_gate::DEFAULT_TIMEOUT_MS,
    }
}

/// One native permit-verifier entry in the configured `[gates.permit]` set: a
/// native name plus its pinned params. This is the resolved SP-wire dispatch
/// unit — the daemon builds it from `VerifierSpec` + the folded per-run state
/// injected as content-pinned params (CRITERION 3, pinned at the MCP layer).
fn native(name: &str, params: Value) -> PermitVerifierSpec {
    PermitVerifierSpec::native(name, params)
}

/// CRITERION 1 + 2 — a permit gate configured with THREE natives runs ALL three
/// (tool-allowlist, path-scope, intent-lock) when each passes, and aggregates to
/// GRANT. This is the residual proof at the aggregation layer: the hardcoded
/// single-`ToolAllowlist` path could NEVER run path-scope or intent-lock, so an
/// out-of-scope path or off-task tool would slip through. Here all three run and
/// all pass → allow.
///
/// COMPILE-RED until `permit::aggregate` / `PermitVerifierSpec` exist.
#[test]
fn configured_set_runs_all_verifiers_in_order_and_grants_when_all_pass() {
    let (_dir, cas) = empty_cas();
    // The request axis: tool "Read", target path in scope, on-task.
    let input = permit_input(json!({
        "tool": "Read",
        "paths": ["src/checkout/lib.rs"],
        "allowed_tools": ["Read", "Grep", "Glob"],
    }));
    let set = vec![
        native(
            "tool-allowlist",
            json!({ "allow": ["Read", "Edit", "Bash"] }),
        ),
        native("path-scope", json!({ "allow": ["src/checkout/**"] })),
        native("intent-lock", json!({})),
    ];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        PermitDecision::Grant,
        "all three configured verifiers ran and passed → allow (CRITERION 1: the set is dispatched, not a single hardcode)"
    );
    assert_eq!(
        outcome.verifiers_run, 3,
        "the whole configured set runs when nothing short-circuits (in-order dispatch, CRITERION 1)"
    );
}

/// CRITERION 1 + 5 (aggregation leg) — the CONFIGURED path-scope verifier
/// actually RUNS: an out-of-scope target path DENIES even though the tool is
/// allowlisted. The hardcoded single-`ToolAllowlist` path would GRANT this (it
/// never sees the path), so this is exactly the residual SP-wire closes.
///
/// COMPILE-RED until the aggregation API exists.
#[test]
fn configured_path_scope_denies_out_of_scope_even_when_tool_is_allowed() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({
        "tool": "Edit",
        "paths": ["/etc/shadow"],
    }));
    let set = vec![
        native(
            "tool-allowlist",
            json!({ "allow": ["Read", "Edit", "Bash"] }),
        ),
        native("path-scope", json!({ "allow": ["src/checkout/**"] })),
    ];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "path-scope RAN and denied the out-of-scope path — the residual the single-verifier hardcode leaks (CRITERION 5)"
    );
    assert_eq!(
        outcome.deciding_verifier, "path-scope",
        "the DECIDING verifier is path-scope, not the passing tool-allowlist (CRITERION 4)"
    );
}

/// CRITERION 2 — first `Fail` SHORT-CIRCUITS → Deny, and no later verifier runs
/// (mirrors `run_gate`'s first-failure short-circuit, §8). A denying
/// tool-allowlist in position 1 must stop before path-scope in position 2 —
/// aggregation is not "run everything then combine".
///
/// COMPILE-RED until the aggregation API exists.
#[test]
fn first_fail_short_circuits_and_skips_later_verifiers() {
    let (_dir, cas) = empty_cas();
    // tool "Bash" is NOT in the allowlist → the FIRST verifier fails.
    let input = permit_input(json!({
        "tool": "Bash",
        // a path that path-scope would ALSO reject — but path-scope must not run.
        "paths": ["/etc/shadow"],
    }));
    let set = vec![
        native("tool-allowlist", json!({ "allow": ["Read", "Edit"] })),
        native("path-scope", json!({ "allow": ["src/checkout/**"] })),
    ];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the first failing verifier decides → Deny (CRITERION 2)"
    );
    assert_eq!(
        outcome.deciding_verifier, "tool-allowlist",
        "the FIRST failure is the deciding verifier (short-circuit, CRITERION 2/4)"
    );
    assert_eq!(
        outcome.verifiers_run, 1,
        "path-scope must NOT have run — first Fail short-circuits (CRITERION 2)"
    );
}

/// CRITERION 2 — three-valued PRECEDENCE: with NO Fail but at least one
/// `Inconclusive`, the aggregate is ESCALATE (never coerced to allow, I6). A
/// passing tool-allowlist followed by a soft-cap-crossing spend-cap (which
/// returns Inconclusive) escalates — the honesty guard the whole product rests
/// on, applied to aggregation.
///
/// COMPILE-RED until the aggregation API exists.
#[test]
fn any_inconclusive_without_fail_escalates_never_coerced() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({
        "tool": "Read",
        // spend-cap inputs: projected (cumulative + cost) lands in the soft band
        // (soft <= projected < hard) → Inconclusive (escalate).
        "cumulative_spend_usd": 8.0,
        "action_cost_usd": 1.0,
        "soft_cap_usd": 5.0,
        "hard_cap_usd": 20.0,
    }));
    let set = vec![
        native(
            "tool-allowlist",
            json!({ "allow": ["Read", "Edit", "Bash"] }),
        ),
        native("spend-cap", json!({})),
    ];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "no Fail but an Inconclusive → Escalate (route to a human), NEVER coerced to Grant (I6, CRITERION 2)"
    );
    assert_eq!(
        outcome.deciding_verifier, "spend-cap",
        "the escalating verifier is the deciding one (CRITERION 4)"
    );
}

/// CRITERION 2 — Fail BEATS Inconclusive regardless of order: an escalating
/// spend-cap in position 1 followed by a denying tool-allowlist in position 2
/// must yield DENY, because a later Fail is short-circuited BEFORE it... no —
/// this pins the OTHER direction of the precedence rule: a Fail anywhere in the
/// scanned prefix wins over an Inconclusive seen earlier. The `run_gate` rule is
/// "first Fail short-circuits" — so an Inconclusive in position 1 does NOT
/// short-circuit; scanning continues; a Fail in position 2 then decides Deny.
/// This proves Inconclusive does not stop the scan (only Fail does), and Fail
/// takes precedence over an already-seen Inconclusive (I6: deny is stronger than
/// escalate).
///
/// COMPILE-RED until the aggregation API exists.
#[test]
fn inconclusive_does_not_short_circuit_and_a_later_fail_denies() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({
        "tool": "Bash",
        // spend-cap → Inconclusive (soft band) in position 1.
        "cumulative_spend_usd": 8.0,
        "action_cost_usd": 1.0,
        "soft_cap_usd": 5.0,
        "hard_cap_usd": 20.0,
    }));
    let set = vec![
        native("spend-cap", json!({})),
        // tool "Bash" not in allowlist → Fail in position 2.
        native("tool-allowlist", json!({ "allow": ["Read", "Edit"] })),
    ];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "an Inconclusive does NOT short-circuit; a later Fail decides Deny (Fail > Escalate, CRITERION 2)"
    );
    assert_eq!(
        outcome.deciding_verifier, "tool-allowlist",
        "the Fail is the deciding verifier even though an Inconclusive preceded it (CRITERION 4)"
    );
    assert_eq!(
        outcome.verifiers_run, 2,
        "both verifiers ran — Inconclusive never short-circuits, only Fail does (CRITERION 2)"
    );
}

/// CRITERION 4 — the aggregate result carries the DECIDING verifier's EVIDENCE
/// (a `cas:blake3:` ref), not the passing verifiers', so `gate_explain` surfaces
/// the real reason (I6). A denying path-scope's evidence blob is the one carried.
///
/// COMPILE-RED until the aggregation API exists.
#[test]
fn aggregate_carries_the_deciding_verifiers_evidence() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({
        "tool": "Edit",
        "paths": ["/etc/shadow"],
    }));
    let set = vec![
        native("tool-allowlist", json!({ "allow": ["Read", "Edit"] })),
        native("path-scope", json!({ "allow": ["src/checkout/**"] })),
    ];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(outcome.decision, PermitDecision::Deny);
    let deciding_evidence = outcome.evidence.first().unwrap_or_else(|| {
        panic!("a deny carries the deciding verifier's evidence (I6): {outcome:?}")
    });
    assert!(
        deciding_evidence
            .cas_ref
            .as_deref()
            .is_some_and(|r| r.starts_with("cas:blake3:")),
        "the deciding evidence rides as a CAS ref, never inline bytes (I2): {deciding_evidence:?}"
    );
    assert!(
        deciding_evidence.msg.contains("/etc/shadow"),
        "the deciding evidence names the offending path so the deny is interrogable (I6): {deciding_evidence:?}"
    );
}

/// CRITERION 2 (I6 honesty) — an empty configured set never SYNTHESIZES an
/// allow. With no verifiers to decide, aggregation escalates (undecidable →
/// route to a human), NEVER Grant. This is the "no policy configured is not a
/// permit" honesty guard — the hardcoded path answered deny-by-default on a bare
/// surface, but an EMPTY *configured* set is undecidable, not a silent allow.
///
/// COMPILE-RED until the aggregation API exists.
#[test]
fn empty_configured_set_escalates_never_synthesizes_allow() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" }));
    let set: Vec<PermitVerifierSpec> = vec![];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_ne!(
        outcome.decision,
        PermitDecision::Grant,
        "an empty configured set must NEVER synthesize an allow (I6 — undecidable is not a pass)"
    );
    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "no configured verifier means undecidable → escalate to a human (I6)"
    );
}

/// CRITERION 2 — the aggregate verdict maps through `permit::decision_for` (the
/// SP0 total mapping), so the aggregation layer reuses the ratified honesty
/// mapping rather than re-deriving it. All-pass → the `Verdict::Pass` mapping;
/// the aggregate `PermitDecision` equals `decision_for` of the aggregate
/// three-valued verdict.
///
/// COMPILE-RED until the aggregation API exposes the aggregate `Verdict`.
#[test]
fn aggregate_verdict_maps_via_decision_for() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" }));
    let set = vec![native("tool-allowlist", json!({ "allow": ["Read"] }))];
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        permit::decision_for(outcome.verdict),
        "the aggregate decision is EXACTLY decision_for(aggregate verdict) — no bespoke coercion (I6)"
    );
    assert_eq!(outcome.verdict, Verdict::Pass);
    assert_eq!(outcome.decision, PermitDecision::Grant);
}
