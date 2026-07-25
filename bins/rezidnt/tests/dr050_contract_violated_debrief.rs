//! DR-050-set oracle — `rezidnt debrief --json` surfaces the folded
//! `run.contract.violated` record (`spec/ontology.md` "### DR-050 set",
//! reducer-obligation consumer (2): "a run whose recorded totals are
//! premise-broken must say so at the done-gate surface").
//!
//! BEHAVIORAL and cross-platform, on the `debrief_cost_absence.rs` mechanism:
//! seed a log directly (log-read only; no socket, no daemon append — the
//! seeded runs carry no gate facts, so replay raises no alarms), run the real
//! CLI binary, assert on its JSON report. The daemon EMITTER of the fact is
//! NOT drivable end-to-end today (see the disclosure in
//! `bins/rezidentd/tests/dr050_contract_violated_surfacing.rs`), but the
//! debrief surface only requires the fact to be ON THE LOG — which a seeded
//! log provides honestly, exactly as a rebuilt-from-log daemon would see it.
//!
//! Report shape pinned here (oracle call, mirroring the ratified fold field
//! name): a top-level `contract_violated` object `{harness, detail}` on the
//! `--json` report, present IFF the fold holds a record — the debrief cost
//! block's per-key absence discipline (omit when None, NEVER null; DR-051
//! §Decision 5) applied to this key.
//!
//! ## RED MODE (stated plainly, per test)
//!
//! - `debrief_surfaces_the_contract_violation` and
//!   `surfaced_detail_is_the_structural_field_not_the_display_wrapping` are
//!   ASSERT-RED today: the report json carries no `contract_violated` key.
//!   (They stay red through the reducer landing until the CLI surfaces the
//!   fold — two implementation steps, one judge at the surface.)
//! - `debrief_omits_contract_violated_when_no_fact_folded` is
//!   GREEN-BY-ABSENCE today (no such key exists at all) — retained, per the
//!   dr006 green-by-absence precedent, as the guard that the key stays
//!   ABSENT-not-null once the surfacing lands. Flagged for the auditor.

use std::path::Path;
use std::process::Command;

use rezidnt_fabric::EventLog;
use rezidnt_types::Event;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_160_000_000;

/// The structural refusal ground, exactly as `AdapterError::ContractViolated
/// { detail, .. }` carries it. The `Display` rendering of that variant wraps
/// this in "codex: recorded stream contract violated — {detail}; refusing
/// rather than …" — the fact (and therefore this report) must carry the FIELD,
/// never the wrapping (ontology: "lifted STRUCTURALLY … never parsed back out
/// of the prose detail").
const DETAIL: &str = "a second turn-terminal line arrived on one `codex exec --json` stream \
                      (terminal turn 2), falsifying the single-shot premise the recorded \
                      contract rests on";

fn evt(i: u64, subject: &str, payload: serde_json::Value) -> Event {
    let id = Ulid::from_parts(T0_MS + i, i as u128 + 1);
    serde_json::from_value(json!({
        "id": id.to_string(),
        "ts": "2026-07-25T00:00:00Z",
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

/// The log of a run whose recorded totals were later premise-broken: the run
/// completed (turn-1 accounting published as the RUN total), then the adapter
/// refused the stream — the exact sequence the ontology's timing bullet names
/// as today's only constructible path.
fn seed_violated_run(db: &Path, run: &str) {
    seed(
        db,
        &[
            evt(0, "agent.spawned", json!({"run": run})),
            evt(
                1,
                "agent.completed",
                json!({
                    "run": run,
                    "status": "completed",
                    "cost": {"total_usd": 0.19, "input_tokens": 7441, "output_tokens": 45},
                    "num_turns": 1,
                }),
            ),
            evt(
                2,
                "run.contract.violated",
                json!({"run": run, "harness": "codex", "detail": DETAIL}),
            ),
        ],
    );
}

/// Consumer (2), the presence direction: a folded violation rides the report
/// as a `contract_violated` object naming the refusing harness and its
/// ground — the dossier says so at the done-gate surface.
#[test]
fn debrief_surfaces_the_contract_violation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed_violated_run(&db, "run-violated");

    let report = debrief_json(dir.path(), &db, "run-violated");
    let cv = report["contract_violated"].as_object().unwrap_or_else(|| {
        panic!(
            "a run with a run.contract.violated fact on its log must surface a \
             `contract_violated` object on the debrief report — its recorded totals are \
             premise-broken and the done-gate surface must say so (ontology DR-050 set, \
             consumer (2)). Got report: {report:#}"
        )
    });
    assert_eq!(
        cv.get("harness").and_then(serde_json::Value::as_str),
        Some("codex"),
        "the refusing harness rides the report verbatim"
    );
    assert_eq!(
        cv.get("detail").and_then(serde_json::Value::as_str),
        Some(DETAIL),
        "the refusal ground rides the report verbatim"
    );
}

/// The authorship/structure boundary at the surface: the surfaced `detail`
/// is the variant's structural field EXACTLY — not the `Display` rendering
/// that wraps it ("codex: recorded stream contract violated — …; refusing
/// rather than …"). An emitter that logged `e.to_string()` (or a surface
/// that re-wrapped) turns this red even after the key exists.
#[test]
fn surfaced_detail_is_the_structural_field_not_the_display_wrapping() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed_violated_run(&db, "run-violated");

    let report = debrief_json(dir.path(), &db, "run-violated");
    let detail = report["contract_violated"]["detail"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "the report must carry contract_violated.detail (see \
                 debrief_surfaces_the_contract_violation) — got {report:#}"
            )
        });
    assert_eq!(detail, DETAIL, "detail is the structural field, byte-exact");
    assert!(
        !detail.contains("recorded stream contract violated"),
        "the Display wrapping must never leak into the surfaced detail — `harness` and \
         `detail` are lifted structurally from the variant, never parsed back out of (or \
         wrapped in) the prose text (ontology DR-050 set). Got {detail:?}"
    );
}

/// The absence direction (the cost block's per-key discipline, DR-051
/// §Decision 5): a run with NO violation fact carries NO `contract_violated`
/// key — omitted, never null. `null` would claim "we looked and there is
/// nothing" where the truth is "no fact was ever folded".
///
/// ORACLE HONESTY NOTE (GREEN-BY-ABSENCE today): the report carries no such
/// key for ANY run yet, so this holds trivially pre-implementation. Retained
/// as the guard that the surfacing lands per-key-on-presence, not
/// unconditionally — an implementer who serializes `None` as `null` turns
/// this red. Flagged for the auditor, per the dr006 precedent.
#[test]
fn debrief_omits_contract_violated_when_no_fact_folded() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed(
        &db,
        &[
            evt(0, "agent.spawned", json!({"run": "run-clean"})),
            evt(
                1,
                "agent.completed",
                json!({"run": "run-clean", "status": "completed", "cost": {"total_usd": 0.19}}),
            ),
        ],
    );

    let report = debrief_json(dir.path(), &db, "run-clean");
    let obj = report.as_object().expect("debrief --json prints an object");
    assert!(
        !obj.contains_key("contract_violated"),
        "a run with no run.contract.violated fact must OMIT the key entirely — absent, \
         not null (the debrief cost block's per-key discipline, DR-051 §Decision 5). \
         Got {:?}",
        obj.get("contract_violated")
    );
}
