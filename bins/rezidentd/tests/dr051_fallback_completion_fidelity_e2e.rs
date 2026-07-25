//! TRIALS-SLICE-B ENTRY ORACLE — criterion (c) of DR-050 §Decision 2, as
//! SHARPENED by DR-051 §Decision 4: the daemon's fallback `agent.completed`
//! literal (published when a child dies without a result line) must be pinned
//! by a CROSS-CRATE test against `Completion::into_fact`'s FAILURE-shaped
//! output — a run that terminated with NO usage reported — and must carry the
//! error text the dying child's last stream line reported, rather than
//! discarding the failure reason into a bare zeroed literal.
//!
//! WHY NOT A KEY-SET TEST OVER SUCCESS PAYLOADS (DR-051 §Decision 4, the whole
//! reason this file compares failure shapes): a key-set-equality test over the
//! two SUCCESS payloads would pass today while the fallback keeps emitting
//! `{"input_tokens": 0, "output_tokens": 0}` — same keys, opposite meaning.
//! `0` says "measured, and it was nothing"; the fallback's `0` says that about
//! a run that was NEVER measured. The ontology's `agent.completed` v1 cost
//! bullet now RATIFIES "a failed candidate's cost is ABSENT, not zero" (the
//! token keys are omitted, present-or-absent together), so those zeros
//! contradict a ratified clause: a zero-token failed candidate reads as a FREE
//! run on the DR-048 slice-C leaderboard.
//!
//! The cross-crate reference is derived from the REAL recorded failing
//! transcript (`spec/fixtures/transcripts/codex_exec_v0.145.0_turn_failed.jsonl`)
//! driven through the public `AgentSubstrate` seam — the exact
//! `Completion::into_fact` failure rendering the daemon literal must agree
//! with, obtained from the adapter crate rather than restated here, so the two
//! crates cannot drift apart by coincidence again (DR-050 §Context finding 2,
//! third trap).
//!
//! ## RED MODE (stated plainly, per test)
//!
//! All three tests are ASSERT-RED today against the fallback literal in
//! `bins/rezidentd/src/runs.rs` (`drive_run`, the `completed_id.is_none()`
//! arm), which hardcodes zeroed token keys and carries no `error` key at all:
//!
//! - `fallback_completion_reports_usage_as_absent_not_zero` fails because the
//!   fallback emits `cost.input_tokens = 0` / `cost.output_tokens = 0` for a
//!   run whose harness reported no accounting;
//! - `fallback_completion_carries_the_dying_childs_error_text` fails because
//!   the fallback discards the failure reason the child's last stream line
//!   reported (DR-051 §Context finding 3: "discarded, not carried");
//! - `fallback_completion_matches_the_adapters_failure_shape` fails on the
//!   key-set comparison against the recorded failure rendering (the reference
//!   carries `error` and no token keys; the fallback is the mirror image).
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{connect, open_request, read_until, send_line, start_daemon};
use rezidnt_run::RunId;
use rezidnt_run::adapter::{AgentSubstrate, CodexAdapter, MappedFact};
use serde_json::Value;
use ulid::Ulid;

/// The distinctive failure text the dying stub's LAST stream line reports.
/// The fallback must carry this text (containment, not equality — whether the
/// implementer carries the raw line verbatim or lifts its message field is a
/// shape choice this oracle does not pin).
const SENTINEL: &str = "SENTINEL-DR051 upstream refusal: model not supported";

/// A temp project whose stub harness announces a session, reports a failure
/// reason as its LAST stream line, and dies WITHOUT a result line — the exact
/// run the daemon's exit-status fallback exists for.
fn make_dying_harness_project() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());

    let script = dir.path().join("harness.sh");
    let body = format!(
        r#"#!/bin/sh
echo '{{"type":"system","subtype":"init","session_id":"fixture-session","claude_code_version":"2.1.191","tools":[]}}'
echo '{{"type":"error","message":"{SENTINEL}"}}'
exit 3
"#
    );
    std::fs::write(&script, body).expect("write harness stub");
    set_executable(&script);

    let spec = format!(
        r#"[project]
name = "dr051-fallback"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
bin_override = "{script}"
"#,
        repo = repo.display(),
        script = script.display(),
    );
    (dir, spec)
}

fn set_executable(script: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(script).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(script, perms).expect("chmod");
}

/// Run the dying stub through a live daemon and return the fallback
/// `agent.completed` payload off the tail.
fn fallback_completion() -> Value {
    let daemon = start_daemon();
    let (_project, spec) = make_dying_harness_project();

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.completed"
    });
    lines
        .iter()
        .find(|v| v["subject"] == "agent.completed")
        .expect("read_until stopped on agent.completed")["payload"]
        .clone()
}

