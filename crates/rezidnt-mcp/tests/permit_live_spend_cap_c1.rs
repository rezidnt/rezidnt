//! C1 oracle (DR-021 — live spend-cap enforcement) — the SpendCap enforcement
//! path becomes LIVE and HONEST end-to-end through the PDP (`request_permission`).
//!
//! The verdict logic of the `spend-cap` native already exists and is unit-tested
//! in `crates/rezidnt-gate/tests/permit_natives.rs` (it is CORRECT). What DR-021
//! makes live is TWO wiring facts these tests pin, both currently ABSENT:
//!
//!   1. The PDP injects the run's caps AND its `window_action_count` into the
//!      permit input, so SpendCap RUNS instead of returning cannot-run. Today
//!      `decide_permit` (crates/rezidnt-mcp/src/lib.rs ~:831) injects ONLY
//!      `cumulative_spend_usd` — no `window_action_count` — so the rate-limit leg
//!      can never fire on the live path (CRITERION 3 is RED for want of injection).
//!   2. The C1 FOLD SOURCE moved OFF the pre-action permit fact onto a POST-action
//!      `action.metered` fact (DR-021 B2). The `action.metered` reducer arm does
//!      NOT EXIST yet (crates/rezidnt-state/src/lib.rs), so an `action.metered`
//!      fact folds ZERO spend today. Every test here seeds cumulative spend via
//!      `action.metered` — which means the folded `cumulative_spend_usd` the PDP
//!      injects is 0.0 until the arm lands, so SpendCap sees "under soft" and
//!      GRANTS where these tests demand escalate/deny. That is the honest RED.
//!
//! RED MODE (honest — feature ABSENT):
//!   - CRITERION 1: an `action.metered` fact does NOT move the accumulator, so the
//!     "Pass under soft" precondition seeds fine, but see 2/4/5 — the fold source
//!     is what these pin.
//!   - CRITERION 2/4/5: cumulative seeded via `action.metered` folds to 0.0 today,
//!     so projected stays under soft → GRANT, NOT the escalate/deny asserted → RED.
//!   - CRITERION 3: `window_action_count` is injected NOWHERE by the PDP today, so
//!     it defaults to 0 in the verifier and the rate-limit leg never fires → the
//!     rate-limited request is GRANTED, NOT denied → RED.
//!
//! These are host-runnable (native verifiers, no POSIX-sh reference policy — the
//! permit reducer/state/native path is platform-neutral, per the C1 slice notes).

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

/// A core whose permit gate is CONFIGURED with `config`, badge pre-admitted, over
/// a fresh temp log. Mirrors the SP-wire `core_with_permit` harness.
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
fn seed_spawn(core: &McpCore, run: &str) {
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

/// Publish a POST-action `action.metered` fact carrying a MEASURED spend delta —
/// the DR-021 B2 C1 fold source. The reducer must fold `spend_delta_usd` into
/// `cumulative_spend_usd` (keyed on `run`). This is the ONLY honest spend source
/// after DR-021; seeding via `permit.*` would fold nothing.
fn seed_metered(core: &McpCore, run: &str, req_ulid: &str, spend_delta_usd: f64) {
    let metered = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("action.metered"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": run,
            "spend_delta_usd": spend_delta_usd,
            "input_tokens": 1000u64,
            "output_tokens": 250u64,
            // an optional out-of-band attribution ref — recorded, not folded.
            "action_ref": {"hash": format!("ac710nm37{req_ulid}0000000000000000000000000000000000000000000000"), "bytes": 16, "mime": "application/octet-stream"},
        }),
    )
    .expect("action.metered envelope");
    core.fabric()
        .publish(metered)
        .expect("publish action.metered");
}

