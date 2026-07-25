//! DR-051 §Decision 5 oracle — `rezidnt debrief --json` cost-block absence
//! discipline. An unfolded `Option` cost field must be OMITTED from the cost
//! object, never serialized as JSON `null`: `null` claims "we looked and there
//! is nothing" where the truth is "no value was ever folded". This mirrors
//! `Completion::into_fact`'s convention and the ontology's "absent, not zero"
//! rule (spec/ontology.md), applied at the CLI report surface.
//!
//! Reachable today on any harness by running `debrief` before a run completes:
//! the fold has an `agent_runs` entry (from `agent.spawned`) whose accounting
//! fields are all `None`. Cross-platform (log-read only; no socket, no daemon
//! append — the seeded runs carry no gate facts, so replay raises no alarms).

use std::path::Path;
use std::process::Command;

use rezidnt_fabric::EventLog;
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

fn seed(db: &Path, events: &[Event]) {
    let mut log = EventLog::open(db).expect("seed log");
    for e in events {
        log.append(e).expect("append");
    }
}

/// Run `rezidnt debrief <run> --json` against a seeded log and return the
/// parsed report. `REZIDNT_CAS` is pinned inside the tempdir so replay never
/// touches ambient state.
fn debrief_json(dir: &Path, db: &Path, run: &str) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .arg("debrief")
        .arg(run)
        .arg("--json")
        .env("REZIDNT_DB", db)
        .env("REZIDNT_CAS", dir.join("cas"))
        .output()
        .expect("run rezidnt debrief");
    assert!(
        out.status.success(),
        "debrief with no alarms and no gate facts must exit 0 (DR-004), got {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout)
        .expect("`rezidnt debrief --json` must print the report as JSON on stdout")
}

/// A run that has spawned but not completed has folded NO accounting values.
/// The cost block must therefore contain NO accounting keys at all — an
/// `Option` that was never folded is absent, not `null`.
#[test]
fn debrief_cost_omits_unfolded_fields_not_null() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed(
        &db,
        &[evt(0, "agent.spawned", json!({"run": "run-spawned"}))],
    );

    let report = debrief_json(dir.path(), &db, "run-spawned");
    let cost = report["cost"]
        .as_object()
        .expect("debrief report must carry a cost object");

    for key in ["total_usd", "input_tokens", "output_tokens"] {
        assert!(
            !cost.contains_key(key),
            "cost.{key} was never folded for an incomplete run; the key must be \
             OMITTED (absent, not null) — got {:?}",
            cost.get(key)
        );
    }
}

/// The presence direction: a completion carrying FULL usage folds all three
/// accounting fields, and all three must ride the report with their values.
/// Without this case, an implementation that omits the token keys
/// UNCONDITIONALLY (rather than per-key on absence) would pass the two
/// absence tests above — omission must mean "never folded", not "always".
#[test]
fn debrief_cost_carries_all_folded_fields_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed(
        &db,
        &[
            evt(0, "agent.spawned", json!({"run": "run-full-cost"})),
            evt(
                1,
                "agent.completed",
                json!({
                    "run": "run-full-cost",
                    "status": "completed",
                    "cost": {
                        "total_usd": 1.75,
                        "input_tokens": 12_345,
                        "output_tokens": 678,
                    },
                }),
            ),
        ],
    );

    let report = debrief_json(dir.path(), &db, "run-full-cost");
    let cost = report["cost"]
        .as_object()
        .expect("debrief report must carry a cost object");

    assert_eq!(
        cost.get("total_usd").and_then(serde_json::Value::as_f64),
        Some(1.75),
        "folded total_usd must be PRESENT with its value"
    );
    assert_eq!(
        cost.get("input_tokens").and_then(serde_json::Value::as_u64),
        Some(12_345),
        "folded input_tokens must be PRESENT with its value — omission is \
         only for never-folded fields, never unconditional"
    );
    assert_eq!(
        cost.get("output_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(678),
        "folded output_tokens must be PRESENT with its value — omission is \
         only for never-folded fields, never unconditional"
    );
}

/// A completion whose cost payload carried only `total_usd` folds exactly that
/// field. The report must carry the present value verbatim and OMIT only the
/// keys that were never folded — per-key absence, not all-or-nothing.
#[test]
fn debrief_cost_partial_fold_omits_only_absent_keys() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed(
        &db,
        &[
            evt(0, "agent.spawned", json!({"run": "run-partial"})),
            evt(
                1,
                "agent.completed",
                json!({
                    "run": "run-partial",
                    "status": "completed",
                    "cost": {"total_usd": 0.25},
                }),
            ),
        ],
    );

    let report = debrief_json(dir.path(), &db, "run-partial");
    let cost = report["cost"]
        .as_object()
        .expect("debrief report must carry a cost object");

    assert_eq!(
        cost.get("total_usd").and_then(serde_json::Value::as_f64),
        Some(0.25),
        "a folded cost value must ride the report verbatim"
    );
    for key in ["input_tokens", "output_tokens"] {
        assert!(
            !cost.contains_key(key),
            "cost.{key} was never folded (completion carried no token counts); \
             the key must be OMITTED (absent, not null) — got {:?}",
            cost.get(key)
        );
    }
}
