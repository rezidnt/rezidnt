//! IMPLEMENTER-ADDED regression tests (NOT oracle criteria) — provenance:
//! the /debrief fail verdict on `e2d766f`, DR-048 slice A phase 1.
//!
//! The oracle's `codex_adapter.rs` pins the adapter's behavior against the
//! recorded transcript. This file pins the four claims the first
//! implementation ASSERTED IN PROSE but left unprovable — the silent-wrong
//! class this arc has produced repeatedly (a doc or a record naming a
//! mechanism the code lacks, with no test that could catch the gap):
//!
//! 1. the single-turn premise the run-terminal `agent.completed` mapping rests
//!    on is GUARDED, not assumed (I3);
//! 2. the codex version gate actually refuses something inside major 0 (I4);
//! 3. an `UntestedMajor` refusal carries the list it was judged against (I6);
//! 4. `agent.completed`'s shape is genuinely single-sourced across both
//!    substrates, rather than two independent literals that happen to agree;
//! 5. a codex spec declaring `[gates.permit]` is refused, not silently
//!    stripped of its PEP.
//!
//! Two further tests settle the calls DR-050's `turn.failed` work order
//! explicitly LEFT to the implementer, so neither stays a prose-only judgment:
//! whether a failed turn counts toward `num_turns`, and whether `turn.failed`
//! trips the single-shot guard.
//!
//! None of these weakens an oracle assertion; each pins behavior the oracle
//! left open or did not reach.

use rezidnt_run::RunId;
use rezidnt_run::adapter::{
    AdapterError, AgentSubstrate, ClaudeCodeAdapter, CodexAdapter, MappedFact,
    TESTED_CODEX_VERSIONS, codex_version_gate,
};
use rezidnt_run::badge::Badge;
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;
use serde_json::Value;
use ulid::Ulid;

const RECORDED_CODEX: &str = "codex_exec_v0.145.0.jsonl";
const RECORDED_CODEX_FAILED: &str = "codex_exec_v0.145.0_turn_failed.jsonl";
const RECORDED_CLAUDE: &str = "claude_code_stream_v2.1.191.jsonl";

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/transcripts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"))
}

fn drive(substrate: &mut dyn AgentSubstrate, transcript: &str) -> Vec<MappedFact> {
    transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| substrate.map_line(l).expect("recorded lines map cleanly"))
        .collect()
}

/// I3, the load-bearing one. `agent.completed` means the RUN finished and the
/// reducer folds it by overwriting the run's cost totals, so a per-turn
/// completion would report one turn's tokens as the whole run's cost — the
/// number DR-048 slice C collates into a leaderboard. `codex exec` is
/// single-shot, so stream end is run end; this test proves that premise is
/// GUARDED rather than assumed. A second turn-terminal line must refuse, not
/// emit a second "the run finished" fact and not silently drop its tokens.
#[test]
fn a_second_completed_turn_refuses_instead_of_emitting_a_second_completion() {
    let mut adapter = CodexAdapter::new(RunId::new(Ulid::from_parts(48, 20)));
    let facts = drive(&mut adapter, &fixture(RECORDED_CODEX));
    assert_eq!(
        facts
            .iter()
            .filter(|f| f.subject == "agent.completed")
            .count(),
        1,
        "the recorded single-shot stream yields exactly one run-terminal fact"
    );

    let second = adapter
        .map_line(r#"{"type":"turn.completed","usage":{"input_tokens":7,"output_tokens":3}}"#);
    match second {
        Err(AdapterError::ContractViolated { harness, detail }) => {
            assert_eq!(harness, "codex");
            assert!(
                detail.contains("re-record"),
                "the refusal must say what would make it valid again: {detail}"
            );
        }
        other => panic!(
            "a second completed turn falsifies the single-shot premise and must refuse; got {other:?}"
        ),
    }
}

/// OPEN CALL (b), settled: `turn.failed` DOES count against the single-shot
/// guard, because the guard counts run-TERMINAL lines rather than successes.
/// Two terminal lines on one stream leave the run's OUTCOME as ambiguous as
/// two successes leave its TOTALS — which terminal line is the run's verdict?
/// — and that is the same defect, so it earns the same refusal. Pinned in both
/// orders so the policy cannot quietly become outcome-sensitive.
#[test]
fn a_terminal_line_of_either_outcome_trips_the_single_shot_guard() {
    let failed_line = r#"{"type":"turn.failed","error":{"message":"boom"}}"#;
    let completed_line =
        r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}"#;

    // success recording, then a failure line.
    let mut after_success = CodexAdapter::new(RunId::new(Ulid::from_parts(50, 40)));
    drive(&mut after_success, &fixture(RECORDED_CODEX));
    match after_success.map_line(failed_line) {
        Err(AdapterError::ContractViolated { harness, .. }) => assert_eq!(harness, "codex"),
        other => panic!("turn.failed after a terminal line must refuse; got {other:?}"),
    }

    // failure recording, then a success line.
    let mut after_failure = CodexAdapter::new(RunId::new(Ulid::from_parts(50, 41)));
    drive(&mut after_failure, &fixture(RECORDED_CODEX_FAILED));
    match after_failure.map_line(completed_line) {
        Err(AdapterError::ContractViolated { harness, .. }) => assert_eq!(harness, "codex"),
        other => panic!("turn.completed after a terminal line must refuse; got {other:?}"),
    }
}

