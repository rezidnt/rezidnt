//! DR-049 oracle — the fold's collapsed `status` field is SPLIT. The derived
//! worktree entry carries `lifecycle` (`allocated` -> `released`) and
//! `outcome` (`merged` | `failed` | `abandoned` | absent) as SEPARATE fields,
//! replacing the single `status` String. `worktree.released` sets lifecycle
//! ONLY; `diff.merged` sets outcome ONLY; neither can clobber the other
//! (DR-049 §Decision 2 — the exact trade DR-047 §Decision 5 declined is
//! dissolved, not chosen between).
//!
//! ## RED MODE (assert-red on today's tree)
//!
//! `WorktreeState` carries one `status` String today: the `worktree.released`
//! reducer arm sets it (`crates/rezidnt-state/src/lib.rs:847`) and the
//! `diff.merged` arm sets it (`:866`), so a release fact folding after a merge
//! fact clobbers `"merged"`. Every test here asserts on the SERIALIZED entry
//! (`serde_json::to_value`) rather than on typed fields, so this file compiles
//! against today's shape and fails on ASSERTIONS, not on a build error — and
//! the shape it pins is the one every consumer reads: golden fixture expected
//! files, `rezidnt rebuild --json`, the `board_view` MCP payload, the board.
//!
//! ## Movers, named (DR-049 risk (2): a missed `status` consumer is a
//! silent-wrong of the class this arc produced eleven of)
//!
//! When the implementer lands the split, these existing consumers move IN THE
//! SAME SLICE and their `status` assertions are rewritten to the split shape
//! (a disclosed criterion form change — the replacements assert strictly
//! more): `tests/s2_worktrees.rs`; the `tests/fixture_replay.rs` expected
//! files that serialize worktrees (`s2_diff_ready`, `s2_worktree_conflict`,
//! `s4_verified_run`, `s4_vet_refusal`, `s5b_board_permit`); the
//! `WorktreeRow` projection (pinned below); the tui worktrees table and both
//! s5 render goldens (re-blessed via `REZIDNT_BLESS_GOLDEN=1`, see
//! `crates/rezidnt-tui/tests/dr049_board_split_render_note.rs`); and the
//! folded-`status` assertions in
//! `bins/rezidentd/tests/registry_convergence_e2e.rs`.
//!
//! ## Spec gap, FLAGGED rather than vibes-tested
//!
//! DR-049 names `abandoned` as a legal `outcome` value, but no minted fact can
//! set it: `worktree.released` sets lifecycle ONLY (§Decision 2, pinned here),
//! `diff.merged` sets `merged`, and the failed-run outcome is judged on the
//! daemon e2e. `abandoned` is UNREACHABLE from the current taxonomy. Reaching
//! it needs a future ruling (route `/dr`); this board deliberately does not
//! invent a fact to test it with.
//!
//! `worktree.observed` trees are NOT pinned here either: DR-049 defines the
//! split for the allocate/merge/release lifecycle; what an out-of-band
//! observed tree's `lifecycle` reads as is the implementer's rewrite of the
//! existing `s2_worktrees.rs::observed_folds_with_allocator_human`, judged at
//! `/debrief` against I3 (never a synthesized outcome).

use proptest::prelude::*;
use rezidnt_state::{Graph, fold, project};
use rezidnt_types::Event;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_784_246_400_000; // fixed epoch, mirrors s2_worktrees.rs

const WT: &str = "/repos/demo/wt-feat";
const MERGE_HASH: &str = "a3f1c0de5b9a4e7d8c2b6f0a1d4e7c8b9a0f1e2d3c4b5a69788796a5b4c3d2e1";

fn evt(seq: u32, subject: &str, payload: serde_json::Value) -> Event {
    serde_json::from_value(json!({
        "id": Ulid::from_parts(T0_MS + u64::from(seq), u128::from(seq) + 7).to_string(),
        "ts": "2026-07-25T12:00:00Z",
        "v": 1,
        "source": "git-adapter",
        "subject": subject,
        "correlation": Ulid::from_parts(T0_MS, 1).to_string(),
        "payload": payload,
    }))
    .expect("test event must parse")
}

fn allocated(seq: u32) -> Event {
    evt(
        seq,
        "worktree.allocated",
        json!({"path": WT, "branch": "feat/dr049", "allocator": "rezidnt"}),
    )
}

fn merged(seq: u32) -> Event {
    evt(
        seq,
        "diff.merged",
        json!({"run": "01DR049RUN000000000000R01", "worktree": WT, "diff": {"hash": MERGE_HASH, "bytes": 412, "mime": "text/x-diff"}}),
    )
}

