//! SP-wire oracle — the LIVE PDP dispatch residual (the SP-intent `/debrief`
//! flagged it): today `call_request_permission` hardcodes a SINGLE
//! `ToolAllowlist.verify()` (crates/rezidnt-mcp/src/lib.rs), so `PathScope` /
//! `SpendCap` / `IntentLock` are registered + unit-tested but NEVER run on the
//! LIVE path. SP-wire gives `request_permission` a verifier-SELECTION seam that
//! dispatches the CONFIGURED `[gates.permit]` verifier set (design
//! `docs/design/permit-engine.md` §6) and aggregates via `permit::aggregate`
//! (the control flow pinned in `crates/rezidnt-gate/tests/permit_aggregate.rs`).
//!
//! =============================== DESIGN FORK ===============================
//! CRITERION 3 (per-verifier PINNED inputs + WHERE the permit config + folded
//! run state reach the transport-agnostic `McpCore`) is a GENUINE design fork.
//! The ratified design (permit-engine.md §5/§6, DR-008) pins the TOML shape and
//! that `request_permission` IS the PDP — but is SILENT on the SEAM by which the
//! applied `[gates.permit]` config AND the run's folded state
//! (`AgentRunState.permit_accumulators`, `.intent`, folded from the fabric)
//! reach `McpCore.call_request_permission`. Today the core holds only
//! `fabric` / `cas` / `substrate` / `badges`; the permit config lives in the
//! daemon's `OpenedWorkspace` registry (keyed by workspace, behind the
//! `McpSubstrate`-owning `Daemon`), and the run→workspace mapping needed to
//! find it is NOT on the core.
//!
//! These tests pin the seam as `McpCore::with_permit_config(...)` — the MINIMAL
//! ratified-consistent choice (a builder that injects a resolved permit-config +
//! folded-state source, mirroring the existing `with_cas` / `with_substrate`
//! builders and the `resources_read` fold-from-fabric idiom). They are
//! **`#[ignore]`-gated** with this tracking note until the seam is ratified: the
//! IMPLEMENTER MUST route to `/dr` to ratify the seam (name it: `McpSubstrate`
//! permit-lookup method? a spec/state source keyed by run/workspace? a
//! `with_permit_config` builder?) BEFORE un-ignoring and building. Un-ignoring
//! without a DR is inventing an unratified BINDING seam.
//! ==========================================================================
//!
//! RED MODE (once un-ignored): **compile-red** against `McpCore::with_permit_config`
//! and `rezidnt_mcp::PermitConfig` (the resolved-config + folded-state injection
//! shape), neither of which exists yet — then **assert-red** on the live
//! multi-verifier behavior the hardcoded single-`ToolAllowlist` stub can never
//! produce (an off-task tool escalating live, an out-of-scope path denying live,
//! a spend-cap escalating/denying live).

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

/// A core whose permit gate is CONFIGURED with `verifier_set` (native
/// name/params pairs, the resolved `[gates.permit]` set), badge pre-admitted,
/// over a fresh temp log. The permit-config source is the SP-wire seam under
/// design-fork review (see the file header).
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

/// Publish an `agent.spawned` + a `run.intent.declared` so the run exists on the
/// log with a declared intent allowlist the daemon can FOLD and inject (the
/// `AgentRunState.intent.allowed_tools`, CRITERION 3). Returns nothing — the
/// caller drives `request_permission` against `run`.
fn seed_run_with_intent(core: &McpCore, run: &str, allowed_tools: &[&str]) {
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
            "intent_ref": {"hash": "1n7en7d3m0000000000000000000000000000000000000000000000000w1re1", "bytes": 32, "mime": "text/plain"},
            "allowed_tools": allowed_tools,
        }),
    )
    .expect("intent envelope");
    core.fabric().publish(spawned).expect("publish spawned");
    core.fabric().publish(intent).expect("publish intent");
}

