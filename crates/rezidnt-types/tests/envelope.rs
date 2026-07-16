//! S0 oracle — envelope criteria (doc §5, I2).
//!
//! Every test here MUST fail until the S0 implementation lands (the
//! constructors and wire codec are `todo!()` stubs). A test that passes
//! before implementation exists tests nothing.

use std::path::PathBuf;

use rezidnt_types::{
    Event, EventError, EventParts, MAX_PAYLOAD_BYTES, SourceId, Subject, WorkspaceId,
};
use serde_json::json;
use time::OffsetDateTime;
use time::macros::datetime;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000; // 2026-07-16T00:00:00Z

fn parts(payload: serde_json::Value) -> EventParts {
    EventParts {
        id: Ulid::from_parts(T0_MS, 42),
        ts: datetime!(2026-07-16 00:00:00 UTC),
        v: 1,
        source: SourceId::new("daemon"),
        workspace: Some(WorkspaceId::new(Ulid::from_parts(T0_MS, 7))),
        subject: Subject::new("workspace.opened"),
        correlation: Ulid::from_parts(T0_MS, 9),
        causation: None,
        payload,
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// I2: payload hard cap is enforced at construction, not by convention.
/// Size is measured on the compact JSON encoding (a bare string of N 'a's
/// encodes to N+2 bytes).
#[test]
fn payload_over_32kib_rejected_at_construction() {
    let huge = json!("a".repeat(MAX_PAYLOAD_BYTES)); // encodes to cap+2 bytes
    let err = Event::from_parts(parts(huge)).unwrap_err();
    assert!(
        matches!(err, EventError::PayloadTooLarge { actual } if actual == MAX_PAYLOAD_BYTES + 2),
        "expected PayloadTooLarge {{ actual: {} }}, got {err:?}",
        MAX_PAYLOAD_BYTES + 2
    );

    let huge = json!("a".repeat(MAX_PAYLOAD_BYTES));
    let err = Event::new(
        SourceId::new("daemon"),
        None,
        Subject::new("daemon.warning"),
        Ulid::from_parts(T0_MS, 9),
        None,
        1,
        huge,
    )
    .unwrap_err();
    assert!(
        matches!(err, EventError::PayloadTooLarge { .. }),
        "got {err:?}"
    );
}

/// Boundary: exactly-at-cap payloads are legal.
#[test]
fn payload_at_cap_accepted() {
    let at_cap = json!("a".repeat(MAX_PAYLOAD_BYTES - 2)); // encodes to exactly the cap
    let event = Event::from_parts(parts(at_cap)).expect("payload at exactly 32 KiB must be legal");
    assert_eq!(event.subject.as_str(), "workspace.opened");
}

/// Doc §5: `id` is time-ordered and globally unique; `ts` is the daemon
/// clock, UTC.
#[test]
fn minted_events_have_time_ordered_ids_and_utc_ts() {
    let mint = || {
        Event::new(
            SourceId::new("daemon"),
            None,
            Subject::new("daemon.started"),
            Ulid::from_parts(T0_MS, 9),
            None,
            1,
            json!({"pid": 1}),
        )
        .expect("small payload must construct")
    };
    let a = mint();
    let b = mint();
    assert_ne!(a.id, b.id, "ids must be globally unique");
    assert!(a.id < b.id, "sequential mints must be time-ordered (ULID)");
    let now = OffsetDateTime::now_utc();
    for e in [&a, &b] {
        assert_eq!(e.ts.offset(), time::UtcOffset::UTC, "ts must be UTC");
        assert!(
            (now - e.ts).whole_seconds().abs() < 300,
            "ts must be the current daemon clock"
        );
    }
}

/// JSONL round-trip through the wire codec preserves every field.
#[test]
fn envelope_json_line_round_trip() {
    let event = Event::from_parts(EventParts {
        causation: Some(Ulid::from_parts(T0_MS, 41)),
        ..parts(json!({"name": "acme", "n": 3}))
    })
    .expect("construct");
    let line = event.to_json_line().expect("encode");
    assert!(!line.contains('\n'), "one event = one line (JSONL)");
    let back = Event::from_json_line(&line).expect("decode");
    assert_eq!(back, event);
}

/// Wire shape is BINDING: exact field names, optional fields absent (never
/// null) when None.
#[test]
fn envelope_wire_shape_pinned() {
    let event = Event::from_parts(EventParts {
        workspace: None,
        ..parts(json!({"pid": 7}))
    })
    .expect("construct");
    let line = event.to_json_line().expect("encode");
    let v: serde_json::Value = serde_json::from_str(&line).expect("wire frame must be JSON");
    let obj = v.as_object().expect("envelope is an object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "correlation",
            "id",
            "payload",
            "source",
            "subject",
            "ts",
            "v"
        ],
        "workspace/causation must be absent when None; no extra or renamed fields"
    );
    assert_eq!(obj["v"], 1);
    assert_eq!(obj["subject"], "workspace.opened");
}

/// Additive evolution (doc §5, BINDING): unknown fields — at the envelope
/// level and inside the payload — never break deserialization, and payload
/// contents are preserved verbatim.
#[test]
fn envelope_tolerates_unknown_fields_additive_evolution() {
    let path = fixtures_dir().join("s0_envelope_additive.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()));
    let events: Vec<Event> = text
        .lines()
        .map(|l| {
            Event::from_json_line(l)
                .unwrap_or_else(|e| panic!("additive line must decode, got {e}: {l}"))
        })
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].subject.as_str(), "workspace.opened");
    assert_eq!(
        events[0].payload()["zz_added_in_v2"]["nested"],
        json!(true),
        "unknown payload fields are opaque data and must be preserved"
    );
    assert_eq!(events[1].v, 2, "a v+1 payload rides the same envelope");
    assert!(events[2].workspace.is_none());
}

/// The wire entry point enforces I2 too: a peer cannot smuggle an oversized
/// payload past the constructor by handing us JSON.
#[test]
fn oversized_payload_rejected_on_wire_decode() {
    let big = "a".repeat(MAX_PAYLOAD_BYTES + 1);
    let line = format!(
        r#"{{"id":"01KXM3M0K0000000000400006A","ts":"2026-07-16T00:01:00Z","v":1,"source":"daemon","subject":"daemon.warning","correlation":"01KXM3J60000000000000C0002","payload":"{big}"}}"#
    );
    let err = Event::from_json_line(&line).unwrap_err();
    assert!(
        matches!(err, EventError::PayloadTooLarge { .. }),
        "got {err:?}"
    );
}
