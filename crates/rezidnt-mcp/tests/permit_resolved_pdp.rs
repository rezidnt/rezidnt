//! DR-033 slice-2 (operator-resolve-escalation) ORACLE — CRITERION 2 (THE
//! CRUX): "honored on next ask" as a REPLAYABLE log→decision contract.
//!
//! ## The deterministic judge (testing-oracles: build where a judge exists)
//! This is NOT a flaky live path. It is a log-replay → one-decision judge:
//!
//! 1. seed a run log fixture: `permit.requested(action X)` →
//!    `permit.escalated(X)` → `permit.resolved(X, allow, operator_badge_id)`;
//! 2. drive ONE fresh `request_permission` for the SAME `(run, tool,
//!    action/target)` with a DIFFERENT `request_id`;
//! 3. assert the outcome FACT: the PDP APPLIED the resolution (allow/granted,
//!    NOT escalated) and the emitted `permit.granted` carries `resolved_from`
//!    = the resolution's `request_id`.
//!
//! No wall-clock, no process timing — `decide_permit` folds the log every call
//! (DR-033 §Context; `crates/rezidnt-mcp/src/lib.rs:924`), so the fixture IS the
//! input and the decision is a pure function of it.
//!
//! ## The override is PROVEN, not assumed
//! The config is the EMPTY verifier set (`PermitConfig::from_specs(vec![])`),
//! which the aggregator ESCALATES (DR-011 §3; `crates/rezidnt-mcp/src/lib.rs:908`,
//! `crates/rezidnt-gate/src/permit.rs`). So WITHOUT a folded resolution this run
//! escalates — `no_resolution_still_escalates` pins exactly that. WITH the
//! resolution folded, the SAME config must instead grant. The difference is
//! purely the folded `permit.resolved`, so a green suite proves the PDP
//! ledger-check (DR-033 §Decision 1), not verifier behavior.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!   - `decide_permit` gains a pre-verifier ledger-check: if the incoming
//!     `permit.requested` matches a folded `permit.resolved` for the same
//!     `(run, tool, action/target)`, APPLY the human decision — emit
//!     `permit.granted` (allow) / `permit.denied` (deny) carrying `resolved_from`
//!     = the `permit.resolved.request_id` — instead of re-escalating.
//!   - the applied outcome is a REAL logged decision fact (so `gate_explain` /
//!     `debrief` can chain it, CRITERION 5) — the PDP does not just return a word.
//!   - request-scoped: a DIFFERENT action on the same run STILL escalates
//!     (DR-033 §Decision 3 — the resolution is not an over-broad grant).
//!
//! RED MODE: ASSERT-RED — the ledger-check does not exist, so with the empty
//! config `decide_permit` escalates (returns `ask`) even when a resolution is
//! folded; the "applies the resolution → allow" assertion fails. The
//! `resolved_from` and request-scoped assertions are red for the same reason.
//! Deterministic: the fixture is the whole input.
//!
//! NOT `#![cfg(unix)]`-gated: the PDP path here uses only the EMPTY verifier set
//! (the aggregator's pure native escalate) and the pure `request_permission`
//! fold — no exec/`/bin/sh` verifier — so it runs host-side, exactly like
//! `request_permission.rs`. The assert-red is therefore observable on the host
//! /vet gauntlet, not only under WSL.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore, PermitConfig};
use rezidnt_run::badge::Badge;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR033PDPRES0LVE0000000R1";
/// The `request_id` of the ESCALATED ask — the one the resolution answers, and
/// the value the applied grant must cite in `resolved_from`.
const ESCALATED_REQ: &str = "01DR033PDPESCALATEDREQ00R1";

/// A core whose ONLY permit config is the empty verifier set — which escalates
/// (DR-011 §3). So the ONLY thing that can turn an escalate into a grant is a
/// folded `permit.resolved` the PDP applies. The daemon root key is wired so an
/// operator badge could later be admitted (parity with the live surface).
fn core_empty_permit(badge: &Badge) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_permit_config(PermitConfig::from_specs(vec![]));
    (dir, Arc::new(core))
}

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// Seed the escalation history for `tool` on `RUN`: the request, the escalation
/// routed to a human, and (when `decision` is `Some`) the human resolution.
/// This is the golden log-replay fixture, inline so the judge's whole input is
/// visible. `request_permission` on the NEXT ask folds exactly this.
fn seed_escalation(core: &McpCore, tool: &str, resolution: Option<&str>) {
    // agent.spawned so the run exists (parity with a real run's log).
    core.fabric()
        .publish(ev(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
        ))
        .expect("publish spawned");
    core.fabric()
        .publish(ev(
            "permit.requested",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "action": "tool.invoke", "target": {"tool": tool}
            }),
        ))
        .expect("publish requested");
    core.fabric()
        .publish(ev(
            "permit.escalated",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "policy_ref": {"hash": "e5ca1a7e00000000", "bytes": 8, "mime": "application/json"},
                "reason": "no policy configured — routed to a human"
            }),
        ))
        .expect("publish escalated");
    if let Some(decision) = resolution {
        core.fabric()
            .publish(ev(
                "permit.resolved",
                json!({
                    "run": RUN, "request_id": ESCALATED_REQ,
                    "action": "tool.invoke", "target": {"tool": tool},
                    "decision": decision,
                    "operator_badge_id": "0badc0de",
                    "reason": "operator approved after review"
                }),
            ))
            .expect("publish resolved");
    }
}

