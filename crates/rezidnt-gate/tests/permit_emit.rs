//! SP0/SP1 oracle — the permit EMIT path (the PDP turning a decision into a
//! `permit.*` fact payload). DR-008 / DR-009; design §5 (wire shape), §8 (new
//! subjects); ontology "permit set" (payload schemas, lines 316-342).
//!
//! RED MODE: **compile-red**. Two layers pin here.
//!
//! SP0 (already landed for the OLD 6-arg `decided_fact`): a decision fact
//! carries `policy_ref` + (optional) `evidence_ref` + `reason` (CRITERION 4),
//! and a request with large action context carries `context_ref: CasRef`
//! (CRITERION 5). Those assertions are unchanged below.
//!
//! SP1 CRITERION 4 (carried SP0 flag — the emit-side pin): the C1 spend-cap
//! verifier is the PRODUCER of the accumulator deltas the reducer already folds
//! (`rezidnt-state` reads `spend_delta_usd` / `risk_delta`; `permit_ledger.rs`
//! fixtures CARRY them) — but nothing on the PRODUCER side pins those keys.
//! This closes that drift: `permit::decided_fact` gains the accumulator/cost
//! params (`spend_delta_usd`, `risk_delta`, `cost_ms`) and MUST emit them onto
//! the decision payload, omitted-when-absent (never JSON `null`), so producer
//! and consumer agree on the wire shape the ledger folds.
//!
//! This is **compile-red** now: every `decided_fact(...)` call site below uses
//! the NEW signature (a trailing `DecisionDeltas`), which does not exist yet.
//! The implementer widens the signature to make the crate compile; the
//! assertions then pin that the deltas ride the payload (and are omitted when
//! `None`). Widening `decided_fact` is what turns the pre-existing SP0 six-arg
//! call sites red — that is the intended forcing function for item 4.

use rezidnt_types::MAX_PAYLOAD_BYTES;
use rezidnt_types::refs::CasRef;

use rezidnt_gate::Verdict;
use rezidnt_gate::permit;
use rezidnt_gate::permit::DecisionDeltas;

fn cas_ref(hash: &str, bytes: u64) -> CasRef {
    CasRef {
        hash: hash.to_string(),
        bytes,
        mime: "application/octet-stream".to_string(),
    }
}

/// The all-absent deltas (`None`/`None`/`None`) — a decision that measured no
/// spend, risk, or cost. Used by the SP0 assertions, which must NOT emit any
/// of the optional accumulator/cost keys.
fn no_deltas() -> DecisionDeltas {
    DecisionDeltas {
        spend_delta_usd: None,
        risk_delta: None,
        cost_ms: None,
    }
}

/// CRITERION 4 — a GRANT decision fact carries `run`, `request_id`, and the
/// deciding `policy_ref` so the grant is interrogable (I6). The fact's subject
/// is `permit.granted` (verdict carried by the subject, ontology line 153).
///
/// COMPILE-RED until `permit::decided_fact` exists.
#[test]
fn granted_fact_carries_policy_ref_for_interrogability() {
    let policy = cas_ref(
        "po11c1000000000000000000000000000000000000000000000000000000gr",
        128,
    );
    let (subject, payload) = permit::decided_fact(
        Verdict::Pass,
        "01SP0RUN00000000000000000R1",
        "01SP0REQ00000000000000000Q1",
        &policy,
        None, // evidence_ref: a trivially-permitted action may have none
        None, // reason: grants carry none
        no_deltas(),
    );
    assert_eq!(
        subject, "permit.granted",
        "verdict carried by the subject (I6)"
    );
    assert_eq!(payload["run"], "01SP0RUN00000000000000000R1");
    assert_eq!(payload["request_id"], "01SP0REQ00000000000000000Q1");
    assert_eq!(
        payload["policy_ref"]["hash"], policy.hash,
        "the deciding policy is on the fact so `gate_explain` can resolve it (I6)"
    );
    // A grant has no denial/escalation reason.
    assert!(payload.get("reason").is_none() || payload["reason"].is_null());
    // All-absent deltas: the optional accumulator/cost keys are OMITTED, never
    // emitted as JSON null (the reducer reads `.as_f64()` — a null is not a 0).
    assert!(
        payload.get("spend_delta_usd").is_none(),
        "absent spend delta is an omitted key, never null"
    );
    assert!(
        payload.get("risk_delta").is_none(),
        "absent risk delta is an omitted key, never null"
    );
    assert!(
        payload.get("cost_ms").is_none(),
        "absent cost is an omitted key, never null"
    );
}

