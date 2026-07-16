//! S1 oracle: client request frames (protocol addition on top of the S0
//! hello-then-stream contract).

use rezidnt_proto::{Request, decode_request, encode_request};
use ulid::Ulid;

#[test]
fn requests_round_trip() {
    for request in [
        Request::Tail { subject: None },
        Request::Tail {
            subject: Some("agent.message".into()),
        },
        Request::Open {
            spec_toml: "[project]\nname = \"x\"\nrepo = \".\"".into(),
        },
        Request::Attach {
            run: Ulid::from_parts(9, 9),
        },
    ] {
        let line = encode_request(&request).expect("encode");
        assert!(!line.contains('\n'), "JSONL frame must be a single line");
        let back = decode_request(&line).expect("decode");
        assert_eq!(back, request);
    }
}

/// The wire shape is pinned: `op` tag in snake_case; Tail omits an absent
/// subject rather than writing null.
#[test]
fn request_wire_shape_pinned() {
    let open = encode_request(&Request::Open {
        spec_toml: "t".into(),
    })
    .expect("encode");
    let v: serde_json::Value = serde_json::from_str(&open).expect("json");
    assert_eq!(v["op"], "open");
    assert_eq!(v["spec_toml"], "t");

    let tail = encode_request(&Request::Tail { subject: None }).expect("encode");
    let v: serde_json::Value = serde_json::from_str(&tail).expect("json");
    assert_eq!(v["op"], "tail");
    assert!(
        v.get("subject").is_none(),
        "absent subject must be omitted, not null"
    );
}

/// Additive evolution on the decode side; honest failure on garbage.
#[test]
fn decode_tolerates_unknown_fields_rejects_unknown_ops() {
    let decoded =
        decode_request(r#"{"op":"tail","subject":null,"future_field":42}"#).expect("decode");
    assert_eq!(decoded, Request::Tail { subject: None });

    assert!(
        decode_request(r#"{"op":"launch_missiles"}"#).is_err(),
        "unknown op must error"
    );
    assert!(decode_request("not json").is_err());
}