/// The decision word from a FRESH `request_permission` for `tool` on `RUN`, with
/// a DIFFERENT `request_id` than the escalated one (the daemon mints a new id;
/// we let it — the match must be by ACTION IDENTITY, not request_id).
async fn ask(core: &McpCore, id: u64, badge: &Badge, tool: &str) -> Value {
    let result = util::tool_call(
        core,
        id,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": RUN, "action": "tool.invoke", "tool": tool}),
    )
    .await;
    util::tool_payload(&result)["decision"].clone()
}

/// The LATEST decision fact for `RUN` on the log — the applied outcome the PDP
/// emitted on this ask (the interrogable I3 fact, not just the returned word).
fn latest_decision_fact(core: &McpCore) -> Event {
    util::log_events(core)
        .into_iter()
        .rev()
        .find(|e| {
            matches!(
                e.subject.as_str(),
                "permit.granted" | "permit.denied" | "permit.escalated"
            ) && e.payload()["run"] == json!(RUN)
        })
        .expect("a decision fact was emitted for the run")
}

/// BASELINE (proves the override is real) — with NO folded resolution, the empty
/// verifier set ESCALATES. This is the control: any grant in the tests below is
/// attributable to the resolution, not to a permissive config. GREEN today
/// (the empty-set escalate already works) — it is the honest control, not the
/// red pin.
#[tokio::test]
async fn no_resolution_still_escalates() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    seed_escalation(&core, "Bash", None);

    let decision = ask(&core, 1, &badge, "Bash").await;
    assert_eq!(
        decision,
        json!("ask"),
        "with NO resolution folded, the empty verifier set escalates (ask) — the \
         control that makes the grant tests below attributable to the resolution"
    );
}

/// CRITERION 2 (THE CRUX — allow) — a folded `permit.resolved(allow)` is APPLIED
/// on the NEXT ask for the SAME action: the SAME empty config that escalated
/// above now GRANTS (allow), and the emitted `permit.granted` carries
/// `resolved_from` = the resolution's `request_id`. ASSERT-RED: the ledger-check
/// does not exist, so this still escalates.
#[tokio::test]
async fn folded_allow_resolution_is_applied_on_next_ask() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    seed_escalation(&core, "Bash", Some("allow"));

    let decision = ask(&core, 1, &badge, "Bash").await;
    assert_eq!(
        decision,
        json!("allow"),
        "the PDP APPLIED the human allow-resolution on the next ask — the SAME \
         empty config that escalates without a resolution now grants (DR-033 §Decision 1). \
         An `ask` here means the ledger-check is missing."
    );

    let fact = latest_decision_fact(&core);
    assert_eq!(
        fact.subject.as_str(),
        "permit.granted",
        "the applied allow emits a REAL permit.granted fact (I3 — interrogable, \
         not just a returned word) — got {}",
        fact.subject.as_str()
    );
    assert_eq!(
        fact.payload()["resolved_from"],
        json!(ESCALATED_REQ),
        "the applied grant carries resolved_from = the permit.resolved's request_id, \
         so `granted via human resolution X` is a structured, log-derivable read \
         (I6, DR-033 §Decision 1; ontology permit.granted.resolved_from)"
    );
}

/// CRITERION 2 (symmetric — deny) — a folded `permit.resolved(deny)` is APPLIED
/// as a `permit.denied` carrying `resolved_from`. ASSERT-RED (ledger-check
/// absent → escalates instead of denying).
#[tokio::test]
async fn folded_deny_resolution_is_applied_on_next_ask() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    seed_escalation(&core, "Bash", Some("deny"));

    let decision = ask(&core, 1, &badge, "Bash").await;
    assert_eq!(
        decision,
        json!("deny"),
        "the PDP APPLIED the human deny-resolution on the next ask (DR-033 §Decision 1)"
    );

    let fact = latest_decision_fact(&core);
    assert_eq!(
        fact.subject.as_str(),
        "permit.denied",
        "the applied deny emits a REAL permit.denied fact (I3)"
    );
    assert_eq!(
        fact.payload()["resolved_from"],
        json!(ESCALATED_REQ),
        "the applied denial carries resolved_from = the resolution's request_id \
         (I6, DR-033 §Decision 1; permit.denied inherits resolved_from)"
    );
}

/// CRITERION 2 (request-scoped guard — DR-033 §Decision 3) — a resolution for
/// `Bash` does NOT over-broadly grant a DIFFERENT action on the same run: a
/// fresh ask for `Write` STILL escalates. This pins that the applied resolution
/// is action-matched, not a run-wide grant. ASSERT-RED for the resolved action
/// (Bash grants), but this test's `Write` leg must ALWAYS stay escalated — a
/// green implementation must keep it so.
#[tokio::test]
async fn resolution_for_one_action_does_not_grant_another() {
    let badge = Badge::mint().expect("mint badge");
    let (_dir, core) = core_empty_permit(&badge);
    // Resolve ONLY Bash.
    seed_escalation(&core, "Bash", Some("allow"));

    // The resolved action is applied (allow) — the positive leg.
    let bash = ask(&core, 1, &badge, "Bash").await;
    assert_eq!(
        bash,
        json!("allow"),
        "the RESOLVED action (Bash) is granted by the applied resolution"
    );

    // A DIFFERENT action on the SAME run has NO resolution → still escalates.
    let write = ask(&core, 2, &badge, "Write").await;
    assert_eq!(
        write,
        json!("ask"),
        "a DIFFERENT action (Write) on the same run has no resolution and STILL \
         escalates — the resolution is request-scoped, NOT an over-broad run grant \
         (DR-033 §Decision 3). A grant here would be the over-broad bug."
    );
}
