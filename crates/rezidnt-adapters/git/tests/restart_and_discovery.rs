//! S2 remediation oracle — written against the auditor's FAIL verdict of
//! 2026-07-17, BEFORE the fix. Two findings are encoded:
//!
//! - **Blocker — exactly-once is process-lifetime only:** the observed /
//!   conflicted dedup marks live only in memory and are rebuilt EMPTY at
//!   [`GitAdapter::open`], while the JSONL registry reloads from disk. The
//!   ratified ontology obligation ("exactly one `worktree.conflict` per
//!   collision") and the observed-tree silence rule are FOREVER guarantees,
//!   not process-lifetime ones — so restart must not resurface facts, and a
//!   pre-restart allocation must remain releasable.
//! - **Major — `observe` has no production caller:** nothing discovers
//!   out-of-band trees. The decided remediation is an on-open reconciliation
//!   scan: `open` reconciles the registry against reality and routes
//!   unregistered out-of-band worktrees through the same dedup path as
//!   `observe`. Scan-at-open is a synchronous, deterministic moment; its
//!   facts predate any possible subscriber, so they are pinned through the
//!   `GitAdapter::startup_facts` seam (todo!()-stubbed until the implementer
//!   builds it — the same red-stub pattern the original S2 oracle used).
//!
//! Restart is simulated by dropping the adapter and calling `open` again over
//! the same repo + CAS roots: exactly what a daemon restart does (in-memory
//! state gone, disk state reloaded). No sleeps for discovery — the debounce /
//! timing patterns belong to the diff_ready tests only.
//!
//! Deliberately UNPINNED (implementer latitude, noted in the work order):
//! how allocation identity and the dedup marks persist (additive fields on
//! the DEFAULT-format registry are the obvious route — no /subject needed, no
//! payload changes); the `causation` of a post-restart `worktree.released`
//! (the allocated event id may or may not be recoverable across restart —
//! DEFAULT chain, not re-ratified here); the mechanism by which the scan
//! tells "rezidnt's own intact tree" from "a human tree occupying a
//! rezidnt-registered path" (the fixtures make the registered branch vs.
//! actual-checkout mismatch available as a sufficient discriminator).

mod util;

use std::time::Duration;

use rezidnt_adapter_git::{GitAdapter, RepoSubstrate, SOURCE_ID, WorktreeReq};

const OUTER: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_secs(1);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
    }
}

/// Blocker consequence 1: an already-observed human tree stays silent across
/// restart. The registry entry (allocator "human") survives on disk; the
/// in-memory observed mark must survive with it — today it does not, and
/// re-observation after restart resurfaces the tree as a spurious
/// `worktree.conflict` with holder "human".
#[tokio::test]
async fn restart_then_reobserving_known_human_tree_emits_zero_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);
    let human = tmp.path().join("human-wt");

    {
        // First daemon lifetime: the human tree is observed and registered.
        let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
        let mut rx = adapter.subscribe();
        util::git(
            &repo,
            &["worktree", "add", "--detach", human.to_str().unwrap()],
        );
        adapter.observe(&human).await.unwrap();
        util::recv_subject(&mut rx, "worktree.observed", OUTER).await;
    } // adapter dropped: daemon stops

    // Restart. The tree is a KNOWN human tree (registry says so); nothing
    // about it is news.
    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let mut rx = adapter.subscribe();
    adapter.observe(&human).await.unwrap();

    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        0,
        "a known human tree re-observed after restart is NOT a collision — the \
         spurious conflict (holder \"human\") is the auditor's blocker — got {events:#?}"
    );
    assert_eq!(
        util::count_subject(&events, "worktree.observed"),
        0,
        "one observation fact per tree, FOREVER — not per process lifetime"
    );
    assert!(
        events.is_empty(),
        "restart + re-observation of a known tree emits ZERO new facts — got {events:#?}"
    );
    assert_eq!(
        util::registry_entries_for(&repo, &human).len(),
        1,
        "the surviving registry entry stays single — never double-tracked"
    );

    // The open-time reconciliation scan must reach the same verdict: a
    // registered, already-observed tree is not news at startup either.
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        0,
        "the startup scan must not resurface a known human tree as a conflict"
    );
    assert_eq!(
        util::count_subject(&startup, "worktree.observed"),
        0,
        "the startup scan must not re-announce an already-observed tree"
    );
}

