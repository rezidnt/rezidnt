//! DR-048 slice A oracle: codex CLI adapter contract, replayed from a REAL
//! recorded `codex exec --json` transcript (codex-cli 0.145.0, thread
//! `019f99a4-…`, captured 2026-07-25 — see the fixtures README for
//! provenance). Zero network: the second `AgentSubstrate` impl is proven
//! against the recording, exactly the house pattern the claude-code adapter
//! set.
//!
//! Contract these tests pin (from the recorded stream, not from memory):
//!
//! `thread.started` → `agent.status.changed` (spawning→running); the carried
//! `thread_id` is the resume identity and is captured as the session id.
//! `item.completed` with item type `agent_message` → `agent.message` carrying
//! the text. `turn.completed` → `agent.completed` carrying the `usage` token
//! counts in the SAME payload shape `ClaudeCodeAdapter` emits. Everything
//! else on the recorded stream (`turn.started`, the machine-local `error`
//! item) is tolerated noise: no facts, no error. The codex format carries NO
//! `duration_ms` and NO dollar cost; per the house zero-default convention
//! (see `ClaudeCodeAdapter::map_result`) those land as 0, never as a missing
//! field and never as a fabricated number.
//!
//! RED MODE: compile-red today. Neither `CodexAdapter` nor `AgentSubstrate`
//! exists in `rezidnt_run::adapter` — this file does not compile until slice
//! A phase 1 lands. That IS the failing state.

use rezidnt_run::RunId;
use rezidnt_run::adapter::{AdapterError, AgentSubstrate, CodexAdapter, MappedFact};
use ulid::Ulid;

const RECORDED: &str = "codex_exec_v0.145.0.jsonl";
const RECORDED_THREAD: &str = "019f99a4-8953-7d22-9ef0-6091937f8f72";

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/transcripts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"))
}

/// Deliberately `&mut dyn AgentSubstrate`: the codex adapter is only proof
/// the seam is a seam if it is driven through the same trait object shape the
/// daemon will hold for claude-code.
fn drive(substrate: &mut dyn AgentSubstrate, transcript: &str) -> Vec<MappedFact> {
    transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| {
            substrate
                .map_line(l)
                .expect("recorded lines must map cleanly")
        })
        .collect()
}

/// The recorded stream maps to the SAME subject sequence the claude-code
/// probe pins: spawn transition, message, completion. `turn.started` and the
/// `error` item are tolerated noise, never mapped, never an error.
#[test]
fn real_codex_transcript_maps_to_expected_subject_sequence() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(48, 10))));
    let facts = drive(boxed.as_mut(), &fixture(RECORDED));

    let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["agent.status.changed", "agent.message", "agent.completed"],
        "turn.started and the error item must be tolerated, not mapped"
    );
}

/// `thread.started` is the spawn fact: spawning→running, and the thread id
/// (the `codex exec resume` identity) is captured as the session id —
/// readable through the trait, mirroring claude-code's `system/init` capture.
#[test]
fn thread_started_maps_to_running_and_captures_thread_id() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(48, 11))));
    assert_eq!(boxed.session_id(), None);
    let facts = drive(boxed.as_mut(), &fixture(RECORDED));

    let status = &facts[0];
    assert_eq!(status.subject, "agent.status.changed");
    assert_eq!(status.payload["from"], "spawning");
    assert_eq!(status.payload["to"], "running");
    assert_eq!(boxed.session_id(), Some(RECORDED_THREAD));
}

/// The `agent_message` item carries the probe text verbatim in the same
/// payload shape claude-code emits (`role` + `text`).
#[test]
fn agent_message_item_maps_to_agent_message_fact() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(48, 12))));
    let facts = drive(boxed.as_mut(), &fixture(RECORDED));

    let msg = facts
        .iter()
        .find(|f| f.subject == "agent.message")
        .expect("message fact");
    assert_eq!(msg.payload["role"], "assistant");
    assert_eq!(msg.payload["text"], "rezidnt codex transcript probe");
}

/// Dossier accounting (DR-048: tokens are collated in slice C): the recorded
/// `turn.completed` usage lands on `agent.completed` in the claude-code
/// payload shape. Fields the codex format does not carry — dollar cost,
/// `duration_ms` — are honest zeros (the house zero-default convention), and
/// `num_turns` is the deterministic count of completed turns observed on the
/// stream (the format carries no aggregate; 1 in this recording). The session
/// id rides the completion fact for resume/checkpoint parity.
#[test]
fn turn_completed_carries_tokens_with_zero_defaults_for_absent_fields() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(48, 13))));
    let facts = drive(boxed.as_mut(), &fixture(RECORDED));
    let done = facts
        .iter()
        .find(|f| f.subject == "agent.completed")
        .expect("completion fact");

    let p = &done.payload;
    assert_eq!(p["status"], "success");
    assert_eq!(p["cost"]["input_tokens"], 26184);
    assert_eq!(p["cost"]["output_tokens"], 11);
    assert_eq!(
        p["cost"]["total_usd"], 0,
        "codex carries no dollar cost — honest zero"
    );
    assert_eq!(
        p["duration_ms"], 0,
        "codex carries no duration — honest zero"
    );
    assert_eq!(p["num_turns"], 1);
    assert_eq!(p["session_id"], RECORDED_THREAD);
}

/// Additive evolution, same bar as claude-code: an item/line type this
/// adapter has never seen is tolerated (no facts, no error); malformed JSON
/// is an honest `BadLine`, never a panic.
#[test]
fn unknown_line_types_tolerated_malformed_json_errors() {
    let mut adapter = CodexAdapter::new(RunId::new(Ulid::from_parts(48, 14)));
    let facts = adapter
        .map_line(r#"{"type":"totally_new_future_event_kind","data":{"x":1}}"#)
        .expect("unknown types are not errors");
    assert!(facts.is_empty());

    match adapter.map_line("this is not json {") {
        Err(AdapterError::BadLine(_)) => {}
        other => panic!("malformed input must be AdapterError::BadLine, got {other:?}"),
    }
}

/// The version gate covers the second substrate: the RECORDED codex-cli
/// version passes; a clearly-untested major refuses machine-readably; garbage
/// is `BadVersion`. (The recording is codex-cli 0.145.0 — major 0. Whether
/// the implementer gates finer than major inside the 0.x line is an open
/// call; this test only requires that the recorded version passes and an
/// untested major refuses.)
#[test]
fn codex_version_gate_accepts_recorded_refuses_untested() {
    let adapter: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(48, 15))));

    adapter
        .version_gate("0.145.0")
        .expect("the recorded codex-cli version must pass");
    match adapter.version_gate("9.0.0") {
        Err(AdapterError::UntestedMajor { major: 9 }) => {}
        other => panic!("untested major must refuse, got {other:?}"),
    }
    match adapter.version_gate("not-a-version") {
        Err(AdapterError::BadVersion { .. }) => {}
        other => panic!("garbage version must be BadVersion, got {other:?}"),
    }
}
