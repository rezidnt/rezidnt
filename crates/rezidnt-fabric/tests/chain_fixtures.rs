//! S0 oracle — golden chain fixtures. The chain values in
//! `spec/fixtures/s0_chain_valid.jsonl` were precomputed independently of the
//! implementation; this pins the exact chain formula (an implementation that
//! hashes the wrong bytes verifies its own logs but fails these).
//!
//! Also pins doc §6 schema compatibility: the tests create the table with the
//! doc-verbatim DDL and `EventLog::open` must accept the existing database.
//!
//! Run via `scripts/replay-fixtures.sh` (the /vet gauntlet).

use std::path::{Path, PathBuf};

use rezidnt_fabric::{EventLog, FabricError};
use serde::Deserialize;

/// Doc §6, verbatim.
const DDL: &str = "
CREATE TABLE events (
  seq        INTEGER PRIMARY KEY,
  id         TEXT NOT NULL UNIQUE,
  ts         TEXT NOT NULL,
  v          INTEGER NOT NULL,
  source     TEXT NOT NULL,
  workspace  TEXT,
  subject    TEXT NOT NULL,
  correlation TEXT NOT NULL,
  causation  TEXT,
  payload    TEXT NOT NULL,
  chain      BLOB NOT NULL
);
CREATE INDEX idx_events_subject ON events(subject, seq);
CREATE INDEX idx_events_ws       ON events(workspace, seq);
CREATE INDEX idx_events_corr     ON events(correlation);
";

#[derive(Deserialize)]
struct FixtureRow {
    seq: i64,
    chain: String,
    event: serde_json::Value,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Load a chain fixture into a fresh doc §6 database.
fn load_fixture_db(fixture: &str, db: &Path) {
    let text = std::fs::read_to_string(fixtures_dir().join(fixture))
        .unwrap_or_else(|e| panic!("fixture {fixture} must exist: {e}"));
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(DDL).unwrap();
    for line in text.lines() {
        let row: FixtureRow = serde_json::from_str(line).expect("fixture row must parse");
        let e = &row.event;
        let payload_text = serde_json::to_string(&e["payload"]).unwrap();
        conn.execute(
            "INSERT INTO events (seq, id, ts, v, source, workspace, subject, correlation, causation, payload, chain)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                row.seq,
                e["id"].as_str().unwrap(),
                e["ts"].as_str().unwrap(),
                e["v"].as_i64().unwrap(),
                e["source"].as_str().unwrap(),
                e.get("workspace").and_then(|w| w.as_str()),
                e["subject"].as_str().unwrap(),
                e["correlation"].as_str().unwrap(),
                e.get("causation").and_then(|c| c.as_str()),
                payload_text,
                unhex(&row.chain),
            ],
        )
        .unwrap();
    }
}

/// A log whose chain column matches the precomputed golden values verifies.
#[test]
fn fixture_s0_chain_valid_verifies_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    load_fixture_db("s0_chain_valid.jsonl", &db);
    let log = EventLog::open(&db).expect("open must accept an existing doc §6 database");
    assert_eq!(log.verify_chain().expect("golden chain must verify"), 6);
}

/// The tamper fixture is byte-identical except row 4's payload was edited
/// after the chain was written; verification must name seq 4.
#[test]
fn fixture_s0_chain_tamper_detected_at_seq_4() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    load_fixture_db("s0_chain_tamper.jsonl", &db);
    let log = EventLog::open(&db).expect("open must accept an existing doc §6 database");
    let err = log.verify_chain().unwrap_err();
    assert!(
        matches!(err, FabricError::ChainBroken { seq: 4, .. }),
        "tamper fixture must break at seq 4, got {err:?}"
    );
}
