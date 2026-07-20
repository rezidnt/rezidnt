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

use crate::{Evidence, ExecVerifier, GateError, Verdict, VerifierInput, builtin_natives};

/// The fourth gate lifecycle point (design §4; project spec `[gates.permit]`).
/// String-modeled like the existing three — a `GateDef` whose `name` is this is
/// a permit gate.
pub const LIFECYCLE_POINT: &str = "permit";

/// The policy LAYER a permit-verifier was sourced from (SP4c — C8 layered
/// precedence, DR-019 Decision 3). Three layers compose by CONCATENATING their
/// specs in the fixed `Admin → Dev → Session` order ([`compose_layers`]);
/// stricter-wins is INHERITED from the existing monotone aggregate (no
/// allow-override primitive exists, so a later layer can never un-Fail an
/// earlier layer's deny). The layer is provenance ONLY — it rides each spec so
/// the decision fact / `gate_explain` can name the deciding *layer*, not merely
/// the deciding verifier (I6 interrogability). It changes no verdict logic.
///
/// `Session` is the LEAST-authority layer and the default the layer-agnostic
/// [`PermitVerifierSpec::native`]/[`PermitVerifierSpec::exec`] constructors
/// stamp, so pre-SP4c call sites keep their behavior unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermitLayer {
    /// Host/daemon-sourced policy — the highest authority. An admin deny is
    /// non-overridable by a later (dev/session) layer.
    Admin,
    /// Workspace-applied policy (`workspace.spec.applied`, I3).
    Dev,
    /// Run/agent-sourced policy — the least authority (and the default).
    Session,
}

impl PermitLayer {
    /// The stable string the decision fact / `gate_explain` carries for this
    /// layer: `"admin"` / `"dev"` / `"session"` (I6 — "why blocked" answers with
    /// the layer name).
    pub fn as_str(&self) -> &'static str {
        match self {
            PermitLayer::Admin => "admin",
            PermitLayer::Dev => "dev",
            PermitLayer::Session => "session",
        }
    }
}

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

/// The kind of a configured permit-verifier entry (DR-015 §Decision 1). A
/// permit entry is EITHER a built-in native (resolved by name via
/// [`builtin_natives`]) or an exec program (an argv speaking the §8 JSON
/// contract, dispatched through [`crate::ExecVerifier`]). The two dispatch
/// differently — natives run sync/in-process, exec runs as an `await`ed
/// subprocess — so the async aggregator ([`aggregate_async`]) branches on this.
#[derive(Debug, Clone, PartialEq)]
pub enum PermitVerifierKind {
    /// A built-in native verifier resolved by [`PermitVerifierSpec::name`].
    Native,
    /// An exec verifier: the argv (interpreter + policy path + args) the
    /// operator provides. rezidnt dispatches it, never bundles the engine (I7).
    Exec { argv: Vec<String> },
}

/// One resolved entry in the configured `[gates.permit]` verifier set: a display
/// name, its content-pinned params, and its [`PermitVerifierKind`] (native or
/// exec, DR-015 §Decision 1). This is the SP-wire dispatch unit — the daemon
/// builds it from `VerifierSpec` plus the folded per-run state injected as
/// content-pinned params (determinism BINDING). The aggregator merges each
/// verifier's own `params` with the shared request axis (the requested `tool`,
/// target `paths`, etc.) so both the request and the policy config reach the
/// verifier as one pinned `inputs.params` object.
#[derive(Debug, Clone, PartialEq)]
pub struct PermitVerifierSpec {
    /// The display name: a native's registry name (resolved via
    /// [`builtin_natives`]) or an exec verifier's operator-chosen label. It is
    /// the `deciding_verifier` the decision fact records (I6 interrogability).
    pub name: String,
    /// The verifier's pinned params (its `[gates.permit].verifiers` config, e.g.
    /// `{ "allow": [...] }`), merged over the shared request axis.
    pub params: Value,
    /// Native vs exec dispatch (DR-015 §Decision 1). Private so the kind is only
    /// set through the [`Self::native`]/[`Self::exec`] constructors — a caller
    /// cannot mint a half-formed exec entry with no argv.
    kind: PermitVerifierKind,
    /// The policy layer this spec was sourced from (SP4c — C8, DR-019 Decision
    /// 3). Provenance only: it names the deciding LAYER on the outcome and
    /// changes no verdict logic. Private so it is only set through a constructor;
    /// the layer-agnostic [`Self::native`]/[`Self::exec`] default it to
    /// [`PermitLayer::Session`] (least authority) so no pre-SP4c site regresses.
    layer: PermitLayer,
}

