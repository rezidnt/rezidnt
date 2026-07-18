//! SP1 oracle — the socket-side `request_permission` path (design
//! `docs/design/permit-engine.md` §3/§5; DR-008/DR-009). The harness PEP reaches
//! the daemon PDP over the socket (not only over MCP): a new `Request` op and a
//! matching decision `Reply` pin the wire shape.
//!
//! RED MODE: **compile-red** — these reference `Request::RequestPermission` and
//! `Reply::PermitDecision`, the SP1 variant pair the implementer adds to the
//! proto enums; the crate fails to compile until they land. Then the
//! assertions pin the round-trip + the never-coerced three-valued decision.
//!
//! Decision vocabulary (I6, DR-008 §4): the reply carries exactly one of
//! `allow | deny | ask`. `ask` is the escalate/inconclusive branch surfaced to
//! the PEP — the harness routes it to a human, it is NEVER coerced to `allow`.
//! Large action context is a `context_ref` CAS-ref string, never inline (I2).

use rezidnt_proto::{Reply, Request, decode_reply, decode_request, encode_reply, encode_request};

/// The request op round-trips as a single JSONL frame and preserves every
/// field (the descriptor is small + inline; bulk context is a ref string).
///
/// COMPILE-RED until `Request::RequestPermission` exists.
#[test]
fn request_permission_round_trips() {
    let request = Request::RequestPermission {
        run: "01SP1SOCKRUN0000000000R001".into(),
        request_id: "01SP1SOCKREQ0000000000Q001".into(),
        action: "tool.invoke".into(),
        tool: "Bash".into(),
        badge: Some("cafef00d01234567".into()),
        context_ref: Some("cas:blake3:c0n7ex700000000000000000000000000".into()),
        // DR-014 §Decision 4 added `paths`; a pre-DR-014 constructor omits it.
        // Mechanical additive update — the round-trip assertion is unchanged.
        paths: None,
    };
    let line = encode_request(&request).expect("encode");
    assert!(!line.contains('\n'), "JSONL frame must be a single line");
    let back = decode_request(&line).expect("decode");
    assert_eq!(back, request);
}

/// Wire shape pinned: `op` tag in snake_case is `request_permission`; the small
/// descriptor is inline; an absent optional (`badge`/`context_ref`) is OMITTED,
/// never emitted as null.
///
/// COMPILE-RED until the variant exists.
#[test]
fn request_permission_wire_shape_pinned() {
    let request = Request::RequestPermission {
        run: "01SP1SOCKRUN0000000000R002".into(),
        request_id: "01SP1SOCKREQ0000000000Q002".into(),
        action: "tool.invoke".into(),
        tool: "Read".into(),
        badge: None,
        context_ref: None,
        // DR-014 §Decision 4 added `paths`; absent = OMITTED (asserted below).
        paths: None,
    };
    let line = encode_request(&request).expect("encode");
    let v: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(v["op"], "request_permission", "snake_case op tag");
    assert_eq!(v["action"], "tool.invoke");
    assert_eq!(v["tool"], "Read");
    assert!(
        v.get("badge").is_none(),
        "absent badge must be omitted, not null"
    );
    assert!(
        v.get("context_ref").is_none(),
        "absent context_ref must be omitted, not null (I2: ref only when present)"
    );
}

/// The decision reply round-trips and carries exactly one of `allow|deny|ask`.
/// All three decisions are representable — the reply NEVER lacks a `deny`/`ask`
/// arm, so the PEP is never forced to synthesize an allow when the PDP could
/// not (I6, DR-008 §4).
///
/// COMPILE-RED until `Reply::PermitDecision` exists.
#[test]
fn permit_decision_reply_round_trips_all_three() {
    for decision in ["allow", "deny", "ask"] {
        let reply = Reply::PermitDecision {
            request_id: "01SP1SOCKREQ0000000000Q003".into(),
            decision: decision.into(),
            reason: if decision == "allow" {
                None
            } else {
                Some("policy said so".into())
            },
        };
        let line = encode_reply(&reply).expect("encode");
        assert!(!line.contains('\n'), "JSONL frame must be a single line");
        let back = decode_reply(&line).expect("decode");
        assert_eq!(back, reply);
    }
}

/// The decision reply wire shape: `reply` tag `permit_decision`, decision word
/// carried verbatim (`ask` is the escalate branch, never coerced to `allow`).
///
/// COMPILE-RED until the variant exists.
#[test]
fn permit_decision_ask_is_carried_verbatim_not_coerced() {
    let reply = Reply::PermitDecision {
        request_id: "01SP1SOCKREQ0000000000Q004".into(),
        decision: "ask".into(),
        reason: Some("cumulative spend crossed soft cap".into()),
    };
    let line = encode_reply(&reply).expect("encode");
    let v: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(v["reply"], "permit_decision", "snake_case reply tag");
    assert_eq!(
        v["decision"], "ask",
        "inconclusive → ask, carried verbatim on the wire, NEVER coerced to allow (I6, DR-008 §4)"
    );
    assert_eq!(v["reason"], "cumulative spend crossed soft cap");
}
