//! C6 oracle (DR-024 — live running-risk-cap enforcement) — the `RiskCap`
//! enforcement path becomes LIVE and HONEST end-to-end through the PDP
//! (`request_permission`), the RISK analogue of C1's `permit_live_spend_cap_c1.rs`.
//!
//! Three wiring facts these tests pin, all currently ABSENT (RED):
//!
//!   1. `RiskCap` does not exist / is not in `builtin_natives()`, so a
//!      `[gates.permit]` `risk-cap` entry resolves to a cannot-run → the
//!      aggregate ESCALATES (ask) instead of the projected-vs-caps verdict.
//!   2. The PDP does not inject `cumulative_risk_score` (mirror the
//!      `cumulative_spend_usd` injection at rezidnt-mcp/src/lib.rs:831-834), so
//!      even once RiskCap exists it sees cumulative 0.0.
//!   3. The C6 fold still folds risk on ALL outcomes (not granted-only), so the
//!      cumulative these tests seed via GRANTED actions is the honest running
//!      score only after the reducer narrows (DR-024 Q3).
//!
//! The cumulative running risk is seeded via prior GRANTED permit facts carrying
//! `risk_delta` — the honest C6 source (a granted action RAN, so it counts).
//!
//! Host-runnable (native verifiers + state fold + live PDP, platform-neutral —
//! same class as the C1 suites; no #[cfg(unix)]).

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

/// A core whose permit gate is CONFIGURED with `config`, badge pre-admitted,
/// over a fresh temp log. Mirrors the C1 `core_with_permit` harness.
fn core_with_permit(badge: &Badge, config: PermitConfig) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_permit_config(config);
    (dir, Arc::new(core))
}

/// Publish an `agent.spawned` so the run exists on the log for the PDP to fold.
/// The `role`, when `Some`, rides the spawn payload — the DR-016 AUTHORITY path:
/// role is sourced from FOLDED RBAC state (`agent.spawned.role` → `AgentRunState.role`
/// → the PDP injection at `rezidnt-mcp/src/lib.rs:828`), NEVER self-declared on the
/// request (a requester cannot lower its own risk by claiming `admin`). This
/// mirrors `permit_role_live.rs::seed_run_with_role` exactly. A `None` role omits
/// the key (the honest roleless spawn).
fn seed_spawn(core: &McpCore, run: &str, role: Option<&str>) {
    let mut payload = json!({"run": run, "agent": "impl", "harness": "claude-code"});
    if let (Some(role), Some(obj)) = (role, payload.as_object_mut()) {
        obj.insert("role".to_string(), json!(role));
    }
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("spawned envelope");
    core.fabric().publish(spawned).expect("publish spawned");
}

/// Publish a prior GRANTED permit fact carrying a `risk_delta` — the honest C6
/// running-risk source (a granted action RAN, so its assessed risk counts, DR-024
/// Q3). The reducer folds `risk_delta` into `cumulative_risk_score` (keyed on
/// `run`) ONLY on the granted arm; the PDP injects that folded scalar.
fn seed_granted_risk(core: &McpCore, run: &str, req: &str, risk_delta: f64) {
    let granted = Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new("permit.granted"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": run,
            "request_id": req,
            "policy_ref": {"hash": format!("r15kc6pr10r{req}00000000000000000000000000000000000000000000000"), "bytes": 32, "mime": "application/octet-stream"},
            "risk_delta": risk_delta,
        }),
    )
    .expect("prior grant envelope");
    core.fabric().publish(granted).expect("publish prior grant");
}

/// A TEST scorer table with KNOWN weights (NOT production magic numbers — DR-024
/// leaves the real weights to tuning). `Bash` bases 6.0, `Read` 1.0; a path under
/// `secrets/**` adds 4.0; role `untrusted` adds 3.0, `admin` subtracts 2.0.
fn test_table() -> Value {
    json!({
        "base": { "Bash": 6.0, "Read": 1.0 },
        "sensitive_paths": ["secrets/**", "/etc/**"],
        "path_modifier": 4.0,
        "role_modifier": { "admin": -2.0, "untrusted": 3.0 }
    })
}

