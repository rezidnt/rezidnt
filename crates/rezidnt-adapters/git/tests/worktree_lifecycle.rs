//! S2 oracle — worktree lifecycle (allocate → use → release) against a REAL
//! committed repo, including detach behavior (S2 planning triage), plus the
//! `worktree.allocated` v1 payload contract (ratified in spec/ontology.md —
//! matched exactly) and the sole-allocator `.rezidnt/worktrees` registry.
//!
//! Every test fails `todo!()`-unimplemented until the implementer builds the
//! adapter; the assertions are the work order.

mod util;

use std::path::PathBuf;
use std::time::Duration;

use rezidnt_adapter_git::{GitAdapter, RepoSubstrate, SOURCE_ID, WorktreeReq};

const OUTER: Duration = Duration::from_secs(5);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
    }
}

/// `worktree.allocated` v1, ratified (spec/ontology.md payload baselines):
/// `path` canonicalized and existing on disk at emission; `branch` when
/// requested; `allocator` is `"rezidnt"` — the value `"human"` is NEVER
/// emitted on this subject (DR-001 sole-allocator model). Envelope: `v = 1`,
/// `source` owned by the git adapter.
#[tokio::test]
async fn alloc_emits_allocated_fact_matching_ontology_v1() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-s2", "feat/s2"))
        .await
        .unwrap();

    let ev = util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
    assert_eq!(ev.v, 1, "taxonomy v0 mints worktree.allocated at v = 1");
    assert_eq!(
        ev.source.as_str(),
        SOURCE_ID,
        "allocation is owned by the git adapter (RepoSubstrate) — ontology emitter column"
    );

    let payload = ev.payload();
    let path = PathBuf::from(
        payload["path"]
            .as_str()
            .expect("v1 payload requires `path`"),
    );
    assert!(
        path.exists(),
        "v1: `path` exists on disk at emission time — got {}",
        path.display()
    );
    assert_eq!(
        util::canon(&path),
        util::canon(&wt.path),
        "v1: `path` is the canonicalized worktree path"
    );
    assert_eq!(
        payload["branch"].as_str(),
        Some("feat/s2"),
        "v1: `branch` carries the requested branch"
    );
    assert_eq!(
        payload["allocator"].as_str(),
        Some("rezidnt"),
        "v1: allocator is \"rezidnt\"; \"human\" is reserved for out-of-band observation and never emitted here (DR-001)"
    );
}

/// Sole-allocator registry (DR-001 BINDING rule): the allocation is
/// registered under its canonicalized path, with the allocator recorded, in
/// the `.rezidnt/worktrees` JSONL registry.
#[tokio::test]
async fn alloc_registers_canonicalized_path_in_dot_rezidnt_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let wt = adapter
        .alloc_worktree(branch_req("feat-reg", "feat/reg"))
        .await
        .unwrap();

    let entries = util::registry_entries_for(&repo, &wt.path);
    assert_eq!(
        entries.len(),
        1,
        "exactly one registry entry per canonicalized path — never double-tracked"
    );
    assert_eq!(
        entries[0]["allocator"].as_str(),
        Some("rezidnt"),
        "registry records the allocator (DR-001)"
    );
}

/// Detach behavior (S2 triage): `detach` yields a worktree at the repo's
/// HEAD commit with a detached HEAD and no branch in the allocated fact.
#[tokio::test]
async fn detach_alloc_creates_detached_head_worktree_at_repo_head() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);
    let repo_head = util::git(&repo, &["rev-parse", "HEAD"]);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(WorktreeReq {
            name: "detached".to_string(),
            branch: None,
            detach: true,
        })
        .await
        .unwrap();

    assert_eq!(
        util::git(&wt.path, &["rev-parse", "HEAD"]),
        repo_head,
        "detached worktree checks out the committed repo's HEAD"
    );
    let (has_symbolic_ref, _, _) = util::git_status(&wt.path, &["symbolic-ref", "-q", "HEAD"]);
    assert!(
        !has_symbolic_ref,
        "HEAD must be detached (no symbolic ref) in a detach allocation"
    );

    let ev = util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
    assert!(
        ev.payload()["branch"].is_null(),
        "v1: `branch` is optional and absent when no branch was requested"
    );
}

/// Release closes the lifecycle: exactly one `worktree.released`, the
/// registry entry closed, and git no longer tracking the worktree. Payload
/// `path` spelling matches the allocated fact (same registry key).
#[tokio::test]
async fn release_emits_released_once_and_closes_registry_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-rel", "feat/rel"))
        .await
        .unwrap();
    let allocated = util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;
    let canonical = allocated.payload()["path"]
        .as_str()
        .expect("v1 payload requires `path`")
        .to_string();

    adapter.release_worktree(&wt.id).await.unwrap();

    let events = util::drain_for(&mut rx, Duration::from_secs(2)).await;
    assert_eq!(
        util::count_subject(&events, "worktree.released"),
        1,
        "exactly one worktree.released per release"
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
        "registry entry is closed on release"
    );
    let listed = util::git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(
        !listed.contains(wt.path.file_name().unwrap().to_str().unwrap()),
        "git no longer tracks the released worktree:\n{listed}"
    );
}
