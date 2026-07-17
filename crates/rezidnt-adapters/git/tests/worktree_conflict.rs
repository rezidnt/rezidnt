//! S2 oracle — exit criterion 2: a deliberate out-of-band worktree collision
//! emits EXACTLY ONE `worktree.conflict` (DR-001: conflict is emitted instead
//! of silently double-tracking; rezidnt is the sole allocator and
//! observed/conflict exist only as out-of-band human-git-activity guards).
//!
//! Determinism note: the collision is injected through `GitAdapter::observe`
//! — the discovery ingest seam — so the exactly-once assertion never races
//! platform filesystem events. `observe`'s production caller is the on-open
//! reconciliation scan, pinned by `restart_and_discovery.rs` (S2 remediation);
//! the notify wiring feeds only the debounce→`diff.ready` path, exercised by
//! the diff_ready timing tests. These tests pin single-process semantics;
//! restart durability of the exactly-once marks is restart_and_discovery's.
//!
//! Payload-shape caveat (flagged in the oracle work order): the ontology
//! ratifies no v1 payload baseline for `worktree.observed`/`worktree.conflict`
//! — these tests pin only the semantically forced minimum (`path`; observed
//! `allocator: "human"`). Warden ratification via /subject is required before
//! the implementer freezes a richer shape.

mod util;

use std::time::Duration;

use rezidnt_adapter_git::{GitAdapter, RepoSubstrate, WorktreeReq};

const OUTER: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_secs(1);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
    }
}

/// THE criterion: a second claim on an already-registered canonicalized path
/// emits exactly one `worktree.conflict` — and re-observing the same
/// collision emits nothing further (exactly one means one, not one-per-scan).
#[tokio::test]
async fn out_of_band_second_claim_emits_exactly_one_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-a", "feat/a"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
    let canonical = util::canon(&wt.path);

    // The deliberate out-of-band collision: the watcher reports a claim on a
    // path rezidnt already registered.
    adapter.observe(&wt.path).await.unwrap();

    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        1,
        "S2 exit criterion: EXACTLY ONE worktree.conflict per collision — got {events:#?}"
    );
    let conflict = events
        .iter()
        .find(|e| e.subject.as_str() == "worktree.conflict")
        .unwrap();
    assert_eq!(conflict.v, 1);
    let contested = conflict.payload()["path"]
        .as_str()
        .expect("conflict payload names the contested `path`");
    assert_eq!(
        util::canon(std::path::Path::new(contested)),
        canonical,
        "conflict names the contested canonicalized path"
    );
    assert_eq!(
        util::count_subject(&events, "worktree.observed"),
        0,
        "conflict is emitted INSTEAD of double-tracking — no worktree.observed for a registered path"
    );

    // Redelivery: the same collision observed again (watcher rescan) stays
    // silent. One collision, one fact, forever.
    adapter.observe(&wt.path).await.unwrap();
    let more = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&more, "worktree.conflict"),
        0,
        "re-observation of an already-emitted collision must not emit again"
    );

    // No silent double-tracking in the registry either.
    assert_eq!(
        util::registry_entries_for(&repo, &wt.path).len(),
        1,
        "the registry still holds exactly one entry for the contested path"
    );
}

/// The registry keys CANONICALIZED paths (DR-001 BINDING): a differently
/// spelled claim on the same tree (`<wt>/../<name>` traversal) still collides.
#[tokio::test]
async fn non_canonical_spelling_of_registered_path_still_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-b", "feat/b"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    let respelled = wt
        .path
        .parent()
        .unwrap()
        .join("..")
        .join(wt.path.parent().unwrap().file_name().unwrap())
        .join(wt.path.file_name().unwrap());
    assert_eq!(
        util::canon(&respelled),
        util::canon(&wt.path),
        "test setup: same tree"
    );

    adapter.observe(&respelled).await.unwrap();
    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        1,
        "canonicalization is the registry key — a respelled claim on the same tree collides"
    );
}

/// The non-collision guard path: a human `git worktree add` at a FRESH path
/// is observed (allocator "human"), never conflated with a conflict.
#[tokio::test]
async fn unregistered_out_of_band_tree_emits_observed_not_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();

    // Real out-of-band human git activity, via the actual git CLI.
    let human = tmp.path().join("human-wt");
    util::git(
        &repo,
        &["worktree", "add", "--detach", human.to_str().unwrap()],
    );
    adapter.observe(&human).await.unwrap();

    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.observed"),
        1,
        "an unregistered out-of-band tree is observed exactly once"
    );
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        0,
        "no conflict without a prior registered claim"
    );
    let observed = events
        .iter()
        .find(|e| e.subject.as_str() == "worktree.observed")
        .unwrap();
    assert_eq!(
        util::canon(std::path::Path::new(
            observed.payload()["path"]
                .as_str()
                .expect("observed payload carries `path`")
        )),
        util::canon(&human)
    );
    assert_eq!(
        observed.payload()["allocator"].as_str(),
        Some("human"),
        "out-of-band observation records the human allocator (DR-001; \"human\" is reserved for exactly this)"
    );
}