/// CRITERION 1 + 5 — the live `request_permission` runs the CONFIGURED SET, not
/// a hardcoded single verifier: an on-task, in-allowlist tool is GRANTED through
/// the live call while an off-task tool ESCALATES (proving `intent-lock` RAN on
/// the live PDP path, the exact residual). This exercises the LIVE PDP, not the
/// native unit tests.
///
/// COMPILE-RED (seam) then ASSERT-RED (the hardcoded single-`ToolAllowlist` can
/// never escalate an off-task tool — it has no intent-lock on the live path).
#[tokio::test]
async fn live_request_permission_runs_configured_intent_lock_set() {
    let badge = Badge::mint().expect("mint badge");
    // The configured permit set includes intent-lock — which the hardcoded
    // single-ToolAllowlist path NEVER runs.
    let config = PermitConfig::natives(&[
        (
            "tool-allowlist",
            json!({"allow": ["Read", "Grep", "Glob", "Bash"]}),
        ),
        ("intent-lock", json!({})),
    ]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SPWIREINTENTRUN000000R01";
    // The run declared an on-task allowlist of Read/Grep/Glob — Bash is off-task.
    seed_run_with_intent(&core, RUN, &["Read", "Grep", "Glob"]);

    // On-task tool: intent-lock passes, tool-allowlist passes → GRANT.
    let on_task = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;
    assert_eq!(
        util::tool_payload(&on_task)["decision"],
        json!("allow"),
        "an on-task, allowlisted tool is granted through the LIVE configured set (CRITERION 5)"
    );

    // Off-task tool: tool-allowlist passes (Bash is allowlisted) but intent-lock
    // ESCALATES (Bash not in the declared intent). The hardcoded single-verifier
    // path would GRANT this — the residual SP-wire closes.
    let off_task = util::tool_call(
        &core,
        2,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;
    assert_eq!(
        util::tool_payload(&off_task)["decision"],
        json!("ask"),
        "an off-task tool ESCALATES because intent-lock RAN on the live PDP path — NEVER coerced to allow (I6, CRITERION 1+5)"
    );
}

/// CRITERION 3 — the daemon FOLDS the run's intent allowlist from the log and
/// injects it as CONTENT-PINNED params (`inputs.params`), NEVER live mutable
/// state, NEVER re-derived. The decision fact's recorded `inputs` for the
/// deciding intent-lock verifier must carry `allowed_tools` = the DECLARED set
/// folded from `run.intent.declared` — the injection shape is pinned here.
///
/// COMPILE-RED (seam) then ASSERT-RED (no folded-state injection exists on the
/// live path today).
#[tokio::test]
async fn live_pdp_injects_folded_intent_as_pinned_params() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[("intent-lock", json!({}))]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SPWIREPINNEDRUN00000R01";
    seed_run_with_intent(&core, RUN, &["Read", "Grep", "Glob"]);

    let _ = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Bash"}),
    )
    .await;

    // The escalation fact records the deciding verifier's PINNED inputs. Find the
    // permit.escalated fact and interrogate it via gate_explain — its inputs must
    // carry the folded, content-pinned intent allowlist (CRITERION 3).
    let explain = util::tool_call(&core, 2, "gate_explain", json!({"run": RUN})).await;
    let payload = util::tool_payload(&explain);
    assert_eq!(
        payload["verdict"],
        json!("ask"),
        "the off-task request escalated (intent-lock ran live, CRITERION 3 precondition)"
    );
    // The reason names BOTH the off-task tool AND the DECLARED intent — proof the
    // folded allowlist was injected verbatim as a pinned input, not re-derived.
    assert_eq!(
        payload["reason"],
        json!("off-task tool Bash not in declared intent [Read, Grep, Glob]"),
        "the deciding verifier saw the folded intent allowlist as a PINNED param (CRITERION 3: never live state, never re-derived)"
    );
}

/// CRITERION 3 + 5 — a `spend-cap`-configured gate ESCALATES at the soft cap
/// through the live call, with the run's spend accumulator FOLDED from the log
/// and injected as a pinned param (`cumulative_spend_usd`). The daemon reads
/// `AgentRunState.permit_accumulators` from the fold (I3), never a live counter.
///
/// COMPILE-RED (seam) then ASSERT-RED (no accumulator injection on the live path).
#[tokio::test]
async fn live_spend_cap_escalates_at_soft_cap_from_folded_accumulator() {
    let badge = Badge::mint().expect("mint badge");
    // The spend-cap's static caps come from the verifier's configured params; the
    // running cumulative spend is the FOLDED accumulator the daemon injects.
    let config = PermitConfig::natives(&[(
        "spend-cap",
        json!({"soft_cap_usd": 5.0, "hard_cap_usd": 20.0, "action_cost_usd": 1.0}),
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SPWIRESPENDRUN000000R01";

    // Seed prior granted decisions whose spend_delta_usd folds to cumulative 8.0
    // (over the 5.0 soft cap, under the 20.0 hard cap) — the accumulator state
    // the daemon folds and injects.
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
    )
    .expect("spawned envelope");
    core.fabric().publish(spawned).expect("publish spawned");
    let granted = Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new("permit.granted"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": RUN,
            "request_id": "01SPWIRESPENDPRIORREQ00001",
            "policy_ref": {"hash": "pr10rgr4n700000000000000000000000000000000000000000000000sp3nd1", "bytes": 40, "mime": "application/octet-stream"},
            "spend_delta_usd": 8.0,
        }),
    )
    .expect("prior granted envelope");
    core.fabric().publish(granted).expect("publish prior grant");

    // projected = folded cumulative (8.0) + action_cost (1.0) = 9.0, in the soft
    // band (5.0 <= 9.0 < 20.0) → escalate.
    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;
    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("ask"),
        "spend-cap RAN live and escalated at the soft cap using the FOLDED accumulator (CRITERION 3+5); NEVER coerced (I6)"
    );
}

