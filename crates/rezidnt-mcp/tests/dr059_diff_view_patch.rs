//! DR-059 ORACLE — `diff_view` gains `patch: CasRef | null` (criterion 5;
//! DR-059 §Decision 2; ontology `diff.ready` `patch?` "Consumers,
//! work-ordered" clause), with DR-057's null-when-unfolded semantics: a
//! worktree with no folded patch answers `null`, NEVER a fabricated empty
//! patch — and the key is PRESENT either way, because a client cannot tell
//! "null" from "this daemon predates the field" if absence and null are the
//! same wire bytes.
//!
//! The existing `diff` field is UNTOUCHED (DR-059 amends DR-057 §Decision 1
//! by adding a sibling, not by moving the field) — pinned alongside.
//!
//! The response widening's serialized-axis check (DR-059 §Decision 2's
//! obligation): every committed reader of the `diff_view` response asserts
//! field-by-field (`dr057_diff_view.rs`, `dr057_settled_gaps.rs`,
//! `dr058_read_family_unbadged.rs`, `bins/rezidentd/tests/
//! dr057_review_verb_e2e.rs`); none pins the exact key set, so adding the
//! key is ADDITIVE on this surface. `DiffViewArgs` is untouched — the args
//! schema golden (`dr057_diff_view_args.schema.golden.json`) needs NO recut.
//!
//! RED MODE (verified against the tree at cut time): ASSERT-RED. The served
//! response is built at `call_diff_view` as `{worktree, lifecycle, outcome,
//! diff}` — no `patch` key — so the presence assertions below fail. The
//! `contains_key` leg is deliberate: `payload["patch"]` on a missing key
//! indexes to `Null`, and a null-semantics assertion written that way would
//! PASS vacuously today, judging nothing.

mod util;

use rezidnt_types::Event;
use serde_json::{Value, json};

const WT: &str = "/repos/demo/wt-patch-view";
const SUMMARY_HASH: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";
const PATCH_HASH: &str = "beefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeefbeef";

fn evt(id: &str, subject: &str, payload: Value) -> Event {
    serde_json::from_value(json!({
        "id": id,
        "ts": "2026-07-26T12:00:00Z",
        "v": 1,
        "source": "git-adapter",
        "subject": subject,
        "correlation": "01DR059V1EWPATCH00000000C0",
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

/// The fabricated shape the honesty leg forbids (the dr057_diff_view.rs
/// pattern, applied to the sibling).
fn fabricated_empty_ref() -> Value {
    json!({"hash": "", "bytes": 0, "mime": ""})
}

/// A folded patch is SERVED — the full `CasRef` triple verbatim, alongside
/// the untouched `diff` field, on the same unbadged read path.
#[tokio::test]
async fn diff_view_serves_the_folded_patch_ref_alongside_the_diff() {
    let (_dir, core) = util::core();
    core.fabric()
        .publish(evt(
            "01DR059V1EWPATCH00000000E1",
            "worktree.allocated",
            json!({"path": WT, "branch": "feat/patch-view", "allocator": "rezidnt"}),
        ))
        .expect("publish allocation");
    core.fabric()
        .publish(evt(
            "01DR059V1EWPATCH00000000E2",
            "diff.ready",
            json!({"worktree": WT, "diff": summary_ref(), "patch": patch_ref()}),
        ))
        .expect("publish diff.ready");

    // NO badge argument: diff_view stays read-class, unbadged (DR-058's line).
    let result = util::tool_call(&core, 1, "diff_view", json!({"worktree": WT})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "diff_view is a read; it must not error: {result:#}"
    );
    let payload = util::tool_payload(&result);

    assert_eq!(
        payload["patch"],
        patch_ref(),
        "patch is the FULL CasRef from the folded fact — hash AND bytes AND \
         mime, verbatim, ready to hand straight to cas_read (DR-059 \
         §Decision 2): {payload:#}"
    );
    assert_eq!(
        payload["diff"],
        summary_ref(),
        "the existing diff field is UNTOUCHED by the sibling's arrival \
         (DR-059 amends by addition only): {payload:#}"
    );
}

/// THE NULL LEG, NON-VACUOUS — a tree whose folded fact carried no patch
/// answers a PRESENT `patch: null`: never a fabricated ref, never a missing
/// key (a missing key is indistinguishable on the wire from a daemon that
/// predates the field — DR-057's null-when-unfolded semantics require the
/// answer to be STATED).
#[tokio::test]
async fn a_patchless_tree_serves_an_explicit_null_never_a_fabrication() {
    let (_dir, core) = util::core();
    core.fabric()
        .publish(evt(
            "01DR059V1EWPATCH00000000E3",
            "worktree.allocated",
            json!({"path": WT, "branch": "feat/patch-view", "allocator": "rezidnt"}),
        ))
        .expect("publish allocation");
    core.fabric()
        .publish(evt(
            "01DR059V1EWPATCH00000000E4",
            "diff.ready",
            json!({"worktree": WT, "diff": summary_ref()}),
        ))
        .expect("publish patch-less diff.ready");

    let result = util::tool_call(&core, 2, "diff_view", json!({"worktree": WT})).await;
    let payload = util::tool_payload(&result);

    // The row is REAL — the null below is "no patch yet", not a miss.
    assert_eq!(
        payload["diff"],
        summary_ref(),
        "the summary serves; this row is real: {payload:#}"
    );
    let obj = payload
        .as_object()
        .unwrap_or_else(|| panic!("diff_view answers an object: {payload:#}"));
    assert!(
        obj.contains_key("patch"),
        "the patch key is PRESENT and null when unfolded — indexing a missing \
         key also reads as null, which is exactly why this leg asserts \
         presence (the vacuous-pass trap): {payload:#}"
    );
    assert!(
        payload["patch"].is_null(),
        "no patch folded, so patch is NULL (DR-057 null-when-unfolded \
         semantics, applied to the sibling): {payload:#}"
    );
    assert_ne!(
        payload["patch"],
        fabricated_empty_ref(),
        "never a Default-fabricated empty ref: {payload:#}"
    );
}