/// A `risk-cap` permit config with the given caps + the test table.
fn risk_config(soft: f64, hard: f64) -> PermitConfig {
    PermitConfig::natives(&[(
        "risk-cap",
        json!({
            "soft_cap_risk": soft,
            "hard_cap_risk": hard,
            "risk_table": test_table(),
        }),
    )])
}

/// Decide a permission for a request axis through the LIVE PDP and return the
/// decision word (`allow`/`ask`/`deny`). The request carries ONLY the `tool` +
/// `paths` axis — NO `role`: role is folded from the run's `agent.spawned` (seeded
/// upstream via `seed_spawn(.., Some(role))`) and injected by the PDP, so the
/// scorer reads it from the AUTHORITY, never a self-declared request arg (DR-016).
async fn decide(
    core: &McpCore,
    id: u64,
    badge: &Badge,
    run: &str,
    tool: &str,
    paths: Value,
) -> serde_json::Value {
    let result = util::tool_call(
        core,
        id,
        "request_permission",
        json!({
            "badge": badge.token_hex(),
            "run": run,
            "action": "tool.invoke",
            "tool": tool,
            "paths": paths,
        }),
    )
    .await;
    util::tool_payload(&result)["decision"].clone()
}

/// The `reason` on the most-recent permit DECISION fact for `run` (the deciding
/// verifier's evidence msg). Used to rule out a VACUOUS pass: today the unknown
/// `risk-cap` native is a cannot-run whose reason is "unknown native verifier
/// risk-cap" and whose aggregate is `ask`. A test that merely asserted `ask`
/// would pass on that cannot-run — testing nothing. So the soft-band/phantom/
/// gate_explain tests ALSO assert the reason is the RiskCap breakdown, NOT the
/// cannot-run message, so they can only pass once RiskCap actually RUNS.
fn last_decision_reason(core: &McpCore, run: &str) -> String {
    const DECISION_SUBJECTS: [&str; 3] = ["permit.granted", "permit.denied", "permit.escalated"];
    util::log_events(core)
        .into_iter()
        .rfind(|e| {
            DECISION_SUBJECTS.contains(&e.subject.as_str()) && e.payload()["run"] == json!(run)
        })
        .map(|e| e.payload()["reason"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// Assert a decision reason is NOT the cannot-run escapes-hatch — the guard that
/// keeps a soft-band/phantom `ask` from passing vacuously while RiskCap is still
/// an unknown native (cannot-run). Green only once RiskCap RUNS and produces its
/// own risk-factor reason.
fn assert_risk_cap_actually_ran(reason: &str) {
    assert!(
        !reason.to_lowercase().contains("unknown native"),
        "the decision came from a cannot-run (\"unknown native verifier risk-cap\"), NOT from \
         RiskCap running — the assertion would pass VACUOUSLY. RiskCap must be registered and \
         run; reason was: {reason:?}"
    );
}

/// CRITERION 1 — a `[gates.permit]` `risk-cap` config causes the PDP to inject
/// `cumulative_risk_score` and RiskCap to RUN (not cannot-run), returning ALLOW
/// when projected risk is under soft. Cumulative 0.0 (no prior grants) +
/// this-action Read/benign/admin (1.0 - 2.0 = -1.0) = -1.0 < soft 10.0 → allow.
///
/// RED: RiskCap absent → the `risk-cap` entry is an unknown native → cannot-run
/// → the aggregate ESCALATES (ask), NOT allow. Green once the native exists,
/// is registered, and the PDP injects `cumulative_risk_score`.
#[tokio::test]
async fn risk_cap_runs_and_allows_under_soft() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_with_permit(&badge, risk_config(10.0, 30.0));
    const RUN: &str = "01C6LIVEUNDERSOFT00000R01";
    // Role rides the SPAWN (folded RBAC authority, DR-016), not the request.
    seed_spawn(&core, RUN, Some("admin"));

    let decision = decide(&core, 1, &badge, RUN, "Read", json!(["src/app.rs"])).await;
    assert_eq!(
        decision,
        json!("allow"),
        "RiskCap RAN (caps + cumulative_risk_score injected) and projected risk -1.0 < soft \
         10.0 → allow (CRITERION 1). The admin role (folded from agent.spawned, -2.0) reached \
         the scorer axis. A cannot-run would ESCALATE (ask) instead."
    );
}

/// CRITERION 2 (soft band) — the folded cumulative running risk drives projected
/// into `[soft, hard)` → Inconclusive → ASK (escalate). Seed prior GRANTED risk
/// 8.0; this-action Read/benign/admin (-1.0) → projected 8.0 + (-1.0) = 7.0…
/// that is under soft, so instead drive it with a risky action: Bash/sensitive/
/// untrusted (13.0) → projected 8.0 + 13.0 = 21.0, in [soft 10.0, hard 30.0) →
/// ask.
///
/// RED: cumulative folds 0.0 (RiskCap/injection absent) → the whole path escalates
/// on cannot-run anyway; and even with injection, without the granted-only fold
/// the seeded number is not the honest running score. Green end-to-end only once
/// all three wiring facts land.
#[tokio::test]
async fn folded_cumulative_drives_soft_band_escalate() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_with_permit(&badge, risk_config(10.0, 30.0));
    const RUN: &str = "01C6LIVESOFTBAND000000R01";
    // The `untrusted` role rides the SPAWN (folded authority, DR-016).
    seed_spawn(&core, RUN, Some("untrusted"));
    seed_granted_risk(&core, RUN, "01C6SOFTPRIORGRANT00Q001", 8.0);

    let decision = decide(&core, 1, &badge, RUN, "Bash", json!(["secrets/key.pem"])).await;
    assert_eq!(
        decision,
        json!("ask"),
        "cumulative 8.0 (folded from a GRANTED action) + this-action 13.0 = 21.0, in the soft \
         band [10.0, 30.0) → escalate, NEVER coerced (I6, CRITERION 2)"
    );
    // Not-vacuous: the `ask` must be RiskCap's soft-band escalation, not a
    // cannot-run on the unknown native (which also yields `ask` today).
    assert_risk_cap_actually_ran(&last_decision_reason(&core, RUN));
}

/// CRITERION 2 (hard cap) — projected past the hard cap → Fail → DENY. Seed prior
/// GRANTED risk 20.0; this-action Bash/sensitive/untrusted (13.0) → projected
/// 33.0 ≥ hard 30.0 → deny.
///
/// RED: cumulative folds 0.0 today → projected 13.0 < hard → not deny (and the
/// cannot-run path escalates). Green once RiskCap runs on the injected cumulative.
#[tokio::test]
async fn folded_cumulative_past_hard_denies() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_with_permit(&badge, risk_config(10.0, 30.0));
    const RUN: &str = "01C6LIVEHARDDENY000000R01";
    // The `untrusted` role rides the SPAWN (folded authority, DR-016).
    seed_spawn(&core, RUN, Some("untrusted"));
    seed_granted_risk(&core, RUN, "01C6HARDPRIORGRANT00Q001", 20.0);

    let decision = decide(&core, 1, &badge, RUN, "Bash", json!(["secrets/key.pem"])).await;
    assert_eq!(
        decision,
        json!("deny"),
        "cumulative 20.0 (folded) + this-action 13.0 = 33.0 ≥ hard 30.0 → deny (CRITERION 2)"
    );
}

