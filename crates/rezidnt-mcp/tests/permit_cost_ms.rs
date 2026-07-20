//! Oracle — live `cost_ms` on permit decision facts (design §10.2 decision
//! latency; DecisionDeltas recorded-only cost field, SP5).
//!
//! Today the PDP emits EVERY permit decision fact with
//! `rezidnt_gate::permit::DecisionDeltas::default()`
//! (crates/rezidnt-mcp/src/lib.rs:903), so `cost_ms` is NEVER written on a live
//! decision fact. This suite drives a live permit decision through the existing
//! `request_permission` PDP path and asserts the PUBLISHED
//! `permit.granted`/`.denied`/`.escalated` fact payload carries an integer
//! `cost_ms` key. It is RED today: `DecisionDeltas::default()` omits the key, so
//! `payload["cost_ms"]` is JSON `Null` on every emitted decision fact.
//!
//! TARGET the implementer builds (stated so the oracle pins it): in
//! `decide_permit`, wrap the `aggregate_async` call (lib.rs:854) in a monotonic
//! timer (`std::time::Instant::now()` / `.elapsed().as_millis() as u64`) that
//! measures the AGGREGATE span ONLY — NOT the surrounding CAS pin (:878-886) or
//! `publish_fact` (:905), which would conflate policy latency with I/O — and
//! pass `DecisionDeltas { cost_ms: Some(elapsed_ms), ..Default::default() }` to
//! `decided_fact` at :903. No other change: `cost_ms` folds into NO accumulator
//! (recorded-only; the reducer at rezidnt-state/src/lib.rs:725-729 reads only
//! `spend_delta_usd`/`risk_delta`).
//!
//! `cost_ms` is WALL-CLOCK non-deterministic: these tests assert PRESENCE +
//! integer type + `>= 0`, NEVER an exact ms value (a golden pinning an exact
//! cost_ms would be flaky). Platform-neutral: this drives the `McpCore` PDP
//! directly (no `UnixStream`), so it needs no `#![cfg(unix)]` gate.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

use util::{log_events, tool_call, tool_payload};

/// A core whose permit gate is CONFIGURED with `verifier_set` (the resolved
/// `[gates.permit]` set), `badge` pre-admitted, over a fresh temp log. Mirrors
/// the `core_with_permit` helper local to `permit_wire_dispatch.rs` — kept local
/// here because integration-test files compile independently (the shared `util`
/// module carries `core_with_badges` but not the permit-config builder).
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

/// The three permit DECISION subjects (`spec/ontology.md:153-155`). A live
/// decision publishes exactly one of these; the request subject
/// (`permit.requested`) is NOT a decision fact and carries no `cost_ms`.
const DECISION_SUBJECTS: [&str; 3] = ["permit.granted", "permit.denied", "permit.escalated"];

