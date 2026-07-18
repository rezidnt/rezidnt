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

use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;
use serde_json::{Value, json};

use crate::{Evidence, GateError, Verdict, VerifierInput, builtin_natives};

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

/// One resolved entry in the configured `[gates.permit]` verifier set: a native
/// name plus its content-pinned params. This is the SP-wire dispatch unit — the
/// daemon builds it from `VerifierSpec` plus the folded per-run state injected as
/// content-pinned params (determinism BINDING). The aggregator merges each
/// verifier's own `params` with the shared request axis (the requested `tool`,
/// target `paths`, etc.) so both the request and the policy config reach the
/// native as one pinned `inputs.params` object.
#[derive(Debug, Clone, PartialEq)]
pub struct PermitVerifierSpec {
    /// The native name resolved via [`builtin_natives`] (`name()`).
    pub name: String,
    /// The verifier's pinned params (its `[gates.permit].verifiers` config, e.g.
    /// `{ "allow": [...] }`), merged over the shared request axis.
    pub params: Value,
}

/// The aggregate outcome of dispatching a configured permit-verifier set: the
/// terminal three-valued [`Verdict`], its mapped [`PermitDecision`], the deciding
/// verifier's name + evidence (so the caller can pin `policy_ref`/`evidence_ref`
/// to the REAL reason, I6), the deciding verifier's merged params (the policy
/// descriptor the caller pins to CAS as `policy_ref`), and how many verifiers ran
/// (short-circuit proof).
#[derive(Debug, Clone)]
pub struct PermitOutcome {
    /// The aggregate three-valued verdict (all-pass ⇒ Pass; first Fail ⇒ Fail;
    /// else any Inconclusive ⇒ Inconclusive; empty set ⇒ Inconclusive).
    pub verdict: Verdict,
    /// `decision_for(verdict)` — never a bespoke coercion (I6).
    pub decision: PermitDecision,
    /// The deciding verifier's name (the first Fail, else the first Inconclusive,
    /// else the last passing verifier; empty for the empty configured set).
    pub deciding_verifier: String,
    /// The deciding verifier's merged params (request axis + its config) — the
    /// policy descriptor the caller pins to CAS as `policy_ref` so the decision
    /// stays replayable/interrogable (I3).
    pub deciding_params: Value,
    /// The deciding verifier's evidence (a CAS ref, never inline bytes, I2).
    pub evidence: Vec<Evidence>,
    /// The deciding evidence's REAL [`CasRef`] — its true `bytes`/`mime`,
    /// recovered from the store at aggregation time — so the emit site pins an
    /// HONEST `evidence_ref` and never fabricates the blob's own metadata
    /// (`bytes: 0`). `None` when the deciding outcome has no evidence blob (a
    /// grant, an empty set) or the recorded ref is not a resolvable
    /// `cas:blake3:` ref. The `evidence` string above still carries the ref for
    /// display; this is the metadata-honest companion.
    pub deciding_evidence_ref: Option<CasRef>,
    /// How many verifiers ran before the outcome was decided (first Fail
    /// short-circuits; Inconclusive does not — only Fail stops the scan).
    pub verifiers_run: usize,
}

/// Recover the deciding evidence's HONEST [`CasRef`] from the store — the
/// finding this closes: the emit site used to reconstruct a `CasRef` with
/// `bytes: 0` from the bare `cas:blake3:` string, so a durable decision fact
/// misreported its own evidence blob's size. Here we resolve the FIRST
/// evidence's ref hash and recover the blob's TRUE byte length from the store's
/// filesystem metadata — a `stat`, never an inline read of the blob content
/// (I2: we size the blob without routing its bytes anywhere). The store does
/// not persist a per-blob mime, so an opaque CAS blob is honestly
/// `application/octet-stream` (the house convention for a ref whose content
/// format is not recorded — never a fabricated `text/plain` claim).
///
/// Returns `None` when there is no evidence, the evidence carries no ref, the
/// ref is not a `cas:blake3:` ref, or the blob cannot be `stat`ed (a missing
/// blob is not an error here — it just means no honest metadata to pin, so the
/// emit site omits the ref rather than assert a fabricated one).
fn honest_evidence_ref(evidence: &[Evidence], cas: &Cas) -> Option<CasRef> {
    let hash = evidence
        .first()
        .and_then(|e| e.cas_ref.as_deref())
        .and_then(|r| r.strip_prefix("cas:blake3:"))?;
    // `stat` the content-addressed blob for its TRUE size — metadata only, the
    // bytes never leave the store (I2). A blob the store cannot stat yields no
    // honest ref (None), never a fabricated one.
    let bytes = std::fs::metadata(cas.path_for(hash)).ok()?.len();
    Some(CasRef {
        hash: hash.to_string(),
        bytes,
        mime: "application/octet-stream".to_string(),
    })
}

/// Merge a verifier's own pinned `params` over the shared request-axis `params`.
/// The request axis (the requested `tool`, target `paths`, folded run state)
/// rides `input.params`; each verifier's config (`allow`, caps, knobs) rides its
/// `PermitVerifierSpec.params`. Both must reach the native as one pinned object,
/// so we start from the request axis and overlay the verifier's keys (verifier
/// config wins on a key collision — the config is the policy). Non-object
/// verifier params are ignored (there is nothing to overlay).
fn merge_params(request: &Value, verifier: &Value) -> Value {
    let mut merged = request.clone();
    if let (Some(target), Some(overlay)) = (merged.as_object_mut(), verifier.as_object()) {
        for (k, v) in overlay {
            target.insert(k.clone(), v.clone());
        }
    }
    merged
}