/// CRITERION 6 (live honesty) — a DENIED/ESCALATED action does NOT raise the
/// running risk the NEXT decision reads. Seed a prior GRANTED risk 8.0, then a
/// prior DENIED carrying a stray `risk_delta` 20.0 (never ran). The injected
/// cumulative must be 8.0 (granted only), so a Bash/sensitive/untrusted request
/// (13.0) projects to 8.0 + 13.0 = 21.0 → ask (soft band). If the denied risk
/// were WRONGLY folded, cumulative would be 28.0 → projected 41.0 ≥ hard → DENY.
/// The `ask`-vs-`deny` split makes the phantom-charge observable on the live path.
///
/// RED: the fold charges the denied 20.0 today → cumulative 28.0 → projected
/// 41.0 → deny, NOT the asserted ask. Green only once the fold narrows to granted.
#[tokio::test]
async fn denied_risk_does_not_raise_the_next_decisions_running_risk() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_with_permit(&badge, risk_config(10.0, 30.0));
    const RUN: &str = "01C6LIVEPHANTOM0000000R01";
    // The `untrusted` role rides the SPAWN (folded authority, DR-016).
    seed_spawn(&core, RUN, Some("untrusted"));
    seed_granted_risk(&core, RUN, "01C6PHANTOMGRANT0000Q001", 8.0);

    // A DENIED action carrying a STRAY risk_delta 20.0 — never ran, must fold ZERO.
    let denied = Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new("permit.denied"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": RUN,
            "request_id": "01C6PHANTOMDENY00000Q002",
            "policy_ref": {"hash": "ph4n70md3n1ed0000000000000000000000000000000000000000000000c601", "bytes": 32, "mime": "application/octet-stream"},
            "reason": "denied but assessed risky",
            "risk_delta": 20.0,
        }),
    )
    .expect("stray-risk deny envelope");
    core.fabric().publish(denied).expect("publish stray deny");

    let decision = decide(&core, 1, &badge, RUN, "Bash", json!(["secrets/key.pem"])).await;
    assert_eq!(
        decision,
        json!("ask"),
        "cumulative = 8.0 (GRANTED only); the DENIED action's stray 20.0 folds ZERO → projected \
         8.0 + 13.0 = 21.0 → ask. If the denied risk folded, cumulative 28.0 → projected 41.0 → \
         deny — the ask/deny split proves no phantom charge (CRITERION 6, live)"
    );
    // Not-vacuous: the `ask` must be RiskCap's soft-band escalation on the
    // granted-only cumulative, not a cannot-run on the unknown native.
    assert_risk_cap_actually_ran(&last_decision_reason(&core, RUN));
}