impl PermitVerifierSpec {
    /// A native permit entry: a registry name + its pinned params (DR-015
    /// §Decision 1). Dispatched in-process by resolving `name` against
    /// [`builtin_natives`]. Its layer defaults to [`PermitLayer::Session`] (the
    /// least-authority layer) — use [`Self::native_in_layer`] to stamp a
    /// different provenance (SP4c, DR-019).
    pub fn native(name: impl Into<String>, params: Value) -> Self {
        Self::native_in_layer(PermitLayer::Session, name, params)
    }

    /// A native permit entry STAMPED with its source layer (SP4c — C8, DR-019
    /// Decision 3). Provenance-carrying sibling of [`Self::native`]; the layer is
    /// surfaced as the deciding layer on the outcome and changes no verdict logic.
    pub fn native_in_layer(layer: PermitLayer, name: impl Into<String>, params: Value) -> Self {
        Self {
            name: name.into(),
            params,
            kind: PermitVerifierKind::Native,
            layer,
        }
    }

    /// An exec permit entry: a display name + the argv (interpreter + policy
    /// path + args) + its pinned params (DR-015 §Decision 1). Dispatched as an
    /// `await`ed subprocess through [`crate::ExecVerifier`] speaking the §8 JSON
    /// contract — rezidnt ships the dispatch, never the engine (I7). Its layer
    /// defaults to [`PermitLayer::Session`] — use [`Self::exec_in_layer`] to
    /// stamp a different provenance (SP4c, DR-019).
    pub fn exec(name: impl Into<String>, argv: Vec<String>, params: Value) -> Self {
        Self::exec_in_layer(PermitLayer::Session, name, argv, params)
    }

    /// An exec permit entry STAMPED with its source layer (SP4c — C8, DR-019
    /// Decision 3). Provenance-carrying sibling of [`Self::exec`].
    pub fn exec_in_layer(
        layer: PermitLayer,
        name: impl Into<String>,
        argv: Vec<String>,
        params: Value,
    ) -> Self {
        Self {
            name: name.into(),
            params,
            kind: PermitVerifierKind::Exec { argv },
            layer,
        }
    }

    /// This entry's dispatch kind (native vs exec).
    pub fn kind(&self) -> &PermitVerifierKind {
        &self.kind
    }

    /// This entry's source policy layer (SP4c provenance, DR-019).
    pub fn layer(&self) -> PermitLayer {
        self.layer
    }
}

