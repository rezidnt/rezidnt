//! DR-059 ORACLE — the derived-state slot for the `patch?: CasRef` sibling
//! ref (criteria 5 and 6, plus the serialized-axis disclosure DR-059
//! §Decision 2 makes this slice's explicit obligation).
//!
//! The ruled contract (`spec/ontology.md` `diff.ready`/`diff.merged`
//! `patch?` bullets): the fold gains a slot MATCHING `last_diff` — this board
//! names it `WorktreeState::last_patch: Option<CasRef>` — carried VERBATIM
//! from the payload, ABSENT-honest (a log predating the field folds `None`;
//! nothing synthesizes a ref), and paired with the summary it was emitted
//! beside.
//!
//! ## The serialized axis, pinned rather than re-litigated
//!
//! DR-057's `last_diff` widening was information-additive and
//! serialized-BREAKING — three goldens and five reader suites recut, one of
//! them `#[cfg(unix)]` and missed. DR-059 §Decision 2 makes the axis this
//! slice's obligation to CHECK. Checked against the tree (enumeration in the
//! oracle board): `fixture_replay` deserializes every `*.expected.json` into
//! `Graph`, and four committed goldens carry `worktrees` entries WITHOUT any
//! patch key (`s2_diff_ready`, `s2_worktree_conflict`, `s4_verified_run`,
//! `dr057_diff_ref_retained`). A bare `pub last_patch: Option<CasRef>` (the
//! shape `last_diff` landed with — no serde attrs) makes all four goldens
//! FAIL TO PARSE: serde rejects a missing field even for `Option` without
//! `#[serde(default)]`. The additive landing exists and is already the house
//! pattern (`WorktreeState::outcome`): `#[serde(default,
//! skip_serializing_if = "Option::is_none")]`. The serialization test below
//! PINS that landing, so the widening is ADDITIVE on the serialized axis and
//! ZERO goldens are recut for this field — a golden recut DR-059 never
//! sanctioned.
//!
//! RED MODE (verified against the tree at cut time): COMPILE-RED —
//! `WorktreeState` has no `last_patch` field (grep-verified: the quoted
//! literal `"patch"` appears nowhere in `crates/rezidnt-state/src/lib.rs`).
//! Every test in this binary references the field, so the binary fails to
//! build until the slot exists; then ASSERT-RED until the reducer arms fold
//! it as ruled.

use rezidnt_state::{WorktreeState, fold};
use rezidnt_types::Event;
use rezidnt_types::refs::CasRef;
use serde_json::{Value, json};

const WT: &str = "/repos/demo/wt-patch";
const SUMMARY_HASH: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";
const PATCH_HASH: &str = "beefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeef";
const SECOND_SUMMARY_HASH: &str =
    "cafecafecafecafecafecafecafecafecafecafecafecafecafecafecafecafe";

fn evt(id: &str, subject: &str, payload: Value) -> Event {
    serde_json::from_value(json!({
        "id": id,
        "ts": "2026-07-26T12:00:00Z",
        "v": 1,
        "source": "git-adapter",
        "subject": subject,
        "correlation": "01DR059PATCHF0LD00000000C0",
        "payload": payload,
    }))
    .expect("test event must parse")
}

fn summary_ref() -> Value {
    json!({"hash": SUMMARY_HASH, "bytes": 21, "mime": "text/x-diff-summary"})
}

fn patch_ref() -> Value {
    json!({"hash": PATCH_HASH, "bytes": 512, "mime": "text/x-diff"})
}

fn cas(hash: &str, bytes: u64, mime: &str) -> CasRef {
    CasRef {
        hash: hash.to_string(),
        bytes,
        mime: mime.to_string(),
    }
}

