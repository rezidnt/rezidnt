//! SP0 oracle — the permit EMIT path (the PDP turning a decision into a
//! `permit.*` fact payload). DR-008 / DR-009; design §5 (wire shape), §8 (new
//! subjects); ontology "permit set" (payload schemas, lines 316-342).
//!
//! RED MODE: **compile-red**. The emit side does not exist: this references
//! `rezidnt_gate::permit::{decided_fact, requested_fact}` — the constructors
//! the PDP uses to build the fact payloads it logs. The crate fails to compile
//! until they land; then these assert the two invariants the emit path must
//! honor:
//!   - CRITERION 4: a decision fact carries `policy_ref` + (optional)
//!     `evidence_ref` so `gate why` / `gate_explain` can resolve the deciding
//!     policy and evidence (I6 interrogability; ontology lines 324-342).
//!   - CRITERION 5: a request with large action context carries a
//!     `context_ref: CasRef`, NEVER the bytes inline, and the payload stays
//!     under the 32 KiB envelope cap (I2; ontology line 321, design §5).
//!
//! These pin the EMIT shape the reducer (already landed, `rezidnt-state`) folds
//! — the fold side is locked by the state-crate fixtures; this locks the
//! producer so producer and consumer agree on the wire shape.

use rezidnt_types::MAX_PAYLOAD_BYTES;
use rezidnt_types::refs::CasRef;

use rezidnt_gate::Verdict;
use rezidnt_gate::permit;

fn cas_ref(hash: &str, bytes: u64) -> CasRef {
    CasRef {
        hash: hash.to_string(),
        bytes,
        mime: "application/octet-stream".to_string(),
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
