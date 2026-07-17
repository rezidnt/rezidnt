//! S5 oracle — live update via the WATCH SEAM. The board's render loop consumes
//! ONLY a `tokio::sync::watch<Graph>`; it never touches a raw `Event`. This
//! suite feeds an in-memory event log through `ingest_into_watch` into a
//! `watch::Sender<Graph>` and asserts the receiver-driven projection reflects
//! the fleet transition (a run spawning -> completed shows the new status and
//! recorded cost). No socket: the pure ingest core is testable with a `Vec`.
//!
//! Criterion: "consuming only watch channels." The receiver side calls
//! `project` on each observed `Graph` snapshot — the render path is a pure fn
//! of derived state carried over the watch channel.
//!
//! RED MODE: assert-red. `ingest_into_watch` exists (oracle scaffold) but drops
//! every event on the floor and publishes nothing, so the receiver never
//! observes the folded transition — every assertion below fails until the
//! implementer folds each event and `send`s the snapshot. Mirrors the S4 /
//! DR-006 scaffold discipline.

use rezidnt_state::Graph;
use rezidnt_tui::{RunRow, ingest_into_watch, project};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use tokio::sync::watch;
use ulid::Ulid;

const RUN: &str = "01S5WATCH0LIVE0000000000R01";

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("test"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// Find a run row by id in a projected view.
fn run_row<'v>(view: &'v rezidnt_tui::BoardView, run: &str) -> Option<&'v RunRow> {
    view.runs.iter().find(|r| r.run == run)
}

/// Feed a spawn-then-complete log through the watch seam; the receiver's LAST
/// observed snapshot must project to the completed run with recorded cost.
/// Proves the watch channel carries state transitions to the render loop.
#[tokio::test]
async fn watch_receiver_observes_the_spawn_to_completed_transition() {
    let events = [
        ev("agent.spawned", json!({"run": RUN, "agent": "impl"})),
        ev(
            "agent.status.changed",
            json!({"run": RUN, "from": "spawning", "to": "running"}),
        ),
        ev(
            "agent.completed",
            json!({
                "run": RUN,
                "status": "success",
                "cost": {"total_usd": 0.5, "input_tokens": 100, "output_tokens": 7},
                "session_id": "watch-seam-session"
            }),
        ),
    ];

    let (tx, rx) = watch::channel(Graph::default());
    ingest_into_watch(events.iter(), &tx);

    // The render loop only ever reads the watch channel — never a raw Event.
    let latest = rx.borrow().clone();
    let view = project(&latest);

    let row = run_row(&view, RUN).expect("the watch channel carried the run into view");
    assert_eq!(
        row.status, "completed",
        "the watch snapshot reflects the final transition"
    );
    assert_eq!(
        row.total_usd,
        Some(0.5),
        "recorded cost rode the watch seam"
    );
    assert_eq!(row.output_tokens, Some(7));
}

/// The seam publishes a fresh snapshot AFTER EACH event (not just the final
/// one): the intermediate `spawning` snapshot must be observable, then the
/// terminal `completed` snapshot. A render loop that redraws on every watch
/// change depends on this.
#[tokio::test]
async fn watch_seam_publishes_a_snapshot_per_event() {
    let events = [
        ev("agent.spawned", json!({"run": RUN, "agent": "impl"})),
        ev(
            "agent.completed",
            json!({
                "run": RUN,
                "status": "success",
                "cost": {"total_usd": 0.01, "input_tokens": 1, "output_tokens": 1}
            }),
        ),
    ];

    let (tx, rx) = watch::channel(Graph::default());
    ingest_into_watch(events.iter(), &tx);

    // events_folded advances one-per-event; the terminal snapshot has folded
    // both, and its projection shows the completed run.
    let latest = rx.borrow().clone();
    assert_eq!(
        latest.events_folded, 2,
        "the seam folded every event into the published snapshot"
    );
    let view = project(&latest);
    let row = run_row(&view, RUN).expect("run present in the final snapshot");
    assert_eq!(row.status, "completed");
}

/// The initial `watch::channel` seed is the empty graph, and its projection is
/// the empty board — a clean before-state so the transition above is a real
/// change, not a coincidence. (Guards against a scaffold that "passes" by
/// never distinguishing empty from populated.)
#[tokio::test]
async fn empty_seed_projects_to_an_empty_board() {
    let (_tx, rx) = watch::channel(Graph::default());
    let view = project(&rx.borrow());
    assert_eq!(view.runs.len(), 0, "no runs before any event is ingested");
    assert_eq!(view.events_folded, 0);
}
