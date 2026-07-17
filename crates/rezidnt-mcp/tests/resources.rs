//! S3 oracle — MCP resources (doc §9) and the read-only event tool.
//!
//! The dossier resource is DERIVED state (I3): folded from the log by the
//! rezidnt-state reducers, never a side store. The oracle seeds the log with
//! the committed S1 golden fixture and asserts the resource equals what the
//! reducers say.

mod util;

use serde_json::json;

const S1_RUN: &str = "01ARZ3NDEKTSV4RRFFQ69G5A01";

/// Exit criterion (c): reading a run's dossier over MCP returns the folded
/// accounting — status, cost, tokens, session — matching the reducers'
/// verdict on the same log.
#[tokio::test]
async fn dossier_resource_serves_the_folded_run_state() {
    let (_dir, core) = util::core();
    let seeded = util::seed_fixture(&core, "s1_agent_run.jsonl");
    let expected_graph = rezidnt_state::fold(seeded.iter());
    let expected_run = &expected_graph.agent_runs[S1_RUN];

    let uri = format!("rezidnt://run/{S1_RUN}/dossier");
    let result = util::call_ok(&core, 1, "resources/read", json!({"uri": uri})).await;
    let contents = result["contents"]
        .as_array()
        .unwrap_or_else(|| panic!("resources/read result carries contents: {result:#}"));
    let text = contents[0]["text"]
        .as_str()
        .expect("dossier contents[0].text is the JSON body");
    assert_eq!(
        contents[0]["uri"],
        json!(uri),
        "contents echo the requested uri"
    );
    let dossier: serde_json::Value = serde_json::from_str(text).expect("dossier body is JSON");

    assert_eq!(dossier["status"], json!(expected_run.status));
    assert_eq!(dossier["total_usd"], json!(expected_run.total_usd));
    assert_eq!(dossier["input_tokens"], json!(expected_run.input_tokens));
    assert_eq!(dossier["output_tokens"], json!(expected_run.output_tokens));
    assert_eq!(dossier["session_id"], json!(expected_run.session_id));
}

/// An unknown run's dossier is a machine-readable `run.unknown` — never an
/// empty dossier and never a hang.
#[tokio::test]
async fn unknown_run_dossier_is_machine_readable() {
    let (_dir, core) = util::core();
    let result = util::call_ok(
        &core,
        2,
        "resources/read",
        json!({"uri": "rezidnt://run/01ARZ3NDEKTSV4RRFFQ69G5ZZZ/dossier"}),
    )
    .await;
    // Resource misses ride the same machine-readable shape as tool refusals:
    // contents[0].text parses as JSON carrying the code.
    let text = result["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("miss must still answer with contents[0].text: {result:#}"));
    let body: serde_json::Value = serde_json::from_str(text).expect("machine-readable JSON");
    assert_eq!(
        body["code"],
        json!(rezidnt_mcp::codes::RUN_UNKNOWN),
        "unknown run must name run.unknown: {body:#}"
    );
}

/// `tail_events` returns real envelopes from the log — every line reparses
/// as a `rezidnt_types::Event`, in log order, respecting `since`.
#[tokio::test]
async fn tail_events_returns_envelopes_in_log_order() {
    let (_dir, core) = util::core();
    let seeded = util::seed_fixture(&core, "s1_agent_run.jsonl");

    let result = util::tool_call(&core, 3, "tail_events", json!({})).await;
    let payload = util::tool_payload(&result);
    let events = payload["events"]
        .as_array()
        .unwrap_or_else(|| panic!("tail_events payload carries an events array: {payload:#}"));
    assert_eq!(events.len(), seeded.len(), "the whole log, absent `since`");
    for (got, want) in events.iter().zip(&seeded) {
        let reparsed = rezidnt_types::Event::from_json_line(&got.to_string())
            .expect("every returned envelope is a valid Event");
        assert_eq!(reparsed.id, want.id, "log order, verbatim envelopes");
    }

    // `since` is an exclusive ULID lower bound.
    let after_first = util::tool_call(
        &core,
        4,
        "tail_events",
        json!({"since": seeded[0].id.to_string()}),
    )
    .await;
    let payload = util::tool_payload(&after_first);
    assert_eq!(
        payload["events"].as_array().map(Vec::len),
        Some(seeded.len() - 1),
        "since=<first id> skips exactly the first event"
    );
}
