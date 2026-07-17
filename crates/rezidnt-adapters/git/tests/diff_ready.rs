//! S2 oracle — exit criterion 1: `diff.ready` within 1 s of a write
//! (post-debounce; the ontology fixes the debounce at 250 ms, emitter = git
//! adapter notify watcher). Plus the I2 contract: the diff summary is a CAS
//! ref, never inline diff bytes.
//!
//! Timing discipline: the 1 s bound IS the slice criterion, so it is asserted
//! directly as wall time from the last write to fact receipt. The OUTER
//! tolerance (test hang guard) is generous; the criterion assertion is not.
//! With a 250 ms debounce the bound leaves 750 ms of real margin — a miss is
//! an adapter defect, not CI weather.
//!
//! Payload-shape caveat (flagged in the work order): `diff.ready` has no
//! ratified v1 payload baseline; these tests pin the semantically forced
//! minimum — the worktree it concerns (`worktree`) and the summary ref
//! (`diff: CasRef`). Warden ratification via /subject required.

mod util;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rezidnt_adapter_git::{GitAdapter, RepoSubstrate, WorktreeReq};
use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;

const OUTER: Duration = Duration::from_secs(5);
const BOUND: Duration = Duration::from_secs(1);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
    }
}

fn payload_diff_ref(ev: &rezidnt_types::Event) -> CasRef {
    serde_json::from_value(ev.payload()["diff"].clone())
        .expect("diff.ready payload carries the summary as `diff: CasRef` (I2)")
}

/// THE criterion: write → `diff.ready` in ≤ 1 s, carrying a resolvable CAS
/// ref whose blob is a diff summary naming the changed file.
#[tokio::test]
async fn diff_ready_within_one_second_of_write() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-diff", "feat/diff"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    std::fs::write(wt.path.join("oracle_change.txt"), "the write under test\n").unwrap();
    let written_at = Instant::now();

    let ev = util::recv_subject(&mut rx, "diff.ready", OUTER).await;
    let elapsed = written_at.elapsed();
    assert!(
        elapsed <= BOUND,
        "S2 exit criterion: diff.ready within 1 s of write (250 ms debounce leaves 750 ms margin) — took {elapsed:?}"
    );

    assert_eq!(ev.v, 1, "taxonomy v0 mints diff.ready at v = 1");
    assert_eq!(ev.source.as_str(), rezidnt_adapter_git::SOURCE_ID);
    let for_wt = PathBuf::from(
        ev.payload()["worktree"]
            .as_str()
            .expect("diff.ready payload names the `worktree` it concerns"),
    );
    assert_eq!(util::canon(&for_wt), util::canon(&wt.path));

    // I2: the summary is a ref, and the ref resolves.
    let r = payload_diff_ref(&ev);
    assert_eq!(r.hash.len(), 64, "blake3 hex, 64 lowercase chars");
    assert!(
        r.hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert!(r.bytes > 0, "an actual change produces a non-empty summary");
    let blob = Cas::open(&cas_root).unwrap().get(&r).unwrap();
    let text = String::from_utf8_lossy(&blob);
    assert!(
        text.contains("oracle_change.txt"),
        "the diff summary names the changed file — got:\n{text}"
    );
}

/// Post-debounce semantics: a burst of writes inside one 250 ms debounce
/// window coalesces into EXACTLY ONE `diff.ready`, followed by quiescence.
#[tokio::test]
async fn write_burst_coalesces_into_one_diff_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-burst", "feat/burst"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    // Five writes spanning ~80 ms — all inside a single 250 ms debounce window.
    for i in 0..5u8 {
        std::fs::write(
            wt.path.join(format!("burst_{i}.txt")),
            format!("write {i}\n"),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The whole criterion window from the LAST write: one coalesced fact.
    let events = util::drain_for(&mut rx, BOUND).await;
    assert_eq!(
        util::count_subject(&events, "diff.ready"),
        1,
        "a debounced burst yields exactly one diff.ready within the 1 s bound — got {events:#?}"
    );

    // Quiescence: nothing trailing once the tree is quiet.
    let more = util::drain_for(&mut rx, Duration::from_millis(600)).await;
    assert_eq!(
        util::count_subject(&more, "diff.ready"),
        0,
        "no trailing diff.ready after the coalesced emission"
    );
}

/// The RepoSubstrate read path (doc §7): `diff_summary` returns a CAS ref and
/// is deterministic over an unchanged tree — same state, same hash (I6-adjacent;
/// this ref is future gate-verifier input, so it must be content-stable).
#[tokio::test]
async fn diff_summary_is_deterministic_cas_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let wt = adapter
        .alloc_worktree(branch_req("feat-sum", "feat/sum"))
        .await
        .unwrap();
    std::fs::write(wt.path.join("summed.txt"), "content under summary\n").unwrap();

    let first = adapter.diff_summary(&wt.id).await.unwrap();
    let second = adapter.diff_summary(&wt.id).await.unwrap();
    assert_eq!(
        first.hash, second.hash,
        "unchanged tree state must summarize to the identical CAS ref (deterministic reads)"
    );
    let blob = Cas::open(&cas_root).unwrap().get(&first).unwrap();
    assert!(
        String::from_utf8_lossy(&blob).contains("summed.txt"),
        "the summary names the changed file"
    );
}
