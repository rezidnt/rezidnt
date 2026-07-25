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
//! A fifth test —
//! `the_merged_diff_is_not_clobbered_by_a_post_merge_watcher_fact` — was added
//! by the same remediation and is likewise a guard rather than an oracle: it
//! pins the golden-path consequence of the watcher outliving the run, which the
//! repoint made reachable and which reproduced under WSL (never under Windows;
//! the backends differ on whether reads are reported). See its own header.
//!
//! ## RED MODE
//!
//! The three ORACLE tests were ASSERT-RED on the pre-repoint tree, for distinct
//! reasons (below). The fourth —
//! `diff_ready_ownership_one_gate_time_fact_per_pre_merge_and_the_rest_are_the_watcher`
//! — is NOT an oracle test and was never red: it is a REMEDIATION guard, added
//! after the repoint was found to have given `diff.ready` a second live emitter
//! with no disclosure anywhere, and it pins the ownership ruling that followed
//! (two emitters, deliberately, with distinct semantics — see the test).
//!
//! The three, as written:
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
use rezidnt_state::fold;
use rezidnt_types::Event;
use serde_json::Value;

const FACT_DEADLINE: Duration = Duration::from_secs(30);

/// Stub-harness inter-message gap for the merge-driving fixture, milliseconds.
///
/// Deliberately larger than the other boards' 50 ms, and the reason is the
/// whole point of the ownership guard: the stub writes its change and the run
/// then lives `2 * gap` before completing. At 50 ms the run is OVER — daemon
/// killed, log cold-read — before the adapter watcher's 250 ms trailing
/// debounce has even elapsed, so the second emitter is invisible and the guard
/// would pass while observing only one of the two emitters it exists to
/// separate. 700 ms leaves ~1.4 s of post-write life against a 250 ms debounce
/// and the S2 criterion's 1 s write-to-fact bound, which is a slice criterion
/// and not CI weather.
const HARNESS_GAP_MS: u64 = 700;

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

/// Drive one gated `open` through `diff.merged`, then keep the daemon ALIVE
/// for [`POST_MERGE_WATCH`] before cold-reading.
///
/// The wait is the whole point and is not padding: the adapter's watcher is
/// debounced 250 ms and nothing releases it, so a fact provoked by the merge
/// lands roughly a quarter-second after `diff.merged`. [`cold_read`] kills the
/// daemon the instant the merge is seen, which is early enough to cut that
/// window off entirely — a board that skipped the wait would be reporting the
/// daemon's death, not the adapter's behavior.
fn open_and_cold_read_after_merge_settles() -> (tempfile::TempDir, Vec<Event>) {
    let mut daemon = common::start_daemon();
    let (project, spec) = make_gated_project(HARNESS_GAP_MS);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let _ = read_until(&mut tail, Duration::from_secs(45), |v: &Value| {
        v["subject"] == "diff.merged"
    });
    std::thread::sleep(POST_MERGE_WATCH);

    let log = cold_read(&mut daemon);
    (project, log)
}

/// How long the daemon outlives `diff.merged` before the log is read.
///
/// 1.5 s against a 250 ms debounce: six windows, so a merge-provoked fact has
/// no timing excuse for being absent.
const POST_MERGE_WATCH: Duration = Duration::from_millis(1500);

