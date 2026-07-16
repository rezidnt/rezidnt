//! Crash-test harness: append a burst of well-formed events, durably, so a
//! SIGKILL at any instant leaves a valid log prefix (WAL recovery + intact
//! chain). Spawned as the `burst-writer` bin by `tests/crash_safety.rs`.

use std::path::Path;

use rezidnt_types::taxonomy::SUBJECTS_V0;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

use crate::FabricError;
use crate::log::EventLog;

/// Append `count` events to the log at `db`, one durable transaction per
/// append (the commit point must be per-event so a mid-burst SIGKILL truncates
/// to a whole row, never a torn one).
///
/// Event recipe (deterministic in shape, minted ids/ts): subjects cycle
/// through `rezidnt_types::taxonomy::SUBJECTS_V0`, source `burst-writer`,
/// payload `{"n": <i>}`.
pub fn run_burst(db: &Path, count: u64) -> Result<(), FabricError> {
    let mut log = EventLog::open(db)?;
    let correlation = Ulid::new();
    for i in 0..count {
        let subject = SUBJECTS_V0[(i % SUBJECTS_V0.len() as u64) as usize];
        let event = Event::new(
            SourceId::new("burst-writer"),
            None,
            Subject::new(subject),
            correlation,
            None,
            1,
            json!({"n": i}),
        )?;
        log.append(&event)?;
    }
    Ok(())
}