/// Blocker consequence 2: THE ratified obligation — exactly one
/// `worktree.conflict` per collision, forever, ACROSS restarts. A collision
/// surfaced pre-restart must not be surfaced again by a later process
/// lifetime (today the conflicted mark dies with the process and the same
/// collision re-emits).
#[tokio::test]
async fn restart_then_reobserving_surfaced_collision_emits_no_second_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let wt_path;
    {
        // First lifetime: allocate, then surface the deliberate out-of-band
        // collision exactly once (this half already passes — it is the S2
        // exit criterion).
        let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
        let mut rx = adapter.subscribe();
        let wt = adapter
            .alloc_worktree(branch_req("feat-again", "feat/again"))
            .await
            .unwrap();
        util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
        adapter.observe(&wt.path).await.unwrap();
        let surfaced = util::drain_for(&mut rx, QUIET).await;
        assert_eq!(
            util::count_subject(&surfaced, "worktree.conflict"),
            1,
            "precondition: the collision is surfaced exactly once pre-restart"
        );
        wt_path = wt.path;
    } // daemon stops

    // Restart: same collision observed again. One collision, one fact,
    // FOREVER — the ontology obligation has no process-lifetime footnote.
    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let mut rx = adapter.subscribe();
    adapter.observe(&wt_path).await.unwrap();

    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        0,
        "an already-surfaced collision must emit nothing after restart — got {events:#?}"
    );

    // Nor may the open-time scan re-surface it (rezidnt's own intact tree at
    // its registered path is not a collision at all).
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        0,
        "the startup scan must not re-emit a pre-restart collision, nor treat \
         rezidnt's own registered tree as one"
    );
}

/// Blocker consequence 3 (auditor tracked item 4, same root cause): a
/// rezidnt allocation survives restart as a RELEASABLE worktree. Today the
/// live map is process-local, so `release_worktree` after restart returns
/// UnknownWorktree and the registry entry is permanently stale.
#[tokio::test]
async fn restart_then_release_of_prerestart_allocation_succeeds_exactly_once() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let (wt, canonical);
    {
        let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
        let mut rx = adapter.subscribe();
        wt = adapter
            .alloc_worktree(branch_req("feat-durable", "feat/durable"))
            .await
            .unwrap();
        let allocated = util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
        canonical = allocated.payload()["path"]
            .as_str()
            .expect("v1 payload requires `path`")
            .to_string();
    } // daemon stops; the allocation and its registry entry persist

    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let mut rx = adapter.subscribe();
    adapter.release_worktree(&wt.id).await.expect(
        "a rezidnt allocation must remain releasable after restart — the registry \
         reloads from disk, so allocation identity must reload with it \
         (today: UnknownWorktree, leaving a permanently stale registry entry)",
    );

    let events = util::drain_for(&mut rx, Duration::from_secs(2)).await;
    assert_eq!(
        util::count_subject(&events, "worktree.released"),
        1,
        "exactly one worktree.released per release, restart notwithstanding"
    );
    let released = events
        .iter()
        .find(|e| e.subject.as_str() == "worktree.released")
        .unwrap();
    assert_eq!(
        released.payload()["path"].as_str(),
        Some(canonical.as_str()),
        "released `path` is the same canonicalized registry key the allocation minted"
    );

    assert!(
        util::registry_entries(&repo)
            .iter()
            .all(|e| e["path"].as_str() != Some(canonical.as_str())),
        "the registry entry is closed — no permanent stale entry"
    );
    let listed = util::git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(
        !listed.contains(wt.path.file_name().unwrap().to_str().unwrap()),
        "git no longer tracks the released worktree:\n{listed}"
    );
}