/// `diff.ready` carrying `patch` folds the WHOLE ref verbatim into the
/// matching slot, and `last_diff` keeps folding the summary exactly as
/// before — two slots, one fact, nothing spliced.
#[test]
fn diff_ready_with_a_patch_folds_the_whole_ref() {
    let events = vec![evt(
        "01DR059PATCHF0LD00000000E1",
        "diff.ready",
        json!({"worktree": WT, "diff": summary_ref(), "patch": patch_ref()}),
    )];
    let graph = fold(events.iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry folded");

    assert_eq!(
        wt.last_patch,
        Some(cas(PATCH_HASH, 512, "text/x-diff")),
        "the patch ref folds WHOLE — hash, bytes, mime, verbatim from the \
         wire (the DR-057 whole-ref discipline, applied to the sibling)"
    );
    assert_eq!(
        wt.last_diff,
        Some(cas(SUMMARY_HASH, 21, "text/x-diff-summary")),
        "the summary slot is UNTOUCHED by the sibling's arrival (criterion 3)"
    );
}

/// `diff.merged` republishes the gate-time patch ref (criterion 2) and the
/// fold takes it, alongside the `outcome = "merged"` write it already does.
#[test]
fn diff_merged_folds_the_republished_patch() {
    let events = vec![evt(
        "01DR059PATCHF0LD00000000E2",
        "diff.merged",
        json!({
            "run": "01DR059PATCHF0LD00000000R1",
            "worktree": WT,
            "diff": summary_ref(),
            "patch": patch_ref(),
        }),
    )];
    let graph = fold(events.iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry folded");

    assert_eq!(wt.outcome.as_deref(), Some("merged"));
    assert_eq!(
        wt.last_patch,
        Some(cas(PATCH_HASH, 512, "text/x-diff")),
        "diff.merged carries the SAME gate-time patch ref through, exactly \
         as `diff` already rides (DR-059 §Decision 1; ontology `diff.merged` \
         `patch?` bullet)"
    );
}

/// ABSENCE IS HONEST (criterion 6) — a patch-less fact folds `None`, never a
/// fabricated ref; and a LATER patch-less fact clears an earlier patch, so
/// the pair `diff_view` serves can never be a stale patch bolted onto a
/// newer summary. The two refs travel together or not at all: serving a
/// patch that does not describe the summary beside it is the same
/// silent-wrong class the mime lie was. (Oracle ruling derived from the
/// ontology's never-synthesized / null-when-unfolded semantics — named in
/// the board so `/dr` can overrule the clearing leg without touching the
/// rest.)
#[test]
fn a_patchless_fact_folds_none_and_clears_a_stale_pairing() {
    // Leg 1: patch-less only — None, never CasRef::default().
    let events = vec![evt(
        "01DR059PATCHF0LD00000000E3",
        "diff.ready",
        json!({"worktree": WT, "diff": summary_ref()}),
    )];
    let graph = fold(events.iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry folded");
    assert_eq!(
        wt.last_patch, None,
        "no patch on the fact, no patch in the fold — never a fabricated \
         Default ref addressing a blob the log never pinned"
    );

    // Leg 2: a patch-bearing fact, then a NEWER patch-less summary.
    let events = vec![
        evt(
            "01DR059PATCHF0LD00000000E4",
            "diff.ready",
            json!({"worktree": WT, "diff": summary_ref(), "patch": patch_ref()}),
        ),
        evt(
            "01DR059PATCHF0LD00000000E5",
            "diff.ready",
            json!({
                "worktree": WT,
                "diff": {"hash": SECOND_SUMMARY_HASH, "bytes": 34, "mime": "text/x-diff-summary"},
            }),
        ),
    ];
    let graph = fold(events.iter());
    let wt = graph.worktrees.get(WT).expect("worktree entry folded");
    assert_eq!(
        wt.last_diff,
        Some(cas(SECOND_SUMMARY_HASH, 34, "text/x-diff-summary")),
        "the newer summary won, last-write-wins as ever"
    );
    assert_eq!(
        wt.last_patch, None,
        "the stale patch does NOT survive to be paired with a summary it \
         does not describe — the pair travels together (see test doc)"
    );
}

/// A LOG PREDATING THE FIELD FOLDS `None` (criterion 6) — the committed S2
/// golden, untouched, folds a patch-less entry; `v` stays 1 on the wire.
#[test]
fn a_pre_dr059_fixture_folds_patchless_and_stays_valid() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/s2_diff_ready.jsonl");
    let events: Vec<Event> = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .map(|l| serde_json::from_str(l).expect("fixture line parses — the old shape stays valid"))
        .collect();
    assert!(
        events.iter().all(|e| e.v == 1),
        "`v` stays 1 on both subjects — the field is additive"
    );
    let graph = fold(events.iter());
    let (_, wt) = graph
        .worktrees
        .iter()
        .find(|(_, w)| w.last_diff.is_some())
        .expect("the S2 fixture folds a diff-bearing worktree");
    assert_eq!(
        wt.last_patch, None,
        "a log written before the field exists folds an honest None — \
         nothing synthesizes a patch for history (criterion 6)"
    );
}

/// THE SERIALIZED AXIS IS ADDITIVE, PINNED (DR-059 §Decision 2's disclosure
/// obligation, answered with a judge instead of prose). Two legs:
///
/// 1. a pre-DR-059 serialized entry — the exact shape the four committed
///    `*.expected.json` goldens carry, NO patch key — still deserializes
///    (`serde(default)`), so `fixture_replay` and every committed golden
///    stay green UNCUT;
/// 2. a patch-less entry serializes WITHOUT the key
///    (`skip_serializing_if`), so no golden ever needs a `null` placeholder
///    and re-emitting a golden is byte-stable.
///
/// The bare-`Option` landing `last_diff` took would fail leg 1 for every
/// worktree-bearing golden — the exact DR-057 breakage, third time in one
/// arc if repeated.
#[test]
fn the_serialized_axis_is_additive_not_breaking() {
    // Leg 1 — the committed-golden shape (dr057_diff_ref_retained.expected.json's
    // entry, verbatim minus hashes) parses without a patch key.
    let old_shape = json!({
        "lifecycle": "allocated",
        "outcome": "merged",
        "branch": "feat/review",
        "allocator": "rezidnt",
        "conflicts": 0,
        "last_diff": {"hash": SUMMARY_HASH, "bytes": 21, "mime": "text/x-diff-summary"},
    });
    let state: WorktreeState = serde_json::from_value(old_shape).expect(
        "a pre-DR-059 serialized entry (no patch key) must deserialize — \
         without `#[serde(default)]` every committed worktree-bearing golden \
         fails to PARSE, the DR-057 breakage repeated",
    );
    assert_eq!(state.last_patch, None, "the missing key parses to None");

    // Leg 2 — absence serializes as absence, not a null placeholder.
    let out = serde_json::to_value(&state).expect("worktree state serializes");
    assert!(
        !out.as_object().unwrap().keys().any(|k| k.contains("patch")),
        "a patch-less entry serializes WITHOUT the key (the `outcome` serde \
         pattern) — goldens stay byte-stable and the widening is ADDITIVE on \
         the serialized axis: {out:#}"
    );
}