/// CRITERION 4 — a DENY decision fact carries `policy_ref`, `evidence_ref`, and
/// a short `reason` so a blocked agent can always read *why* (I6; ontology
/// lines 334-337). The subject is `permit.denied`.
///
/// COMPILE-RED until `permit::decided_fact` exists.
#[test]
fn denied_fact_carries_policy_evidence_and_reason() {
    let policy = cas_ref(
        "po11c1000000000000000000000000000000000000000000000000000000de",
        96,
    );
    let evidence = cas_ref(
        "ev1dence00000000000000000000000000000000000000000000000000de01",
        42,
    );
    let (subject, payload) = permit::decided_fact(
        Verdict::Fail,
        "01SP0RUN00000000000000000R1",
        "01SP0REQ00000000000000000Q2",
        &policy,
        Some(&evidence),
        Some("path outside allowed scope"),
        no_deltas(),
    );
    assert_eq!(subject, "permit.denied");
    assert_eq!(payload["policy_ref"]["hash"], policy.hash);
    assert_eq!(
        payload["evidence_ref"]["hash"], evidence.hash,
        "denial evidence rides as a CAS ref, never inline (I2)"
    );
    assert_eq!(
        payload["reason"], "path outside allowed scope",
        "a blocked agent can always read WHY (I6, DR-005 read-class interrogation)"
    );
}

/// CRITERION 3+4 (emit leg) — an ESCALATE decision fact is `permit.escalated`,
/// carries `policy_ref` + `reason`, and is NEVER emitted as `permit.granted`.
/// The honesty invariant enforced at the producer: an inconclusive verdict
/// cannot be logged as an allow.
///
/// COMPILE-RED until `permit::decided_fact` exists.
#[test]
fn escalated_fact_is_never_emitted_as_granted() {
    let policy = cas_ref(
        "po11c1000000000000000000000000000000000000000000000000000000es",
        64,
    );
    let (subject, payload) = permit::decided_fact(
        Verdict::Inconclusive,
        "01SP0RUN00000000000000000R1",
        "01SP0REQ00000000000000000Q3",
        &policy,
        None,
        Some("cumulative spend crossed soft cap"),
        no_deltas(),
    );
    assert_eq!(
        subject, "permit.escalated",
        "inconclusive is logged as escalate, NEVER coerced to granted (I6, DR-008 §4)"
    );
    assert_ne!(subject, "permit.granted");
    assert_eq!(payload["reason"], "cumulative spend crossed soft cap");
    assert_eq!(payload["policy_ref"]["hash"], policy.hash);
}

/// CRITERION 5 — **I2: bulk context is a CasRef, never inline.** A request with
/// large action context carries `context_ref: CasRef` and the payload stays
/// well under the 32 KiB hard cap; the raw bytes appear NOWHERE in the payload.
/// This is the I2 boundary tested the way the envelope tests it
/// (`MAX_PAYLOAD_BYTES`, `rezidnt-types`).
///
/// COMPILE-RED until `permit::requested_fact` exists.
#[test]
fn request_with_large_context_carries_a_cas_ref_not_inline_bytes() {
    // A realistic "large action context": a big argv / file blob the agent
    // wants to act on. Far bigger than the 32 KiB envelope cap on its own.
    let big_context = "x".repeat(64 * 1024);
    let context_ref = cas_ref(
        "c0n7ex7000000000000000000000000000000000000000000000000000big1",
        big_context.len() as u64,
    );

    let (subject, payload) = permit::requested_fact(
        "01SP0RUN00000000000000000R1",
        "01SP0REQ00000000000000000Q4",
        "tool.invoke",
        "Bash", // small inline target descriptor (tool name)
        Some(&context_ref),
    );

    assert_eq!(subject, "permit.requested");
    assert_eq!(
        payload["context_ref"]["hash"], context_ref.hash,
        "bulk context rides as a CAS ref (I2)"
    );

    // The bytes must NOT be inlined anywhere in the payload.
    let serialized = serde_json::to_vec(&payload).expect("payload serializes");
    assert!(
        !serialized
            .windows(big_context.len())
            .any(|w| w == big_context.as_bytes()),
        "the large context bytes must NEVER appear inline in the fact payload (I2)"
    );
    assert!(
        serialized.len() <= MAX_PAYLOAD_BYTES,
        "the request payload ({} bytes) must stay under the 32 KiB envelope cap ({}) — I2",
        serialized.len(),
        MAX_PAYLOAD_BYTES
    );
}

