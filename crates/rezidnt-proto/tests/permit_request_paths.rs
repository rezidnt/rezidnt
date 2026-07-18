//! SP2 hook sub-slice oracle — CRITERION 6 (path parity, the WIRE leg). DR-014
//! §Decision 4 / design §7: add `paths: Option<Value>` (optional, additive) to
//! the socket `Request::RequestPermission` so `path-scope` decides identically
//! over socket and MCP. This board pins the wire shape of that new field; the
//! transport-parity DECISION (socket denies where it previously escalated) is
//! pinned in `bins/rezidentd/tests/permit_socket_decision.rs`.
//!
//! RED MODE: **compile-red**. `Request::RequestPermission` has no `paths` field
//! today (crates/rezidnt-proto/src/lib.rs lines 98-107). The construction below
//! names `paths`, so this crate fails to compile until the field lands. That is
//! red for the right reason: the additive wire axis is absent.
//!
//! COMPANION-EDIT OBLIGATION (flagged, not applied — do NOT weaken green tests):
//! adding `paths` to the struct variant makes the EXISTING green constructors in
//! `permit_request.rs` (which do not set `paths`) fail to compile. When the
//! implementer adds the field they must add `paths: None` to those two
//! constructors in the SAME change — a mechanical additive update that does NOT
//! weaken those assertions (they still pin the round-trip). This oracle file
//! does not edit them: they are green today and it is the implementer's field
//! addition that necessitates (and owns) their one-line update. Absence of the
//! field is honest OMISSION on the wire, never null (I2 additive-evolution rule,
//! same discipline as `badge`/`context_ref`).

use rezidnt_proto::{Request, decode_request, encode_request};
use serde_json::json;

/// CRITERION 6 (wire round-trip) — a `Request::RequestPermission` carrying a
/// `paths` axis round-trips as one JSONL frame, preserving the paths. This is
/// the axis `path-scope` reads (`params.paths`), threaded over the socket so the
/// verifier decides the same on both transports (design §7).
///
/// COMPILE-RED until the `paths` field exists on the variant.
#[test]
fn request_permission_with_paths_round_trips() {
    let request = Request::RequestPermission {
        run: "01SP2PATHSRUN000000000R001".into(),
        request_id: "01SP2PATHSREQ00000000Q001".into(),
        action: "tool.invoke".into(),
        tool: "Edit".into(),
        badge: None,
        context_ref: None,
        // NEW additive axis (DR-014 §Decision 4). Value is opaque JSON — the
        // native reads it as `params.paths` (an array of path strings).
        paths: Some(json!(["src/main.rs", "src/lib.rs"])),
    };
    let line = encode_request(&request).expect("encode");
    assert!(!line.contains('\n'), "JSONL frame must be a single line");
    let back = decode_request(&line).expect("decode");
    assert_eq!(back, request, "the paths axis round-trips verbatim");
}

/// CRITERION 6 (absent-is-omitted) — a request with NO paths omits the field on
/// the wire (never null), exactly the additive-evolution discipline of `badge` /
/// `context_ref`. A pre-DR-014 sender (which cannot set `paths`) is unaffected.
///
/// COMPILE-RED until the field exists.
#[test]
fn request_permission_absent_paths_is_omitted_not_null() {
    let request = Request::RequestPermission {
        run: "01SP2PATHSRUN000000000R002".into(),
        request_id: "01SP2PATHSREQ00000000Q002".into(),
        action: "tool.invoke".into(),
        tool: "Read".into(),
        badge: None,
        context_ref: None,
        paths: None,
    };
    let line = encode_request(&request).expect("encode");
    let v: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert!(
        v.get("paths").is_none(),
        "absent paths is OMITTED, never null — additive-evolution rule (DR-014 §Decision 4): {line}"
    );
}

/// CRITERION 6 (forward-compat decode) — a frame from a NEWER sender carrying
/// `paths` decodes on this end, and a frame from an OLDER sender omitting it
/// decodes to `paths: None`. The additive field breaks no existing sender
/// (DR-014 §Consequences: "the additive `paths` field breaks no existing sender").
///
/// COMPILE-RED until the field exists.
#[test]
fn request_permission_paths_is_additive_both_directions() {
    // Older sender: no `paths` key at all → decodes to None.
    let older = r#"{"op":"request_permission","run":"01SP2PATHSRUN000000000R003","request_id":"01SP2PATHSREQ00000000Q003","action":"tool.invoke","tool":"Read"}"#;
    let Request::RequestPermission { paths, .. } = decode_request(older).expect("decode older")
    else {
        panic!("expected a RequestPermission");
    };
    assert!(
        paths.is_none(),
        "an older sender's frame (no paths) decodes to None — no break (DR-014 §Consequences)"
    );

    // Newer sender: carries `paths` → decodes to Some.
    let newer = r#"{"op":"request_permission","run":"01SP2PATHSRUN000000000R004","request_id":"01SP2PATHSREQ00000000Q004","action":"tool.invoke","tool":"Edit","paths":["/etc/passwd"]}"#;
    let Request::RequestPermission { paths, .. } = decode_request(newer).expect("decode newer")
    else {
        panic!("expected a RequestPermission");
    };
    assert_eq!(
        paths,
        Some(json!(["/etc/passwd"])),
        "a newer sender's paths axis is carried through to the PDP"
    );
}
