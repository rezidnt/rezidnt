//! S0 oracle — socket protocol (doc §9): versioned hello, proto-major gate,
//! socket path resolution. Socket-path tests are unix-gated per the S0
//! platform decision (the named pipe is designed but not S0-tested).

use rezidnt_proto::{Hello, PROTO_VERSION, ProtoError, check_hello, decode_hello, encode_hello};

fn hello() -> Hello {
    Hello {
        proto: PROTO_VERSION,
        schema: "blake3:0f0f".into(),
        daemon: "0.0.1".into(),
    }
}

/// The hello is one JSONL frame with exactly the doc §9 fields:
/// `{proto: 1, schema: <ontology hash>, daemon: <semver>}`.
#[test]
fn hello_round_trips_and_wire_shape_is_pinned() {
    let line = encode_hello(&hello()).expect("encode");
    assert!(!line.contains('\n'), "hello is a single JSONL frame");

    let v: serde_json::Value = serde_json::from_str(&line).expect("hello must be JSON");
    let obj = v.as_object().expect("hello is an object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["daemon", "proto", "schema"]);
    assert_eq!(obj["proto"], 1);

    let back = decode_hello(&line).expect("decode");
    assert_eq!(back, hello());
}

/// Additive evolution applies to the hello too: unknown fields don't break it.
#[test]
fn hello_decode_tolerates_unknown_fields() {
    let line = r#"{"proto":1,"schema":"blake3:0f0f","daemon":"0.0.1","zz_future":"ok"}"#;
    let h = decode_hello(line).expect("unknown fields must be tolerated");
    assert_eq!(h.proto, 1);
}

/// Doc §9: mismatched proto majors disconnect with a machine-readable
/// upgrade hint.
#[test]
fn proto_major_mismatch_yields_machine_readable_upgrade_hint() {
    let peer = Hello {
        proto: 99,
        ..hello()
    };
    let err = check_hello(&peer).unwrap_err();
    match err {
        ProtoError::ProtoMismatch { got, want, hint } => {
            assert_eq!(got, 99);
            assert_eq!(want, PROTO_VERSION);
            assert!(
                !hint.is_empty(),
                "the upgrade hint must be machine-usable, not empty"
            );
        }
        other => panic!("expected ProtoMismatch, got {other:?}"),
    }
    assert!(check_hello(&hello()).is_ok(), "matching majors must pass");
}

/// Doc §9: UDS at `$XDG_RUNTIME_DIR/rezidnt.sock`.
#[cfg(unix)]
#[test]
fn socket_path_prefers_xdg_runtime_dir() {
    use std::path::Path;
    let p =
        rezidnt_proto::socket_path_from(Some(Path::new("/run/user/1000")), Path::new("/home/ada"));
    assert_eq!(p, Path::new("/run/user/1000/rezidnt.sock"));
}

/// Doc §9: fallback is `~/.local/state/rezidnt/`.
#[cfg(unix)]
#[test]
fn socket_path_falls_back_to_local_state() {
    use std::path::Path;
    let p = rezidnt_proto::socket_path_from(None, Path::new("/home/ada"));
    assert_eq!(p, Path::new("/home/ada/.local/state/rezidnt/rezidnt.sock"));
}