/// CRITERION 5 (envelope leg) — the emitted request payload is a legal Event
/// envelope: constructing an `Event` from it does not trip the payload-size
/// guard. This proves the emit path respects the same I2 boundary the fabric
/// enforces (payload ≤ 32 KiB → else CAS ref).
///
/// COMPILE-RED until `permit::requested_fact` exists.
#[test]
fn request_fact_is_a_legal_envelope_payload() {
    use rezidnt_types::{Event, SourceId, Subject};
    use ulid::Ulid;

    let context_ref = cas_ref(
        "c0n7ex7000000000000000000000000000000000000000000000000000big2",
        64 * 1024,
    );
    let (subject, payload) = permit::requested_fact(
        "01SP0RUN00000000000000000R1",
        "01SP0REQ00000000000000000Q5",
        "tool.invoke",
        "Bash",
        Some(&context_ref),
    );
    let event = Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    );
    assert!(
        event.is_ok(),
        "a request carrying a context_ref (not inline bytes) is a legal ≤32KiB envelope (I2)"
    );
}

// --- SP1 CRITERION 4: the emit-side accumulator/cost pin (carried SP0 flag) --
//
// The reducer already READS `spend_delta_usd` / `risk_delta` and folds them
// into `PermitAccumulators` (`rezidnt-state`), and the golden fixtures CARRY
// them — but no PRODUCER test pins that `decided_fact` EMITS them. The C1
// spend-cap verifier is the producer; these lock the wire keys it must write so
// producer and the already-green consumer agree.

/// A GRANT that charged spend and risk and measured its cost emits all three
/// keys with the EXACT values passed — the reducer folds `spend_delta_usd`
/// into cumulative spend and `risk_delta` into the running risk score
/// (`rezidnt-state::apply_permit_decision`; `permit_ledger.rs` locks the fold).
///
/// COMPILE-RED until `decided_fact` takes `DecisionDeltas` and emits the keys.
#[test]
fn granted_fact_emits_spend_risk_and_cost_deltas() {
    let policy = cas_ref(
        "po11c1000000000000000000000000000000000000000000000000000000g2",
        100,
    );
    let (subject, payload) = permit::decided_fact(
        Verdict::Pass,
        "01SP1RUN00000000000000000R1",
        "01SP1REQ00000000000000000Q1",
        &policy,
        None,
        None,
        DecisionDeltas {
            spend_delta_usd: Some(0.25),
            risk_delta: Some(1.0),
            cost_ms: Some(3),
        },
    );
    assert_eq!(subject, "permit.granted");
    assert_eq!(
        payload["spend_delta_usd"].as_f64(),
        Some(0.25),
        "the granted action's incremental spend rides the payload (C1 producer → reducer fold)"
    );
    assert_eq!(
        payload["risk_delta"].as_f64(),
        Some(1.0),
        "the granted action's incremental risk rides the payload (C6 running score)"
    );
    assert_eq!(
        payload["cost_ms"].as_u64(),
        Some(3),
        "the §8 stdout decision cost rides the payload (design §10.2 latency tracking)"
    );
}

