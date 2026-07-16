//! Reaper (DR-001): exit status collection, TERM→KILL escalation with grace,
//! orphan cleanup on daemon restart via pidfile reconciliation.

#[cfg(unix)]
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
use crate::RunError;
use crate::RunId;

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
///
/// Files that are not `<ulid>.pid` with a numeric pid are skipped — a
/// half-written pidfile from a crashed daemon must not wedge reconciliation
/// (unpinned call, flagged for the auditor).
///
/// Unix-only like the rest of this module's probing: liveness is
/// kill(pid, 0)-based (S1 design; the test suite pins it on unix).
#[cfg(unix)]
pub fn reconcile_pidfiles(dir: &Path) -> Result<Vec<OrphanAction>, RunError> {
    let mut actions = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "pid") {
            continue;
        }
        let Some(run) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<ulid::Ulid>().ok())
            .map(RunId::new)
        else {
            continue;
        };
        let Some(pid) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| content.trim().parse::<u32>().ok())
        else {
            continue;
        };
        let action = if pid_is_alive(pid)? {
            OrphanAction::Adopt { run, pid }
        } else {
            OrphanAction::Reap { run, pid }
        };
        actions.push(action);
    }
    Ok(actions)
}

/// kill(pid, 0): signal 0 delivers nothing but performs the liveness check.
/// EPERM means "exists but not ours" — alive; ESRCH means dead.
#[cfg(unix)]
fn pid_is_alive(pid: u32) -> Result<bool, RunError> {
    let pid = i32::try_from(pid)
        .map_err(|_| RunError::Spawn(format!("pid {pid} exceeds the platform pid range")))?;
    // SAFETY: kill with signal 0 only error-checks the target pid; it
    // delivers no signal and touches no memory.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(std::io::Error::last_os_error().into()),
    }
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: i32) -> Result<(), RunError> {
    let pid = i32::try_from(pid)
        .map_err(|_| RunError::Spawn(format!("pid {pid} exceeds the platform pid range")))?;
    // SAFETY: plain kill(2); no memory is shared with the callee.
    if unsafe { libc::kill(pid, signal) } == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        // Already gone: stopping a dead process is success, not failure.
        Some(libc::ESRCH) => Ok(()),
        _ => Err(std::io::Error::last_os_error().into()),
    }
}

/// Stop a child: TERM, wait out [`TERM_GRACE`], then KILL. Returns the exit
/// description for the terminal fact.
///
/// The grace wait is a plain `tokio::time::sleep` — the full grace is always
/// waited (a zombie child is indistinguishable from a live one by
/// kill(pid, 0), so early-exit polling would be a lie half the time).
#[cfg(unix)]
pub async fn stop_with_escalation(pid: u32, grace: Duration) -> Result<String, RunError> {
    send_signal(pid, libc::SIGTERM)?;
    tokio::time::sleep(grace).await;
    if pid_is_alive(pid)? {
        send_signal(pid, libc::SIGKILL)?;
        Ok(format!(
            "escalated: SIGTERM unanswered after {grace:?}, sent SIGKILL"
        ))
    } else {
        Ok(format!("exited on SIGTERM within {grace:?}"))
    }
}
