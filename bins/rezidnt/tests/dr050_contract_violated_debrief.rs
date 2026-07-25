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
//! ## STATE (truth pass, session 32)
//!
//! ASSERT-RED when written; the fold arm and the CLI surfacing have since
//! landed, and every test here is a GREEN regression pin.
//! `debrief_omits_contract_violated_when_no_fact_folded` was
//! GREEN-BY-ABSENCE at introduction (flagged then, per the dr006 precedent);
//! with the key now real it is the live guard that the surfacing stays
//! per-key-on-presence — an implementer who serializes `None` as `null`
//! turns it red.
//!
//! The session-32 remediation adds the HUMAN (non-`--json`) surface pin —
//! consumer (2)'s load-bearing half, previously unjudged: the default form
//! is what a human reads at the done gate, and the JSON report alone
//! satisfies the letter of consumer (2) and none of its point (the surface's
//! own comment says exactly this). Added AFTER the surface landed; proven
//! able to go red by mutation (human block removed, test red, source
//! restored) before being reported green.
//!
//! EXIT-CLASS DISCLOSURE: every test here asserts exit 0 through the shared
//! runner. For the clean run that is settled law (DR-004: no alarms, no gate
//! facts). For the VIOLATED runs it pins the STATUS QUO, not a ruling —
//! whether a contract-violated run should exit 3 (the `integrity.alarm`
//! analogy: neither trusted nor coerced) is an OPEN owner-level question for
//! a decision record. If that ruling lands, the runner's assertion goes red
//! on purpose and follows the DR: the red is the ruling's judge arriving,
//! not a regression.

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

/// Run `rezidnt debrief <run>` (with or without `--json`) against a seeded
/// log and return the raw output. `REZIDNT_CAS` is pinned inside the tempdir
/// so replay never touches ambient state.
///
/// The success assertion pins the STATUS QUO, not a ruling — see the
/// header's EXIT-CLASS DISCLOSURE.
fn debrief(dir: &Path, db: &Path, run: &str, json: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("debrief").arg(run);
    if json {
        cmd.arg("--json");
    }
    let out = cmd
        .env("REZIDNT_DB", db)
        .env("REZIDNT_CAS", dir.join("cas"))
        .output()
        .expect("run rezidnt debrief");
    assert!(
        out.status.success(),
        "debrief exited {:?}, expected 0. For the clean run this is DR-004 law (no \
         alarms, no gate facts). For the violated runs it is a STATUS-QUO pin on an \
         OPEN question — whether a contract-violated run should exit 3 (the \
         integrity.alarm analogy) has NOT been ruled; this assertion deliberately \
         goes red the day a DR changes the exit class, and should then be updated \
         to follow the DR, never quietly deleted. stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// The `--json` form of [`debrief`], parsed.
fn debrief_json(dir: &Path, db: &Path, run: &str) -> serde_json::Value {
    let out = debrief(dir, db, run, true);
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
/// ORACLE HONESTY NOTE: introduced GREEN-BY-ABSENCE (the report carried no
/// such key for any run, so it held trivially), and flagged as such for the
/// auditor per the dr006 precedent. It is now a REAL per-key guard — the
/// surfacing has landed and `debrief_surfaces_the_contract_violation` proves
/// the key exists, so this test discriminates omitted-on-absence from
/// emitted-unconditionally. An implementer who serializes `None` as `null`
/// turns it red.
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

/// Consumer (2)'s LOAD-BEARING half (session-32 auditor finding): the
/// DEFAULT (non-`--json`) form is the done-gate surface a human actually
/// reads — the JSON report alone satisfies the letter of consumer (2) and
/// none of its point. Pins what the surface actually prints: the
/// `CONTRACT VIOLATED (<harness>)` block, its do-not-score consequence, and
/// the refusal ground verbatim — plus the absence direction: a clean run
/// prints no block, mirroring the JSON key's per-key discipline.
#[test]
fn debrief_human_output_names_the_violation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("events.db");
    seed_violated_run(&db, "run-violated");

    let out = debrief(dir.path(), &db, "run-violated", false);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("CONTRACT VIOLATED (codex)"),
        "a premise-broken run must say so on the HUMAN surface: the default form \
         prints a `CONTRACT VIOLATED (<harness>)` block (ontology DR-050 set, \
         consumer (2): \"must say so at the done-gate surface\" — the surface a \
         human reads, not only the JSON report). Got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("do not score"),
        "the block carries its consequence — the run's recorded totals rest on a \
         premise the substrate withdrew and must not be scored. Got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains(DETAIL),
        "the refusal ground rides the human surface verbatim, exactly as it rides \
         the JSON report — a violation the reader cannot interrogate is a verdict \
         without evidence (I6). Got stdout:\n{stdout}"
    );

    let clean_dir = tempfile::tempdir().unwrap();
    let clean_db = clean_dir.path().join("events.db");
    seed(
        &clean_db,
        &[
            evt(0, "agent.spawned", json!({"run": "run-clean"})),
            evt(
                1,
                "agent.completed",
                json!({"run": "run-clean", "status": "completed", "cost": {"total_usd": 0.19}}),
            ),
        ],
    );
    let out = debrief(clean_dir.path(), &clean_db, "run-clean", false);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("CONTRACT VIOLATED"),
        "a run with no folded violation prints no block — absence stays absence on \
         the human surface too. Got stdout:\n{stdout}"
    );
}