/// The decision word for a `Read` request under `run` through the LIVE PDP.
async fn decide_read(core: &McpCore, id: u64, badge: &Badge, run: &str) -> serde_json::Value {
    let result = util::tool_call(
        core,
        id,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": run, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;
    util::tool_payload(&result)["decision"].clone()
}

/// CRITERION 1 (the fold-driven boundary) — `[gates.permit]` caps cause SpendCap
/// to RUN (not cannot-run) and the LIVE folded-from-`action.metered` cumulative
/// DRIVES the verdict across the soft boundary. SAME config, SAME request; the
/// only difference is the measured spend on the log:
///   - measured 3.0 → projected 3.0 + cost 1.0 = 4.0 < soft 5.0 → ALLOW (Pass).
///   - measured 4.5 → projected 4.5 + cost 1.0 = 5.5 ≥ soft 5.0 → ASK (escalate).
/// The transition proves BOTH that the caps were injected (SpendCap ran, else a
/// cannot-run escalates BOTH legs to `ask`, no transition) AND that the measured
/// fold moved the verdict.
///
/// RED today: `action.metered` folds ZERO (arm absent), so BOTH legs fold
/// cumulative 0.0 → projected 1.0 < soft → BOTH ALLOW → the over-soft leg's
/// asserted `ask` FAILS. The transition is impossible until the fold arm lands.
/// (A cannot-run would instead make both `ask`; either broken state fails one
/// leg — the test cannot pass vacuously.)
#[tokio::test]
async fn caps_injected_and_metered_fold_drives_soft_boundary() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[(
        "spend-cap",
        json!({"soft_cap_usd": 5.0, "hard_cap_usd": 20.0, "action_cost_usd": 1.0}),
    )]);

    // Under-soft leg: measured 3.0 → projected 4.0 < soft → ALLOW.
    let (_dir_u, core_u) = core_with_permit(&badge, config.clone());
    const RUN_UNDER: &str = "01C1BOUNDARYUNDERSOFT0R01";
    seed_spawn(&core_u, RUN_UNDER);
    seed_metered(&core_u, RUN_UNDER, "UND01", 3.0);
    let under = decide_read(&core_u, 1, &badge, RUN_UNDER).await;
    assert_eq!(
        under,
        json!("allow"),
        "measured 3.0 (folded) + cost 1.0 = 4.0 < soft 5.0 → SpendCap RAN and passed \
         under soft (CRITERION 1)"
    );

    // Over-soft leg: measured 4.5 → projected 5.5 ≥ soft → ASK. Same config.
    let (_dir_o, core_o) = core_with_permit(&badge, config);
    const RUN_OVER: &str = "01C1BOUNDARYOVERSOFT00R01";
    seed_spawn(&core_o, RUN_OVER);
    seed_metered(&core_o, RUN_OVER, "OVR01", 4.5);
    let over = decide_read(&core_o, 2, &badge, RUN_OVER).await;
    assert_eq!(
        over,
        json!("ask"),
        "measured 4.5 (folded) + cost 1.0 = 5.5 ≥ soft 5.0 → escalate — the LIVE metered \
         fold drove the verdict across the boundary (CRITERION 1)"
    );
    // The transition itself: the fold changed the outcome, not a static cap.
    assert_ne!(
        under, over,
        "the SAME config decided DIFFERENTLY driven only by the measured spend on the log \
         (the fold is live, CRITERION 1)"
    );
}

