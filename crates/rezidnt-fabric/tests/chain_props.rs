//! S0 oracle — chain properties over arbitrary appends: every honestly
//! appended log verifies; any single payload tamper is detected at its seq.

use std::path::Path;

use proptest::prelude::*;
use rezidnt_fabric::{EventLog, FabricError};
use rezidnt_types::{Event, taxonomy::SUBJECTS_V0};
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000;

fn evt(i: u64, subject: &str) -> Event {
    let id = Ulid::from_parts(T0_MS + i, i as u128 + 1);
    serde_json::from_value(json!({
        "id": id.to_string(),
        "ts": "2026-07-16T00:00:00Z",
        "v": 1,
        "source": "test",
        "subject": subject,
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": {"n": i},
    }))
    .expect("test event construction")
}

fn append_all(db: &Path, subject_indices: &[usize]) {
    let mut log = EventLog::open(db).expect("open");
    for (i, si) in subject_indices.iter().enumerate() {
        log.append(&evt(i as u64, SUBJECTS_V0[*si]))
            .expect("append");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn prop_chain_verifies_after_arbitrary_appends(
        subject_indices in prop::collection::vec(0..SUBJECTS_V0.len(), 1..40)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("events.db");
        append_all(&db, &subject_indices);
        let log = EventLog::open(&db).expect("reopen");
        prop_assert_eq!(log.verify_chain().expect("honest log must verify"), subject_indices.len() as u64);
    }

    #[test]
    fn prop_any_single_payload_tamper_detected(
        (subject_indices, tamper_at) in prop::collection::vec(0..SUBJECTS_V0.len(), 2..40)
            .prop_flat_map(|v| { let len = v.len(); (Just(v), 0..len) })
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("events.db");
        append_all(&db, &subject_indices);

        let seq = tamper_at as i64 + 1;
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            "UPDATE events SET payload = '{\"tampered\":true}' WHERE seq = ?1",
            rusqlite::params![seq],
        ).unwrap();
        drop(conn);

        let log = EventLog::open(&db).expect("reopen");
        let err = log.verify_chain().unwrap_err();
        prop_assert!(
            matches!(err, FabricError::ChainBroken { seq: s, .. } if s == seq),
            "tamper at seq {} must be reported there, got {:?}", seq, err
        );
    }
}