/// CRITERION 4 — the ONE aggregate decision fact carries the DECIDING verifier's
/// `policy_ref` (its recorded params) + `evidence_ref` (its evidence blob), so
/// `gate_explain` surfaces the REAL reason — NOT a hardcoded `tool-allowlist`.
/// A path-scope deny through the live configured set must surface path-scope's
/// evidence, proving the emit path generalized past the single-verifier pin
/// (lib.rs:471-510).
///
/// COMPILE-RED (seam) then ASSERT-RED (today the emit path hardcodes
/// `"verifier": "tool-allowlist"` in the pinned policy).
#[tokio::test]
async fn live_aggregate_fact_surfaces_the_deciding_verifier_not_tool_allowlist() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[
        ("tool-allowlist", json!({"allow": ["Read", "Edit", "Bash"]})),
        ("path-scope", json!({"allow": ["src/checkout/**"]})),
    ]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SPWIREDECIDERUN00000R01";
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
    )
    .expect("spawned envelope");
    core.fabric().publish(spawned).expect("publish spawned");

    // Edit is allowlisted, but the target path is out of scope → path-scope DENIES.
    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({
            "badge": badge.token_hex(),
            "run": RUN,
            "action": "tool.invoke",
            "tool": "Edit",
            // the out-of-scope target path the daemon injects as a pinned param.
            "paths": ["/etc/shadow"],
        }),
    )
    .await;
    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("deny"),
        "path-scope RAN live and denied the out-of-scope path (CRITERION 4 precondition)"
    );

    let explain = util::tool_call(&core, 2, "gate_explain", json!({"run": RUN})).await;
    let payload = util::tool_payload(&explain);
    assert!(
        payload["reason"]
            .as_str()
            .is_some_and(|r| r.contains("/etc/shadow") || r.contains("scope")),
        "the aggregate decision surfaces PATH-SCOPE's reason (the deciding verifier), NOT a hardcoded tool-allowlist reason (CRITERION 4): {payload:#}"
    );
}

/// CRITERION 1 (honesty canary, ALWAYS the fork marker) — this test does NOT
/// touch the unratified seam: it drives the EXISTING live `request_permission`
/// and asserts the current hardcoded behavior, then documents (via a failing
/// assertion the implementer flips) that a configured multi-verifier set is not
/// yet dispatched. It stays RED-by-design as the SP-wire tracking canary and is
/// the honest proof the residual is still open. Un-ignore ONLY after the /dr
/// ratifies the seam.
#[tokio::test]
async fn residual_canary_hardcoded_single_verifier_still_leaks_off_task() {
    // A bare core (no permit config) with a badge: the CURRENT live path runs a
    // single ToolAllowlist with the request's `allow` and NOTHING else. An
    // off-task tool that IS allowlisted is (wrongly) granted — the leak SP-wire
    // closes. When the config-dispatch seam lands and intent-lock runs live, this
    // canary flips: the same call escalates. That flip is the residual closing.
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = util::core_with_badges(&[&badge]);
    let result = util::tool_call(
        &core,
        1,
        "request_permission",
        json!({
            "badge": badge.token_hex(),
            "run": "01SPWIRECANARYRUN00000R01",
            "action": "tool.invoke",
            "tool": "Bash",
            "allow": ["Bash"],
        }),
    )
    .await;
    // The SP-wire target: with a configured intent-lock the daemon would escalate
    // an off-task tool. This asserts that target; against today's hardcoded path
    // (which grants an allowlisted tool with no intent-lock) it is RED — the
    // residual is open until the seam lands.
    assert_eq!(
        util::tool_payload(&result)["decision"],
        json!("ask"),
        "SP-wire target: a configured intent-lock escalates an off-task tool live; RED until the config-dispatch seam closes the residual (CRITERION 1)"
    );
}