/// A DENY may still contribute risk (a denied sensitive attempt is signal, C6)
/// while charging no spend and measuring cost. Only the PRESENT deltas appear;
/// the absent `spend_delta_usd` is OMITTED, never null (the reducer's
/// `payload["spend_delta_usd"].as_f64()` must see absence, not a null-that-is-0).
///
/// COMPILE-RED until the new signature lands.
#[test]
fn denied_fact_emits_only_present_deltas_and_omits_absent() {
    let policy = cas_ref(
        "po11c1000000000000000000000000000000000000000000000000000000d2",
        80,
    );
    let evidence = cas_ref(
        "ev1dence00000000000000000000000000000000000000000000000000de02",
        24,
    );
    let (subject, payload) = permit::decided_fact(
        Verdict::Fail,
        "01SP1RUN00000000000000000R1",
        "01SP1REQ00000000000000000Q2",
        &policy,
        Some(&evidence),
        Some("path outside allowed scope"),
        DecisionDeltas {
            spend_delta_usd: None, // a denied action was never charged
            risk_delta: Some(3.0),
            cost_ms: Some(2),
        },
    );
    assert_eq!(subject, "permit.denied");
    assert!(
        payload.get("spend_delta_usd").is_none(),
        "an absent spend delta is OMITTED, never emitted as JSON null (I3 fold correctness)"
    );
    assert_eq!(payload["risk_delta"].as_f64(), Some(3.0));
    assert_eq!(payload["cost_ms"].as_u64(), Some(2));
}

/// The emitted decision is a payload the reducer folds: this is the
/// producer↔consumer round-trip. Build the `permit.granted` payload with
/// `decided_fact`, fold it through `rezidnt-state`, and assert the accumulators.
/// Per DR-021 (C1 fold source moved) the permit fact folds RISK but NO SPEND —
/// even if a `spend_delta_usd` still rides the payload, the reducer must IGNORE
/// it. The MEASURED spend is folded from a SEPARATE post-action `action.metered`
/// fact. This is the drift-closing test: emit and fold must agree on the keys AND
/// on which fact carries spend.
///
/// RED today: the reducer still folds `spend_delta_usd` off the permit fact
/// (crates/rezidnt-state/src/lib.rs:725-726). The permit fact carries a stray
/// 8.0 and the metered fact carries the MEASURED 0.5; today cumulative folds
/// 8.0 (permit) + 0.0 (metered arm absent) = 8.0 ≠ the asserted 0.5. Green only
/// once the permit arm stops reading spend and only the metered fact folds (→
/// 0.0 + 0.5 = 0.5). The divergent values make the fold-source move observable.
#[test]
fn emitted_deltas_fold_through_the_state_reducer() {
    use rezidnt_state::fold;
    use rezidnt_types::{Event, SourceId, Subject};
    use ulid::Ulid;

    const RUN: &str = "01SP1EMITFOLDRUN0000000R01";
    const REQ: &str = "01SP1EMITFOLDREQ0000000Q01";
    let policy = cas_ref(
        "po11c1000000000000000000000000000000000000000000000000000000f2",
        64,
    );
    let (subject, payload) = permit::decided_fact(
        Verdict::Pass,
        RUN,
        REQ,
        &policy,
        None,
        None,
        DecisionDeltas {
            // A stray spend delta on the permit fact — RETIRED as the C1 fold
            // source (DR-021); the reducer must IGNORE it. Deliberately DIFFERENT
            // from the metered delta so a lingering permit-fact fold is observable
            // (it would fold 8.0, not the measured 0.5).
            spend_delta_usd: Some(8.0),
            risk_delta: Some(2.0),
            cost_ms: Some(7),
        },
    );
    let granted = Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("decision payload is a legal envelope");
    // The MEASURED spend rides a POST-action `action.metered` fact — the C1 fold
    // source after DR-021.
    let metered = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("action.metered"),
        Ulid::new(),
        None,
        1,
        serde_json::json!({"run": RUN, "spend_delta_usd": 0.5, "input_tokens": 80, "output_tokens": 30}),
    )
    .expect("metering payload is a legal envelope");
    let events = [granted, metered];
    let graph = fold(events.iter());
    let acc = &graph
        .agent_runs
        .get(RUN)
        .expect("the permit decision mints the run entry (I3)")
        .permit_accumulators;
    assert_eq!(
        acc.cumulative_spend_usd, 0.5,
        "cumulative spend = the MEASURED delta from action.metered ALONE (0.5); the stray \
         spend_delta_usd on the permit fact folds ZERO (retired fold source, DR-021)"
    );
    assert_eq!(
        acc.risk_score, 2.0,
        "the emitted risk_delta STILL folds off the permit fact — C6 untouched by DR-021"
    );
    assert_eq!(acc.granted, 1, "the grant is counted");
}
