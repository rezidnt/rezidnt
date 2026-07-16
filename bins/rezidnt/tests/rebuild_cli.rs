//! S0 oracle — `rezidnt rebuild` at the CLI surface: refold from seq 0 and
//! print the graph as JSON (`--json`, stable exit code 0). Must equal the
//! reference fold over the same events. Cross-platform (no socket involved).

use std::process::Command;

use rezidnt_fabric::EventLog;
use rezidnt_state::Graph;
use rezidnt_types::Event;
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

#[test]
fn rebuild_from_seq0_matches_reference_fold() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");

    let subjects = [
        "daemon.started",
        "agent.spawned",
        "agent.status.changed",
        "gate.entered",
    ];
    let events: Vec<Event> = (0..12)
        .map(|i| evt(i, subjects[i as usize % subjects.len()]))
        .collect();
    {
        let mut log = EventLog::open(&db).expect("seed log");
        for e in &events {
            log.append(e).expect("append");
        }
    }

    let out = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .arg("rebuild")
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .output()
        .expect("run rezidnt rebuild");
    assert!(
        out.status.success(),
        "rezidnt rebuild must exit 0, got {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let got: Graph = serde_json::from_slice(&out.stdout)
        .expect("`rezidnt rebuild --json` must print the Graph as JSON on stdout");
    assert_eq!(
        got,
        rezidnt_state::fold(events.iter()),
        "CLI rebuild diverged from reference fold"
    );
}
