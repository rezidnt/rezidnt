//! DR-057 ORACLE — `WorktreeState.last_diff` retains the FULL `CasRef`
//! (DR-057 §Decision 1, the widening the `diff_view` tool requires).
//!
//! Today the fold keeps only the hash string and DISCARDS the `bytes`/`mime`
//! that were already on the wire (`diff.ready`/`diff.merged` both carry a full
//! `{hash, bytes, mime}` ref, pinned by ontology v1 and the committed
//! `s2_diff_ready`/`s4_verified_run` fixtures). `diff_view` must serve the full
//! ref or serve nothing (null) — so the fold must stop throwing two thirds of
//! it away. These tests pin the widened fold:
//!
//! - all THREE fields fold VERBATIM from `diff.ready` (and from `diff.merged`);
//! - a later diff-bearing fact overwrites the WHOLE ref (last-write-wins on
//!   the triple, never a splice of two events);
//! - a tree with no diff-bearing fact stays `None` — the honest absence
//!   `diff_view`'s null leg rides (never a fabricated `Default`);
//! - the standing fold properties hold over the widened field: incremental
//!   apply == whole-log fold, and the serialized graph round-trips (the
//!   snapshot/resume seam, I3) WITHOUT dropping `bytes`/`mime` back to a bare
//!   hash;
//! - a proptest folds arbitrary interleavings of diff/worktree facts and
//!   asserts `last_diff` is always exactly the LAST diff-bearing event's full
//!   ref.
//!
//! ## RED MODE (against the tree at cut time — post-`1094f40`)
//!
//! COMPILE-RED: `WorktreeState.last_diff` is `Option<String>` today
//! (`crates/rezidnt-state/src/lib.rs:792`), so every `Option<CasRef>`
//! comparison below is a type error. That red IS the work order: widen the
//! field, update the `diff.ready`/`diff.merged` arms to fold the whole ref.
//!
//! ## Consequence the implementer must carry (disclosed, not hidden)
//!
//! The widening changes the SERIALIZED shape of derived state: four committed
//! goldens pin `last_diff` as a bare hash string or null
//! (`s2_diff_ready.expected.json`, `s2_worktree_conflict.expected.json`,
//! `s4_verified_run.expected.json`) and several suites read it as a string
//! (`s2_worktrees.rs`, `s4_gates.rs`, `dr049_lifecycle_outcome_split.rs`, the
//! tui board projection via `WorktreeRow.last_diff`). Those goldens/reads must
//! be updated to the object shape (or, for `WorktreeRow`, mapped from
//! `ref.hash` — DR-057 leaves `board_view`'s shape UNTOUCHED). Updating them
//! adds information and weakens nothing; the new
//! `dr057_diff_ref_retained.jsonl` golden pins the target shape in the replay
//! gate. DR-057 calls this widening "additive" — additive on the information
//! axis, but NOT shape-preserving on the serialized graph; the record's word
//! undersells the blast radius and this header is the honest account.

use proptest::prelude::*;
use rezidnt_state::{Graph, apply, fold};
use rezidnt_types::Event;
use rezidnt_types::refs::CasRef;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_785_027_600_000; // 2026-07-26T09:00:00Z, arbitrary fixed epoch

const WT: &str = "/repos/demo/wt-review";
const RUN: &str = "01DR057D1FF0000000000000R1";

const HASH_A: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";
const HASH_B: &str = "beefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeef";

fn evt(seq: u32, subject: &str, payload: serde_json::Value) -> Event {
    serde_json::from_value(json!({
        "id": Ulid::from_parts(T0_MS + u64::from(seq), u128::from(seq) + 7).to_string(),
        "ts": "2026-07-26T09:00:00Z",
        "v": 1,
        "source": "git-adapter",
        "subject": subject,
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": payload,
    }))
    .expect("test event must parse")
}

fn cas_ref(hash: &str, bytes: u64, mime: &str) -> CasRef {
    CasRef {
        hash: hash.to_string(),
        bytes,
        mime: mime.to_string(),
    }
}

fn allocated(seq: u32) -> Event {
    evt(
        seq,
        "worktree.allocated",
        json!({"path": WT, "branch": "feat/review", "allocator": "rezidnt"}),
    )
}