/// THE MERGED DIFF SURVIVES THE MERGE — no watcher fact lands after
/// `diff.merged` and overwrites what it recorded.
///
/// ## The defect this pins (reproduced, then fixed)
///
/// `gates::merge_worktree` runs `git add -A` + `git commit` INSIDE the
/// worktree, and the worktree is still watched — `release_worktree` has no
/// production caller, so the watch outlives the run it was started for. Those
/// commands write nothing inside a linked tree (its index and refs live in the
/// private gitdir, its objects in the shared repo); they only READ the tracked
/// files. But the inotify
/// backend arms `IN_OPEN`, so every read arrived as `EventKind::Access(Open)`
/// and the adapter's watcher treated it as a change. 250 ms later the tree —
/// clean, because the commit had just absorbed it — summarized to the bare
/// 26-byte header and appended a fresh `diff.ready` carrying the finished run's
/// correlation. `WorktreeState.last_diff` is last-write-wins, so that
/// header-only summary replaced the merged diff `diff.merged` had set moments
/// before, on a worktree already folded `outcome = "merged"` (one collapsed
/// `status` field at the time; DR-049 §Decision 2 split it).
///
/// Nothing failed. The log stayed append-only and honest; only the DERIVED
/// state ended up asserting a diff that was not what was merged — I3's failure
/// mode exactly, and invisible to every board that stopped reading at
/// `diff.merged`.
///
/// The fix is in the adapter (`is_change_event`): reads no longer wake the
/// debounce loop. This board judges the CONSEQUENCE on the golden path rather
/// than the mechanism, so it stays a valid judge if the mechanism is ever
/// replaced by releasing the watch at merge.
///
/// ## Non-vacuity
///
/// Two legs make the pass mean something. The fixture must have observed the
/// WATCHER as a live emitter before the merge (otherwise a world with no
/// watcher at all passes), and the daemon must have outlived the debounce
/// window ([`POST_MERGE_WATCH`]).
#[test]
fn the_merged_diff_is_not_clobbered_by_a_post_merge_watcher_fact() {
    let (_project, log) = open_and_cold_read_after_merge_settles();

    let merged_at = log
        .iter()
        .position(|e| e.subject.as_str() == "diff.merged")
        .expect("precondition: the gated run merged (the fixture waits for `diff.merged`)");
    let merged = &log[merged_at];
    let worktree = merged.payload()["worktree"]
        .as_str()
        .expect("`diff.merged` names the worktree it closed")
        .to_string();
    let merged_hash = merged.payload()["diff"]["hash"]
        .as_str()
        .expect("`diff.merged` carries the merged summary as a CAS ref (I2)")
        .to_string();

    // NON-VACUITY: the watcher was live on this log before the merge. Without
    // this, a run whose watcher never fired would satisfy everything below.
    let watcher_before = log[..merged_at]
        .iter()
        .filter(|e| {
            e.subject.as_str() == "diff.ready"
                && e.source.as_str() == rezidnt_adapter_git::SOURCE_ID
        })
        .count();
    assert!(
        watcher_before > 0,
        "precondition: the adapter's watcher appended at least one `diff.ready` BEFORE the merge, \
         so this board is observing a live watcher rather than the absence of one. Zero means \
         either the watch never started or the fixture stopped outliving the 250 ms debounce \
         (see `HARNESS_GAP_MS`), and the assertions below would then be vacuous"
    );

    // The regression, stated on the log: nothing about this worktree lands
    // after the merge closed it.
    let after: Vec<&Event> = log[merged_at + 1..]
        .iter()
        .filter(|e| {
            e.subject.as_str() == "diff.ready"
                && e.payload()["worktree"].as_str() == Some(&worktree)
        })
        .collect();
    assert!(
        after.is_empty(),
        "a `diff.ready` for {worktree} landed AFTER `diff.merged` closed it. The merge commits \
         inside the still-watched tree, so a fact here means the merge's own filesystem activity \
         woke the watcher: the post-commit tree is clean, its summary is the bare header, and it \
         overwrites the merged diff in derived state. Facts: {:#?}",
        after
            .iter()
            .map(|e| e.payload().clone())
            .collect::<Vec<_>>()
    );

    // And the consequence, stated on the FOLD — the thing a client actually
    // reads. Asserted independently of the leg above so that a future
    // post-merge fact which is genuinely new information (a real write into the
    // tree after the merge) is judged on whether it corrupts the merge record,
    // not merely on existing.
    let graph = fold(log.iter());
    let state = graph
        .worktrees
        .get(&worktree)
        .unwrap_or_else(|| panic!("the fold holds a worktree entry for {worktree}"));
    assert_eq!(
        state.outcome.as_deref(),
        Some("merged"),
        "the worktree stays folded `outcome = merged` — the merge is what the run came to"
    );
    assert_eq!(
        state.lifecycle, "released",
        "and DR-049 §Decision 1 releases a merged tree at merge, so its lifecycle has moved on \
         while the merged outcome above SURVIVES. Before the DR-049 split these were one \
         `status` field and the release would have erased `\"merged\"` — the derived-state \
         regression that made DR-047 §Decision 5 refuse to release here at all"
    );
    assert_eq!(
        state.last_diff.as_deref(),
        Some(merged_hash.as_str()),
        "derived state records the diff that was MERGED. Any other value means the log's last \
         word about this tree is not the merge — and since the log is truth (I3), a `last_diff` \
         that disagrees with `diff.merged` is derived state asserting a merge that did not \
         happen. Folded: {:?}",
        state.last_diff
    );
}

