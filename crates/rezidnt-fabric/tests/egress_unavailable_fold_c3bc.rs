//! C3b+c oracle (DR-026 — the L7 egress-MITM + credential-brokering slice),
//! CRITERION 7 (the log arm) — an `egress.unavailable` CLOSED degrade is a
//! DURABLE, REPLAYABLE fact on the log, not a swallowed silent open. "The absence
//! of the mediation path fails closed and announces itself" / "with the connector
//! or CA absent, the same run has no network and says so on the log
//! (`egress.unavailable`) and injects nothing" (DR-026 §Acceptance-criteria 7,
//! §Exit-demo).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (fabric append + read-back + fold,
//! platform-neutral; no connector, no #[cfg(unix)]). This is the fabric/state half
//! of criterion 7's connector/CA-absent arm; the availability PROBE half is
//! `crates/rezidnt-run/tests/egress_degrade_and_deps_c3bc.rs`. Placed in the
//! fabric crate (not state) because it drives a REAL `EventLog` round-trip — the
//! same footprint the C3a `sandbox_unavailable_fold_c3a.rs` oracle uses.
//!
//! ## WARDEN-GATED ONTOLOGY — DO NOT MINT HERE.
//! DR-026 §Consequences flags the `egress.*`/`credential.*` subject family
//! (`egress.requested`/`.allowed`/`.denied`/`.unavailable`, `credential.injected`)
//! vs. riding the existing `permit.*` axis as a DEFERRED warden `/subject`
//! question — NOT decided in the DR and NOT minted here. This test pins ONLY the
//! invariant TRUE regardless of the choice:
//!   - the CLOSED-degrade fact is DURABLE (append is the commit point, I3) and
//!     REPLAYABLE (read-from-seq-0 reproduces it) — it is NOT swallowed;
//!   - a fact the current reducers do not fold does NOT crash the fold
//!     (tolerated-noise `_ => {}`), so the log carries it losslessly until a
//!     reducer is wired.
//!
//! TODO(warden, /subject): once the `egress.*`/`credential.*` family (or a
//! `permit.denied` variant for egress-unavailable) is minted WITH its folding
//! reducer (no consumer-less subjects — DR-006), STRENGTHEN this to assert the
//! CLOSED degrade folds onto a run-state field (e.g. `egress_degraded: true` +
//! `network: sealed` + `injected: false`) and that `gate_explain` surfaces it.
//! Until then the placeholder subject stands in for the implementer's chosen
//! wiring; the SUBJECT STRING is not ratified.

use rezidnt_fabric::EventLog;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

const RUN: &str = "01C3BCEGRESSDEGRADE00R001";

/// The PLACEHOLDER CLOSED-degrade fact (subject name NOT warden-ratified — see
/// header). Shape mirrors the DR-026 design §5 candidate `egress.unavailable
/// {run, reason}`. The daemon emits this INSTEAD of opening egress when the
/// connector/proxy/CA is absent, so the log records the degrade cause AND that
/// the run kept the sealed netns (no network) and injected nothing — the honest,
/// loud, CLOSED alternative to a silent open (I6, the inverse of C3a's degrade).
fn egress_unavailable(run: &str) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        // WARDEN-GATED: placeholder subject, not a ratified ontology name.
        Subject::new("egress.unavailable"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": run,
            "backend": "pasta+rustls",
            "reason": "pasta not found on PATH",
            // The CLOSED-degrade posture recorded on the fact: the run kept the
            // sealed netns and injected nothing (never a silent open).
            "network": "sealed",
            "injected": false,
        }),
    )
    .expect("degrade fact under 32KiB")
}

/// CRITERION 7 (log arm) — the CLOSED-degrade fact is DURABLE and REPLAYABLE, and
/// the fold does not crash on it. Append it to a real log, read from seq 0, and
/// confirm the fact survives the round-trip — the "no network and says so on the
/// log" property, independent of the warden's not-yet-made subject choice.
///
/// This HOLDS today for the durability half (the fabric tolerates any subject)
/// and is the TRACKING PIN the warden strengthens once a reducer exists. It fails
/// LOUDLY if a future change makes the fabric reject/drop an unmodeled subject —
/// which would let the implementer wire a SILENT closed degrade (the DR requires
/// it be a LOUD fact).
#[test]
fn egress_unavailable_closed_degrade_is_durable_and_replayable_on_the_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut log = EventLog::open(&dir.path().join("events.db")).expect("open log");

    let degrade = egress_unavailable(RUN);
    let degrade_id = degrade.id;
    log.append(&degrade)
        .expect("append is the commit point — the CLOSED degrade is durable (I3, CRITERION 7)");

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
        .expect("the CLOSED-degrade fact survives the log round-trip — durable (CRITERION 7)");
    assert_eq!(
        found.subject.as_str(),
        "egress.unavailable",
        "the durable degrade fact carries the (warden-gated placeholder) degrade subject"
    );
    assert_eq!(
        found.payload()["reason"].as_str(),
        Some("pasta not found on PATH"),
        "the degrade fact records the LOGGABLE reason so the absence of the mediation path is \
         interrogable (I6, CRITERION 7) — never a silent open"
    );
    // The CLOSED-degrade posture is on the fact: no network, no injection — the
    // strictest degrade, announced loudly (DR-026 §Consequences).
    assert_eq!(
        found.payload()["network"].as_str(),
        Some("sealed"),
        "the CLOSED degrade kept the sealed netns — no unmediated egress (CRITERION 7)"
    );
    assert_eq!(
        found.payload()["injected"].as_bool(),
        Some(false),
        "the CLOSED degrade injected NO credential — never a leaked secret without the \
         mediation path intact (CRITERION 7)"
    );

    // The fold does not crash on the unmodeled subject (tolerated-noise
    // discipline): the run graph builds, the log carries the fact losslessly
    // until the warden mints the reducer. This is the seam the TODO strengthens.
    let graph = rezidnt_state::fold(replayed.iter());
    // No reducer folds this subject yet, so the run may have no entry; the
    // ASSERTION is only that folding a log CONTAINING it does not panic.
    let _ = graph.agent_runs.get(RUN);
}
