//! DR-057 ORACLE — `diff_view` behavior (DR-057 §Decision 1/3/4): the
//! worktree-keyed, read-class, unbadged Review read.
//!
//! The ruled boundary, restated as falsifiable assertions:
//!
//! - Returns `{worktree, lifecycle, outcome, diff}` where `diff` is the FULL
//!   `CasRef` folded from `diff.ready`/`diff.merged` — hash, bytes, mime, all
//!   three verbatim from the wire.
//! - THE HONESTY LEG: `diff` is NULL when no diff-bearing fact has folded for
//!   that tree — never a fabricated empty `CasRef`. An implementer reaching
//!   for `Default` fabricates `{hash: "", bytes: 0, mime: ""}`; that exact
//!   shape is asserted AGAINST below.
//! - Unbadged and ADMITTED, non-vacuously: the core under test has an EMPTY
//!   badge book, NO substrate and NO root key, so the only thing that can
//!   answer is the read path itself (the substrate-less fall-through, the
//!   DR-055 board's admit-leg pattern). Real folded data coming back proves
//!   the door was reached and is open.
//! - Keyed by worktree, NEVER by run: a `run`-keyed call is refused at args
//!   (worktree is required), and a run ULID planted in the worktree slot joins
//!   to NOTHING (DR-057 §Decision 3; DR-049 ruled the correlation join
//!   UNSOUND).
//!
//! ## What DR-057 left open, disclosed rather than guessed
//!
//! The record does not rule the unknown-worktree answer (refusal code vs a
//! machine-readable miss body — both live on this surface: `gate_explain`
//! refuses `gate.no_verdict`, the dossier resource answers a miss BODY with
//! `run.unknown`). The unknown-tree test below pins the MECHANISM exactly
//! (machine-readable `code`, never a badge code, never a fabricated row) and
//! pins the code only negatively. When a code is ruled, one assertion
//! tightens; nothing else moves.
//!
//! ## RED MODE (against the tree at cut time — post-`1094f40`)
//!
//! ASSERT-RED: `diff_view` is an unknown tool, so `tools_call` answers
//! JSON-RPC -32602 and `util::call_ok`'s "expected a result" panic fires in
//! every test (and `find_tool` panics in the run-keyed test). All red for the
//! right reason: the tool does not exist.

mod util;

use rezidnt_types::Event;
use serde_json::{Value, json};

/// The known entities of the committed `s4_verified_run.jsonl` golden: one
/// verified run and one allocated+merged worktree whose diff ref is pinned in
/// the fixture line itself.
const S4_RUN: &str = "01S4VER1F1ED00000000000R01";
const S4_WORKTREE: &str = "/tmp/rezidnt-s4/impl";
const S4_HASH: &str = "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e";

/// A diff-less allocated tree, published inline (no fixture carries this
/// shape alone).
const BARE_WT: &str = "/repos/demo/wt-bare";

fn evt(id: &str, subject: &str, payload: Value) -> Event {
    serde_json::from_value(json!({
        "id": id,
        "ts": "2026-07-26T09:00:00Z",
        "v": 1,
        "source": "git-adapter",
        "subject": subject,
        "correlation": "01DR057D1FFV1EW000000000C0",
        "payload": payload,
    }))
    .expect("test event must parse")
}

/// The s4 fixture's diff ref as `diff_view` must serve it: the full triple,
/// nothing added, nothing dropped.
fn s4_ref() -> Value {
    json!({"hash": S4_HASH, "bytes": 23, "mime": "text/plain"})
}

/// The `Default`-fabrication shape the honesty leg forbids.
fn fabricated_empty_ref() -> Value {
    json!({"hash": "", "bytes": 0, "mime": ""})
}

/// FULL-REF + ADMIT LEG — an unbadged `diff_view` on a bare core (empty badge
/// book, no substrate, no root key) serves the merged tree's row with the
/// COMPLETE `CasRef` verbatim from the folded log. Non-vacuity: nothing but
/// the read path exists on this core, so the served triple proves the
/// unbadged door is open and reached (DR-057 §Decision 4; the DR-055 board's
/// admit-leg pattern, substrate-less here because a read needs no substrate).
#[tokio::test]
async fn an_unbadged_call_serves_the_full_ref_for_the_merged_tree() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "s4_verified_run.jsonl");

    // NO badge argument, deliberately: read-class, unbadged.
    let result = util::tool_call(&core, 1, "diff_view", json!({"worktree": S4_WORKTREE})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "diff_view is a read; an unbadged call is ADMITTED (DR-057 §Decision 4): {result:#}"
    );

    let payload = util::tool_payload(&result);
    assert_eq!(
        payload["worktree"],
        json!(S4_WORKTREE),
        "the row echoes its worktree key: {payload:#}"
    );
    assert_eq!(
        payload["lifecycle"],
        json!("allocated"),
        "lifecycle verbatim from the fold (s4: allocated, merged, not released): {payload:#}"
    );
    assert_eq!(
        payload["outcome"],
        json!("merged"),
        "outcome verbatim from the fold: {payload:#}"
    );
    assert_eq!(
        payload["diff"],
        s4_ref(),
        "diff is the FULL CasRef from the folded diff.ready/diff.merged — \
         hash AND bytes AND mime, verbatim; a hash-only or spliced ref is the \
         defect DR-057 §Decision 1 retires: {payload:#}"
    );
}

