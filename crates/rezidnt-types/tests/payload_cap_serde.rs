//! S0 remediation regression tests (implementer-owned, auditor finding): the
//! I2 payload cap must be structural — enforced inside `Deserialize` itself —
//! not an entry-point convention. A plain `serde_json::from_str::<Event>`
//! (log reads, reducers, any future transport) must reject an oversized
//! payload exactly like `Event::from_json_line` does.

use rezidnt_types::{Event, MAX_PAYLOAD_BYTES};

fn frame(payload_json: &str) -> String {
    format!(
        r#"{{"id":"01KXM3M0K0000000000400006A","ts":"2026-07-16T00:01:00Z","v":1,"source":"daemon","subject":"daemon.warning","correlation":"01KXM3J60000000000000C0002","payload":{payload_json}}}"#
    )
}

#[test]
fn plain_serde_from_str_enforces_the_payload_cap() {
    let big = format!("\"{}\"", "a".repeat(MAX_PAYLOAD_BYTES + 1));
    assert!(
        serde_json::from_str::<Event>(&frame(&big)).is_err(),
        "plain serde deserialization must not bypass the I2 cap"
    );

    let at_cap = format!("\"{}\"", "a".repeat(MAX_PAYLOAD_BYTES - 2)); // encodes to exactly the cap
    let ok = serde_json::from_str::<Event>(&frame(&at_cap));
    assert!(
        ok.is_ok(),
        "at-cap payload must stay legal through plain serde: {:?}",
        ok.err()
    );
}

#[test]
fn plain_serde_from_value_enforces_the_payload_cap() {
    let v = serde_json::json!({
        "id": "01KXM3M0K0000000000400006A",
        "ts": "2026-07-16T00:01:00Z",
        "v": 1,
        "source": "daemon",
        "subject": "daemon.warning",
        "correlation": "01KXM3J60000000000000C0002",
        "payload": "a".repeat(MAX_PAYLOAD_BYTES), // encodes to cap+2 bytes
    });
    assert!(
        serde_json::from_value::<Event>(v).is_err(),
        "from_value must not bypass the I2 cap"
    );
}
