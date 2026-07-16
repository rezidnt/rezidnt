//! S0 oracle — reducer determinism (doc §15, the Phase-1 oracle; release
//! blocking). Over arbitrary well-formed event interleavings generated from
//! the subject taxonomy:
//!
//! - `fold(log) == live materialized state`
//! - `fold(log) == snapshot + fold(tail)` (rebuild equals snapshot/resume)
//! - folding is a pure, deterministic replay
//! - conservation: every event is counted exactly once

use proptest::prelude::*;
use rezidnt_state::{Materializer, fold};
use rezidnt_types::{Event, taxonomy::SUBJECTS_V0};
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000;

fn ws_ulid(k: u8) -> Ulid {
    Ulid::from_parts(T0_MS, k as u128 + 1)
}

/// Well-formed events straight from the taxonomy: valid ULIDs, RFC3339 ts,
/// subjects drawn from `SUBJECTS_V0`, small JSON payloads (well under the I2
/// cap by construction).
fn arb_event() -> impl Strategy<Value = Event> {
    (
        any::<u64>(),                                                    // id entropy
        1_600_000_000i64..4_100_000_000i64,                              // ts seconds
        0..SUBJECTS_V0.len(),                                            // subject
        prop::option::of(0u8..3),                                        // workspace
        0u8..3,                                                          // correlation group
        prop::option::of(any::<u64>()),                                  // causation entropy
        1u16..=3,                                                        // payload schema version
        prop::collection::btree_map("[a-z]{1,6}", -1000i64..1000, 0..4), // payload
    )
        .prop_map(|(ide, secs, si, ws, corr, caus, v, payload)| {
            let ts = time::OffsetDateTime::from_unix_timestamp(secs).unwrap();
            let mut value = json!({
                "id": Ulid::from_parts(T0_MS + (ide % 1_000_000), ide as u128).to_string(),
                "ts": ts.format(&time::format_description::well_known::Rfc3339).unwrap(),
                "v": v,
                "source": "proptest",
                "subject": SUBJECTS_V0[si],
                "correlation": ws_ulid(corr + 10).to_string(),
                "payload": serde_json::Value::Object(
                    payload.into_iter().map(|(k, n)| (k, json!(n))).collect()
                ),
            });
            if let Some(k) = ws {
                value["workspace"] = json!(ws_ulid(k).to_string());
            }
            if let Some(c) = caus {
                value["causation"] = json!(Ulid::from_parts(T0_MS, c as u128).to_string());
            }
            serde_json::from_value(value).expect("generator produces well-formed envelopes")
        })
}

fn arb_events_and_split() -> impl Strategy<Value = (Vec<Event>, usize)> {
    prop::collection::vec(arb_event(), 1..120).prop_flat_map(|events| {
        let len = events.len();
        (Just(events), 0..=len)
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    /// rebuild == live state: folding the whole log from seq 0 equals the
    /// incrementally materialized graph.
    #[test]
    fn prop_fold_equals_live_materializer(events in prop::collection::vec(arb_event(), 1..120)) {
        let mut live = Materializer::new();
        for e in &events {
            live.apply(e);
        }
        prop_assert_eq!(&fold(events.iter()), live.graph());
    }

    /// fold(log) == snapshot: resume from a snapshot taken at ANY point and
    /// fold the tail — the result must equal folding everything from seq 0.
    #[test]
    fn prop_snapshot_resume_equals_full_fold((events, split) in arb_events_and_split()) {
        let mut head = Materializer::new();
        for e in &events[..split] {
            head.apply(e);
        }
        let snapshot = head.snapshot();

        let mut resumed = Materializer::resume(snapshot);
        for e in &events[split..] {
            resumed.apply(e);
        }
        prop_assert_eq!(&fold(events.iter()), resumed.graph());
    }

    /// Reducers are pure: replaying the same log twice yields the same graph
    /// (no clocks, no randomness, no iteration-order leaks).
    #[test]
    fn prop_fold_is_deterministic_replay(events in prop::collection::vec(arb_event(), 1..120)) {
        prop_assert_eq!(fold(events.iter()), fold(events.iter()));
    }

    /// Conservation pins the S0 reducer semantics: every event is folded
    /// exactly once — `events_folded` and the per-subject counts sum to the
    /// log length, and `last_event` is the final event's id.
    #[test]
    fn prop_every_event_counted_exactly_once(events in prop::collection::vec(arb_event(), 1..120)) {
        let g = fold(events.iter());
        prop_assert_eq!(g.events_folded, events.len() as u64);
        prop_assert_eq!(g.counts_by_subject.values().sum::<u64>(), events.len() as u64);
        prop_assert_eq!(g.last_event, events.last().map(|e| e.id));
    }
}