/// THE HONESTY LEG (DR-057 §Decision 1, judged hard) — a tree the log knows
/// but with NO diff-bearing fact serves `diff: null`, NEVER a fabricated
/// empty `CasRef`. The row itself is real (lifecycle serves), so a null diff
/// here is the honest "no diff yet", not unknown-tree fallout.
#[tokio::test]
async fn a_diffless_tree_serves_null_never_a_fabricated_ref() {
    let (_dir, core) = util::core();
    let event = evt(
        "01DR057D1FFV1EW000000000E1",
        "worktree.allocated",
        json!({"path": BARE_WT, "branch": "feat/bare", "allocator": "rezidnt"}),
    );
    core.fabric().publish(event).expect("publish allocation");

    let result = util::tool_call(&core, 2, "diff_view", json!({"worktree": BARE_WT})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "a known-but-diffless tree is a valid row, not a refusal: {result:#}"
    );

    let payload = util::tool_payload(&result);
    assert_eq!(
        payload["lifecycle"],
        json!("allocated"),
        "the row is REAL — null-diff must not be unknown-tree fallout: {payload:#}"
    );
    assert!(
        payload["diff"].is_null(),
        "no diff.ready/diff.merged has folded for this tree, so diff is NULL \
         (DR-057 §Decision 1): {payload:#}"
    );
    assert_ne!(
        payload["diff"],
        fabricated_empty_ref(),
        "never a Default-fabricated empty CasRef: {payload:#}"
    );
    assert!(
        !payload["diff"].is_object() && !payload["diff"].is_string(),
        "diff must not be a fabricated object or a bare hash string — the \
         contract is CasRef | null: {payload:#}"
    );
}

/// UNKNOWN TREE — a worktree the log has never seen is answered with a
/// machine-readable `code` and NO fabricated row. Mechanism pinned exactly;
/// the code pinned only negatively (see module header: the exact code/shape
/// is a disclosed DR-057 gap, not this board's to mint).
#[tokio::test]
async fn an_unknown_tree_is_never_answered_with_a_fabricated_row() {
    let (_dir, core) = util::core();
    let result = util::tool_call(&core, 3, "diff_view", json!({"worktree": "/no/such/tree"})).await;
    let payload = util::tool_payload(&result);

    let code = payload["code"].as_str().unwrap_or_else(|| {
        panic!(
            "an unknown tree must answer a machine-readable code (refusal or \
             miss body — either mechanism), never a fabricated row: {result:#}"
        )
    });
    assert!(!code.is_empty(), "the code is non-empty");
    assert!(
        code != rezidnt_mcp::codes::BADGE_REQUIRED && code != rezidnt_mcp::codes::BADGE_INVALID,
        "an unbadged READ never fails on badges — the answer must name the \
         missing tree, not the absent badge (I6: a refusal never misstates \
         why); got {code:?}"
    );
    assert!(
        payload["lifecycle"].as_str().is_none_or(str::is_empty),
        "no fabricated lifecycle on a tree the log never folded (a Default \
         row's empty lifecycle is already dead above: it carries no code): {payload:#}"
    );
    assert!(
        !payload["diff"].is_object(),
        "no fabricated diff ref on an unknown tree: {payload:#}"
    );
}

/// NOT RUN-KEYED (DR-057 §Decision 3) — the tool's only key is the worktree
/// path. A `run`-keyed call is refused at args (worktree required), and a run
/// ULID planted in the worktree slot joins to NOTHING: the s4 run exists on
/// the log, its diff exists on the log, and the answer still must not be that
/// diff — `RunRow` carries no worktree reference, so there is nothing sound
/// to join on.
#[tokio::test]
async fn diff_view_is_not_run_keyed() {
    let (_dir, core) = util::core();
    util::seed_fixture(&core, "s4_verified_run.jsonl");

    // Guard first: the tool must EXIST for the refusal legs to mean anything
    // (today the same -32602 would come from "unknown tool" — a vacuous pass
    // this find_tool forecloses).
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "diff_view");

    // Leg 1 — run-keyed args are an args-level refusal: worktree is required.
    let response = core
        .handle(util::rpc(
            4,
            "tools/call",
            json!({"name": "diff_view", "arguments": {"run": S4_RUN}}),
        ))
        .await
        .expect("requests get a response");
    assert_eq!(
        response["error"]["code"],
        json!(-32602),
        "a run-keyed call is refused at args — diff_view has no run key \
         (DR-057 §Decision 3): {response:#}"
    );

    // Leg 2 — a run ULID in the worktree slot joins to nothing: never the
    // s4 diff, however the miss is answered.
    let result = util::tool_call(&core, 5, "diff_view", json!({"worktree": S4_RUN})).await;
    let payload = util::tool_payload(&result);
    assert_ne!(
        payload["diff"],
        s4_ref(),
        "a run id must NEVER resolve to that run's diff — there is no sound \
         run→worktree join (DR-057 §Decision 3, DR-049): {payload:#}"
    );
    assert!(
        !payload["diff"].is_object(),
        "a run id in the worktree slot is a miss, not a row: {payload:#}"
    );
}
