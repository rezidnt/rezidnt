//! Reaper (DR-001): exit status collection, TERM→KILL escalation with grace,
//! orphan cleanup on daemon restart via pidfile reconciliation.

use std::path::Path;
use std::time::Duration;

use crate::{RunError, RunId};

/// Grace between SIGTERM and SIGKILL (DEFAULT).
pub const TERM_GRACE: Duration = Duration::from_secs(5);

/// What reconciliation decided about one pidfile found on disk.
#[derive(Debug, Clone, PartialEq)]
pub enum OrphanAction {
    /// Pid is dead: the run is an orphan; emit its terminal fact and clean up.
    Reap { run: RunId, pid: u32 },
    /// Pid is alive and matches the recorded run: re-adopt it.
    Adopt { run: RunId, pid: u32 },
}

/// Scan a pidfile directory (one `<run-ulid>.pid` per live run, written at
/// spawn) and decide, per file, whether the run survived the daemon restart.
pub fn reconcile_pidfiles(dir: &Path) -> Result<Vec<OrphanAction>, RunError> {
    let _ = dir;
    todo!("S1: read pidfiles, liveness-probe, Reap or Adopt")
}

/// Stop a child: TERM, wait out [`TERM_GRACE`], then KILL. Returns the exit
/// description for the terminal fact.
#[cfg(unix)]
pub async fn stop_with_escalation(pid: u32, grace: Duration) -> Result<String, RunError> {
    let _ = (pid, grace);
    todo!("S1: TERM → grace → KILL")
}
