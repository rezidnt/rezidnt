//! DR-040 oracle (C2) — `get_escalations` result == the pure projection.
//!
//! Criterion (I3: the tool re-interprets nothing): `get_escalations` returns
//! exactly `rezidnt_state::escalations(&rezidnt_state::fold(&events), filter)`
//! over the same log. There is no second derivation — the served payload
//! deserializes to a `Vec<rezidnt_state::EscalationRow>` that EQUALS the pure
//! fold-then-project of the seeded events. Seeds the committed
//! `s5b_board_permit.jsonl` golden fixture (one agent run + a spread of permit
//! decisions: one granted, one denied, and ONE OUTSTANDING escalation — request
//! `01S5BB0ARDPERMFXTRERQ003`, reason "cumulative spend crossed soft cap" — that
//! is never resolved), calls the tool through the server core, and asserts
//! projection equality plus the run FILTER semantics.
//!
//! RED MODE — intended reds, all "missing type/tool/projection", not typos:
//! - `rezidnt_state::escalations` / `rezidnt_state::EscalationRow` do NOT exist
//!   yet: DR-040 Decision 2/3 adds the pure projection + the view type in
//!   `rezidnt-state`. Until they land, this file fails to COMPILE (unresolved
//!   path). That red is the projection work order.
//! - `rezidnt_types::mcp::GetEscalationsArgs` does not exist yet (used for the
//!   run filter arg shape), a second compile red.
//! - `get_escalations` is not advertised/dispatched yet, so once it compiles the
//!   `tool_call` assertion goes red until the tool is served.

mod util;

use serde_json::json;

/// The run whose escalations the s5b fixture folds. Pinned so the filter tests
/// are specific: this is the run that owns the outstanding escalation.
const S5B_RUN: &str = "01S5BB0ARDPERMFXTRE000RN01";
/// The OUTSTANDING escalation's request_id (permit.escalated, never resolved).
const S5B_ESCALATED_REQ: &str = "01S5BB0ARDPERMFXTRERQ003";
/// A run id NOT present in the fixture — its filter must return empty.
const ABSENT_RUN: &str = "01ABSENTRUN000000000000R99";

/// The tool result IS the pure projection of the folded log — no
/// re-interpretation (DR-040 Decision 2, I3). Fold the seeded events with the
/// real reducers, project with the pure `escalations(.., None)` projection, and
/// assert the served `get_escalations` payload deserializes to the identical
/// `Vec<EscalationRow>`.
#[tokio::test]
async fn get_escalations_equals_pure_projection_of_fold() {
    let (_dir, core) = util::core();
    // The s5b fixture folds to one run with a granted, a denied, a pending, and
    // exactly ONE outstanding escalation — enough permit spread that a projection
    // filtering on decision == Some("escalated") could never match by accident.
    let seeded = util::seed_fixture(&core, "s5b_board_permit.jsonl");

    // The oracle's ground truth: the pure fold-then-project. This is the exact
    // pipeline DR-040 pins the tool to (`fold` then `escalations`), computed here
    // independently of the tool so equality means "the tool re-derives nothing".
    let expected = rezidnt_state::escalations(&rezidnt_state::fold(seeded.iter()), None);

    // The tool takes `GetEscalationsArgs { run: Option<String> }`; absent run =
    // all escalations across the fleet (full fold), mirroring board_view.
    let result = util::tool_call(&core, 1, "get_escalations", json!({})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "get_escalations is a read; it must not error: {result:#}"
    );

    let payload = util::tool_payload(&result);
    let served: Vec<rezidnt_state::EscalationRow> = serde_json::from_value(payload.clone())
        .unwrap_or_else(|e| {
            panic!("get_escalations payload must deserialize to a Vec<EscalationRow> ({e}): {payload:#}")
        });

    assert_eq!(
        served, expected,
        "get_escalations result MUST EQUAL rezidnt_state::escalations(&fold(&events), None) — \
         the tool is exactly the pure projection, it re-interprets nothing (I3)"
    );

    // Non-vacuity guard: the fixture has exactly one OUTSTANDING escalation
    // (the granted/denied ledger entries and the pending one must NOT surface).
    // A matching empty Vec would be an oracle bug, not a pass.
    assert_eq!(
        served.len(),
        1,
        "s5b folds to exactly ONE outstanding escalation (granted/denied/pending must not surface): {served:#?}"
    );
    let row = &served[0];
    assert_eq!(row.run, S5B_RUN, "the escalation is on the seeded run");
    assert_eq!(
        row.request_id, S5B_ESCALATED_REQ,
        "the escalation carries its request_id"
    );
    assert_eq!(
        row.action, "tool.invoke",
        "the requested action folds verbatim onto the row"
    );
    assert_eq!(
        row.reason.as_deref(),
        Some("cumulative spend crossed soft cap"),
        "the escalation reason surfaces verbatim (I6 — interrogable, never coerced)"
    );
    assert!(
        row.policy_ref.is_some(),
        "the deciding policy_ref folds so the escalation is interrogable (I6): {row:#?}"
    );
}

/// DR-040 Decision 1: the `run` filter. Calling with `{run: <that run>}` returns
/// only that run's escalations — EQUAL to the pure projection filtered to the
/// same run — and calling with an absent run returns an EMPTY array. The tool's
/// filter is exactly the projection's `filter: Option<&str>` argument (I3).
#[tokio::test]
async fn get_escalations_run_filter_scopes_to_one_run() {
    let (_dir, core) = util::core();
    let seeded = util::seed_fixture(&core, "s5b_board_permit.jsonl");
    let graph = rezidnt_state::fold(seeded.iter());

    // Filter to the run that owns the escalation: equals the pure projection
    // filtered to that same run, and is the same single row as the unfiltered
    // read (the fixture has exactly one run with an escalation).
    let expected_this_run = rezidnt_state::escalations(&graph, Some(S5B_RUN));
    let result = util::tool_call(&core, 2, "get_escalations", json!({"run": S5B_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "get_escalations is a read; it must not error: {result:#}"
    );
    let served_this_run: Vec<rezidnt_state::EscalationRow> =
        serde_json::from_value(util::tool_payload(&result)).expect("Vec<EscalationRow>");
    assert_eq!(
        served_this_run, expected_this_run,
        "{{run: <that run>}} MUST EQUAL escalations(&graph, Some(run)) — the filter is the \
         projection's own filter (I3)"
    );
    assert_eq!(
        served_this_run.len(),
        1,
        "the seeded run owns exactly one outstanding escalation: {served_this_run:#?}"
    );
    assert_eq!(served_this_run[0].request_id, S5B_ESCALATED_REQ);

    // Filter to a run NOT in the log: empty, matching the pure projection.
    let expected_absent = rezidnt_state::escalations(&graph, Some(ABSENT_RUN));
    assert!(
        expected_absent.is_empty(),
        "sanity: the pure projection returns empty for an absent run"
    );
    let result = util::tool_call(&core, 3, "get_escalations", json!({"run": ABSENT_RUN})).await;
    assert_ne!(
        result["isError"],
        json!(true),
        "get_escalations is a read; it must not error even for an empty filter: {result:#}"
    );
    let served_absent: Vec<rezidnt_state::EscalationRow> =
        serde_json::from_value(util::tool_payload(&result)).expect("Vec<EscalationRow>");
    assert!(
        served_absent.is_empty(),
        "{{run: <absent run>}} returns an empty array — no escalations for a run not in the log: {served_absent:#?}"
    );
}