/// Dispatch a configured permit-verifier set IN ORDER and aggregate the verdicts
/// into one permit outcome (the SP-wire aggregation seam; mirrors the S4
/// `run_gate` first-fail short-circuit, mapped to a [`PermitDecision`]).
///
/// Semantics (the aggregation the oracle pins):
/// - run the natives in order; the FIRST `Fail` SHORT-CIRCUITS → the outcome is
///   Deny carrying THAT verifier's evidence/params (Fail > Escalate; deny is
///   stronger, I6);
/// - an `Inconclusive` does NOT short-circuit — the scan continues (only Fail
///   stops it), but the FIRST inconclusive is remembered as the fallback deciding
///   verifier;
/// - if the scan completes with no Fail but at least one Inconclusive → Escalate
///   carrying the first inconclusive's evidence (route to a human, NEVER coerced
///   to allow, I6);
/// - all `Pass` → Grant;
/// - an EMPTY configured set → Escalate (undecidable is not a synthesized allow,
///   I6) — the deciding verifier is empty and there is no evidence.
///
/// The aggregate verdict maps via [`decision_for`] — the aggregation layer reuses
/// the ratified honesty mapping, never re-deriving it.
///
/// An unknown native name is a can't-run → `Inconclusive` (honest), NEVER a pass.
pub fn aggregate(
    set: &[PermitVerifierSpec],
    input: &VerifierInput,
    cas: &Cas,
) -> Result<PermitOutcome, GateError> {
    let natives = builtin_natives();

    // Empty configured set: undecidable → Escalate, never a synthesized allow (I6).
    if set.is_empty() {
        return Ok(PermitOutcome {
            verdict: Verdict::Inconclusive,
            decision: decision_for(Verdict::Inconclusive),
            deciding_verifier: String::new(),
            deciding_params: Value::Null,
            evidence: Vec::new(),
            deciding_evidence_ref: None,
            verifiers_run: 0,
        });
    }

    // The first inconclusive is the escalate-fallback deciding verifier; it is
    // only promoted to the outcome if the scan completes without a Fail.
    let mut first_inconclusive: Option<(String, Value, Vec<Evidence>)> = None;
    let mut verifiers_run = 0usize;

    for spec in set {
        verifiers_run += 1;
        let merged = merge_params(&input.params, &spec.params);
        let per_input = VerifierInput {
            gate: input.gate.clone(),
            workspace: input.workspace.clone(),
            refs: input.refs.clone(),
            params: merged.clone(),
            timeout_ms: input.timeout_ms,
        };

        // Resolve the native by name; an unknown name is a can't-run →
        // Inconclusive (honest), never a pass (I6).
        let out = match natives.iter().find(|n| n.name() == spec.name) {
            Some(native) => native.verify(&per_input, cas)?,
            None => {
                let evidence = vec![Evidence {
                    kind: "cannot-run".to_string(),
                    msg: format!("unknown native verifier {}", spec.name),
                    cas_ref: None,
                }];
                if first_inconclusive.is_none() {
                    first_inconclusive =
                        Some((spec.name.clone(), merged.clone(), evidence.clone()));
                }
                continue;
            }
        };

        match out.verdict {
            Verdict::Fail => {
                // First Fail short-circuits → Deny carrying THIS verifier's
                // evidence/params (Fail > any earlier Inconclusive, I6).
                let deciding_evidence_ref = honest_evidence_ref(&out.evidence, cas);
                return Ok(PermitOutcome {
                    verdict: Verdict::Fail,
                    decision: decision_for(Verdict::Fail),
                    deciding_verifier: spec.name.clone(),
                    deciding_params: merged,
                    evidence: out.evidence,
                    deciding_evidence_ref,
                    verifiers_run,
                });
            }
            Verdict::Inconclusive => {
                // Does NOT short-circuit; remember the first inconclusive as the
                // fallback deciding verifier and keep scanning for a later Fail.
                if first_inconclusive.is_none() {
                    first_inconclusive = Some((spec.name.clone(), merged, out.evidence));
                }
            }
            Verdict::Pass => {}
        }
    }

    // No Fail. If any Inconclusive was seen → Escalate carrying the FIRST one's
    // evidence; else all passed → Grant.
    match first_inconclusive {
        Some((name, params, evidence)) => {
            let deciding_evidence_ref = honest_evidence_ref(&evidence, cas);
            Ok(PermitOutcome {
                verdict: Verdict::Inconclusive,
                decision: decision_for(Verdict::Inconclusive),
                deciding_verifier: name,
                deciding_params: params,
                evidence,
                deciding_evidence_ref,
                verifiers_run,
            })
        }
        None => {
            // All passed. The deciding verifier is the last one that ran (a
            // grant has no single "reason"; the caller pins the whole set's tail
            // as the policy descriptor). No evidence on a grant.
            let last = set.last().expect("non-empty set has a last entry");
            let merged = merge_params(&input.params, &last.params);
            Ok(PermitOutcome {
                verdict: Verdict::Pass,
                decision: decision_for(Verdict::Pass),
                deciding_verifier: last.name.clone(),
                deciding_params: merged,
                evidence: Vec::new(),
                deciding_evidence_ref: None,
                verifiers_run,
            })
        }
    }
}