/// OPEN CALL (a), settled: a failed turn COUNTS toward `num_turns`. The stream
/// positively shows one `turn.started` and one terminal line, so one turn was
/// taken — the count is observed, not inferred, and reporting 0 would deny a
/// turn the recording shows happening. This is deliberately the opposite call
/// from `usage` on the same fact, and the contrast is the point: absent tokens
/// mean "never measured", while `num_turns: 1` means "measured, and it was
/// one". Pinned together so the distinction cannot silently collapse.
#[test]
fn a_failed_turn_counts_toward_num_turns_while_its_tokens_stay_absent() {
    let mut adapter = CodexAdapter::new(RunId::new(Ulid::from_parts(50, 42)));
    let facts = drive(&mut adapter, &fixture(RECORDED_CODEX_FAILED));
    let done = facts
        .iter()
        .find(|f| f.subject == "agent.completed")
        .expect("the failed run yields a completion fact");

    assert_eq!(
        done.payload["num_turns"], 1,
        "a turn that started and terminated is a turn the run took"
    );
    let cost = done.payload["cost"]
        .as_object()
        .expect("cost object is present even when tokens are not");
    assert!(
        !cost.contains_key("input_tokens") && !cost.contains_key("output_tokens"),
        "tokens the harness never measured stay absent on the same fact that counts the turn"
    );
}

/// I4. `TESTED_CODEX_MAJORS == [0]` alone accepts every future 0.x codex
/// release against a contract recorded at 0.145.0 — semver's 0.y.z rule makes
/// MINOR the breaking axis below 1.0, so major-depth gating is vacuous here.
/// The recorded version passes; a different minor INSIDE the tested major
/// refuses with both the version and the tested list named.
#[test]
fn codex_gate_refuses_an_untested_minor_inside_the_tested_major() {
    codex_version_gate("0.145.0").expect("the recorded version must pass");
    codex_version_gate("0.145.7").expect("a patch of the recorded minor must pass");

    match codex_version_gate("0.200.0") {
        Err(AdapterError::UntestedMinor {
            major: 0,
            minor: 200,
            tested,
        }) => {
            assert_eq!(
                tested, TESTED_CODEX_VERSIONS,
                "the refusal must carry the list it judged against (I6 interrogability)"
            );
        }
        other => panic!("an untested 0.x minor must refuse; got {other:?}"),
    }
}

/// I6. A refusal that does not say what it judged against cannot answer "why
/// blocked". The tested list rides the error rather than being named in the
/// message text, because the variant is shared across substrates that own
/// different lists.
#[test]
fn untested_major_refusal_carries_the_list_it_judged_against() {
    let codex: Box<dyn AgentSubstrate> = Box::new(CodexAdapter::new(RunId::default()));
    match codex.version_gate("9.0.0") {
        Err(AdapterError::UntestedMajor { major: 9, tested }) => {
            assert_eq!(tested, &[0], "codex's own tested majors, not claude-code's");
        }
        other => panic!("expected UntestedMajor carrying codex's list; got {other:?}"),
    }

    let claude: Box<dyn AgentSubstrate> = Box::new(ClaudeCodeAdapter::new(RunId::default()));
    match claude.version_gate("9.0.0") {
        Err(AdapterError::UntestedMajor { major: 9, tested }) => {
            assert_eq!(tested, &[2], "claude-code's own tested majors");
        }
        other => panic!("expected UntestedMajor carrying claude-code's list; got {other:?}"),
    }
}

/// The `agent.completed` fact is rendered from ONE payload literal shared by
/// every substrate, so its key set cannot drift as harnesses are added. Pinned
/// by test and not only by construction: the previous implementation built two
/// independent `json!` literals that merely happened to agree, and nothing
/// would have caught them diverging.
#[test]
fn completion_fact_shape_is_single_sourced_across_substrates() {
    fn completion_payload(substrate: &mut dyn AgentSubstrate, transcript: &str) -> Value {
        drive(substrate, transcript)
            .into_iter()
            .find(|f| f.subject == "agent.completed")
            .expect("a completion fact")
            .payload
    }
    fn keys(v: &Value) -> Vec<String> {
        v.as_object()
            .expect("payload is an object")
            .keys()
            .cloned()
            .collect()
    }

    let mut claude = ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(48, 21)));
    let mut codex = CodexAdapter::new(RunId::new(Ulid::from_parts(48, 22)));
    let claude_payload = completion_payload(&mut claude, &fixture(RECORDED_CLAUDE));
    let codex_payload = completion_payload(&mut codex, &fixture(RECORDED_CODEX));

    assert_eq!(
        keys(&claude_payload),
        keys(&codex_payload),
        "both substrates must emit the same agent.completed key set"
    );
    assert_eq!(
        keys(&claude_payload["cost"]),
        keys(&codex_payload["cost"]),
        "both substrates must emit the same cost key set"
    );
}

/// The permit PEP is claude-code's `PreToolUse` hook and has no recorded codex
/// equivalent. A codex spec that DECLARES `[gates.permit]` must be refused at
/// the plan seam — silently returning a plan would let a run be recorded as
/// permit-governed (the daemon stamps `pep = "enforced"` off the spec's gates
/// list) while running unintercepted.
#[test]
fn codex_plan_refuses_a_declared_permit_gate_rather_than_ignoring_it() {
    let badge = Badge::mint().expect("mint");
    let spec = AgentSpec {
        name: "trial-codex".into(),
        harness: "codex".into(),
        worktree: "auto".into(),
        gates: vec!["vet".into(), "permit".into()],
        ..AgentSpec::default()
    };

    let err = SpawnPlan::for_codex(&spec, &badge.token_hex(), std::iter::empty())
        .expect_err("a codex spec declaring [gates.permit] must not produce a plan");
    let msg = err.to_string();
    assert!(
        msg.contains("permit") && msg.contains("codex"),
        "the refusal must name the gate and the harness: {msg}"
    );
}
