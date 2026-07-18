//! SP-empty oracle — the LIVE PDP must distinguish a DECLARED-empty intent
//! (`run.intent.declared` with `allowed_tools: []`) from an ABSENT intent (no
//! `run.intent.declared` fact at all). DR-012 option B (ACCEPTED,
//! `docs/decisions/DR-012-empty-vs-absent-intent.md`): a declared-empty
//! allowlist means "every tool is off-task" and routes through the off-task
//! path (escalate default / DENY under `on_off_task = deny` — a real
//! least-privilege lockdown); an absent intent stays cannot-run → escalate
//! (genuinely no declared intent, never a deny even under the knob).
//!
//! ## The collapse this pins (crates/rezidnt-mcp/src/lib.rs ~537-541)
//! The SP-wire injection today does:
//! ```ignore
//! if let Some(intent) = &folded.intent
//!     && !intent.allowed_tools.is_empty()
//! {
//!     obj.insert("allowed_tools".to_string(), json!(intent.allowed_tools));
//! }
//! ```
//! The `&& !is_empty()` guard OMITS the `allowed_tools` param key for a
//! declared-empty intent (`Some([])`) EXACTLY as it does for absent (`None`) —
//! so the native cannot tell them apart on the live path (both arrive key-absent
//! and hit the native's cannot-run guard). Per DR-012 the injection must OMIT
//! the key ONLY when `intent == None`, and INJECT it (possibly `[]`) when
//! `Some`. Paired with the native's key-presence fix (pinned in
//! `crates/rezidnt-gate/tests/intent_lock_native.rs`), this propagates
//! declared-ness through the LIVE PDP.
//!
//! RED MODE: **assert-red** on the live behavior. Against today's injection the
//! declared-empty run's `allowed_tools` key is dropped, so with `on_off_task =
//! deny` configured the run ESCALATES (cannot-run, key-absent) instead of the
//! required DENY — this test expects "deny" and gets "ask". The absent-run leg
//! stays green (absent must escalate), pinning that the fix does not regress it.
//!
//! Reuses the SP-wire live-dispatch idiom (`core_with_permit`, the folded intent
//! seeded on the log via `run.intent.declared`).

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

/// A core whose permit gate is configured with `verifier_set`, badge
/// pre-admitted, over a fresh temp log (the SP-wire `core_with_permit` idiom).
fn core_with_permit(
    badge: &Badge,
    verifier_set: PermitConfig,
) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_permit_config(verifier_set);
    (dir, Arc::new(core))
}

/// Publish `agent.spawned` so the run exists on the log — but NO
/// `run.intent.declared`, so the folded `AgentRunState.intent == None`: intent
/// genuinely ABSENT (DR-012). The daemon must OMIT the `allowed_tools` key.
fn seed_run_intent_absent(core: &McpCore, run: &str) {
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        json!({"run": run, "agent": "impl", "harness": "claude-code"}),
    )
    .expect("spawned envelope");
    core.fabric().publish(spawned).expect("publish spawned");
}

/// Publish `agent.spawned` + a `run.intent.declared` carrying `allowed_tools:
/// []` — a DECLARED-empty intent: the folded `AgentRunState.intent ==
/// Some(IntentState { allowed_tools: [] })` (the reducer sets `Some` on any
/// declared fact, even an empty list — verified in rezidnt-state). The daemon
/// must INJECT the `allowed_tools` key as `[]` (present-but-empty), NOT omit it.
fn seed_run_intent_declared_empty(core: &McpCore, run: &str) {
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        json!({"run": run, "agent": "impl", "harness": "claude-code"}),
    )
    .expect("spawned envelope");
    let intent = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("run.intent.declared"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": run,
            "intent_ref": {"hash": "e0pt3in73n7000000000000000000000000000000000000000000000000mpty", "bytes": 32, "mime": "text/plain"},
            // DECLARED-empty: the operator declared this run may use NO tools.
            "allowed_tools": [],
        }),
    )
    .expect("intent envelope");
    core.fabric().publish(spawned).expect("publish spawned");
    core.fabric().publish(intent).expect("publish intent");
}

