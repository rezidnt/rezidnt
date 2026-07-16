//! S0 remediation regression tests (implementer-owned, auditor fail-driver):
//! append order — not id order — is the truth for delivery, dedup, and the
//! resync cursor. Minting an id and appending it are separate critical
//! sections, so two concurrent publishers can invert them: the event with the
//! SMALLER id lands at the LATER seq. The inversion is constructed
//! deterministically here with pre-minted ids; delivery must still be
//! at-least-once (doc §5, BINDING) with no silent drop and no repeat replay.

use std::sync::Arc;
use std::time::Duration;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_types::Event;
use serde_json::json;
use tokio::time::timeout;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000;

fn evt(i: u64) -> Event {
    let id = Ulid::from_parts(T0_MS + i, i as u128 + 1);
    serde_json::from_value(json!({
        "id": id.to_string(),
        "ts": "2026-07-16T00:00:00Z",
        "v": 1,
        "source": "test-race",
        "subject": "agent.status.changed",
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": {"n": i},
    }))
    .expect("test event construction")
}

fn open_fabric(dir: &tempfile::TempDir, capacity: usize) -> Arc<Fabric> {
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    Arc::new(Fabric::new(log, capacity))
}

/// The fail-driver itself: an event minted first (smaller id) but appended
/// second must still reach a live subscriber — a dedup keyed on id order
/// silently drops it with no `Lagged` and no resync obligation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn later_appended_smaller_id_event_is_delivered() {
    let dir = tempfile::tempdir().unwrap();
    let fabric = open_fabric(&dir, 64);
    let mut sub = fabric.subscribe();

    let a = evt(1); // smaller id — minted first, loses the append race
    let b = evt(2); // larger id — minted second, appended FIRST
    let (a_id, b_id) = (a.id, b.id);
    {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || {
            fabric.publish(b).expect("publish b");
            fabric.publish(a).expect("publish a");
        })
        .await
        .unwrap();
    }

    let first = timeout(Duration::from_secs(5), sub.recv())
        .await
        .expect("first delivery")
        .expect("recv");
    assert_eq!(first.id, b_id, "delivery order is append order");

    let second = timeout(Duration::from_secs(5), sub.recv())
        .await
        .expect(
            "second delivery timed out — the later-appended smaller-id event \
             was silently dropped (at-least-once violation, doc §5)",
        )
        .expect("recv");
    assert_eq!(second.id, a_id);
}

/// The resync cursor is the subscriber's append position, not its maximum
/// observed id: after the subscriber has seen a LARGE id live, an overflow of
/// smaller-id events (minted earlier, appended later) must surface `Lagged`
/// (an id-keyed dedup silently drops them and hangs instead), the resync must
/// return exactly the missed tail, and a repeat resync must be empty (a
/// max-id cursor re-delivers the smaller-id rows forever).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resync_cursor_is_append_position_not_max_id() {
    let dir = tempfile::tempdir().unwrap();
    let fabric = open_fabric(&dir, 2); // tiny ring to force Lagged
    let mut sub = fabric.subscribe();

    // Live phase: the subscriber's latest observation carries the LARGEST id.
    let head = vec![evt(1), evt(9)];
    {
        let fabric = Arc::clone(&fabric);
        let head = head.clone();
        tokio::task::spawn_blocking(move || {
            for e in head {
                fabric.publish(e).expect("publish head");
            }
        })
        .await
        .unwrap();
    }
    for want in &head {
        let got = timeout(Duration::from_secs(5), sub.recv())
            .await
            .expect("live delivery")
            .expect("recv");
        assert_eq!(got.id, want.id);
    }

    // Overflow phase: three events with ids SMALLER than evt(9) — minted
    // before it, appended after it — overflow the capacity-2 ring.
    let tail = vec![evt(4), evt(5), evt(6)];
    let want_missed: Vec<Ulid> = tail.iter().map(|e| e.id).collect();
    {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || {
            for e in tail {
                fabric.publish(e).expect("publish tail");
            }
        })
        .await
        .unwrap();
    }

    let first = timeout(Duration::from_secs(5), sub.recv()).await.expect(
        "recv timed out — smaller-id ring survivors were silently dropped \
             instead of surfacing Lagged (at-least-once violation, doc §5)",
    );
    assert!(
        first.is_err(),
        "expected Lagged after overflowing a capacity-2 ring, got {first:?}"
    );

    let missed = sub.resync(&fabric).expect("resync");
    assert_eq!(
        missed.iter().map(|e| e.id).collect::<Vec<_>>(),
        want_missed,
        "resync must replay exactly the missed append-order tail"
    );

    let again = sub.resync(&fabric).expect("second resync");
    assert!(
        again.is_empty(),
        "repeat resync re-delivered {} event(s) — the cursor must be the \
         append position (seq), not the max observed id",
        again.len()
    );

    // Liveness after resync.
    let live = evt(20);
    let live_id = live.id;
    {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || fabric.publish(live).expect("publish live"))
            .await
            .unwrap();
    }
    let got = timeout(Duration::from_secs(5), sub.recv())
        .await
        .expect("post-resync delivery")
        .expect("recv");
    assert_eq!(got.id, live_id);
}