/// Major finding, scenario 4: a human `git worktree add` performed while NO
/// adapter is running is DISCOVERED at the next `open` — without anyone
/// calling `observe`. This is the production caller `observe` never had:
/// exactly one `worktree.observed` (allocator "human"), the tree registered,
/// and the discovery routed through the same dedup path.
#[tokio::test]
async fn open_discovers_out_of_band_human_tree_without_observe() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    // Real out-of-band human git activity, with no adapter alive to see it.
    let human = tmp.path().join("human-wt");
    util::git(
        &repo,
        &["worktree", "add", "--detach", human.to_str().unwrap()],
    );

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();

    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.observed"),
        1,
        "the on-open scan discovers the unregistered out-of-band tree and \
         emits exactly one worktree.observed — got {startup:#?}"
    );
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        0,
        "no conflict without a prior registered claim"
    );
    let observed = startup
        .iter()
        .find(|e| e.subject.as_str() == "worktree.observed")
        .unwrap();
    assert_eq!(
        observed.v, 1,
        "taxonomy v0 mints worktree.observed at v = 1"
    );
    assert_eq!(observed.source.as_str(), SOURCE_ID);
    assert_eq!(
        util::canon(std::path::Path::new(
            observed.payload()["path"]
                .as_str()
                .expect("v1 payload requires `path`")
        )),
        util::canon(&human)
    );
    assert_eq!(
        observed.payload()["allocator"].as_str(),
        Some("human"),
        "v1: allocator is fixed \"human\" — observation is by definition out-of-band"
    );
    assert!(
        observed.payload()["branch"].is_null(),
        "v1: `branch` is absent for a detached human tree"
    );

    // Discovery registered the tree under its canonical key…
    let entries = util::registry_entries_for(&repo, &human);
    assert_eq!(
        entries.len(),
        1,
        "discovery registers the tree exactly once"
    );
    assert_eq!(entries[0]["allocator"].as_str(), Some("human"));

    // …through the same dedup path as observe: explicit re-observation of a
    // discovered tree is not news.
    let mut rx = adapter.subscribe();
    adapter.observe(&human).await.unwrap();
    let events = util::drain_for(&mut rx, QUIET).await;
    assert!(
        events.is_empty(),
        "a tree the startup scan discovered is already known — observe emits \
         nothing further, proving both share one dedup path — got {events:#?}"
    );
}

/// Major finding, scenario 5: an out-of-band collision that HAPPENS while the
/// daemon is down — a human tree now occupies a path the registry holds for
/// rezidnt — is surfaced at startup as exactly one `worktree.conflict`, and a
/// second open cycle (restart) emits zero more. Exactly one per collision,
/// forever, across restarts.
#[tokio::test]
async fn open_surfaces_startup_collision_exactly_once_across_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let wt_path;
    {
        let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
        let mut rx = adapter.subscribe();
        let wt = adapter
            .alloc_worktree(branch_req("feat-taken", "feat/taken"))
            .await
            .unwrap();
        util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
        wt_path = wt.path;
    } // daemon stops; registry still holds the path for rezidnt (branch feat/taken)

    // The human takeover, out-of-band: rezidnt's tree is removed and a
    // DIFFERENT (detached) human tree is created at the very same path. The
    // plain spelling canonicalizes to the registered key.
    let plain = tmp.path().join(wt_path.file_name().unwrap());
    util::git(
        &repo,
        &["worktree", "remove", "--force", plain.to_str().unwrap()],
    );
    util::git(
        &repo,
        &["worktree", "add", "--detach", plain.to_str().unwrap()],
    );
    assert_eq!(
        util::canon(&plain),
        util::canon(&wt_path),
        "test setup: same key"
    );

    // Restart 1: the startup scan discovers the collision.
    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        1,
        "a human tree occupying a rezidnt-registered path is a collision the \
         startup scan surfaces exactly once — got {startup:#?}"
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
        "conflict is emitted INSTEAD of double-tracking — no observed for a registered path"
    );
    assert_eq!(
        util::registry_entries_for(&repo, &plain).len(),
        1,
        "the registry still holds exactly one entry for the contested path"
    );
    drop(adapter);

    // Restart 2: the same collision is old news. Zero further conflicts —
    // from the scan and from explicit re-observation alike.
    let adapter = GitAdapter::open(&repo, &cas).await.unwrap();
    let startup = adapter.startup_facts();
    assert_eq!(
        util::count_subject(&startup, "worktree.conflict"),
        0,
        "a second open cycle must not re-surface an already-surfaced collision"
    );
    let mut rx = adapter.subscribe();
    adapter.observe(&plain).await.unwrap();
    let events = util::drain_for(&mut rx, QUIET).await;
    assert_eq!(
        util::count_subject(&events, "worktree.conflict"),
        0,
        "one collision, one fact, forever — across restarts"
    );
}