fn released(seq: u32) -> Event {
    evt(
        seq,
        "worktree.released",
        json!({"path": WT, "branch": "feat/dr049"}),
    )
}

/// The worktree entry as its consumers see it: the SERIALIZED shape.
fn wt_json(graph: &Graph) -> serde_json::Value {
    let wt = graph
        .worktrees
        .get(WT)
        .unwrap_or_else(|| panic!("the fold holds a worktree entry for {WT}"));
    serde_json::to_value(wt).expect("worktree entry serializes")
}

/// `outcome` is ABSENT: missing key or JSON null both count (absence is the
/// honest representation — never a sentinel string, DR-012 discipline).
fn outcome_is_absent(entry: &serde_json::Value) -> bool {
    entry.get("outcome").is_none_or(serde_json::Value::is_null)
}

#[test]
fn allocated_folds_lifecycle_allocated_with_no_outcome() {
    let graph = fold([allocated(1)].iter());
    let entry = wt_json(&graph);
    assert_eq!(
        entry["lifecycle"],
        json!("allocated"),
        "DR-049 §Decision 2: the derived worktree entry exposes `lifecycle`, and an \
         allocation folds it to \"allocated\". Entry: {entry:#}"
    );
    assert!(
        outcome_is_absent(&entry),
        "a freshly allocated tree has NO outcome yet — absent, never a sentinel: {entry:#}"
    );
}

#[test]
fn the_single_status_field_is_replaced_not_kept() {
    // DR-049 §Decision 2 says REPLACING, and this leg is why: a retained
    // `status` alongside the split would be two shapes answering one question
    // — the exact two-answers state the arc's silent-wrong ledger documents.
    let graph = fold([allocated(1)].iter());
    let entry = wt_json(&graph);
    assert!(
        entry.get("status").is_none(),
        "the collapsed `status` field is REPLACED by `lifecycle` + `outcome`, not kept \
         beside them (DR-049 §Decision 2). A lingering `status` gives every consumer two \
         ways to answer \"what state is this tree in\". Entry: {entry:#}"
    );
}

#[test]
fn merged_sets_outcome_only_lifecycle_stays_allocated() {
    let graph = fold([allocated(1), merged(2)].iter());
    let entry = wt_json(&graph);
    assert_eq!(
        entry["outcome"],
        json!("merged"),
        "`diff.merged` folds the OUTCOME: {entry:#}"
    );
    assert_eq!(
        entry["lifecycle"],
        json!("allocated"),
        "`diff.merged` sets outcome ONLY (DR-049 §Decision 2). The tree is merged but not \
         yet released — lifecycle stays \"allocated\" until `worktree.released` says \
         otherwise; the merge fact never touches it. Entry: {entry:#}"
    );
    assert_eq!(
        entry["last_diff"],
        json!(MERGE_HASH),
        "the merged summary ref is retained on the entry (existing S4 semantics, unchanged)"
    );
}

#[test]
fn released_sets_lifecycle_only_outcome_stays_absent() {
    // A release with no prior outcome: lifecycle moves, outcome does NOT get
    // invented. (`abandoned` is a named-but-unreachable outcome value today —
    // see the module header's spec-gap flag; this test pins the DR's literal
    // sentence "worktree.released sets lifecycle only".)
    let graph = fold([allocated(1), released(2)].iter());
    let entry = wt_json(&graph);
    assert_eq!(
        entry["lifecycle"],
        json!("released"),
        "`worktree.released` folds lifecycle to \"released\": {entry:#}"
    );
    assert!(
        outcome_is_absent(&entry),
        "`worktree.released` sets lifecycle ONLY (DR-049 §Decision 2): it never invents an \
         outcome for a tree that earned none. Entry: {entry:#}"
    );
    assert_eq!(
        entry["branch"],
        json!("feat/dr049"),
        "release keeps the entry's identity (existing S2 semantics, unchanged)"
    );
}

#[test]
fn released_after_merged_keeps_both() {
    // THE test of the slice: the exact clobber DR-049 dissolves. Today the
    // `worktree.released` arm overwrites the single `status` String, so this
    // sequence folds to `status = "released"` and the merge disappears from
    // derived state — which is why DR-047 §Decision 5 declined to release at
    // merge at all. The split makes the sequence honest.
    let graph = fold([allocated(1), merged(2), released(3)].iter());
    let entry = wt_json(&graph);
    assert_eq!(
        entry["lifecycle"],
        json!("released"),
        "after merge-then-release the tree IS released: {entry:#}"
    );
    assert_eq!(
        entry["outcome"],
        json!("merged"),
        "AND its outcome is still \"merged\" — the release fact must not clobber the merge \
         (DR-049 §Decision 2 dissolves the DR-047 §Decision 5 trade; today's single \
         `status` at crates/rezidnt-state/src/lib.rs:847 makes exactly this fail). \
         Entry: {entry:#}"
    );
}