/// CRITERION 5 (determinism/replay) — the SAME request axis decided TWICE over
/// the SAME log yields the SAME decision. The scorer is a pure fn of the
/// content-pinned axis; no network or inference is reachable (the whole path is
/// synchronous native + folded state, no I/O in the scorer). A divergence would
/// be a determinism-BINDING violation.
#[tokio::test]
async fn same_axis_decides_identically_twice() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_with_permit(&badge, risk_config(10.0, 30.0));
    const RUN: &str = "01C6LIVEDETERM00000000R01";
    // The `untrusted` role rides the SPAWN (folded authority, DR-016).
    seed_spawn(&core, RUN, Some("untrusted"));
    seed_granted_risk(&core, RUN, "01C6DETERMGRANT00000Q001", 8.0);

    let first = decide(&core, 1, &badge, RUN, "Bash", json!(["secrets/key.pem"])).await;
    let second = decide(&core, 2, &badge, RUN, "Bash", json!(["secrets/key.pem"])).await;
    assert_eq!(
        first, second,
        "same request axis + same log → same decision (determinism BINDING, replayable, CRITERION 5)"
    );
    assert_eq!(
        first,
        json!("ask"),
        "and the deterministic decision is the projected-vs-caps verdict (soft band → ask)"
    );
    // Not-vacuous: determinism holds even for a cannot-run, so pin that the
    // decision is RiskCap's actual verdict, not the unknown-native escape hatch.
    assert_risk_cap_actually_ran(&last_decision_reason(&core, RUN));
}

