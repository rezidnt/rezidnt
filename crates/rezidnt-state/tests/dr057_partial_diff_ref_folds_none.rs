//! DR-057 debrief finding F7 — the ALL-OR-NOTHING leg of the widened
//! `WorktreeState.last_diff` fold, judged.
//!
//! `crates/rezidnt-state/src/lib.rs` gates both diff arms on
//! `payload_cas_ref(event)`, which deserializes the payload's `diff` as a WHOLE
//! `CasRef`. A `diff` that is missing `bytes` or `mime` therefore folds NOTHING
//! on that field — the choice is disclosed in the `WorktreeState` doc comment
//! ("A `diff` that does not parse as a full ref folds NOTHING on that field: the
//! reducer never part-fills a ref it was not given"), but until now no test
//! exercised it, so the prose carried a mechanism no judge held.
//!
//! It is the right choice and the point of pinning it is that it stays chosen.
//! `cas_read`'s args ARE the triple, so a part-filled ref (real hash, fabricated
//! `bytes: 0`, fabricated `mime: ""`) would be a ref the log never pinned,
//! handed to `diff_view` to serve and to a client to resolve. Absence is honest;
//! a synthesized two-thirds is not (I3, DR-012 declared-vs-absent).
//!
//! The trade this pins, stated so nobody has to rediscover it: a MALFORMED
//! `diff.ready` is invisible in derived state. The fold does not alarm, because
//! the reducer is pure and has no channel to alarm on; the fact stays on the log
//! where a human or a later verifier can see it. If that silence is ever judged
//! too quiet, the fix is a fact on the fabric, not a part-filled ref.

use rezidnt_state::fold;
use rezidnt_types::Event;
use serde_json::json;
use ulid::Ulid;

const T0_MS: u64 = 1_785_027_600_000; // 2026-07-26T09:00:00Z, arbitrary fixed epoch
const WT: &str = "/repos/demo/wt-partial";
const RUN: &str = "01DR057F7PART0000000000R01";
const HASH: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";

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

fn allocated(seq: u32) -> Event {
    evt(
        seq,
        "worktree.allocated",
        json!({"path": WT, "branch": "feat/partial", "allocator": "rezidnt"}),
    )
}

/// A `diff.ready` whose `diff` carries ONLY a hash folds `None` — never a
/// part-filled ref. `{hash}` with no `bytes`/`mime` is precisely the shape a
/// pre-DR-057 emitter (or a hand-edited log) produces, and the tempting
/// shortcut is to keep the hash and default the rest. That would hand
/// `diff_view` a `{hash, bytes: 0, mime: ""}` to serve and `cas_read` a lie to
/// resolve: `bytes: 0` contradicts the real blob and `mime: ""` is not a text
/// type, so the fabricated ref refuses `cas.not_text` and blames the CALLER for
/// a value the reducer invented.
#[test]
fn a_hash_only_diff_folds_none_never_a_part_filled_ref() {
    let graph = fold(
        [
            allocated(1),
            evt(
                2,
                "diff.ready",
                json!({"worktree": WT, "diff": {"hash": HASH}}),
            ),
        ]
        .iter(),
    );

    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    assert_eq!(
        wt.lifecycle, "allocated",
        "the tree is REAL — a None diff here must not be missing-entry fallout: {wt:#?}"
    );
    assert_eq!(
        wt.last_diff, None,
        "a `diff` that does not parse as a WHOLE CasRef folds NOTHING on that \
         field: the reducer never part-fills a ref it was not given (I3). A \
         retained hash with a fabricated bytes/mime would be a ref the log \
         never pinned: {wt:#?}"
    );
}

/// The same all-or-nothing rule on the OTHER diff arm — and the arm split is
/// load-bearing: `diff.merged`'s outcome write is NOT gated on the ref, so a
/// malformed diff loses the ref and keeps the merge. Collapsing the two would
/// make a malformed payload silently erase a merge from derived state, which is
/// a far worse lie than an absent ref.
#[test]
fn a_partial_merged_diff_loses_the_ref_and_keeps_the_outcome() {
    let graph = fold(
        [
            allocated(1),
            evt(
                2,
                "diff.merged",
                json!({
                    "run": RUN,
                    "worktree": WT,
                    "diff": {"hash": HASH, "bytes": 21},
                }),
            ),
        ]
        .iter(),
    );

    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    assert_eq!(
        wt.outcome.as_deref(),
        Some("merged"),
        "the merge is a fact of its own and survives a malformed ref: {wt:#?}"
    );
    assert_eq!(
        wt.last_diff, None,
        "a ref missing `mime` is not a whole CasRef, so nothing folds on that \
         field — no splice, no default: {wt:#?}"
    );
}

/// A GOOD ref folding after a malformed one still lands. The all-or-nothing
/// rule drops the bad fact, it does not poison the field: a tree whose emitter
/// hiccupped once still gets its next real diff.
#[test]
fn a_malformed_ref_does_not_poison_a_later_good_one() {
    let graph = fold(
        [
            allocated(1),
            evt(
                2,
                "diff.ready",
                json!({"worktree": WT, "diff": {"hash": HASH}}),
            ),
            evt(
                3,
                "diff.ready",
                json!({
                    "worktree": WT,
                    "diff": {"hash": HASH, "bytes": 21, "mime": "text/x-diff-summary"},
                }),
            ),
        ]
        .iter(),
    );

    let wt = graph.worktrees.get(WT).expect("worktree entry exists");
    let got = wt.last_diff.as_ref().expect("the whole later ref folds");
    assert_eq!(got.hash, HASH);
    assert_eq!(got.bytes, 21);
    assert_eq!(got.mime, "text/x-diff-summary");
}