/// Seed an `agent.spawned` so the run exists on the log (the PDP folds run
/// state from the fabric). Returns nothing — the caller drives
/// `request_permission` against `run`.
fn seed_run(core: &rezidnt_mcp::McpCore, run: &str) {
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

/// The single `permit.granted`/`.denied`/`.escalated` decision fact the PDP
/// published on the log for `run` (asserting exactly one so the test reads THE
/// live decision, not a stale one).
fn sole_decision_fact(core: &rezidnt_mcp::McpCore, run: &str) -> Event {
    let decisions: Vec<Event> = log_events(core)
        .into_iter()
        .filter(|e| {
            DECISION_SUBJECTS.contains(&e.subject.as_str())
                && e.payload()["run"].as_str() == Some(run)
        })
        .collect();
    assert_eq!(
        decisions.len(),
        1,
        "exactly one permit decision fact for {run}; got {} — {decisions:#?}",
        decisions.len()
    );
    decisions.into_iter().next().unwrap()
}

/// CRITERION 1 — every emitted permit decision fact carries a `cost_ms` key
/// (`u64`, non-negative). Contrast today: `DecisionDeltas::default()` omits it,
/// so `payload["cost_ms"]` is JSON `Null`. Drive a live GRANT through the PDP
/// and assert the published `permit.granted` payload has an integer `cost_ms`.
///
/// ASSERT-RED today: the emit site passes `DecisionDeltas::default()`, so no
/// `cost_ms` key is ever written — `.as_u64()` returns `None` and this fails.
#[tokio::test]
async fn granted_decision_fact_carries_integer_cost_ms() {
    let badge = Badge::mint().expect("mint badge");
    // A tool-allowlist that admits `Read` → the live decision GRANTS.
    let config = PermitConfig::natives(&[("tool-allowlist", json!({"allow": ["Read"]}))]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01COSTMSGRANTRUN000000R01";
    seed_run(&core, RUN);

    let result = tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;
    assert_eq!(
        tool_payload(&result)["decision"],
        json!("allow"),
        "precondition: the allowlisted tool GRANTS on the live PDP path"
    );

    let fact = sole_decision_fact(&core, RUN);
    assert_eq!(
        fact.subject.as_str(),
        "permit.granted",
        "the grant published a permit.granted decision fact"
    );
    let cost = fact.payload()["cost_ms"].as_u64().unwrap_or_else(|| {
        panic!(
            "the emitted permit.granted fact must carry an integer `cost_ms` (CRITERION 1); \
             today DecisionDeltas::default() omits it — payload: {:#}",
            fact.payload()
        )
    });
    // Wall-clock, non-deterministic: PRESENCE + integer type + >= 0 ONLY, never
    // an exact ms value. `as_u64() == Some(_)` already proves >= 0 (u64), so the
    // bind above is the whole assertion; this line documents the >= 0 contract.
    let _ = cost; // u64: inherently non-negative — the integer type IS the >= 0 proof.
}

/// CRITERION 2 — the timer wraps the aggregate span UNCONDITIONALLY. An
/// escalate on an EMPTY configured verifier set (which skips the verifier scan —
/// `aggregate_async` short-circuits `set.is_empty()` at permit.rs:630) STILL
/// emits a `cost_ms` key, proving the timer wraps the `aggregate_async` call at
/// lib.rs:854, NOT only the non-empty path. Value may be small/zero-ish — assert
/// PRESENCE + integer type + `>= 0`, NOT an exact value (wall-clock).
///
/// ASSERT-RED today: the empty-set escalate publishes `permit.escalated` with
/// `DecisionDeltas::default()` — no `cost_ms` key.
#[tokio::test]
async fn empty_set_escalate_still_emits_cost_ms_timer_wraps_unconditionally() {
    let badge = Badge::mint().expect("mint badge");
    // EMPTY configured set → aggregate escalates (undecidable is never a
    // synthesized allow, I6) WITHOUT scanning any verifier.
    let config = PermitConfig::natives(&[]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01COSTMSEMPTYRUN000000R01";
    seed_run(&core, RUN);

    let result = tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;
    assert_eq!(
        tool_payload(&result)["decision"],
        json!("ask"),
        "precondition: an EMPTY permit set ESCALATES (I6, never coerced to allow)"
    );

    let fact = sole_decision_fact(&core, RUN);
    assert_eq!(
        fact.subject.as_str(),
        "permit.escalated",
        "the empty-set undecidable published a permit.escalated fact"
    );
    let cost = fact.payload()["cost_ms"].as_u64();
    assert!(
        cost.is_some(),
        "even the EMPTY-set escalate (verifier scan skipped) carries an integer `cost_ms` — \
         proof the timer wraps the aggregate_async call unconditionally at lib.rs:854, not only \
         the non-empty path (CRITERION 2); payload: {:#}",
        fact.payload()
    );
    // >= 0 is inherent in u64; do NOT assert an exact value — the span is
    // wall-clock non-deterministic and may legitimately be 0 on a fast machine.
}

/// CRITERION 4 — `cost_ms` is an INTEGER key, never JSON `null`. Absence would
/// be OMISSION (an unmeasured field), but since the PDP always MEASURES it, it
/// is always PRESENT as an integer on a live decision fact (the `DecisionDeltas`
/// doc contract, crates/rezidnt-gate/src/permit.rs:113-116: absence = omitted
/// key, never `null`). This pins the type discipline explicitly: the key exists
/// AND is a JSON number, not `Value::Null`.
///
/// ASSERT-RED today: the key is absent, so `payload["cost_ms"]` reads as
/// `Value::Null` — which is exactly the `null`-not-integer state this refuses.
#[tokio::test]
async fn cost_ms_is_an_integer_never_json_null() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::natives(&[("tool-allowlist", json!({"allow": ["Read"]}))]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01COSTMSTYPERUN0000000R01";
    seed_run(&core, RUN);

    let _ = tool_call(
        &core,
        1,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": "Read"}),
    )
    .await;

    let fact = sole_decision_fact(&core, RUN);
    let cost = &fact.payload()["cost_ms"];
    assert!(
        !cost.is_null(),
        "`cost_ms` is PRESENT (the PDP always measures it), never JSON `null` \
         (CRITERION 4 / DecisionDeltas contract permit.rs:113-116); payload: {:#}",
        fact.payload()
    );
    assert!(
        cost.is_u64(),
        "`cost_ms` is a non-negative INTEGER (u64), not a float/string/null \
         (CRITERION 4); got {cost:#}"
    );
}