/// CRITERION 7 (the stamped delta, live) — a GRANTED action's `permit.granted`
/// fact carries a `risk_delta` EQUAL to the score RiskCap used for its verdict:
/// both the verdict and the stamp call the SAME shared `risk_score` fn on the
/// SAME axis (DR-024 Q5). Grant a Read/benign/admin action (score = base 1.0
/// plus admin role -2.0, i.e. -1.0) under generous caps; the emitted granted
/// fact must stamp risk_delta = -1.0, the exact scalar the verdict used.
///
/// RED: nothing stamps `risk_delta` on the granted fact today (the emit site at
/// rezidnt-mcp:921-924 threads only `cost_ms`), so the key is ABSENT → the
/// assertion FAILS. Green once the emit site calls the shared fn to stamp it.
#[tokio::test]
async fn granted_fact_stamps_risk_delta_equal_to_the_verdict_score() {
    let badge = Badge::mint().expect("mint badge");
    // Generous caps so the action is GRANTED (its fact is what we inspect).
    let (_dir, core) = core_with_permit(&badge, risk_config(100.0, 200.0));
    const RUN: &str = "01C6LIVESTAMP000000000R01";
    // The `admin` role rides the SPAWN (folded authority, DR-016).
    seed_spawn(&core, RUN, Some("admin"));

    let decision = decide(&core, 1, &badge, RUN, "Read", json!(["src/app.rs"])).await;
    assert_eq!(
        decision,
        json!("allow"),
        "the action is granted under generous caps"
    );

    // Find the emitted permit.granted fact and read its stamped risk_delta.
    let granted = util::log_events(&core)
        .into_iter()
        .find(|e| e.subject.as_str() == "permit.granted")
        .expect("the grant emits a permit.granted fact");
    assert_eq!(
        granted.payload()["risk_delta"].as_f64(),
        Some(-1.0),
        "the granted fact stamps risk_delta = the SHARED-fn score the verdict used \
         (Read base 1.0 + admin role -2.0 = -1.0) — verdict and stamp cannot diverge \
         (DR-024 Q5, CRITERION 7)"
    );
}

/// CRITERION 3 (interrogability, live) — a RiskCap escalation is interrogable via
/// `gate_explain`: the decision fact's `reason` NAMES the contributing factors so
/// a human reads WHY the risk crossed. Drive a soft-band escalation, then
/// `gate_explain` the run and assert the surfaced reason names the risky tool.
///
/// RED: the escalation is a cannot-run today (RiskCap absent), whose reason is a
/// generic "unknown native", not the factor breakdown. Green once RiskCap
/// produces the escalation with factor-naming evidence (CRITERION 3).
#[tokio::test]
async fn gate_explain_surfaces_the_risk_factor_breakdown() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_with_permit(&badge, risk_config(10.0, 30.0));
    const RUN: &str = "01C6LIVEEXPLAIN0000000R01";
    // The `untrusted` role rides the SPAWN (folded authority, DR-016).
    seed_spawn(&core, RUN, Some("untrusted"));
    seed_granted_risk(&core, RUN, "01C6EXPLAINGRANT0000Q001", 8.0);

    // Produce a soft-band escalation on the log.
    let _ = decide(&core, 1, &badge, RUN, "Bash", json!(["secrets/key.pem"])).await;

    let explained = util::tool_call(&core, 2, "gate_explain", json!({"run": RUN})).await;
    assert_ne!(
        explained["isError"],
        json!(true),
        "an escalated run is interrogable — gate_explain must not answer gate.no_verdict: {explained:#}"
    );
    let payload = util::tool_payload(&explained);
    assert_eq!(
        payload["verdict"],
        json!("ask"),
        "the RiskCap escalation surfaces as `ask`, NEVER coerced to allow (I6)"
    );
    let reason = payload["reason"].as_str().unwrap_or("");
    // Not-vacuous: the cannot-run reason ("unknown native verifier risk-cap")
    // contains the substring "risk", so a loose "contains risk" check would pass
    // on it. Require the FACTOR breakdown — the risky TOOL named — AND that the
    // reason is not the cannot-run message.
    assert_risk_cap_actually_ran(reason);
    assert!(
        reason.contains("Bash") || reason.to_lowercase().contains("soft cap"),
        "the escalation reason names the risk factor (the tool Bash) or the soft-cap crossing so \
         a human reads WHY (I6, CRITERION 3); got {reason:?}"
    );
}

