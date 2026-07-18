//! The permit lifecycle point (SP0 — DR-008 / DR-009; design
//! `docs/design/permit-engine.md` §4, §5, §8; ontology "permit set").
//!
//! The pre-hoc "may" axis: a permit-verifier decides whether an agent action is
//! authorized *before* it runs. There is no second policy engine — the gate
//! engine IS the policy engine (design §4). This module adds the fourth
//! lifecycle point alongside `vet` / `pre_merge` / `post_run`, plus the
//! authorization mapping and the emit-side fact constructors.
//!
//! Lifecycle points are STRINGS today (`GateDef.name`; see the crate root), so
//! the point is a canonical name constant ([`LIFECYCLE_POINT`]), not a new enum
//! arm. The load-bearing SP0 unit is [`decision_for`]: it turns the BINDING
//! three-valued [`Verdict`] into an authorization [`PermitDecision`] with ZERO
//! new vocabulary — and, critically, `Inconclusive → Escalate` is TOTAL and
//! never coerced to `Grant`/`Deny` (I6, DR-008 §4).
//!
//! SP0 scope: the lifecycle point + the mapping + the fact constructors. The
//! `request_permission` MCP tool/socket, the native permit-verifier pack, and
//! policy engines (exec/OPA/Cedar) are SP1–SP4, not here.

use rezidnt_types::refs::CasRef;
use serde_json::{Value, json};

use crate::Verdict;

/// The fourth gate lifecycle point (design §4; project spec `[gates.permit]`).
/// String-modeled like the existing three — a `GateDef` whose `name` is this is
/// a permit gate.
pub const LIFECYCLE_POINT: &str = "permit";

/// The authorization decision a permit-verifier's [`Verdict`] maps to. Carried
/// by the fact SUBJECT (never a bare boolean), matching the house pattern of
/// `gate.passed`/`gate.failed`/`gate.inconclusive` (I6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermitDecision {
    /// allow (`pass`).
    Grant,
    /// deny (`fail`).
    Deny,
    /// route to a human (`inconclusive`) — never coerced to allow (I6).
    Escalate,
}

impl PermitDecision {
    /// The `permit.*` fact subject this decision is logged as. The emit path
    /// and the reducer key on the SAME string (ontology lines 153-155).
    pub fn subject(self) -> &'static str {
        match self {
            PermitDecision::Grant => "permit.granted",
            PermitDecision::Deny => "permit.denied",
            PermitDecision::Escalate => "permit.escalated",
        }
    }
}

/// The TOTAL verdict → decision mapping (design §4 table; ontology permit set):
/// `Pass → Grant`, `Fail → Deny`, `Inconclusive → Escalate`.
///
/// There is deliberately NO path from `Inconclusive` to `Grant`/`Deny`: an
/// inconclusive verdict escalates to a human, always (I6, DR-008 §4). This is
/// the honesty invariant the SP0 tests exist to guard — weakening it is a
/// verdict-coercion defect, never a valid change.
pub fn decision_for(verdict: Verdict) -> PermitDecision {
    match verdict {
        Verdict::Pass => PermitDecision::Grant,
        Verdict::Fail => PermitDecision::Deny,
        Verdict::Inconclusive => PermitDecision::Escalate,
    }
}

/// The accumulator/cost deltas a decision contributes onto its fact payload —
/// the C1 spend-cap verifier is the PRODUCER of the keys `rezidnt-state`'s
/// reducer already folds (`spend_delta_usd` → cumulative spend, `risk_delta` →
/// running risk score) plus the §10.2 decision `cost_ms`.
///
/// Every field is optional: a decision that measured no spend/risk/cost emits
/// NONE of these keys. Absence is an OMITTED key on the payload, never a JSON
/// `null` — the reducer reads `payload["spend_delta_usd"].as_f64()`, and a
/// `null` there is not a `0` (I3 fold correctness).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DecisionDeltas {
    /// This action's incremental spend (folds into cumulative spend, C1).
    pub spend_delta_usd: Option<f64>,
    /// This action's incremental risk (folds into the running score, C6).
    pub risk_delta: Option<f64>,
    /// The §8 stdout decision cost in milliseconds (design §10.2 latency).
    pub cost_ms: Option<u64>,
}

/// Build a permit DECISION fact (`permit.granted` / `permit.denied` /
/// `permit.escalated`) — the PDP turning a verdict into the fact it logs.
/// Returns `(subject, payload)`; the subject comes from
/// [`decision_for`]`(verdict).subject()`, so an inconclusive verdict can only
/// ever be logged as `permit.escalated`, never coerced to a grant (I6).
///
/// Payload shape (ontology `permit.granted`/`.denied`/`.escalated` v1, and the
/// reducer keys in `rezidnt-state`): `run`, `request_id`, `policy_ref: CasRef`,
/// optional `evidence_ref: CasRef`, optional `reason`, and the optional
/// accumulator/cost deltas ([`DecisionDeltas`]). Optional keys are OMITTED
/// entirely when `None` — never emitted as JSON `null`. `policy_ref` /
/// `evidence_ref` are CAS refs so `gate why` / `gate_explain` can resolve the
/// deciding policy and evidence (I6); bytes never ride inline (I2).
pub fn decided_fact(
    verdict: Verdict,
    run: &str,
    request_id: &str,
    policy_ref: &CasRef,
    evidence_ref: Option<&CasRef>,
    reason: Option<&str>,
    deltas: DecisionDeltas,
) -> (&'static str, Value) {
    let subject = decision_for(verdict).subject();

    let mut payload = json!({
        "run": run,
        "request_id": request_id,
        "policy_ref": policy_ref,
    });
    // Omit optional keys when absent — a missing evidence blob / reason is an
    // absent key, never a `null` (the reducer reads `.as_str()` / `["hash"]`).
    if let Some(evidence_ref) = evidence_ref {
        payload["evidence_ref"] = json!(evidence_ref);
    }
    if let Some(reason) = reason {
        payload["reason"] = Value::String(reason.to_string());
    }
    // Accumulator/cost deltas: PRESENT keys ride the payload verbatim; absent
    // deltas are OMITTED, never JSON `null` (the reducer's `.as_f64()` must see
    // absence, not a null-that-is-0 — I3 fold correctness).
    if let Some(spend) = deltas.spend_delta_usd {
        payload["spend_delta_usd"] = json!(spend);
    }
    if let Some(risk) = deltas.risk_delta {
        payload["risk_delta"] = json!(risk);
    }
    if let Some(cost_ms) = deltas.cost_ms {
        payload["cost_ms"] = json!(cost_ms);
    }

    (subject, payload)
}

/// Build a permit REQUEST fact (`permit.requested`) — the harness PEP asking to
/// perform an action. Returns `("permit.requested", payload)`.
///
/// Payload shape (ontology `permit.requested` v1, and the reducer keys):
/// `run`, `request_id`, `action`, `target: {tool}` (a SMALL inline descriptor),
/// optional `context_ref: CasRef`. Bulk action context (argv, file bytes,
/// diffs) is ALWAYS the ref only — never inline bytes (I2); the descriptor
/// stays short scalars, so the payload sits far under the 32 KiB envelope cap.
pub fn requested_fact(
    run: &str,
    request_id: &str,
    action: &str,
    tool: &str,
    context_ref: Option<&CasRef>,
) -> (&'static str, Value) {
    let mut payload = json!({
        "run": run,
        "request_id": request_id,
        "action": action,
        "target": { "tool": tool },
    });
    if let Some(context_ref) = context_ref {
        payload["context_ref"] = json!(context_ref);
    }

    ("permit.requested", payload)
}
