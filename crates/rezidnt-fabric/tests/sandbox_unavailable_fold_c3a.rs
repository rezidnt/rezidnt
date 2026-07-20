//! C3a oracle (DR-025 — the Linux OS-sandbox slice), CRITERION 4 (the log arm) —
//! a `sandbox.unavailable` DEGRADE is a DURABLE, REPLAYABLE fact on the log, not
//! a swallowed silent allow. "The degrade is visible on the log, not swallowed"
//! (DR-025 §Acceptance-criteria 4, §Exit-demo).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (fabric append + read-back + fold,
//! platform-neutral; no bwrap, no #[cfg(unix)]). This is the fabric/state half of
//! criterion 4's bwrap-absent arm; the availability PROBE half is
//! `crates/rezidnt-run/tests/sandbox_degrade_and_deps_c3a.rs`. Placed in the
//! fabric crate (not state) because it drives a REAL `EventLog` round-trip, the
//! same dev-dep footprint the crash-safety oracle already uses.
//!
//! ## WARDEN-GATED ONTOLOGY — DO NOT MINT HERE.
//! DR-025 §Consequences flags the `sandbox.*` subject family (`sandbox.spawned` /
//! `sandbox.denied` / `sandbox.unavailable`) vs. riding the existing `permit.*`
//! axis as a DEFERRED warden `/subject` question — NOT decided in the DR and NOT
//! minted here. This test pins ONLY the invariant TRUE regardless of the choice:
//!   - the degrade fact is DURABLE (append is the commit point, I3) and REPLAYABLE
//!     (read-from-seq-0 reproduces it) — it is NOT swallowed;
//!   - a fact the current reducers do not fold does NOT crash the fold
//!     (tolerated-noise `_ => {}`), so the log carries it losslessly until a
//!     reducer is wired.
//!
//! TODO(warden, /subject): once the `sandbox.*` family (or a `permit.denied`
//! variant for sandbox-unavailable) is minted WITH its folding reducer (no
//! consumer-less subjects — DR-006), STRENGTHEN this to assert the degrade folds
//! onto a run-state field (e.g. `sandbox_degraded: true` + a `backend` record)
//! and that `gate_explain` surfaces it. Until then the placeholder subject stands
//! in for the implementer's chosen wiring; the SUBJECT STRING is not ratified.

use rezidnt_fabric::EventLog;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

const RUN: &str = "01C3ASANDBOXDEGRADE00R01";

/// The PLACEHOLDER degrade fact (subject name NOT warden-ratified — see header).
/// Shape mirrors the DR-025 §5 candidate `sandbox.unavailable {run, backend,
/// reason}`. The daemon emits this BEFORE it proceeds to the unsandboxed spawn,
/// so the log records the degrade cause and that enforcement was absent for this
/// run — the honest, loud alternative to a silent allow (I6).
fn sandbox_unavailable(run: &str) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        // WARDEN-GATED: placeholder subject, not a ratified ontology name.
        Subject::new("sandbox.unavailable"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": run,
            "backend": "bwrap",
            "reason": "bwrap not found on PATH",
        }),
    )
    .expect("degrade fact under 32KiB")
}

/// CRITERION 4 (log arm) — the degrade fact is DURABLE and REPLAYABLE, and the
/// fold does not crash on it. Append it to a real log, read from seq 0, and
/// confirm the fact survives the round-trip — the "visible on the log, not
/// swallowed" property, independent of the warden's not-yet-made subject choice.
///
/// This HOLDS today for the durability half (the fabric tolerates any subject)
/// and is the TRACKING PIN the warden strengthens once a reducer exists. It fails
/// LOUDLY if a future change makes the fabric reject/drop an unmodeled subject —
/// which would let the implementer wire a silent degrade (the DR forbids it).
#[test]
fn sandbox_unavailable_degrade_is_durable_and_replayable_on_the_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut log = EventLog::open(&dir.path().join("events.db")).expect("open log");

    let degrade = sandbox_unavailable(RUN);
    let degrade_id = degrade.id;
    log.append(&degrade)
        .expect("append is the commit point — the degrade is durable (I3, CRITERION 4)");

    // Replayable: a fresh read from seq 0 reproduces the fact verbatim — the
    // degrade is NOT swallowed, it is on the compliance log forever (I3).
    let replayed: Vec<Event> = log
        .read_from(1)
        .expect("read the log back")
        .into_iter()
        .map(|r| r.event)
        .collect();
    let found = replayed
        .iter()
        .find(|e| e.id == degrade_id)
        .expect("the degrade fact survives the log round-trip — it is durable (CRITERION 4)");
    assert_eq!(
        found.subject.as_str(),
        "sandbox.unavailable",
        "the durable degrade fact carries the (warden-gated placeholder) degrade subject"
    );
    assert_eq!(
        found.payload()["reason"].as_str(),
        Some("bwrap not found on PATH"),
        "the degrade fact records the LOGGABLE reason so the absence of enforcement is \
         interrogable (I6, CRITERION 4) — never a silent allow"
    );

    // The fold does not crash on the unmodeled subject (tolerated-noise
    // discipline): the run graph builds, the log carries the fact losslessly
    // until the warden mints the reducer. This is the seam the TODO strengthens.
    let graph = rezidnt_state::fold(replayed.iter());
    // No reducer folds this subject yet, so the run may have no entry; the
    // ASSERTION is only that folding a log CONTAINING it does not panic.
    let _ = graph.agent_runs.get(RUN);
}
