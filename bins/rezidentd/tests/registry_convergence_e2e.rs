//! REGISTRY-CONVERGENCE ORACLE — the whole-system judges for DR-046
//! §Decision 8 (criteria C2 and C4, plus the proof that the repoint actually
//! happened rather than the values merely being copied).
//!
//! `#[cfg(unix)]` + `*_e2e.rs`, per the house convention, so this board runs in
//! WSL and NOT in host `/vet`. Everything that could be lifted somewhere host
//! `/vet` can see it has been lifted:
//! `bins/rezidentd/tests/registry_convergence_structure.rs` carries the
//! manifest and single-emitter guards as text checks, and the git adapter's
//! `allocation_principal_and_sink.rs` / `worktree_conflict_structural.rs`
//! carry the principal, envelope and conflict guards over real repos. What is
//! left here is the part that genuinely needs a daemon: two emitters in one
//! process, and a persisted log to count on.
//!
//! ## RED MODE
//!
//! ASSERT-RED on the live tree, all three tests, for distinct reasons:
//!
//! - the one-fact test fails once the repoint lands and both emitters run
//!   (`bins/rezidentd/src/runs.rs` publishes `worktree.allocated` and so does
//!   the adapter — DR-046 §Decision 8 calls silencing one of them the
//!   non-negotiable condition); it is GREEN today only because the adapter is
//!   unreachable, which is precisely the state this slice ends;
//! - the registry-backing test fails TODAY, because the daemon's private
//!   allocator never writes an adapter registry entry;
//! - the envelope test is green today and becomes the regression guard that the
//!   repoint does not lose `workspace` / `causation`, which the adapter's own
//!   `emit` drops (`None` workspace, adapter-owned correlation).
//!
//! The first is the honest shape of a guard against a regression a change is
//! ABOUT to introduce: it cannot be red before the change exists. It is stated
//! here rather than dressed up, in the house style of
//! `bench/harness/tests/testkit_dev_only.rs`.
//!
//! ## The registry-backing test is the anti-tautology guard
//!
//! A repoint could be faked: keep the private git-CLI allocator and just make
//! its facts look right. Every value assertion in this arc would stay green.
//! What cannot be faked is the SOLE-ALLOCATOR REGISTRY holding an entry for
//! every allocated path — that file is written by the adapter and by nothing
//! else. So it is the assertion that distinguishes "the convergence happened"
//! from "the values were copied", and it is why this board exists at all.
#![cfg(unix)]

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use common::{DaemonGuard, connect, make_gated_project, open_request, read_until, send_line};
use rezidnt_fabric::EventLog;
use rezidnt_types::Event;
use serde_json::Value;

const FACT_DEADLINE: Duration = Duration::from_secs(30);

/// Drive one `open` to completion and cold-read the daemon's PERSISTED log —
/// the log is the judge, not the live stream (I3).
///
/// The fixture `TempDir` is RETURNED, not dropped: the assertions read the
/// allocated trees and the registry file off disk, so the caller has to keep
/// the project alive for the whole test.
fn open_and_cold_read() -> (tempfile::TempDir, std::path::PathBuf, Vec<Event>) {
    let mut daemon = common::start_daemon();
    let (project, spec) = make_gated_project(50);
    let repo = project.path().join("repo");

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let _ = read_until(&mut tail, FACT_DEADLINE, |v: &Value| {
        v["subject"] == "agent.completed"
    });

    let log = cold_read(&mut daemon);
    (project, repo, log)
}

fn cold_read(daemon: &mut DaemonGuard) -> Vec<Event> {
    let _ = daemon.child.kill();
    let _ = daemon.child.wait();
    let log = EventLog::open(&daemon.db).expect("re-open the daemon's persisted log cold");
    log.read_from(1)
        .expect("read the persisted log from seq 1")
        .into_iter()
        .map(|row| row.event)
        .collect()
}

fn allocations(log: &[Event]) -> Vec<&Event> {
    log.iter()
        .filter(|e| e.subject.as_str() == "worktree.allocated")
        .collect()
}

fn path_of(fact: &Event) -> String {
    fact.payload()["path"]
        .as_str()
        .unwrap_or_else(|| panic!("`path` is a REQUIRED v1 field: {:#}", fact.payload()))
        .to_string()
}

