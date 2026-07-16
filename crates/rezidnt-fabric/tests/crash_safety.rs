//! S0 oracle — crash safety (the S0 exit demo, at the log layer).
//!
//! `kill -9` the writer mid-append burst; restart; `rebuild` (fold from
//! seq 0) reproduces identical graph state; the chain verifies end-to-end.
//!
//! Unix-only (SIGKILL semantics); runs in WSL2 per the S0 platform decision
//! and compiles to an empty test crate on Windows.

#![cfg(unix)]

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rezidnt_fabric::EventLog;
use rezidnt_types::Event;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000;

fn evt(i: u64) -> Event {
    let id = Ulid::from_parts(T0_MS + i, i as u128 + 1);
    serde_json::from_value(json!({
        "id": id.to_string(),
        "ts": "2026-07-16T00:00:00Z",
        "v": 1,
        "source": "test-restart",
        "subject": "daemon.warning",
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": {"n": i},
    }))
    .expect("test event construction")
}

fn max_seq(db: &Path) -> i64 {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return 0;
    };
    conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Spawn `burst-writer`, wait until it has durably appended at least
/// `min_rows`, then SIGKILL it mid-burst.
fn spawn_burst_and_kill(db: &Path, min_rows: i64) {
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_burst-writer"))
        .arg(db)
        .arg("1000000") // far more than we let it write
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn burst-writer");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if max_seq(db) >= min_rows {
            break;
        }
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut err = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            panic!(
                "burst-writer exited (status {status:?}) before appending {min_rows} rows — \
                 run_burst unimplemented? stderr:\n{err}"
            );
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("burst-writer appended <{min_rows} rows in 20s — run_burst unimplemented?");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().expect("SIGKILL burst-writer"); // Child::kill is SIGKILL on unix
    child.wait().expect("reap");
}

/// The exit criterion itself: after a SIGKILL mid-burst, the recovered log is
/// a valid prefix (contiguous seq, chain verifies) and rebuild-from-seq-0
/// equals the incremental fold over the same rows.
#[test]
fn sigkill_mid_burst_then_rebuild_reproduces_identical_graph_and_chain_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    spawn_burst_and_kill(&db, 200);

    let log = EventLog::open(&db).expect("post-crash open must recover (WAL)");
    let verified = log
        .verify_chain()
        .expect("chain must verify end-to-end after SIGKILL");
    let rows = log.read_from(1).expect("read recovered rows");
    assert!(
        rows.len() >= 200,
        "expected the burst to have committed at least 200 rows"
    );
    assert_eq!(verified, rows.len() as u64);
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            row.seq,
            i as i64 + 1,
            "seq must be contiguous — a torn row survived the crash"
        );
    }

    let events: Vec<Event> = rows.into_iter().map(|r| r.event).collect();
    let rebuilt = rezidnt_state::fold(events.iter());
    let mut live = rezidnt_state::Materializer::new();
    for e in &events {
        live.apply(e);
    }
    assert_eq!(
        &rebuilt,
        live.graph(),
        "rebuild-from-seq-0 diverged from the incremental fold — reducer bug (release blocker)"
    );
    assert_eq!(rebuilt.events_folded, events.len() as u64);
}

/// Restart semantics: a reopened log continues the SAME chain — appends after
/// the crash extend it and the whole thing still verifies end-to-end.
#[test]
fn restart_after_sigkill_continues_the_chain() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    spawn_burst_and_kill(&db, 50);

    let mut log = EventLog::open(&db).expect("post-crash open must recover (WAL)");
    let before = log.verify_chain().expect("recovered prefix must verify");
    for i in 0..3 {
        log.append(&evt(i)).expect("append after restart");
    }
    assert_eq!(
        log.verify_chain()
            .expect("chain must verify across the crash boundary"),
        before + 3,
        "post-restart appends must extend the same chain, not start a new one"
    );
}
