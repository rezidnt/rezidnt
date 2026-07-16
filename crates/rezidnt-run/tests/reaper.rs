//! S1 oracle: reaper — pidfile reconciliation and TERM→KILL escalation.
//! Escalation is unix-gated (signals); reconciliation logic is pinned on
//! unix too because liveness probing is kill(0)-based in the S1 design.
#![cfg(unix)]

use std::time::{Duration, Instant};

use rezidnt_run::RunId;
use rezidnt_run::reaper::{OrphanAction, reconcile_pidfiles, stop_with_escalation};
use ulid::Ulid;

/// A pidfile whose pid is dead is an orphan → Reap; a live pid → Adopt.
#[test]
fn reconcile_distinguishes_dead_from_live_pids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dead_run = RunId::new(Ulid::from_parts(3, 1));
    let live_run = RunId::new(Ulid::from_parts(3, 2));

    // A pid that is certainly dead: spawn true, wait it out, use its pid.
    let dead = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let dead_pid = dead.id();
    let mut dead_child = dead;
    dead_child.wait().expect("true exits");

    // A pid that is certainly alive during the test: our own.
    let live_pid = std::process::id();

    std::fs::write(
        dir.path().join(format!("{}.pid", dead_run.ulid())),
        dead_pid.to_string(),
    )
    .expect("write dead pidfile");
    std::fs::write(
        dir.path().join(format!("{}.pid", live_run.ulid())),
        live_pid.to_string(),
    )
    .expect("write live pidfile");

    let mut actions = reconcile_pidfiles(dir.path()).expect("reconcile");
    actions.sort_by_key(|a| match a {
        OrphanAction::Reap { run, .. } | OrphanAction::Adopt { run, .. } => run.ulid(),
    });
    assert_eq!(
        actions,
        [
            OrphanAction::Reap {
                run: dead_run,
                pid: dead_pid
            },
            OrphanAction::Adopt {
                run: live_run,
                pid: live_pid
            },
        ]
    );
}

/// A child that ignores SIGTERM is KILLed after the grace period — within
/// grace + slack, never hanging forever, never left alive.
#[tokio::test]
async fn escalation_kills_a_term_ignoring_child_after_grace() {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(r#"trap "" TERM; sleep 300"#)
        .spawn()
        .expect("spawn TERM-ignoring child");
    let pid = child.id().expect("child pid");

    let grace = Duration::from_millis(500);
    let started = Instant::now();
    let outcome = stop_with_escalation(pid, grace)
        .await
        .expect("escalation completes");
    let elapsed = started.elapsed();

    assert!(elapsed >= grace, "must actually wait the grace period");
    assert!(
        elapsed < grace + Duration::from_secs(5),
        "must not hang past grace + slack"
    );
    assert!(
        outcome.contains("KILL") || outcome.contains("kill"),
        "outcome must record the escalation, got {outcome:?}"
    );
    let status = child.wait().await.expect("child reaped");
    assert!(!status.success(), "killed child must not report success");
}
