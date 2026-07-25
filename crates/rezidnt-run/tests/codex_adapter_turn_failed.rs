//! ORACLE red board: the codex FAILING-TURN arm, replayed from a REAL recorded
//! `codex exec --json` transcript
//! (`spec/fixtures/transcripts/codex_exec_v0.145.0_turn_failed.jsonl`,
//! codex-cli 0.145.0, thread `019f99d8-…`, captured 2026-07-25 via a bogus
//! `-m` model — see the fixtures README for provenance). Zero network.
//!
//! Provenance of the criteria: DR-050 §Decision 3 left the codex
//! `turn.completed` → `status:"success"` mapping FLAGGED, NOT SETTLED — the
//! implementer read `turn.completed` as the harness positively asserting the
//! turn finished; the auditor held it asserts only TERMINATION while `status`
//! is the ontology's OUTCOME, and the evidence then was consistent with the
//! mapping without establishing it. The owner directed that a failing
//! transcript be recorded before any Trials scoring path trusts a codex
//! `"success"`. This recording settles it in the implementer's favour: a turn
//! that ends badly emits a top-level `error` line then `turn.failed`, and does
//! NOT emit `turn.completed`. These tests convert that judgment call into
//! tested arms.
//!
//! Contract pinned here (from the recording, not from memory):
//!
//! 1. `turn.failed` → `agent.completed` with `status:"error"` — the run
//!    terminated, and it terminated badly. Today `turn.failed` is entirely
//!    unmapped, so a failed codex run produces NO completion fact from the
//!    adapter at all; the daemon's exit-status fallback was papering over
//!    this gap.
//! 2. The recorded `error.message` rides the completion fact VERBATIM at
//!    `payload.error.message`, mirroring the nesting the recording itself
//!    carries. Verbatim because the string's inner JSON is the upstream
//!    provider's 400-response encoding riding as text — not a codex stream
//!    contract — and a differently-caused failure (network, interrupt) may
//!    carry plain prose there. Parsing it would pin structure the recording
//!    never promised.
//! 3. The failing recording contains NO `turn.completed`, so `turn.completed`
//!    genuinely means the turn completed — the success mapping is
//!    evidence-backed, no longer an inference.
//! 4. `turn.failed` carries NO `usage` object, and the completion fact must
//!    represent that as ABSENCE, not zero: DR-048 slice C collates these
//!    costs into a leaderboard, and a zero reads as a free run. (This is
//!    deliberately distinct from `total_usd`/`duration_ms`, which the codex
//!    format never carries for ANY outcome and which stay honest zeros per
//!    the house convention.)
//! 5. The two `item.completed` items of type `error` remain tolerated
//!    unmapped noise. This failing run carries two of them (a model-metadata
//!    warning and the same skills-context notice the SUCCESSFUL probe
//!    carries) alongside a turn that genuinely failed — which is what proves
//!    they are noise, not outcome signals.
//!
//! RED MODE: this file compiles against today's tree and every test fails on
//! ASSERTIONS — `turn.failed` currently falls into the tolerated-noise arm,
//! so the driven stream yields no completion fact and every `expect`/`assert`
//! below that demands one goes red. (The noise property of pin 5 already
//! holds today; its test is red only because the subject-sequence pin also
//! demands the missing completion — stated so the failure reads honestly.)

use rezidnt_run::RunId;
use rezidnt_run::adapter::{AgentSubstrate, CodexAdapter, MappedFact};
use serde_json::Value;
use ulid::Ulid;

const FAILED: &str = "codex_exec_v0.145.0_turn_failed.jsonl";
const FAILED_THREAD: &str = "019f99d8-b8a6-7901-ba7c-81273344d294";

/// The recorded `turn.failed` `error.message`, verbatim — a JSON-encoded
/// upstream 400 riding as a string. Pinned as an opaque string on purpose
/// (see pin 2 in the header).
const RECORDED_ERROR_MESSAGE: &str = r#"{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'definitely-not-a-real-model-xyz' model is not supported when using Codex with a ChatGPT account."}}"#;

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/transcripts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"))
}

/// Driven as `&mut dyn AgentSubstrate`, same as the success-transcript tests:
/// the failing arm must be reachable through the seam the daemon will hold.
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

fn completion(facts: &[MappedFact]) -> &MappedFact {
    let completions: Vec<&MappedFact> = facts
        .iter()
        .filter(|f| f.subject == "agent.completed")
        .collect();
    assert_eq!(
        completions.len(),
        1,
        "a failed codex run must yield exactly one run-terminal fact; \
         today `turn.failed` is unmapped and yields none"
    );
    completions[0]
}