#[test]
fn merged_after_released_folds_to_the_same_terminal_state() {
    // The commute pair of the test above, asserted explicitly: because the two
    // facts write DIFFERENT fields, the fold is not order-sensitive between
    // them (the rejected alternative — an ordering guard — would have made it
    // so, which is why it lost).
    let a = fold([allocated(1), merged(2), released(3)].iter());
    let b = fold([allocated(1), released(2), merged(3)].iter());
    let (a, b) = (wt_json(&a), wt_json(&b));
    for field in ["lifecycle", "outcome"] {
        assert_eq!(
            a[field], b[field],
            "merge and release write DISJOINT fields, so their relative order cannot change \
             the terminal {field}. a: {a:#}, b: {b:#}"
        );
    }
    assert_eq!(a["lifecycle"], json!("released"));
    assert_eq!(a["outcome"], json!("merged"));
}

#[test]
fn released_without_allocation_still_materializes() {
    // Same I3 rule the existing suite pins for `status`: inserted even if
    // never allocated — the log is truth, the reducer never gatekeeps.
    let graph = fold([released(1)].iter());
    let entry = wt_json(&graph);
    assert_eq!(
        entry["lifecycle"],
        json!("released"),
        "a release fact materializes its entry even with no allocation on the log (I3): {entry:#}"
    );
}

#[test]
fn projection_worktree_row_moves_with_the_split() {
    // DR-049 §Decision 2: "the board's worktrees table and its golden fixture
    // move with the shape". `WorktreeRow` is the shared projection both the
    // tui board and the `board_view` MCP tool serve (DR-039) — it lives in
    // this crate, so its movement is pinned host-side here; the render/golden
    // side is noted in crates/rezidnt-tui.
    let graph = fold([allocated(1), merged(2), released(3)].iter());
    let view = project(&graph);
    let row = view
        .worktrees
        .iter()
        .find(|w| w.path == WT)
        .unwrap_or_else(|| panic!("the projection carries a row for {WT}"));
    let row = serde_json::to_value(row).expect("WorktreeRow serializes");
    assert_eq!(
        row["lifecycle"],
        json!("released"),
        "WorktreeRow carries the split lifecycle verbatim (I3): {row:#}"
    );
    assert_eq!(
        row["outcome"],
        json!("merged"),
        "WorktreeRow carries the split outcome verbatim (I3) — the board shows a \
         merged-then-released tree TRUTHFULLY (the DR-049 exit demo's last clause): {row:#}"
    );
    assert!(
        row.get("status").is_none(),
        "the projection replaces `status` too — a row keeping the collapsed field would \
         re-introduce the clobbered answer one consumer downstream: {row:#}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    /// ORDER-INVARIANCE (the property leg): over EVERY interleaving of the
    /// post-allocation facts — merge, release, a conflict, a watcher
    /// `diff.ready` — the terminal `lifecycle`/`outcome` pair depends only on
    /// WHICH facts are on the log, never on their order. The allocation stays
    /// first because the log is append-ordered and an allocation causally
    /// precedes every lifecycle fact about its tree; the shuffled tail is the
    /// racy region (the merge burst, the detached watcher, the release).
    #[test]
    fn prop_lifecycle_and_outcome_are_order_invariant_after_allocation(
        perm in Just((0usize..4).collect::<Vec<_>>()).prop_shuffle()
    ) {
        let tail = |kind: usize| -> Event {
            let seq = 2 + u32::try_from(kind).expect("small index");
            match kind {
                0 => merged(seq),
                1 => released(seq),
                2 => evt(seq, "worktree.conflict", json!({"path": WT})),
                _ => evt(
                    seq,
                    "diff.ready",
                    json!({"worktree": WT, "diff": {"hash": MERGE_HASH, "bytes": 412, "mime": "text/x-diff"}}),
                ),
            }
        };
        let mut events = vec![allocated(1)];
        events.extend(perm.iter().map(|&k| tail(k)));
        let entry = wt_json(&fold(events.iter()));
        prop_assert_eq!(
            &entry["lifecycle"],
            &json!("released"),
            "a log containing `worktree.released` terminally folds lifecycle = released in \
             EVERY order; entry: {:#}, order: {:?}",
            entry,
            perm
        );
        prop_assert_eq!(
            &entry["outcome"],
            &json!("merged"),
            "a log containing `diff.merged` terminally folds outcome = merged in EVERY \
             order — no interleaving of release/conflict/watcher facts may clobber it; \
             entry: {:#}, order: {:?}",
            entry,
            perm
        );
    }
}