/// Every line of the adapter's sole-allocator registry ([`REGISTRY_PATH`],
/// JSONL).
///
/// **The path is taken from the CONSTANT, corrected 2026-07-24 (Stage B).** As
/// written, this helper hardcoded `<repo>/.rezidnt/worktrees` and its panic
/// message named the collision it had spotted: the daemon uses that exact path
/// as a worktree DIRECTORY while the adapter used it as its registry FILE, and
/// the two cannot coexist. Stage A (`d56bcc7`) resolved that collision the only
/// way that preserved the shipped on-disk layout — by moving `REGISTRY_PATH` to
/// `.rezidnt/registry.jsonl` — which left this literal naming a directory. The
/// criterion is untouched (every allocated path is claimed in the sole-allocator
/// registry); only the file it reads is corrected, and reading it from the
/// constant is strictly stronger than re-spelling it, since a future move
/// cannot silently desynchronize the two again.
fn registry_entries(repo: &Path) -> Vec<Value> {
    let file = repo.join(rezidnt_adapter_git::REGISTRY_PATH);
    let content = std::fs::read_to_string(&file).unwrap_or_else(|e| {
        panic!(
            "the sole-allocator registry must exist at {} after an allocation: {e}. \
             That file is written by `GitAdapter` and by nothing else, so its absence means \
             the allocation never went through `RepoSubstrate` (DR-046 §Decision 8)",
            file.display()
        )
    });
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("registry line is JSON"))
        .collect()
}

/// CRITERION C4 — EXACTLY ONE `worktree.allocated` per allocation, counted on
/// the replayed log.
///
/// DR-046 §Decision 8 names this the non-negotiable condition of the repoint:
/// the adapter emits `worktree.allocated` and so does the daemon, so a repoint
/// that silences neither emits two facts for one tree. Every downstream fold
/// assumes one — `WorktreeState` would fold the allocation twice, and any count
/// derived from the log doubles.
///
/// The count is SELF-CALIBRATING: it compares the number of allocation facts to
/// the number of DISTINCT allocated paths, so the test never has to know how
/// many agents the spec declared. That also makes it immune to the one cheat
/// available — an implementation that emitted one fact per allocation but
/// allocated twice would show two distinct paths and would be caught by the
/// on-disk leg instead.
#[test]
fn exactly_one_worktree_allocated_fact_per_allocated_path() {
    let (_project, _repo, log) = open_and_cold_read();
    let facts = allocations(&log);

    assert!(
        !facts.is_empty(),
        "precondition: the open allocated at least one worktree; saw subjects {:?}",
        log.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>()
    );

    let paths: Vec<String> = facts.iter().map(|f| path_of(f)).collect();
    let distinct: BTreeSet<&String> = paths.iter().collect();
    assert_eq!(
        facts.len(),
        distinct.len(),
        "EXACTLY ONE `worktree.allocated` per allocation (DR-046 §Decision 8). Two facts for \
         one path is the double-emit the repoint must not introduce: the git adapter emits its \
         own allocated fact and `bins/rezidentd/src/runs.rs` publishes one too, so routing the \
         allocation through the adapter WITHOUT silencing the daemon-side emitter lands both. \
         Every fold that counts worktrees doubles, and `WorktreeState` folds one allocation \
         twice. Paths seen: {paths:?}"
    );

    for path in &paths {
        assert!(
            Path::new(path).exists(),
            "the allocated path exists on disk at emission time (ontology \
             `worktree.allocated` v1): {path}"
        );
    }
}