/// CRITERION 5 (the folded-role modifier is LOAD-BEARING, DR-016 authority) — the
/// role modifier reaches the scorer FROM FOLDED STATE, and it moves the verdict.
/// Two runs, SAME `risk-cap` config, SAME Read/`src/app.rs` request; only the
/// FOLDED role (on `agent.spawned`) differs:
///   - folded `admin` (-2.0): base 1.0 + role -2.0 = -1.0, cumulative 0 → projected
///     -1.0, well under soft 0.5 → ALLOW.
///   - folded `untrusted` (+3.0): base 1.0 + role +3.0 = 4.0, cumulative 0 →
///     projected 4.0 ≥ hard 3.0 → DENY.
/// The allow/deny split is driven ONLY by the folded role, so it proves the
/// folded-role axis is genuinely wired into the scorer (not vacuous) AND pins the
/// DR-016 privilege-escalation guard: role is the AUTHORITY's (folded), not the
/// requester's — a run cannot self-declare `admin` on the request to duck the cap.
#[tokio::test]
async fn folded_role_modifier_moves_the_verdict_never_self_declared() {
    let badge = Badge::mint().expect("mint badge");
    // Tight caps chosen so the role modifier alone crosses the hard cap.
    let config = risk_config(0.5, 3.0);

    // Run A: folded admin role → -2.0 → projected -1.0 < soft → allow.
    let (_dir_a, core_a) = core_with_permit(&badge, config.clone());
    const RUN_ADMIN: &str = "01C6ROLEADMIN000000000R01";
    seed_spawn(&core_a, RUN_ADMIN, Some("admin"));
    let admin_decision = decide(&core_a, 1, &badge, RUN_ADMIN, "Read", json!(["src/app.rs"])).await;
    assert_eq!(
        admin_decision,
        json!("allow"),
        "folded admin role (-2.0): base 1.0 - 2.0 = -1.0 < soft 0.5 → allow"
    );
    // Non-vacuous: the allow is RiskCap's verdict, not a cannot-run.
    assert_risk_cap_actually_ran(&last_decision_reason(&core_a, RUN_ADMIN));

    // Run B: folded untrusted role → +3.0 → projected 4.0 ≥ hard 3.0 → deny.
    let (_dir_b, core_b) = core_with_permit(&badge, config);
    const RUN_UNTRUSTED: &str = "01C6ROLEUNTRUST0000000R01";
    seed_spawn(&core_b, RUN_UNTRUSTED, Some("untrusted"));
    let untrusted_decision = decide(
        &core_b,
        1,
        &badge,
        RUN_UNTRUSTED,
        "Read",
        json!(["src/app.rs"]),
    )
    .await;
    assert_eq!(
        untrusted_decision,
        json!("deny"),
        "folded untrusted role (+3.0): base 1.0 + 3.0 = 4.0 ≥ hard 3.0 → deny"
    );

    // The split is driven ONLY by the folded role — proving it is load-bearing on
    // the scorer, sourced from the folded RBAC authority (DR-016), never the request.
    assert_ne!(
        admin_decision, untrusted_decision,
        "the SAME request decided DIFFERENTLY purely by the FOLDED role — the role modifier is \
         wired from folded state (DR-016 authority), and no request arg supplied it (CRITERION 5)"
    );
}