/// The CROSS-CRATE reference: `Completion::into_fact`'s failure-shaped output,
/// obtained by driving the recorded `turn.failed` transcript through the
/// public `AgentSubstrate` seam — a run that terminated with no usage
/// reported, rendered by the one payload builder the adapter crate owns.
fn adapter_failure_reference() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/transcripts/codex_exec_v0.145.0_turn_failed.jsonl");
    let transcript =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"));
    let mut adapter: Box<dyn AgentSubstrate> =
        Box::new(CodexAdapter::new(RunId::new(Ulid::from_parts(51, 4))));
    let facts: Vec<MappedFact> = transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| {
            adapter
                .map_line(l)
                .expect("recorded lines must map cleanly")
        })
        .collect();
    facts
        .into_iter()
        .find(|f| f.subject == "agent.completed")
        .expect(
            "the recorded failing transcript renders a completion (pinned green in rezidnt-run)",
        )
        .payload
}

fn sorted_keys(v: &Value) -> Vec<String> {
    let mut keys: Vec<String> = v
        .as_object()
        .unwrap_or_else(|| panic!("expected a JSON object, got {v:#}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// The ratified "absent, not zero" clause, applied to the fallback: the child
/// died without reporting usage, so the fallback's cost carries NO token keys
/// — `0` would be a present claim of a measurement that never happened, and a
/// zero-token failure reads as a free run on the slice-C leaderboard.
///
/// ASSERT-RED today: the fallback literal hardcodes both keys as `0`.
#[test]
fn fallback_completion_reports_usage_as_absent_not_zero() {
    let payload = fallback_completion();
    assert_eq!(
        payload["status"], "error",
        "premise: the fallback terminates the run as an honest error — got {payload:#}"
    );
    if let Some(cost) = payload.get("cost").and_then(Value::as_object) {
        for key in ["input_tokens", "output_tokens"] {
            assert!(
                !cost.contains_key(key),
                "the child died without reporting usage, so the fallback's `cost.{key}` \
                 must be ABSENT — not present as zero or null. `0` means \"measured, and \
                 it was nothing\"; this run was never measured (ontology `agent.completed` \
                 v1 cost bullet, ratified \"absent, not zero\"; DR-051 §Decision 4). \
                 Got {payload:#}"
            );
        }
    }
}

/// DR-051 §Decision 4, second obligation: the fallback carries whatever error
/// text the dying child's LAST stream line reported, rather than discarding
/// the failure reason (DR-051 §Context finding 3: the reason is proven to
/// exist and is "discarded, not carried").
///
/// ASSERT-RED today: the fallback publishes no `error` key at all.
#[test]
fn fallback_completion_carries_the_dying_childs_error_text() {
    let payload = fallback_completion();
    let message = payload["error"]["message"].as_str().unwrap_or_else(|| {
        panic!(
            "the fallback must carry `error.message` — the dying child's last stream line \
             reported a failure reason and the fallback discarded it (DR-051 §Decision 4 / \
             §Context finding 3). Got {payload:#}"
        )
    });
    assert!(
        message.contains(SENTINEL),
        "the fallback's `error.message` must carry the failure text the dying child's \
         last stream line reported (containment — verbatim-line vs lifted-message is the \
         implementer's shape choice). Wanted a substring {SENTINEL:?}, got {message:?}"
    );
}

/// The CROSS-CRATE shape pin, against the FAILURE rendering: the fallback's
/// key set — top level and inside `cost` — must equal what
/// `Completion::into_fact` renders for a run that terminated with no usage
/// reported. `session_id` is normalized out of both sides before comparing:
/// its presence records whether the stream announced a resume identity before
/// dying, an axis orthogonal to the failure shape (both sides here happen to
/// carry one; the normalization keeps this pin from freezing that accident).
///
/// ASSERT-RED today: the reference carries `error` and no token keys; the
/// fallback carries token keys and no `error`.
#[test]
fn fallback_completion_matches_the_adapters_failure_shape() {
    let mut fallback = fallback_completion();
    let mut reference = adapter_failure_reference();
    for side in [&mut fallback, &mut reference] {
        if let Some(obj) = side.as_object_mut() {
            obj.remove("session_id");
        }
    }

    assert_eq!(
        fallback["status"], reference["status"],
        "both sides are the failure shape: status error"
    );
    assert_eq!(
        sorted_keys(&fallback),
        sorted_keys(&reference),
        "the daemon's fallback literal must agree with `Completion::into_fact`'s \
         FAILURE-shaped key set (a run that terminated with no usage reported) — \
         cross-crate, so the two `agent.completed` literals cannot drift apart by \
         coincidence (DR-050 §Decision 2(c) as sharpened by DR-051 §Decision 4). \
         fallback: {fallback:#} reference: {reference:#}"
    );
    assert_eq!(
        sorted_keys(&fallback["cost"]),
        sorted_keys(&reference["cost"]),
        "the failure shape's cost object carries `total_usd` only — token keys are \
         absent together (ontology `agent.completed` v1 cost bullet). \
         fallback: {fallback:#} reference: {reference:#}"
    );
}