/// DR-012 — the LIVE distinction. Two runs, SAME `[gates.permit]` config
/// (`intent-lock` + `on_off_task = deny`):
///   - DECLARED-empty intent (`allowed_tools: []` on the log) → every tool is
///     off-task → under the deny knob the live PDP **DENIES** any tool.
///   - ABSENT intent (no `run.intent.declared`) → cannot-run → **ESCALATES**
///     (the deny knob does NOT manufacture an intent; genuinely undecidable).
/// This proves the injection propagates declared-ness (key present-empty vs
/// omitted) through the live PDP end-to-end.
///
/// ASSERT-RED: today the injection's `&& !is_empty()` guard drops the
/// `allowed_tools` key for the declared-empty run just like the absent run, so
/// BOTH hit the native's key-absent cannot-run guard and BOTH escalate — the
/// declared-empty leg expects "deny" and gets "ask". Red reason is the
/// injection collapse (key omitted for `Some([])`), paired with the native
/// collapse. The absent leg is green today and must STAY green after the fix.
#[tokio::test]
async fn live_declared_empty_denies_under_knob_while_absent_escalates() {
    let badge = Badge::mint().expect("mint badge");

    // Declared-empty run under the deny knob → the live PDP must DENY.
    let deny_config = || PermitConfig::natives(&[("intent-lock", json!({"on_off_task": "deny"}))]);
    let (_dir_e, core_e) = core_with_permit(&badge, deny_config());
    const RUN_EMPTY: &str = "01SPEMPTYDECLAREDRUN000E01";
    seed_run_intent_declared_empty(&core_e, RUN_EMPTY);

    let empty_result = util::tool_call(
        &core_e,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN_EMPTY, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;
    assert_eq!(
        util::tool_payload(&empty_result)["decision"],
        json!("deny"),
        "a DECLARED-empty intent under `on_off_task = deny` DENIES every tool live (DR-012 \
         lockdown) — RED today: the injection drops the empty `allowed_tools` key so the run \
         escalates as cannot-run instead of denying"
    );

    // Absent run under the SAME deny knob → the live PDP must ESCALATE.
    let (_dir_a, core_a) = core_with_permit(&badge, deny_config());
    const RUN_ABSENT: &str = "01SPEMPTYABSENTRUN00000A01";
    seed_run_intent_absent(&core_a, RUN_ABSENT);

    let absent_result = util::tool_call(
        &core_a,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN_ABSENT, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;
    assert_eq!(
        util::tool_payload(&absent_result)["decision"],
        json!("ask"),
        "an ABSENT intent (no run.intent.declared) STILL escalates under the deny knob — \
         cannot-run ignores the knob (DR-012); this leg is green today and must STAY green"
    );
}

/// DR-012 — the interrogable evidence half. A DECLARED-empty run under the
/// DEFAULT knob ESCALATES (every tool off-task), and `gate_explain` surfaces the
/// OFF-TASK reason naming the empty declared intent (`[]`) — NOT the cannot-run
/// "no intent allowlist pinned" reason the ABSENT case carries. This is the live
/// mirror of the native discriminator: declared-empty routes through the
/// off-task path, so the live decision fact records the off-task wording.
///
/// ASSERT-RED: today the injection omits the key for the declared-empty run, so
/// the native emits the cannot-run "no intent allowlist pinned" reason — this
/// test expects the off-task "…not in declared intent []" wording and fails on
/// the collapsed cannot-run message.
#[tokio::test]
async fn live_declared_empty_default_escalates_with_off_task_reason() {
    let badge = Badge::mint().expect("mint badge");
    // DEFAULT knob (no on_off_task) → escalate.
    let config = PermitConfig::natives(&[("intent-lock", json!({}))]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SPEMPTYDEFAULTRUN0000D01";
    seed_run_intent_declared_empty(&core, RUN);

    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;
    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("ask"),
        "a DECLARED-empty intent under the default knob ESCALATES (every tool off-task, DR-012)"
    );

    // Interrogate the decision: the reason must be the OFF-TASK wording naming
    // the empty declared intent `[]` — proof it routed through the off-task path
    // (declared-empty), NOT the cannot-run absent path.
    let explain = util::tool_call(&core, 2, "gate_explain", json!({"run": RUN})).await;
    let payload = util::tool_payload(&explain);
    assert_eq!(
        payload["reason"],
        json!("off-task tool Bash not in declared intent []"),
        "declared-empty escalates via the OFF-TASK path (empty declared intent named `[]`), NOT \
         the cannot-run 'no intent allowlist pinned' reason — the live DR-012 discriminator; got \
         {payload:#}"
    );
}