/// Compose the three policy layers into one flat, ordered verifier set by
/// CONCATENATING them in the fixed `admin → dev → session` order (SP4c — C8,
/// DR-019 Decision 1). Each spec's layer provenance is preserved. This is the
/// ONLY new merge: the aggregate ([`aggregate`]/[`aggregate_async`]) and the
/// verdict→decision table are UNCHANGED. Stricter-wins is inherited from the
/// existing monotone aggregate — because there is no allow-override primitive, a
/// later (dev/session) layer can never un-Fail an earlier (admin) layer's deny;
/// admin's specs simply run FIRST, so its first Fail short-circuits before any
/// later layer runs.
///
/// An absent or empty layer contributes ZERO verifiers (an all-empty resolution
/// yields the empty set, which the aggregate ESCALATES — never a synthesized
/// allow, DR-011 §3 / DR-019 criterion 4).
pub fn compose_layers(
    admin: Vec<PermitVerifierSpec>,
    dev: Vec<PermitVerifierSpec>,
    session: Vec<PermitVerifierSpec>,
) -> Vec<PermitVerifierSpec> {
    let mut merged = Vec::with_capacity(admin.len() + dev.len() + session.len());
    merged.extend(admin);
    merged.extend(dev);
    merged.extend(session);
    merged
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
    /// The policy LAYER of the deciding verifier (SP4c — C8, DR-019 Decision 3):
    /// the first Fail's layer for a Deny, the escalating verifier's layer for an
    /// Escalate, the last passing verifier's layer for a Grant. `None` ONLY for
    /// the empty-set escalate (no verifier decided, so no layer to name). This is
    /// provenance surfaced ALONGSIDE the existing decision — the verdict→decision
    /// mapping is unchanged; `gate_explain` uses it to answer "why blocked" with
    /// the deciding *layer*, not merely the verifier (I6).
    pub deciding_layer: Option<PermitLayer>,
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
///
/// `pub` so the emit site can reconstruct a verifier's EXACT pinned view (request
/// axis ∪ its spec params) to stamp a delta the verdict cannot diverge from —
/// DR-024 Q5's shared-input guarantee for the `risk-cap` producer seam.
pub fn merge_params(request: &Value, verifier: &Value) -> Value {
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
    // No verifier decided, so there is no deciding layer to name (DR-019: `None`
    // is reserved for exactly this empty-set escalate).
    if set.is_empty() {
        return Ok(PermitOutcome {
            verdict: Verdict::Inconclusive,
            decision: decision_for(Verdict::Inconclusive),
            deciding_verifier: String::new(),
            deciding_layer: None,
            deciding_params: Value::Null,
            evidence: Vec::new(),
            deciding_evidence_ref: None,
            verifiers_run: 0,
        });
    }

    // The first inconclusive is the escalate-fallback deciding verifier; it is
    // only promoted to the outcome if the scan completes without a Fail. Its
    // LAYER rides alongside so an escalate names the escalating layer (DR-019).
    let mut first_inconclusive: Option<(String, PermitLayer, Value, Vec<Evidence>)> = None;
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
                    first_inconclusive = Some((
                        spec.name.clone(),
                        spec.layer,
                        merged.clone(),
                        evidence.clone(),
                    ));
                }
                continue;
            }
        };

        match out.verdict {
            Verdict::Fail => {
                // First Fail short-circuits → Deny carrying THIS verifier's
                // evidence/params + its LAYER (Fail > any earlier Inconclusive,
                // I6). The deciding layer is admin's when admin ran first (C8).
                let deciding_evidence_ref = honest_evidence_ref(&out.evidence, cas);
                return Ok(PermitOutcome {
                    verdict: Verdict::Fail,
                    decision: decision_for(Verdict::Fail),
                    deciding_verifier: spec.name.clone(),
                    deciding_layer: Some(spec.layer),
                    deciding_params: merged,
                    evidence: out.evidence,
                    deciding_evidence_ref,
                    verifiers_run,
                });
            }
            Verdict::Inconclusive => {
                // Does NOT short-circuit; remember the first inconclusive (with
                // its layer) as the fallback deciding verifier and keep scanning.
                if first_inconclusive.is_none() {
                    first_inconclusive =
                        Some((spec.name.clone(), spec.layer, merged, out.evidence));
                }
            }
            Verdict::Pass => {}
        }
    }

    // No Fail. If any Inconclusive was seen → Escalate carrying the FIRST one's
    // evidence + layer; else all passed → Grant.
    match first_inconclusive {
        Some((name, layer, params, evidence)) => {
            let deciding_evidence_ref = honest_evidence_ref(&evidence, cas);
            Ok(PermitOutcome {
                verdict: Verdict::Inconclusive,
                decision: decision_for(Verdict::Inconclusive),
                deciding_verifier: name,
                deciding_layer: Some(layer),
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
                deciding_layer: Some(last.layer),
                deciding_params: merged,
                evidence: Vec::new(),
                deciding_evidence_ref: None,
                verifiers_run,
            })
        }
    }
}

/// Run ONE native permit-verifier by name and return its owned verdict +
/// evidence. Fully SYNCHRONOUS: it builds and drops the `builtin_natives` pack
/// (`Vec<Box<dyn NativeVerifier>>`, which is not `Send`) entirely within this
/// scope, so [`aggregate_async`] can call it without holding a non-`Send` value
/// across the exec `.await` (the aggregate future is `spawn`ed on the
/// multi-thread runtime). An unknown native name is an honest can't-run →
/// `Inconclusive`, never a pass (I6).
fn native_verdict(
    name: &str,
    input: &VerifierInput,
    cas: &Cas,
) -> Result<(Verdict, Vec<Evidence>), GateError> {
    let natives = builtin_natives();
    match natives.iter().find(|n| n.name() == name) {
        Some(native) => {
            let out = native.verify(input, cas)?;
            Ok((out.verdict, out.evidence))
        }
        None => Ok((
            Verdict::Inconclusive,
            vec![Evidence {
                kind: "cannot-run".to_string(),
                msg: format!("unknown native verifier {name}"),
                cas_ref: None,
            }],
        )),
    }
}

