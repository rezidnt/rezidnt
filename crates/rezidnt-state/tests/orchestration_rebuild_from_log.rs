//! DR-042 ORACLE (I3 rebuild-equivalence, PERSISTED-LOG leg) — the load-bearing
//! owed test named in DR-042 §Consequences: "the eventual slice owes an I3
//! fold-equivalence test (graph rebuildable from log alone)". The sibling
//! `orchestration_rebuild_equivalence.rs` proves fold == in-memory
//! `Materializer` replay; THIS file closes the gap the DR actually names — that
//! the orchestration graph rebuilds through the REAL `rezidnt rebuild` path,
//! from a persisted SQLite `EventLog`, byte-for-byte identically.
//!
//! `rezidnt rebuild` (`bins/rezidnt/src/main.rs::rebuild`) is exactly:
//!   `EventLog::open(db)` → `read_from(1)` → `rezidnt_state::fold(events)`.
//! So the strongest form of the wedge (DR-042 §Decision 2 — the inverse of
//! Omnigent's server-held session) is: append the fan-out to a real log, read it
//! back through that same path, and prove `orchestration_graph` folds the
//! IDENTICAL view. There is NO in-daemon orchestration session object; the log —
//! survivable to disk, re-read cold — is the only source of record (I3).
//!
//! This is a read-side test (no live fan-out, no spawn, no wiring — Phase-3-gated
//! per DR-042 Decision 5). It only PERSISTS existing facts and RE-READS them.

use std::path::PathBuf;

use rezidnt_fabric::EventLog;
use rezidnt_state::{fold, orchestration_graph};
use rezidnt_types::Event;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Load a committed golden fixture as its event vec (the fixture IS the log — I3).
fn load(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

/// Append `events` to a fresh SQLite `EventLog` at `db`, then read them back
/// EXACTLY as `bins/rezidnt/src/main.rs::rebuild` does: `read_from(1)` in seq
/// order. This is the real `rezidnt rebuild` read path, in-process.
fn persist_and_reread(db: &std::path::Path, events: &[Event]) -> Vec<Event> {
    {
        let mut log = EventLog::open(db).expect("open a fresh event log");
        for event in events {
            log.append(event).expect("append fan-out fact to the log");
        }
    } // drop the writer — the log survives to disk (the daemon is "gone")

    // Re-open COLD (a fresh handle — no retained materializer, no session
    // object) and read from seq 0, exactly as `rebuild()` does.
    let log = EventLog::open(db).expect("re-open the persisted log cold");
    log.read_from(1)
        .expect("read the log from seq 1")
        .into_iter()
        .map(|row| row.event)
        .collect()
}

/// CRITERION (DR-042 §Consequences owed test) — the orchestration graph rebuilds
/// from the PERSISTED LOG ALONE. Append the committed fan-out fixture to a real
/// SQLite `EventLog`, read it back through the `rezidnt rebuild` path, fold, and
/// project: the view MUST equal the in-memory fold of the same fixture. A
/// divergence is a reducer bug and a release blocker (I3, DR-042 §Decision 2).
#[test]
fn orchestration_view_rebuilds_from_the_persisted_log() {
    let fixture = load("dr042_orchestration_fanout.jsonl");
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("events.db");

    // Ground truth: the in-memory fold+project of the fixture (the reference the
    // sibling oracle already pins against the Materializer path).
    let from_memory = orchestration_graph(&fold(fixture.iter()));

    // The `rezidnt rebuild` path: persist to disk, re-open cold, read from seq 0.
    let replayed = persist_and_reread(&db, &fixture);
    let from_disk = orchestration_graph(&fold(replayed.iter()));

    assert_eq!(
        from_memory, from_disk,
        "orchestration_graph(fold(in-memory log)) MUST EQUAL \
         orchestration_graph(fold(rebuild-from-persisted-log)) — the graph rebuilds from the \
         log ALONE, no in-daemon orchestration session survives a restart (I3, DR-042 §Decision 2)"
    );

    // Non-vacuity: the round-tripped view actually carries the folded fan-out (a
    // matching pair of EMPTY views would be an oracle bug, not a pass).
    assert_eq!(
        from_disk.leads.len(),
        1,
        "the rebuilt view carries the folded lead (non-vacuity): {from_disk:#?}"
    );
    assert_eq!(
        from_disk.leads[0].fan_out, 2,
        "and its two-wide fan-out survives the persist→rebuild round-trip"
    );
}

/// I3 droppability — the derived graph is disposable. Rebuilding the SAME
/// persisted log a SECOND time (a fresh cold read + fold) yields the byte-for-
/// byte identical view: nothing about the projection depends on retained state,
/// so the whole derived graph can be dropped and rebuilt at will (the crate-
/// level I3 promise: "the whole crate can be deleted and rebuilt from the log").
#[test]
fn orchestration_view_is_droppable_and_rebuildable() {
    let fixture = load("dr042_orchestration_fanout.jsonl");
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("events.db");

    // First rebuild from the persisted log.
    let first = orchestration_graph(&fold(persist_and_reread(&db, &fixture).iter()));

    // "Drop" the derived graph entirely and rebuild AGAIN from the same on-disk
    // log — a second independent cold read + fold + project.
    let log = EventLog::open(&db).expect("re-open the same persisted log");
    let replayed: Vec<Event> = log
        .read_from(1)
        .expect("read the log again from seq 1")
        .into_iter()
        .map(|row| row.event)
        .collect();
    let second = orchestration_graph(&fold(replayed.iter()));

    assert_eq!(
        first, second,
        "rebuilding the orchestration graph a second time from the same persisted log yields \
         the IDENTICAL view — the derived graph is droppable and deterministically rebuildable (I3)"
    );
    // Guard against a matching-but-empty pass.
    assert!(
        !first.leads.is_empty(),
        "the fixture folds a real lead; an empty view is a bug, not a pass"
    );
}
