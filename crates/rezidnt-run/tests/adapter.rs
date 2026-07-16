//! S1 oracle: claude-code adapter contract, replayed from RECORDED
//! transcripts with zero network (testing-oracles skill). The real recording
//! is CLI 2.1.191; the tool_use file is docs-constructed (see the fixtures
//! README for provenance).

use rezidnt_run::RunId;
use rezidnt_run::adapter::{AdapterError, ClaudeCodeAdapter, MappedFact, version_gate};
use ulid::Ulid;

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/transcripts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"))
}

fn map_all(adapter: &mut ClaudeCodeAdapter, transcript: &str) -> Vec<MappedFact> {
    transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| {
            adapter
                .map_line(l)
                .expect("recorded lines must map cleanly")
        })
        .collect()
}

/// The REAL recorded probe: hooks and rate-limit lines are tolerated noise;
/// init/assistant/result map to their subjects in stream order.
#[test]
fn real_transcript_maps_to_expected_subject_sequence() {
    let mut adapter = ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(2, 7)));
    let facts = map_all(&mut adapter, &fixture("claude_code_stream_v2.1.191.jsonl"));

    let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
    assert_eq!(
        subjects,
        [
            "agent.status.changed", // system/init → spawning→running
            "agent.message",        // assistant text
            "agent.completed",      // result envelope
        ],
        "hook_started/hook_response/rate_limit_event lines must be tolerated, not mapped"
    );
}

/// Cost accounting (DR-001: "cost fields → dossier accounting") — the result
/// line's dollars, tokens, turns, duration, and session id all land on the
/// completion fact.
#[test]
fn result_line_cost_fields_land_on_completion_fact() {
    let mut adapter = ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(2, 8)));
    let facts = map_all(&mut adapter, &fixture("claude_code_stream_v2.1.191.jsonl"));
    let done = facts
        .iter()
        .find(|f| f.subject == "agent.completed")
        .expect("completion fact");

    let p = &done.payload;
    assert_eq!(p["cost"]["total_usd"], 0.190075);
    assert_eq!(p["cost"]["input_tokens"], 7441);
    assert_eq!(p["cost"]["output_tokens"], 45);
    assert_eq!(p["num_turns"], 1);
    assert_eq!(p["duration_ms"], 6199);
    assert_eq!(p["session_id"], "83c61e05-aecf-4c70-93f4-ada974db33df");
    assert_eq!(p["status"], "success");
}

/// Session id is captured from `system/init` for run checkpointing
/// (`--resume <session_id>`, DR-001).
#[test]
fn session_id_is_captured_from_init() {
    let mut adapter = ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(2, 9)));
    assert_eq!(adapter.session_id(), None);
    map_all(&mut adapter, &fixture("claude_code_stream_v2.1.191.jsonl"));
    assert_eq!(
        adapter.session_id(),
        Some("83c61e05-aecf-4c70-93f4-ada974db33df")
    );
}

/// tool_use content blocks map to `agent.tool.invoked`, one fact per block,
/// carrying the tool name (docs-constructed fixture — see README).
#[test]
fn tool_use_blocks_map_to_agent_tool_invoked() {
    let mut adapter = ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(2, 10)));
    let facts = map_all(&mut adapter, &fixture("claude_code_stream_tool_use.jsonl"));

    let invocations: Vec<_> = facts
        .iter()
        .filter(|f| f.subject == "agent.tool.invoked")
        .collect();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].payload["tool"], "Bash");

    let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
    assert_eq!(
        subjects,
        [
            "agent.status.changed",
            "agent.tool.invoked",
            "agent.message",
            "agent.completed"
        ]
    );
}

/// Additive evolution: a line type this adapter has never seen is tolerated
/// (no facts, no error). Malformed JSON is an honest error, never a panic.
#[test]
fn unknown_line_types_tolerated_malformed_json_errors() {
    let mut adapter = ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(2, 11)));
    let facts = adapter
        .map_line(r#"{"type":"totally_new_future_line_kind","data":{"x":1}}"#)
        .expect("unknown types are not errors");
    assert!(facts.is_empty());

    match adapter.map_line("this is not json {") {
        Err(AdapterError::BadLine(_)) => {}
        other => panic!("malformed input must be AdapterError::BadLine, got {other:?}"),
    }
}

/// The version gate refuses an untested major with a machine-readable error;
/// the recorded major passes.
#[test]
fn version_gate_accepts_tested_major_refuses_untested() {
    version_gate("2.1.191").expect("recorded major must pass");
    match version_gate("3.0.0") {
        Err(AdapterError::UntestedMajor { major: 3 }) => {}
        other => panic!("untested major must refuse, got {other:?}"),
    }
    match version_gate("not-a-version") {
        Err(AdapterError::BadVersion { .. }) => {}
        other => panic!("garbage version must be BadVersion, got {other:?}"),
    }
}
