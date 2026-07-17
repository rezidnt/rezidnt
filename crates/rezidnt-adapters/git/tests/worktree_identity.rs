//! Pre-S4 remediation oracle — written against the auditor's S2-T3 finding
//! (S2 re-debrief: major, now due) BEFORE the fix. The on-open reconciliation
//! scan discriminates occupancy by BRANCH, and branch is the wrong identity:
//!
//! - **False positive:** an occupant legitimately switching HEAD inside a
//!   rezidnt-registered worktree reads as a takeover — spurious
//!   `worktree.conflict` plus an allocation that is never rebuilt live, so
//!   the tree becomes an unreleasable orphan.
//! - **False negative:** a human removing the registered worktree
//!   out-of-band and re-adding a FOREIGN one at the same path on the same
//!   branch is invisible — the scan sees branch == registered branch and
//!   adopts the impostor as rezidnt's own intact tree.
//!
//! These tests pin the STRENGTHENED identity behavior and are red against
//! today's branch discriminator. They pin behavior only, never mechanism.
//!
//! Identity-probe note for the implementer (design latitude, oracle
//! recommendation): the registry already persists the allocation's
//! `WorktreeId`; the honest discriminator is a marker carrying that id in
//! the worktree's PRIVATE gitdir (`git rev-parse --git-dir` inside the tree
//! resolves it — `<repo>/.git/worktrees/<name>/` for a linked tree), written
//! at `alloc_worktree`. That location survives every in-tree operation
//! (checkout/switch/commit), never pollutes the working tree or its diffs,
//! and is destroyed by `git worktree remove` — so a foreign re-add at the
//! same path has no marker (or a mismatched id) and is detected, while an
//! occupant branch switch keeps the marker intact. A marker file in the
//! working tree itself would ride diffs and could be committed by the
//! occupant; HEAD-oid comparison breaks the moment the occupant commits.
//! Existing green pins are unaffected: the detached foreign re-add in
//! `restart_and_discovery.rs` carries no marker and still conflicts.
//!
//! Deliberately UNPINNED: whether the registry entry's `branch` field is
//! refreshed after a legitimate in-place switch (bookkeeping latitude), and
//! the exact marker location/content — only the verdicts above are pinned.

mod util;

use std::time::Duration;

use rezidnt_adapter_git::{GitAdapter, RepoSubstrate, WorktreeReq};

const QUIET: Duration = Duration::from_secs(1);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
    }
}

/// False positive: the occupant of a rezidnt-allocated worktree checks out a
/// different branch IN PLACE — normal agent behavior, not a takeover. The
/// next open must not surface a `worktree.conflict`, and the allocation must
/// be rebuilt live (releasable under its persisted id), not orphaned.
#[tokio::test]
async fn occupant_branch_switch_in_registered_worktree_is_not_a_takeover() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let wt;
    {
        let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
        wt = adapter
            .alloc_worktree(branch_req("feat-switch", "feat/switch"))
            .await
            .unwrap();
    } // daemon stops; registry holds the path for rezidnt on feat/switch

    // The occupant legitimately moves HEAD inside ITS OWN tree while the
    // daemon is down. Same tree, same identity — only the branch changed.
    util::git(&wt.path, &["switch", "-c", "occupant/side-quest"]);

    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        0,
        "an occupant switching HEAD in a rezidnt-registered worktree is NOT a \
         takeover — the spurious conflict is the S2-T3 false positive (branch \
         is not identity) — got {startup:#?}"
    );
    assert_eq!(
        util::count_subject(&startup, "worktree.observed"),
        0,
        "rezidnt's own tree is never re-announced"
    );

    // And the allocation is rezidnt's, rebuilt live: releasable, not an
    // unreleasable orphan.
    let mut rx = adapter.subscribe();
    adapter.release_worktree(&wt.id).await.expect(
        "the allocation is still rezidnt's own tree and must be rebuilt live \
         across restart — an UnknownWorktree here is the unreleasable-orphan \
         half of the S2-T3 false positive",
    );
    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.released"),
        1,
        "exactly one worktree.released for the release"
    );
    assert!(
        util::registry_entries(&repo).iter().all(|e| {
            e["path"]
                .as_str()
                .is_none_or(|p| std::fs::canonicalize(p).ok() != Some(util::canon(&wt.path)))
        }),
        "the registry entry is closed after release"
    );
}

/// False negative: while the daemon is down, a human removes the registered
/// worktree and re-adds a FOREIGN one at the same path ON THE REGISTERED
/// BRANCH. Branch-as-identity adopts the impostor silently; real identity
/// detects it — exactly one `worktree.conflict`, and (forever rule) zero
/// more on the next restart.
#[tokio::test]
async fn foreign_readd_on_registered_branch_is_detected_exactly_once() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let wt_path;
    {
        let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
        let wt = adapter
            .alloc_worktree(branch_req("feat-worn", "feat/worn"))
            .await
            .unwrap();
        wt_path = wt.path;
    } // daemon stops; registry holds the path for rezidnt on feat/worn

    // The takeover, wearing the registered identity: remove rezidnt's tree,
    // re-add a foreign one at the SAME path checking out the SAME branch.
    let plain = tmp.path().join(wt_path.file_name().unwrap());
    util::git(
        &repo,
        &["worktree", "remove", "--force", plain.to_str().unwrap()],
    );
    util::git(
        &repo,
        &["worktree", "add", plain.to_str().unwrap(), "feat/worn"],
    );
    assert_eq!(
        util::canon(&plain),
        util::canon(&wt_path),
        "test setup: same registry key"
    );

    // Restart 1: the impostor is on the registered path and branch — only a
    // real identity probe can tell it is not rezidnt's tree.
    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        1,
        "a foreign tree re-added at the registered path on the registered \
         branch is a takeover the scan must surface exactly once — branch \
         match making it invisible is the S2-T3 false negative — got {startup:#?}"
    );
    let conflict = startup
        .iter()
        .find(|e| e.subject.as_str() == "worktree.conflict")
        .unwrap();
    assert_eq!(conflict.v, 1);
    assert_eq!(
        util::canon(std::path::Path::new(
            conflict.payload()["path"]
                .as_str()
                .expect("conflict payload names the contested `path`")
        )),
        util::canon(&wt_path),
        "conflict names the contested canonicalized registry key"
    );
    if let Some(holder) = conflict.payload()["holder"].as_str() {
        assert_eq!(
            holder, "rezidnt",
            "holder is the allocator on the STANDING registry entry (v1)"
        );
    }
    assert_eq!(
        util::count_subject(&startup, "worktree.observed"),
        0,
        "conflict is emitted INSTEAD of double-tracking — no observed for a \
         registered path"
    );
    assert_eq!(
        util::registry_entries_for(&repo, &plain).len(),
        1,
        "the registry still holds exactly one entry for the contested path"
    );
    drop(adapter);

    // Restart 2: one collision, one fact, forever — the exactly-once rule
    // the S2 remediation already established carries over to identity-based
    // detection.
    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        0,
        "a second open cycle must not re-surface the surfaced takeover"
    );
    let mut rx = adapter.subscribe();
    adapter.observe(&plain).await.unwrap();
    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        0,
        "explicit re-observation of the surfaced takeover is old news too"
    );
}