/// Pin 1: `turn.failed` is a run-terminal fact — `agent.completed` with
/// `status:"error"` (the same error vocabulary `ClaudeCodeAdapter::map_result`
/// uses; no new subject, no new status word). The resume identity captured
/// from `thread.started` rides the completion, at parity with the success arm.
#[test]
fn turn_failed_maps_to_completion_with_status_error() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(50, 30))));
    let facts = drive(boxed.as_mut(), &fixture(FAILED));

    let done = completion(&facts);
    assert_eq!(
        done.payload["status"], "error",
        "a turn that ended badly must not be silent and must not be success"
    );
    assert_eq!(
        done.payload["session_id"], FAILED_THREAD,
        "the resume identity rides the failed completion too"
    );
}

/// Pin 2: the recorded `error.message` is carried VERBATIM, not discarded and
/// not parsed, at `payload.error.message` — the nesting the recording itself
/// uses. The inner JSON is upstream encoding riding as text; it stays opaque.
#[test]
fn turn_failed_carries_the_recorded_error_message_verbatim() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(50, 31))));
    let facts = drive(boxed.as_mut(), &fixture(FAILED));

    let p = &completion(&facts).payload;
    assert_eq!(
        p["error"]["message"],
        Value::String(RECORDED_ERROR_MESSAGE.to_string()),
        "the failure reason must survive onto the fabric as the verbatim recorded string"
    );
}

/// Pin 3 — the test DR-050 §Decision 3 asked for. The failing recording
/// contains NO `turn.completed` line, so `turn.completed` ⇒ the turn
/// genuinely completed: the `status:"success"` mapping is evidence-backed,
/// not inferred. The auditor's mismapped-failure branch (codex emitting
/// `turn.completed` for a turn that ended badly, scoring `success` under I6)
/// is refuted for this CLI version. The red half: the failed stream must
/// still yield a run-terminal fact, and it must not be success.
#[test]
fn failed_recording_has_no_turn_completed_so_success_mapping_is_evidence_backed() {
    // Evidence half (a fact about the RECORDING): no turn.completed, and the
    // terminal line really is turn.failed.
    let transcript = fixture(FAILED);
    let types: Vec<String> = transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("fixture lines are JSON");
            v["type"]
                .as_str()
                .expect("every line carries a type")
                .to_string()
        })
        .collect();
    assert!(
        !types.iter().any(|t| t == "turn.completed"),
        "a failed codex turn must not emit turn.completed — this is the evidence \
         that converts the success mapping from inference to derivation"
    );
    assert_eq!(
        types.last().map(String::as_str),
        Some("turn.failed"),
        "the recorded failing stream terminates with turn.failed"
    );

    // Adapter half (red today): the failed stream yields exactly one
    // completion and it is NOT success.
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(50, 32))));
    let facts = drive(boxed.as_mut(), &transcript);
    assert!(
        facts.iter().all(|f| f.payload["status"] != "success"),
        "no fact from a failed stream may claim success"
    );
    let done = completion(&facts);
    assert_eq!(done.payload["status"], "error");
}

/// Pin 4: `turn.failed` carries no `usage` object — a failed codex turn
/// reports NO token accounting. The completion fact must represent that as
/// ABSENCE (no key at all — a null key is also a failure here), never as
/// zero: DR-048 slice C collates these costs into a leaderboard, and a
/// zero-token failed candidate reads as a free run. `total_usd` and
/// `duration_ms` are NOT constrained by this test — the codex format never
/// carries them for any outcome and their honest-zero convention stands.
#[test]
fn turn_failed_reports_usage_as_absence_not_zero() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(50, 33))));
    let facts = drive(boxed.as_mut(), &fixture(FAILED));

    let p = &completion(&facts).payload;
    if let Some(cost) = p.get("cost").and_then(Value::as_object) {
        for key in ["input_tokens", "output_tokens"] {
            assert!(
                !cost.contains_key(key),
                "the failed turn reported no usage; `cost.{key}` must be absent, \
                 not present as zero or null — absence is the honest representation \
                 of an unmeasured cost"
            );
        }
    }
    // A payload with no cost object at all also honestly expresses absence;
    // the shape choice belongs to the implementer.
}

/// Pin 5: the two `item.completed` items of type `error` (a model-metadata
/// warning and the same skills-context notice the SUCCESSFUL probe carries)
/// remain tolerated unmapped noise — this failing run carrying both alongside
/// a genuine failure is what proves they are noise, not outcome signals. The
/// whole failed stream maps to exactly the spawn transition and the error
/// completion: the top-level `error` line is subsumed by `turn.failed`'s
/// carried message, not minted as a fact of its own.
#[test]
fn error_items_in_failed_recording_remain_tolerated_noise() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(50, 34))));
    let facts = drive(boxed.as_mut(), &fixture(FAILED));

    let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["agent.status.changed", "agent.completed"],
        "error items and the top-level error line must produce no facts of \
         their own; the failed stream yields exactly the spawn transition and \
         the error-status completion"
    );
}