/// CRITERION C2 — the allocation fact is APPENDED with the envelope the daemon
/// supplies: `Some(workspace)`, and the vet verdict id as causation.
///
/// The adapter's own `emit` builds `Event::new(.., None /* workspace */, ..,
/// self.correlation, ..)` and hands the result to a broadcast channel. A
/// broadcast is not an append: a fact that reaches only live subscribers is not
/// on the log, cannot be replayed, and cannot be folded (I3). So this test
/// reads the COLD log — if the fact is there at all, it was appended — and then
/// checks that the repoint did not quietly lose the two envelope fields the
/// current daemon-side emitter supplies.
///
/// The causation leg is resolved, not just checked for presence: the id must
/// name an event that is ON THE SAME LOG and is a gate verdict for the run.
/// "Some ULID" would pass while pointing at nothing, which is exactly how a
/// causal chain rots without anyone noticing.
#[test]
fn the_allocation_fact_is_appended_with_its_workspace_and_vet_causation() {
    let (_project, _repo, log) = open_and_cold_read();
    let facts = allocations(&log);
    assert!(!facts.is_empty(), "precondition: at least one allocation");

    let opened = log
        .iter()
        .find(|e| e.subject.as_str() == "workspace.opened")
        .expect("the open published `workspace.opened`");
    let workspace = opened
        .workspace
        .expect("`workspace.opened` carries the workspace id in its envelope");

    for fact in &facts {
        assert_eq!(
            fact.workspace,
            Some(workspace),
            "the allocation fact carries `Some(workspace)` — the adapter's `emit` passes `None` \
             today, and a workspace-less allocation folds into no workspace's graph (I3, \
             DR-046 §Decision 8): {fact:#?}"
        );

        let causation = fact.causation.unwrap_or_else(|| {
            panic!(
                "the allocation fact carries a causation — on a vet-gated agent it is the vet \
                 VERDICT id, so \"this tree was allocated BECAUSE vet passed\" is answerable \
                 from the log alone (I3/I6): {fact:#?}"
            )
        });
        let cause = log.iter().find(|e| e.id == causation).unwrap_or_else(|| {
            panic!(
                "the causation id must name an event ON THIS LOG — a dangling causation is a \
                 broken causal chain that no reader can follow: {causation}"
            )
        });
        assert!(
            cause.subject.as_str().starts_with("gate."),
            "and on a vet-gated spawn that event is the gate VERDICT that admitted the spawn, \
             not an unrelated fact: causation names {:?}",
            cause.subject.as_str()
        );
    }
}

/// CRITERION C3 (the convergence itself) — every allocated path is backed by an
/// entry in the ADAPTER's sole-allocator registry.
///
/// This is the test that cannot be satisfied by copying values around. The
/// registry file (`REGISTRY_PATH`) is written by
/// `GitAdapter` and by nothing else in the tree; the daemon's private
/// `allocate_worktree` shells out to `git worktree add` and writes no registry
/// at all. So an allocated path appearing as a registry entry IS the proof that
/// the ordinary spawn path now allocates through `RepoSubstrate` — the thing
/// DR-046 §Decision 8 requires and §Consequences (2) records as absent ("
/// isolation today holds by ULID uniqueness of the worktree path, not by any
/// registry guard").
///
/// The allocator value is compared across the two records as well: a registry
/// entry and a log fact that disagree about who allocated a tree would make
/// "which lead allocated this worktree" answerable two different ways.
#[test]
fn every_allocated_path_is_claimed_in_the_sole_allocator_registry() {
    let (_project, repo, log) = open_and_cold_read();
    let facts = allocations(&log);
    assert!(!facts.is_empty(), "precondition: at least one allocation");

    let entries = registry_entries(&repo);
    assert!(
        !entries.is_empty(),
        "the sole-allocator registry holds at least one claim after an allocation — an empty \
         registry means the allocation did not go through `RepoSubstrate` (DR-046 §Decision 8)"
    );

    for fact in &facts {
        let path = path_of(fact);
        let canonical = std::fs::canonicalize(&path)
            .unwrap_or_else(|e| panic!("canonicalize allocated path {path}: {e}"));
        let entry = entries
            .iter()
            .find(|e| {
                e["path"]
                    .as_str()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .is_some_and(|p| p == canonical)
            })
            .unwrap_or_else(|| {
                panic!(
                    "allocated path {path} has NO entry in the sole-allocator registry. The \
                     registry file is written by `GitAdapter` and by nothing else, so its \
                     absence means this allocation still came from the daemon's private \
                     git-CLI allocator — the split path DR-046 §Decision 8 exists to close. \
                     Registry holds: {entries:#?}"
                )
            });
        assert_eq!(
            entry["allocator"].as_str(),
            fact.payload()["allocator"].as_str(),
            "the registry entry and the log fact must record the SAME allocating principal, or \
             \"who allocated this worktree\" has two answers: entry {entry:#}, fact {:#}",
            fact.payload()
        );
    }
}
