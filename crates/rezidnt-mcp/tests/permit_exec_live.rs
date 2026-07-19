//! SP3 oracle — the LIVE PDP dispatches an exec permit policy (DR-015 ACCEPTED,
//! §Decision 1/2; design `docs/design/permit-exec-verifier-sp3.md` §8). This is
//! the MCP-layer half: a `[gates.permit]` set carrying an EXEC entry reaches the
//! decision through `McpCore::decide_permit` — proving the async lift (option A)
//! actually dispatches the subprocess and maps its §8 verdict to the wire
//! decision (`pass→allow / fail→deny / inconclusive→ask`).
//!
//! The reference policy is the committed local argv
//! `spec/fixtures/policies/permit_tool_policy.sh` (I7 — no vendored engine);
//! dispatched as `sh <abspath>` (interpreter explicit, portable across a
//! Windows checkout).
//!
//! RED MODE (honest — feature ABSENT):
//!   - COMPILE-RED: `PermitVerifierSpec::exec(name, argv, params)` (the exec
//!     kind, DR-015 §Decision 1) does not exist; the async lift of
//!     `decide_permit`'s aggregation (DR-015 §Decision 2, option A) is not
//!     wired — today `decide_permit` calls the SYNC `permit::aggregate` inside
//!     `spawn_blocking`, which cannot `await` an `ExecVerifier`.
//!   - ASSERT-RED: even if it compiled, the current `spawn_blocking` path drops
//!     the exec entry (it dispatches natives by name only) → the exec verdict
//!     never reaches the decision.
//!
//! Unix-only: the reference policy is POSIX-sh.
#![cfg(unix)]

mod util;

use std::path::PathBuf;
use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_gate::permit::PermitVerifierSpec;
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

fn policy_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/policies")
        .join(name)
}

/// An EXEC permit entry — the API shape the implementer must match
/// (`PermitVerifierSpec::exec(name, argv, params)`).
fn exec_policy(name: &str, script: &str) -> PermitVerifierSpec {
    PermitVerifierSpec::exec(
        name,
        vec![
            "/bin/sh".to_string(),
            policy_path(script).display().to_string(),
        ],
        json!({}),
    )
}

/// A core whose permit gate is configured with `config` (which now MAY carry an
/// exec entry — the un-filtered set, DR-015 §Decision 1), badge pre-admitted.
fn core_with_permit(badge: &Badge, config: PermitConfig) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_permit_config(config);
    (dir, Arc::new(core))
}

/// Seed an `agent.spawned` so the run exists on the log.
fn seed_run(core: &McpCore, run: &str) {
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

/// CRITERION 1 + 2 (live headline) — a configured EXEC policy that DENIES a
/// forced-breach request (tool `Bash`) yields `deny` through the LIVE
/// `request_permission` call. Proves the async-lifted `decide_permit` dispatched
/// the subprocess and mapped `fail → deny`. The exec entry is NOT dropped
/// (CRITERION 2 dispatched-half) and NOT silently skipped.
///
/// COMPILE-RED (`::exec`) then ASSERT-RED (the sync `spawn_blocking` path can't
/// run the exec verifier).
#[tokio::test]
async fn live_exec_policy_denies_forced_breach() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::from_specs(vec![exec_policy(
        "reference-policy",
        "permit_tool_policy.sh",
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SP3EXECDENYLIVE00000R01";
    seed_run(&core, RUN);

    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;

    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("deny"),
        "the LIVE PDP dispatched the exec policy and it denied the forced breach — the external policy decided (CRITERION 1+2)"
    );
}

/// CRITERION 1 (live allow leg) — the same configured exec policy ALLOWS a
/// non-breach request (tool `Read`) → `allow` through the live call.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn live_exec_policy_allows_non_breach() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::from_specs(vec![exec_policy(
        "reference-policy",
        "permit_tool_policy.sh",
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SP3EXECALLOWLIVE0000R01";
    seed_run(&core, RUN);

    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;

    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("allow"),
        "the live exec policy allowed the non-breach tool (CRITERION 1, pass→allow)"
    );
}

/// CRITERION 4 (live never-coerce, I6) — a configured exec policy that exits
/// nonzero maps to `ask` on the LIVE path, never `allow` — even though its
/// stdout says pass. The load-bearing negative: decision != "allow".
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn live_exec_nonzero_exit_asks_never_allows() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::from_specs(vec![exec_policy(
        "nonzero-policy",
        "permit_policy_nonzero_exit.sh",
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SP3EXECNONZEROLIV000R01";
    seed_run(&core, RUN);

    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;

    assert_ne!(
        util::tool_payload(&result)["decision"],
        json!("allow"),
        "a nonzero-exit policy is NEVER coerced to allow on the live path (I6, CRITERION 4)"
    );
    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("ask"),
        "nonzero exit → inconclusive → ask (CRITERION 4)"
    );
}

/// CRITERION 5 (determinism/replay, live leg) — the decision fact carries a
/// `policy_ref` (the pinned policy descriptor). A configured exec policy's
/// decision must record `policy_ref` naming the EXEC verifier, so `debrief` can
/// re-execute the recorded §8 stdin against the same pinned policy. This pins
/// the `policy_ref` PRESENCE + that it names the exec verifier; the full replay
/// re-execution wiring is deferred (see `permit_exec_replay.rs`).
///
/// COMPILE-RED then ASSERT-RED (today no exec entry reaches the emit path, so no
/// exec `policy_ref` is ever pinned).
#[tokio::test]
async fn live_exec_decision_pins_policy_ref_naming_the_exec_verifier() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::from_specs(vec![exec_policy(
        "reference-policy",
        "permit_tool_policy.sh",
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SP3EXECPOLICYREF0000R01";
    seed_run(&core, RUN);

    let _ = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;

    // The decision fact must carry a policy_ref (I6 — the deciding policy is
    // content-pinned). Interrogate the log: the permit.denied fact for this run.
    let events = util::log_events(&core);
    let denied = events
        .iter()
        .find(|e| e.subject.as_str() == "permit.denied")
        .unwrap_or_else(|| panic!("a permit.denied decision fact must exist: {events:#?}"));
    let payload = denied.payload();
    let policy_ref = payload.get("policy_ref").unwrap_or_else(|| {
        panic!("the exec decision fact must carry a policy_ref (I6 pinned policy): {payload:#}")
    });
    assert!(
        policy_ref.get("hash").and_then(|h| h.as_str()).is_some(),
        "policy_ref is a CAS ref (hash), never inline bytes (I2): {policy_ref:#}"
    );

    // And `gate_explain` surfaces the EXEC verifier as the decider (not a
    // hardcoded native) — the deciding policy is the external one (design §7).
    let explain = util::tool_call(&core, 2, "gate_explain", json!({"run": RUN})).await;
    let explained = util::tool_payload(&explain);
    assert_eq!(
        explained["verdict"],
        json!("deny"),
        "gate_explain reports the exec policy's deny (CRITERION 5 precondition)"
    );
    assert!(
        explained["reason"]
            .as_str()
            .is_some_and(|r| r.contains("Bash") || r.contains("policy")),
        "gate_explain surfaces the EXTERNAL policy's reason, not a hardcoded native's (CRITERION 5/design §7): {explained:#}"
    );
}