fn diff_ready(seq: u32, r: &CasRef) -> Event {
    evt(
        seq,
        "diff.ready",
        json!({"worktree": WT, "diff": {"hash": r.hash, "bytes": r.bytes, "mime": r.mime}}),
    )
}

fn diff_merged(seq: u32, r: &CasRef) -> Event {
    evt(
        seq,
        "diff.merged",
        json!({"run": RUN, "worktree": WT, "diff": {"hash": r.hash, "bytes": r.bytes, "mime": r.mime}}),
    )
}

/// `diff.ready` folds the WHOLE `{hash, bytes, mime}` triple VERBATIM — the
/// fields were always on the wire (ontology v1); discarding two of them is the
/// defect DR-057 §Decision 1 retires.
#[test]
fn diff_ready_folds_the_full_cas_ref_verbatim() {
    let want = cas_ref(HASH_A, 21, "text/x-diff");
    let graph = fold([allocated(1), diff_ready(2, &want)].iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    assert_eq!(
        wt.last_diff,
        Some(want),
        "the fold must retain the FULL CasRef from diff.ready — hash AND bytes \
         AND mime, verbatim (DR-057 §Decision 1)"
    );
}

/// `diff.merged` carries a full ref too (the `s4_verified_run` fixture shape)
/// and is the OTHER fact `diff_view`'s null-vs-ref decision keys on — it must
/// retain the triple exactly as `diff.ready` does, alongside its outcome write.
#[test]
fn diff_merged_folds_the_full_cas_ref_and_the_outcome() {
    let want = cas_ref(HASH_B, 34, "text/x-diff");
    let graph = fold([allocated(1), diff_merged(2, &want)].iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    assert_eq!(wt.outcome.as_deref(), Some("merged"));
    assert_eq!(
        wt.last_diff,
        Some(want),
        "diff.merged must retain the full CasRef exactly as diff.ready does"
    );
}

/// A later diff-bearing fact overwrites the WHOLE triple. A fold that spliced
/// fields from two events (new hash, stale bytes/mime) would hand `cas_read` a
/// lying ref — the exact class of silent wrongness a review surface cannot
/// carry.
#[test]
fn a_later_diff_overwrites_the_whole_ref_never_a_splice() {
    let first = cas_ref(HASH_A, 21, "text/x-diff");
    let second = cas_ref(HASH_B, 34, "text/plain");
    let graph = fold([allocated(1), diff_ready(2, &first), diff_ready(3, &second)].iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    assert_eq!(
        wt.last_diff,
        Some(second),
        "last-write-wins applies to the WHOLE ref: the second event's hash, \
         bytes AND mime — never a splice of two events"
    );
}

/// A tree with no diff-bearing fact carries NO ref — the honest absence
/// `diff_view`'s null leg serves. Never a fabricated `Default` (DR-012
/// declared-vs-absent; the empty-`CasRef` fabrication is the violation DR-057
/// names as the most likely implementer shortcut).
#[test]
fn no_diff_fact_means_none_never_a_fabricated_default() {
    let graph = fold([allocated(1)].iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    assert_eq!(
        wt.last_diff, None,
        "an allocation alone earns no diff ref — absent, never an empty CasRef"
    );
}

/// The SERIALIZED derived state retains all three fields. The serialized graph
/// is the snapshot (`Materializer::snapshot`/`resume`) and the dossier read —
/// a fold that held the ref in memory but serialized only the hash would lose
/// `bytes`/`mime` across every snapshot/resume (I3: anything not rebuildable
/// from log + CAS is misdesigned, and a snapshot that lossily re-encodes the
/// fold breaks rebuild equality).
#[test]
fn serialized_worktree_state_carries_all_three_fields() {
    let want = cas_ref(HASH_A, 21, "text/x-diff");
    let graph = fold([allocated(1), diff_ready(2, &want)].iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    let entry = serde_json::to_value(wt).expect("worktree state serializes");
    assert_eq!(entry["last_diff"]["hash"], json!(HASH_A));
    assert_eq!(entry["last_diff"]["bytes"], json!(21));
    assert_eq!(entry["last_diff"]["mime"], json!("text/x-diff"));
}

/// The standing fold properties hold over the widened field: applying events
/// one at a time equals the whole-log fold, and the serialized graph
/// round-trips to an EQUAL graph (the snapshot/resume seam). Divergence here
/// is a reducer bug and a release blocker (doc §6).
#[test]
fn incremental_apply_and_serde_round_trip_preserve_the_ref() {
    let first = cas_ref(HASH_A, 21, "text/x-diff");
    let second = cas_ref(HASH_B, 34, "text/plain");
    let events = [
        allocated(1),
        diff_ready(2, &first),
        diff_merged(3, &second),
        evt(4, "worktree.released", json!({"path": WT})),
    ];

    let whole = fold(events.iter());
    let mut live = Graph::default();
    for event in &events {
        apply(&mut live, event);
    }
    assert_eq!(
        live, whole,
        "incremental apply must equal the whole-log fold"
    );

    let snapshot = serde_json::to_value(&whole).expect("graph serializes");
    let resumed: Graph = serde_json::from_value(snapshot).expect("snapshot parses back");
    assert_eq!(
        resumed, whole,
        "the serialized graph must round-trip EQUAL — a snapshot that drops \
         bytes/mime would diverge from rebuild (I3)"
    );
    assert_eq!(
        resumed
            .worktrees
            .get(WT)
            .expect("worktree entry survives the round trip")
            .last_diff,
        Some(second),
        "the resumed graph carries the full ref of the LAST diff-bearing fact"
    );
}

/// One diff-affecting operation over the single tree the proptest folds.
#[derive(Debug, Clone)]
enum DiffOp {
    Ready(usize),
    Merged(usize),
    Released,
    Conflict,
}

/// A small fixed ref pool: distinct triples so a spliced fold cannot pass by
/// coincidence (every hash pairs with unique bytes/mime).
fn ref_pool() -> Vec<CasRef> {
    vec![
        cas_ref(HASH_A, 21, "text/x-diff"),
        cas_ref(HASH_B, 34, "text/plain"),
        cas_ref(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            4096,
            "text/x-diff",
        ),
    ]
}

fn op_strategy() -> impl Strategy<Value = DiffOp> {
    prop_oneof![
        (0usize..3).prop_map(DiffOp::Ready),
        (0usize..3).prop_map(DiffOp::Merged),
        Just(DiffOp::Released),
        Just(DiffOp::Conflict),
    ]
}

proptest! {
    /// Over ARBITRARY interleavings of diff/worktree facts on one tree:
    /// `last_diff` is exactly the LAST diff-bearing event's full ref (or None
    /// when no diff-bearing fact folded), and incremental apply equals the
    /// whole-log fold. Lifecycle facts (`released`) and `conflict` never
    /// touch the ref — the DR-049 disjoint-axis discipline extended to the
    /// widened field.
    #[test]
    fn last_diff_is_always_the_last_diff_bearing_full_ref(
        ops in prop::collection::vec(op_strategy(), 1..24)
    ) {
        let pool = ref_pool();
        let mut events = vec![allocated(1)];
        let mut expected: Option<CasRef> = None;
        for (i, op) in ops.iter().enumerate() {
            let seq = u32::try_from(i).expect("small index") + 2;
            match op {
                DiffOp::Ready(k) => {
                    events.push(diff_ready(seq, &pool[*k]));
                    expected = Some(pool[*k].clone());
                }
                DiffOp::Merged(k) => {
                    events.push(diff_merged(seq, &pool[*k]));
                    expected = Some(pool[*k].clone());
                }
                DiffOp::Released => {
                    events.push(evt(seq, "worktree.released", json!({"path": WT})));
                }
                DiffOp::Conflict => {
                    events.push(evt(seq, "worktree.conflict", json!({"path": WT})));
                }
            }
        }

        let whole = fold(events.iter());
        let mut live = Graph::default();
        for event in &events {
            apply(&mut live, event);
        }
        prop_assert_eq!(&live, &whole, "incremental apply == whole-log fold");

        let wt = whole.worktrees.get(WT).expect("worktree entry exists");
        prop_assert_eq!(
            wt.last_diff.clone(),
            expected,
            "last_diff must be exactly the LAST diff-bearing event's full ref"
        );
    }
}
