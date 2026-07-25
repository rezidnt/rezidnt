//! S2 oracle — worktree reducers. Folds `worktree.*` and `diff.ready` facts
//! into `Graph::worktrees` keyed by the canonicalized path string. The field
//! exists as a stub; the reducer arms DO NOT — every test here fails on an
//! empty map until the implementer lands them (semantics doc'd on
//! `WorktreeState`).
//!
//! Events are built as raw JSON and parsed with plain serde, matching the
//! fixture-replay suite: a failure isolates the reducers, not the codec.
//!
//! ## DR-049 §Decision 2 — rewritten to the SPLIT shape (disclosed)
//!
//! Every assertion here that read the collapsed `status` String now reads
//! `lifecycle` AND `outcome`. This is a criterion FORM change, and the
//! replacements assert strictly more: where one field was checked, two
//! independent fields are, including that the fact under test leaves the OTHER
//! axis alone. No bar moved. The split's own board is
//! `tests/dr049_lifecycle_outcome_split.rs`.
//!
//! `worktree.observed` reads `lifecycle = "observed"` with NO outcome — the
//! implementer settlement the DR-049 oracle left open. Rationale on
//! `WorktreeState`: "observed" answers the same question `allocated`/`released`
//! answer, and an out-of-band tree has earned no verdict for the fold to
//! synthesize (I3).

use rezidnt_state::fold;
use rezidnt_types::Event;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_246_400_000; // 2026-07-16T00:00:00Z, arbitrary fixed epoch

const WT_A: &str = "/repos/demo/wt-feat";
const WT_B: &str = "/repos/demo/wt-human";

fn evt(seq: u32, subject: &str, payload: serde_json::Value) -> Event {
    serde_json::from_value(json!({
        "id": Ulid::from_parts(T0_MS + u64::from(seq), u128::from(seq) + 7).to_string(),
        "ts": "2026-07-17T12:00:00Z",
        "v": 1,
        "source": "git-adapter",
        "subject": subject,
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": payload,
    }))
    .expect("test event must parse")
}

fn allocated(seq: u32, path: &str, branch: &str) -> Event {
    evt(
        seq,
        "worktree.allocated",
        json!({"path": path, "branch": branch, "allocator": "rezidnt"}),
    )
}

#[test]
fn allocated_folds_into_worktrees_keyed_by_path() {
    let graph = fold([allocated(1, WT_A, "feat/s2")].iter());
    let wt = graph
        .worktrees
        .get(WT_A)
        .expect("worktree.allocated must materialize a worktrees entry keyed by payload path");
    assert_eq!(wt.lifecycle, "allocated");
    assert_eq!(
        wt.outcome, None,
        "an allocation earns no outcome — absent, never a sentinel"
    );
    assert_eq!(wt.branch.as_deref(), Some("feat/s2"));
    assert_eq!(wt.allocator.as_deref(), Some("rezidnt"));
    assert_eq!(wt.conflicts, 0);
    assert_eq!(wt.last_diff, None);
}

#[test]
fn observed_folds_with_allocator_human() {
    let ev = evt(
        1,
        "worktree.observed",
        json!({"path": WT_B, "allocator": "human"}),
    );
    let graph = fold([ev].iter());
    let wt = graph
        .worktrees
        .get(WT_B)
        .expect("worktree.observed must materialize an entry (out-of-band guard, DR-001)");
    assert_eq!(
        wt.lifecycle, "observed",
        "an out-of-band tree's provenance is a LIFECYCLE state, not an outcome"
    );
    assert_eq!(
        wt.outcome, None,
        "the fold never synthesizes a verdict for a tree rezidnt did not run (I3)"
    );
    assert_eq!(wt.allocator.as_deref(), Some("human"));
    assert_eq!(wt.conflicts, 0);
}

#[test]
fn conflict_increments_count_without_double_tracking() {
    let events = [
        allocated(1, WT_A, "feat/s2"),
        evt(2, "worktree.conflict", json!({"path": WT_A})),
    ];
    let graph = fold(events.iter());
    assert_eq!(
        graph.worktrees.len(),
        1,
        "a conflict never mints a second entry for the contested path (no double-tracking)"
    );
    let wt = graph.worktrees.get(WT_A).unwrap();
    assert_eq!(wt.conflicts, 1, "the collision is counted");
    assert_eq!(
        wt.lifecycle, "allocated",
        "the first claim's lifecycle survives the conflict"
    );
    assert_eq!(
        wt.outcome, None,
        "a conflict is not an outcome; it touches neither axis"
    );
    assert_eq!(
        wt.allocator.as_deref(),
        Some("rezidnt"),
        "the first claim's allocator survives the conflict"
    );
}

#[test]
fn every_logged_conflict_counts_once() {
    // Exactly-once EMISSION is the adapter's obligation (pinned in
    // rezidnt-adapter-git tests). The reducer's obligation is I3 honesty:
    // fold what the log says, count each logged fact exactly once.
    let events = [
        allocated(1, WT_A, "feat/s2"),
        evt(2, "worktree.conflict", json!({"path": WT_A})),
        evt(3, "worktree.conflict", json!({"path": WT_A})),
    ];
    let graph = fold(events.iter());
    assert_eq!(graph.worktrees.get(WT_A).unwrap().conflicts, 2);
}

#[test]
fn released_sets_lifecycle_released() {
    let events = [
        allocated(1, WT_A, "feat/s2"),
        evt(2, "worktree.released", json!({"path": WT_A})),
    ];
    let graph = fold(events.iter());
    let wt = graph.worktrees.get(WT_A).unwrap();
    assert_eq!(wt.lifecycle, "released");
    assert_eq!(
        wt.outcome, None,
        "a release sets LIFECYCLE only — it never invents an outcome for a tree that \
         earned none (DR-049 §Decision 2)"
    );
    assert_eq!(
        wt.branch.as_deref(),
        Some("feat/s2"),
        "release keeps the entry's identity"
    );
}

#[test]
fn released_without_allocation_still_materializes() {
    // Same rule as workspace.closed: inserted even if never allocated — the
    // log is truth (I3), the reducer never gatekeeps.
    let graph = fold([evt(1, "worktree.released", json!({"path": WT_A}))].iter());
    let wt = graph.worktrees.get(WT_A).expect("entry must exist");
    assert_eq!(wt.lifecycle, "released");
    assert_eq!(wt.outcome, None);
}

#[test]
fn diff_ready_records_last_diff_hash() {
    let hash = "a3f1c0de5b9a4e7d8c2b6f0a1d4e7c8b9a0f1e2d3c4b5a69788796a5b4c3d2e1";
    let events = [
        allocated(1, WT_A, "feat/s2"),
        evt(
            2,
            "diff.ready",
            json!({"worktree": WT_A, "diff": {"hash": hash, "bytes": 412, "mime": "text/x-diff"}}),
        ),
    ];
    let graph = fold(events.iter());
    let wt = graph.worktrees.get(WT_A).unwrap();
    assert_eq!(
        wt.last_diff.as_deref(),
        Some(hash),
        "diff.ready pins the latest summary ref hash on the worktree entry"
    );
    assert_eq!(
        wt.lifecycle, "allocated",
        "a diff does not change the lifecycle"
    );
    assert_eq!(
        wt.outcome, None,
        "a diff being READY is not an outcome — only the merge that follows is"
    );
}