/// Dispatch a HETEROGENEOUS permit-verifier set (natives + exec) IN ORDER and
/// aggregate the verdicts into one permit outcome — the SP3 async lift (DR-015
/// §Decision 2, option A). This is the async sibling of [`aggregate`]: natives
/// run sync/in-process (CPU + CAS by design), exec entries run as an `await`ed
/// subprocess through the existing [`ExecVerifier`] (§8 stdin→stdout,
/// network-off + scrubbed env, nonzero/malformed/timeout → `inconclusive`), and
/// the scan interleaves both kinds in CONFIGURED ORDER so first-`Fail`→Deny
/// short-circuits ACROSS kinds (a native Fail can stop a later exec and an exec
/// Fail a later native). No `block_on` — the exec subprocess stays visible to
/// the scheduler and the hot-path timeout (DR-015 rejects (B)).
///
/// The aggregation semantics are IDENTICAL to [`aggregate`] (the ratified
/// honesty mapping is not re-derived): first `Fail` short-circuits → Deny; an
/// `Inconclusive` does not short-circuit but is remembered as the escalate
/// fallback; a complete scan with any `Inconclusive` → Escalate; all `Pass` →
/// Grant; an EMPTY set → Escalate (undecidable is never a synthesized allow,
/// I6). An unknown native name and a non-running exec both surface as
/// `Inconclusive` (honest can't-run), NEVER a pass (I6).
pub async fn aggregate_async(
    set: &[PermitVerifierSpec],
    input: &VerifierInput,
    cas: &Cas,
) -> Result<PermitOutcome, GateError> {
    // Empty configured set: undecidable → Escalate, never a synthesized allow (I6).
    // No verifier decided, so there is no deciding layer to name (DR-019).
    if set.is_empty() {
        return Ok(PermitOutcome {
            verdict: Verdict::Inconclusive,
            decision: decision_for(Verdict::Inconclusive),
            deciding_verifier: String::new(),
            deciding_layer: None,
            deciding_params: Value::Null,
            evidence: Vec::new(),
            deciding_evidence_ref: None,
            verifiers_run: 0,
        });
    }

    // The first inconclusive is the escalate-fallback deciding verifier; it is
    // only promoted to the outcome if the scan completes without a Fail. Its
    // LAYER rides alongside so an escalate names the escalating layer (DR-019).
    let mut first_inconclusive: Option<(String, PermitLayer, Value, Vec<Evidence>)> = None;
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

        // Resolve the verdict per kind: a native runs sync/in-process; an exec
        // entry is `await`ed through the existing ExecVerifier (§8 contract). An
        // unknown native name and a non-running exec both yield an honest
        // Inconclusive verdict, never a pass (I6).
        //
        // The native pack (`Vec<Box<dyn NativeVerifier>>`) is NOT `Send` and must
        // never be held across the exec `.await` (this future is `spawn`ed on the
        // multi-thread runtime). `native_verdict` builds and drops the pack
        // entirely inside a sync scope, returning an owned verdict; the exec
        // branch never touches it.
        let (verdict, evidence) = match spec.kind() {
            PermitVerifierKind::Native => native_verdict(&spec.name, &per_input, cas)?,
            PermitVerifierKind::Exec { argv } => {
                let verifier = ExecVerifier {
                    name: spec.name.clone(),
                    argv: argv.clone(),
                };
                // Infallible by design: nonzero-exit / malformed / timeout /
                // could-not-run all return an `Inconclusive` VerdictRecord —
                // never a synthesized allow (I6, DR-015 §Decision 3).
                let record = verifier.run(&per_input).await;
                (record.verdict, record.evidence)
            }
        };

        match verdict {
            Verdict::Fail => {
                // First Fail short-circuits → Deny carrying THIS verifier's
                // evidence/params + its LAYER (Fail > any earlier Inconclusive,
                // across native+exec kinds, I6).
                let deciding_evidence_ref = honest_evidence_ref(&evidence, cas);
                return Ok(PermitOutcome {
                    verdict: Verdict::Fail,
                    decision: decision_for(Verdict::Fail),
                    deciding_verifier: spec.name.clone(),
                    deciding_layer: Some(spec.layer),
                    deciding_params: merged,
                    evidence,
                    deciding_evidence_ref,
                    verifiers_run,
                });
            }
            Verdict::Inconclusive => {
                // Does NOT short-circuit; remember the first inconclusive (with
                // its layer) as the fallback deciding verifier and keep scanning.
                if first_inconclusive.is_none() {
                    first_inconclusive = Some((spec.name.clone(), spec.layer, merged, evidence));
                }
            }
            Verdict::Pass => {}
        }
    }

    // No Fail. If any Inconclusive was seen → Escalate carrying the FIRST one's
    // evidence + layer; else all passed → Grant.
    match first_inconclusive {
        Some((name, layer, params, evidence)) => {
            let deciding_evidence_ref = honest_evidence_ref(&evidence, cas);
            Ok(PermitOutcome {
                verdict: Verdict::Inconclusive,
                decision: decision_for(Verdict::Inconclusive),
                deciding_verifier: name,
                deciding_layer: Some(layer),
                deciding_params: params,
                evidence,
                deciding_evidence_ref,
                verifiers_run,
            })
        }
        None => {
            // All passed. The deciding verifier is the last one that ran; no
            // evidence on a grant.
            let last = set.last().expect("non-empty set has a last entry");
            let merged = merge_params(&input.params, &last.params);
            Ok(PermitOutcome {
                verdict: Verdict::Pass,
                decision: decision_for(Verdict::Pass),
                deciding_verifier: last.name.clone(),
                deciding_layer: Some(last.layer),
                deciding_params: merged,
                evidence: Vec::new(),
                deciding_evidence_ref: None,
                verifiers_run,
            })
        }
    }
}