/// CRITERION 2 (soft band) — cumulative MEASURED spend (folded from `action.metered`)
/// past the soft cap → Inconclusive → Escalate/ASK. Seed 8.0 of measured spend via
/// `action.metered`; projected = 8.0 + 1.0 = 9.0, in the soft band (5.0 ≤ 9.0 < 20.0)
/// → escalate.
///
/// RED: `action.metered` folds ZERO today (arm absent) → cumulative 0.0 → projected
/// 1.0 < soft → the request is ALLOWED, not escalated. Green only once the fold
/// arm folds the measured delta.
#[tokio::test]
async fn measured_spend_past_soft_escalates() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[(
        "spend-cap",
        json!({"soft_cap_usd": 5.0, "hard_cap_usd": 20.0, "action_cost_usd": 1.0}),
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01C1SOFTBANDESCALATE00R01";
    seed_spawn(&core, RUN);
    seed_metered(&core, RUN, "SOFT01", 8.0);

    let decision = decide_read(&core, 1, &badge, RUN).await;
    assert_eq!(
        decision,
        json!("ask"),
        "measured cumulative 8.0 (folded from action.metered) + cost 1.0 = 9.0 is in the \
         soft band (5.0..20.0) → escalate, NEVER coerced (I6, CRITERION 2)"
    );
}

/// CRITERION 2 (hard cap) — cumulative MEASURED spend past the hard cap → Fail →
/// Deny. Seed 25.0 of measured spend via `action.metered`; projected = 25.0 + 1.0 =
/// 26.0 ≥ hard 20.0 → deny.
///
/// RED: metered folds zero today → cumulative 0.0 → projected 1.0 < hard → ALLOWED,
/// not denied.
#[tokio::test]
async fn measured_spend_past_hard_denies() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[(
        "spend-cap",
        json!({"soft_cap_usd": 5.0, "hard_cap_usd": 20.0, "action_cost_usd": 1.0}),
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01C1HARDCAPDENY0000000R01";
    seed_spawn(&core, RUN);
    seed_metered(&core, RUN, "HARD01", 25.0);

    let decision = decide_read(&core, 1, &badge, RUN).await;
    assert_eq!(
        decision,
        json!("deny"),
        "measured cumulative 25.0 (folded from action.metered) + cost 1.0 = 26.0 ≥ hard 20.0 \
         → deny (CRITERION 2)"
    );
}

/// CRITERION 3 — `window_action_count ≥ rate_limit` → Deny, INDEPENDENT of spend.
/// The run has ZERO measured spend but has already had `rate_limit` (or more)
/// granted actions in the window; the next request is denied on the rate limit
/// alone. Seed 3 prior GRANTED permit decisions (the window action count the PDP
/// must fold + inject) with a rate_limit of 3 → the 4th request denies.
///
/// RED (two ways): (a) the PDP injects `window_action_count` NOWHERE today, so the
/// verifier sees 0 < 3 → the rate-limit leg never fires → ALLOWED, not denied; and
/// (b) spend is 0.0 so no other leg denies. Green only once the PDP injects the
/// folded window action count.
#[tokio::test]
async fn rate_limit_denies_independent_of_spend() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[(
        "spend-cap",
        json!({"soft_cap_usd": 100.0, "hard_cap_usd": 200.0, "action_cost_usd": 1.0, "rate_limit": 3u64}),
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01C1RATELIMITDENY00000R01";
    seed_spawn(&core, RUN);

    // Three prior granted decisions — the window action count the PDP folds + injects.
    for (i, req) in [
        "01C1RATEPRIORREQ00000001",
        "01C1RATEPRIORREQ00000002",
        "01C1RATEPRIORREQ00000003",
    ]
    .iter()
    .enumerate()
    {
        let granted = Event::new(
            SourceId::new("rezidnt-gate"),
            None,
            Subject::new("permit.granted"),
            Ulid::new(),
            None,
            1,
            json!({
                "run": RUN,
                "request_id": req,
                "policy_ref": {"hash": format!("r473l1m17pr10r{i}00000000000000000000000000000000000000000000000"), "bytes": 32, "mime": "application/octet-stream"},
            }),
        )
        .expect("prior grant envelope");
        core.fabric().publish(granted).expect("publish prior grant");
    }
    // NO spend at all — the deny must come from the rate limit alone.

    let decision = decide_read(&core, 1, &badge, RUN).await;
    assert_eq!(
        decision,
        json!("deny"),
        "window action count (3 prior grants) ≥ rate_limit 3 → deny, INDEPENDENT of spend \
         (cumulative is 0.0). The PDP folds + injects window_action_count (CRITERION 3)."
    );
}

/// CRITERION 4 (the fold-SOURCE move, live) — an `action.metered` fact IS the spend
/// source; a `permit.*` fact folds NO spend. Seed a `permit.granted` carrying a STRAY
/// `spend_delta_usd: 8.0` (a retired field the reducer must now IGNORE) and NO
/// `action.metered`. The folded cumulative must be 0.0, so projected 1.0 < soft 5.0
/// → the request is ALLOWED. If the reducer still folded spend off the permit fact
/// (the pre-DR-021 behavior), cumulative would be 8.0 → escalate → `ask`.
///
/// RED: today the permit reducer STILL folds `spend_delta_usd` off `permit.granted`
/// (crates/rezidnt-state/src/lib.rs:725-726), so cumulative folds to 8.0 → the
/// request ESCALATES (`ask`), NOT `allow`. Green only once that arm is deleted and
/// spend rides `action.metered` alone. This is the fold-source move asserted
/// directly (not just the total).
#[tokio::test]
async fn permit_fact_stray_spend_folds_zero_only_metered_counts() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[(
        "spend-cap",
        json!({"soft_cap_usd": 5.0, "hard_cap_usd": 20.0, "action_cost_usd": 1.0}),
    )]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01C1STRAYPERMITSPEND00R01";
    seed_spawn(&core, RUN);

    // A stray, RETIRED spend_delta_usd on a permit fact — must fold to ZERO now.
    let granted = Event::new(
        SourceId::new("rezidnt-gate"),
        None,
        Subject::new("permit.granted"),
        Ulid::new(),
        None,
        1,
        json!({
            "run": RUN,
            "request_id": "01C1STRAYPERMITREQ0000001",
            "policy_ref": {"hash": "5tr4y5p3nd0000000000000000000000000000000000000000000000000000", "bytes": 32, "mime": "application/octet-stream"},
            // RETIRED as the C1 fold source (DR-021) — the reducer must IGNORE it.
            "spend_delta_usd": 8.0,
        }),
    )
    .expect("stray-spend grant envelope");
    core.fabric().publish(granted).expect("publish stray grant");
    // NO action.metered → the ONLY honest spend source is empty → cumulative 0.0.

    let decision = decide_read(&core, 1, &badge, RUN).await;
    assert_eq!(
        decision,
        json!("allow"),
        "a stray spend_delta_usd on permit.granted folds ZERO (retired fold source, DR-021) → \
         cumulative 0.0 → projected 1.0 < soft 5.0 → ALLOW. If the reducer still folded off the \
         permit fact, cumulative would be 8.0 and this would escalate (CRITERION 4)."
    );
}
