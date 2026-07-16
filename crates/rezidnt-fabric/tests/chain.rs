//! S0 oracle — hash-chain criteria (doc §6/§12): valid chains verify;
//! tampered payloads and reordered rows are detected.
//!
//! The known-answer vectors were computed independently of the implementation
//! (blake3 over `prev.chain || id || payload` with a 32-zero-byte genesis) and
//! match `spec/fixtures/s0_chain_valid.jsonl`.

use rezidnt_fabric::{CHAIN_GENESIS, EventLog, FabricError, chain_hash};
use rezidnt_types::Event;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000;

fn evt(i: u64, subject: &str, payload: serde_json::Value) -> Event {
    let id = Ulid::from_parts(T0_MS + i, i as u128 + 1);
    serde_json::from_value(json!({
        "id": id.to_string(),
        "ts": "2026-07-16T00:00:00Z",
        "v": 1,
        "source": "test",
        "subject": subject,
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": payload,
    }))
    .expect("test event construction")
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Known-answer vectors for the chain-link function. Pins the exact byte
/// recipe: prev.chain (32 raw bytes) || ULID text (26 ASCII bytes) || payload
/// column text (compact JSON).
#[test]
fn chain_hash_known_answer() {
    let id1 = Ulid::from_string("01KXM3M0K0000000000400006A").unwrap();
    let c1 = chain_hash(&CHAIN_GENESIS, &id1, r#"{"pid":7}"#);
    assert_eq!(
        hex(&c1),
        "6f8fc343892108deb66135452b57fad57ea22ecdc7c6c942500155215600c23d"
    );

    let id2 = Ulid::from_string("01KXM3NV60000000000800006A").unwrap();
    let c2 = chain_hash(&c1, &id2, r#"{"name":"acme"}"#);
    assert_eq!(
        hex(&c2),
        "aa16a1cec6a9f797be855820d417fbe6d19cdcaa62e0a76516ccd5c1caea1bfd"
    );
}

/// Happy path: an honestly appended log verifies end-to-end, seq is
/// contiguous from 1, and the first link hangs off the genesis value.
#[test]
fn append_then_verify_chain_ok() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::open(&dir.path().join("events.db")).expect("open");
    for i in 0..20 {
        log.append(&evt(i, "agent.status.changed", json!({"n": i})))
            .expect("append");
    }
    assert_eq!(log.verify_chain().expect("valid chain must verify"), 20);

    let rows = log.read_from(1).expect("read");
    assert_eq!(rows.len(), 20);
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.seq, i as i64 + 1, "seq must be contiguous append order");
    }
    let payload_text = serde_json::to_string(rows[0].event.payload()).unwrap();
    assert_eq!(
        rows[0].chain,
        chain_hash(&CHAIN_GENESIS, &rows[0].event.id, &payload_text),
        "first link must hang off CHAIN_GENESIS"
    );
}

/// Doc §5: append is exactly-once by ULID uniqueness.
#[test]
fn duplicate_ulid_append_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::open(&dir.path().join("events.db")).expect("open");
    let e = evt(1, "daemon.started", json!({"pid": 1}));
    log.append(&e).expect("first append");
    let err = log.append(&e).unwrap_err();
    assert!(
        matches!(err, FabricError::DuplicateId { id } if id == e.id),
        "duplicate ULID must be rejected as DuplicateId, got {err:?}"
    );
}

/// Tamper-evidence: editing a payload on disk breaks verification at exactly
/// that row.
#[test]
fn tampered_payload_detected_at_its_seq() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    {
        let mut log = EventLog::open(&db).expect("open");
        for i in 0..10 {
            log.append(&evt(i, "agent.status.changed", json!({"n": i})))
                .expect("append");
        }
    }
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE events SET payload = '{\"evil\":true}' WHERE seq = 5",
        [],
    )
    .unwrap();
    drop(conn);

    let log = EventLog::open(&db).expect("reopen");
    let err = log.verify_chain().unwrap_err();
    assert!(
        matches!(err, FabricError::ChainBroken { seq: 5, .. }),
        "tamper at seq 5 must be reported at seq 5, got {err:?}"
    );
}

/// Tamper-evidence: reordering rows (swapping two rows' contents) breaks
/// verification at the first displaced row.
#[test]
fn reordered_rows_detected() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    {
        let mut log = EventLog::open(&db).expect("open");
        for i in 0..10 {
            log.append(&evt(i, "agent.status.changed", json!({"n": i})))
                .expect("append");
        }
    }
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "UPDATE events SET seq = -3 WHERE seq = 3;
         UPDATE events SET seq = 3 WHERE seq = 4;
         UPDATE events SET seq = 4 WHERE seq = -3;",
    )
    .unwrap();
    drop(conn);

    let log = EventLog::open(&db).expect("reopen");
    let err = log.verify_chain().unwrap_err();
    assert!(
        matches!(err, FabricError::ChainBroken { seq: 3, .. }),
        "swap of rows 3/4 must be detected at seq 3, got {err:?}"
    );
}
