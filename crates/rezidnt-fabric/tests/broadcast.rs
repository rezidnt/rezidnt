//! S0 oracle — delivery semantics (doc §5).
//!
//! - Two concurrent subscribers each observe the full stream (at-least-once
//!   to live subscribers; append is the commit point).
//! - BINDING client rule: a lagged subscriber receives `Lagged(n)` and MUST
//!   resync from the log by last-seen ULID — the reconstructed stream has no
//!   gaps and no duplicates.

use std::sync::Arc;
use std::time::Duration;

use rezidnt_fabric::{EventLog, Fabric, RecvError, Subscriber};
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
        "source": "test",
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

async fn collect(mut sub: Subscriber, n: usize) -> Vec<Ulid> {
    let mut got = Vec::with_capacity(n);
    for i in 0..n {
        let e = timeout(Duration::from_secs(5), sub.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for event {i}/{n}"))
            .expect("live recv");
        got.push(e.id);
    }
    got
}

/// S0 exit criterion: two concurrent subscribers observe the stream — each
/// sees every event, in append order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_concurrent_subscribers_observe_full_stream() {
    let dir = tempfile::tempdir().unwrap();
    let fabric = open_fabric(&dir, 1024);
    let sub_a = fabric.subscribe();
    let sub_b = fabric.subscribe();

    let events: Vec<Event> = (0..200).map(evt).collect();
    let want: Vec<Ulid> = events.iter().map(|e| e.id).collect();

    let publisher = {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || {
            for e in events {
                fabric.publish(e).expect("publish");
            }
        })
    };
    let (got_a, got_b) = tokio::join!(collect(sub_a, 200), collect(sub_b, 200));
    publisher.await.unwrap();

    assert_eq!(
        got_a, want,
        "subscriber A must observe the full stream in order"
    );
    assert_eq!(
        got_b, want,
        "subscriber B must observe the full stream in order"
    );
}

/// Append is the commit point: by the time a subscriber receives an event,
/// the log already contains it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn append_commit_point_precedes_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let fabric = open_fabric(&dir, 16);
    let mut sub = fabric.subscribe();

    let e = evt(1);
    let id = e.id;
    {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || fabric.publish(e).expect("publish"))
            .await
            .unwrap();
    }
    let received = timeout(Duration::from_secs(5), sub.recv())
        .await
        .expect("delivery")
        .expect("recv");
    assert_eq!(received.id, id);

    let logged = fabric.replay_since(None).expect("log read");
    assert_eq!(
        logged.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![id],
        "the event must already be durable in the log when delivered"
    );
}

/// BINDING client rule (doc §5): overflow → `Lagged(n)`, then resync from the
/// log by last-seen ULID. The reconstruction must be complete — no gaps, no
/// duplicates — and the subscriber must keep working for events published
/// after the resync.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lagged_subscriber_resyncs_from_log_without_gaps_or_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let fabric = open_fabric(&dir, 8); // deliberately tiny ring to force Lagged
    let mut lagger = fabric.subscribe();

    let burst: Vec<Event> = (0..100).map(evt).collect();
    let mut want: Vec<Ulid> = burst.iter().map(|e| e.id).collect();
    {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || {
            for e in burst {
                fabric.publish(e).expect("publish");
            }
        })
        .await
        .unwrap();
    }

    // The lagger never polled during the burst: its next recv MUST surface Lagged.
    let first = timeout(Duration::from_secs(5), lagger.recv())
        .await
        .expect("recv");
    let Err(RecvError::Lagged(n)) = first else {
        panic!(
            "expected Lagged(n) after overflowing a capacity-8 ring with 100 events, got {first:?}"
        );
    };
    assert!(n > 0, "Lagged must report how many events were dropped");

    // The BINDING resync: replay from the log by last-seen ULID.
    let missed = lagger.resync(&fabric).expect("resync from log");
    let mut got: Vec<Ulid> = missed.iter().map(|e| e.id).collect();

    // Liveness after resync: newly published events flow, with the stale
    // broadcast backlog overlap suppressed (no duplicates).
    let tail: Vec<Event> = (100..105).map(evt).collect();
    want.extend(tail.iter().map(|e| e.id));
    {
        let fabric = Arc::clone(&fabric);
        tokio::task::spawn_blocking(move || {
            for e in tail {
                fabric.publish(e).expect("publish");
            }
        })
        .await
        .unwrap();
    }
    for _ in 0..5 {
        let e = timeout(Duration::from_secs(5), lagger.recv())
            .await
            .expect("post-resync delivery")
            .expect("post-resync recv");
        got.push(e.id);
    }

    assert_eq!(
        got, want,
        "resync + live continuation must be gap-free and duplicate-free"
    );
}