/// Drive one gated `open` all the way through `pre_merge` to `diff.merged`,
/// then cold-read the persisted log.
///
/// Distinct from [`open_and_cold_read`], which stops at `agent.completed`:
/// `pre_merge` runs AFTER completion, so a board that asserts anything about
/// gate-time facts must wait for the merge or it is asserting on a log that may
/// simply not have reached them yet.
fn open_and_cold_read_after_merge() -> (tempfile::TempDir, Vec<Event>) {
    let mut daemon = common::start_daemon();
    let (project, spec) = make_gated_project(HARNESS_GAP_MS);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    // The merge is the far end of the chain (spawn → complete → pre_merge →
    // merge), so it gets `golden_path.rs`'s 45 s tolerance rather than the
    // single-fact deadline above.
    let _ = read_until(&mut tail, Duration::from_secs(45), |v: &Value| {
        v["subject"] == "diff.merged"
    });

    let log = cold_read(&mut daemon);
    (project, log)
}

/// OWNERSHIP OF `diff.ready` — counted on the replayed log.
///
/// The repoint gave this subject a second LIVE emitter and named it nowhere:
/// `alloc_worktree` now starts the adapter's notify watcher on every allocated
/// tree, so its debounced `diff.ready` stream reaches this daemon's log
/// alongside the one `run_pre_merge` has always minted at gate time. Two
/// emitters is the right answer here — and precisely the wrong one for
/// `worktree.allocated`, which is why the difference is pinned rather than
/// assumed:
///
/// - `worktree.allocated` is two records of ONE occurrence. Folding it twice
///   doubles every worktree count, so one emitter was silenced (C4 above).
/// - `diff.ready` is two DIFFERENT observations. The watcher's is continuous,
///   debounced 250 ms, deduplicated against the previous summary, and minted by
///   a detached task nothing waits on; the daemon's is one deterministic
///   gate-time pin of the exact ref `pre_merge` verifies (I6 — a gate's inputs
///   are fixed at gate time, never inherited from a race). `WorktreeState`
///   folds `last_diff` last-write-wins and nothing counts them.
///
/// So what must hold is not "exactly one" but OWNERSHIP: exactly one gate-time
/// fact per `pre_merge`, ordered before the gate it feeds, and every other
/// `diff.ready` on the log accounted for by the watcher. A third emitter, or a
/// second gate-time fact per gate, fails here.
#[test]
fn diff_ready_ownership_one_gate_time_fact_per_pre_merge_and_the_rest_are_the_watcher() {
    let (_project, log) = open_and_cold_read_after_merge();

    const GATE_TIME_SOURCE: &str = "rezidnt-adapter-git";
    let watcher_source = rezidnt_adapter_git::SOURCE_ID;

    let ready: Vec<(usize, &Event)> = log
        .iter()
        .enumerate()
        .filter(|(_, e)| e.subject.as_str() == "diff.ready")
        .collect();
    assert!(
        !ready.is_empty(),
        "precondition: the gated run reached `pre_merge`, which mints one; saw subjects {:?}",
        log.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>()
    );

    // No third emitter: every `diff.ready` on the log is one of the two.
    let sources: BTreeSet<&str> = ready.iter().map(|(_, e)| e.source.as_str()).collect();
    assert!(
        sources
            .iter()
            .all(|s| *s == GATE_TIME_SOURCE || *s == watcher_source),
        "every `diff.ready` comes from one of the two disclosed emitters — the gate-time pin \
         ({GATE_TIME_SOURCE}) or the adapter's watcher ({watcher_source}). A third source means \
         a third emitter arrived the way the second one did: silently. Saw: {sources:?}"
    );

    // NON-VACUITY, and the empirical basis of the whole ruling: the watcher IS
    // one of the live emitters on the daemon's log. Without this leg the
    // assertions above are satisfiable by a one-emitter world — which is
    // exactly what this board saw at the other fixtures' 50 ms harness gap,
    // where the run ends before the 250 ms debounce and the watcher's fact
    // never exists (see `HARNESS_GAP_MS`). "Two emitters" would then be an
    // inspection claim about `alloc_worktree` rather than an observed fact.
    assert!(
        sources.contains(watcher_source),
        "the adapter's notify watcher appends `diff.ready` to the DAEMON's log — the repoint \
         put it on the golden path for the first time by starting a watch inside \
         `alloc_worktree`, and this is the leg that observes it rather than inferring it. \
         Absent, either the watcher stopped reaching the sink (its facts would be riding a \
         broadcast into nowhere — I3) or the fixture stopped outliving the debounce. \
         Sources seen: {sources:?}"
    );

    // Exactly one gate-time fact per pre_merge gate.
    let pre_merge_gates = log
        .iter()
        .filter(|e| e.subject.as_str() == "gate.entered" && e.payload()["gate"] == "pre_merge")
        .count();
    assert!(pre_merge_gates > 0, "precondition: a pre_merge gate ran");
    let gate_time: Vec<&(usize, &Event)> = ready
        .iter()
        .filter(|(_, e)| e.source.as_str() == GATE_TIME_SOURCE)
        .collect();
    assert_eq!(
        gate_time.len(),
        pre_merge_gates,
        "ONE gate-time `diff.ready` per `pre_merge`, no more and no fewer. More means the \
         daemon acquired a second emit site; fewer means the gate is verifying a diff whose \
         fact never landed, and `debrief` replays a gate over a ref the log cannot show. \
         Gates: {pre_merge_gates}, gate-time facts: {}",
        gate_time.len()
    );

    // And it precedes the gate it feeds — the golden path's causal order.
    let first_pre_merge = log
        .iter()
        .position(|e| e.subject.as_str() == "gate.entered" && e.payload()["gate"] == "pre_merge")
        .expect("a pre_merge gate.entered is on the log");
    assert!(
        gate_time[0].0 < first_pre_merge,
        "the gate-time fact is appended BEFORE the gate that verifies it — `pre_merge` \
         verifies the CAS-pinned diff, so a fact landing after the gate would be describing \
         something the gate did not read (`golden_path.rs` pins the same order on the stream)"
    );

    // Both emitters honor the ratified v1 payload: the summary is a REF (I2).
    for (_, fact) in &ready {
        assert_eq!(fact.v, 1, "taxonomy v0 mints `diff.ready` at v = 1");
        assert!(
            fact.payload()["worktree"].is_string(),
            "`worktree` is a REQUIRED v1 field: {:#}",
            fact.payload()
        );
        assert!(
            fact.payload()["diff"]["hash"].is_string(),
            "the summary rides as a CAS ref, never inline diff bytes (I2): {:#}",
            fact.payload()
        );
    }
}
