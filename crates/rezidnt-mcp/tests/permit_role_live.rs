//! SP4a oracle — the LIVE PDP injects the folded `role` into the permit input
//! params, and a role-keyed policy decides DIFFERENTLY by role (DR-016
//! §Decision 2; design permit-roles-delegation-sp4 §4). This is the MCP-layer
//! proof that role rides `decide_permit`'s content-pinned per-run params
//! (DR-011 §2) as a new input axis both native and exec permit verifiers read.
//!
//! Covers:
//!   - CRITERION 4 (inject): the folded role reaches the verifier input
//!     (`params.role`) — proven by a policy that keys on it and decides.
//!   - CRITERION 5 (HEADLINE — the SP4a acceptance): the SAME `[gates.permit]`
//!     policy + the SAME `Edit` request yields a DIFFERENT decision for two runs
//!     with different roles (reviewer → DENY a write, contributor → ALLOW).
//!
//! The reference policy is the committed local argv
//! `spec/fixtures/policies/permit_role_policy.sh` (I7 — no vendored RBAC engine);
//! it reads `params.role` and emits fail/pass/inconclusive per role.
//!
//! HOW THE ROLE REACHES THE POLICY (the wiring the implementer must build):
//!   1. `AgentSpec.role` (CRITERION 1) →
//!   2. emitted on `agent.spawned` (CRITERION 2) →
//!   3. folded onto `AgentRunState.role` (CRITERION 3) →
//!   4. injected by `decide_permit` into `base_params` alongside `tool` /
//!      `allowed_tools` / `cumulative_spend_usd` (crates/rezidnt-mcp/src/lib.rs
//!      ~line 691-707): `if let Some(role) = &folded.role { obj.insert("role",
//!      json!(role)); }` — PRESENT iff a role was declared; OMITTED when `None`
//!      (mirror the DR-012 declared-vs-absent discipline the `allowed_tools`
//!      injection already follows). The role is a content-pinned param, NEVER
//!      live state (determinism BINDING).
//!
//! RED MODE (honest — feature ABSENT end-to-end):
//!   - COMPILE-RED: this file seeds an `agent.spawned` whose payload carries
//!     `role`, then relies on `decide_permit` injecting `folded.role`; the fold
//!     field (`AgentRunState.role`) and the injection do not exist yet.
//!   - ASSERT-RED: even reading the current live path, `decide_permit` injects
//!     no `role` key, so `permit_role_policy.sh` always hits its no-role leg
//!     (inconclusive → `ask`) — the reviewer/contributor legs are never reached,
//!     so the headline's "different decision by role" is impossible today.
//!
//! Unix-only: the reference policy is POSIX-sh (mirrors SP3 `permit_exec_live`).
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

/// The committed role-keyed reference policy as an EXEC permit entry.
fn role_policy() -> PermitVerifierSpec {
    PermitVerifierSpec::exec(
        "reference-role-policy",
        vec![
            "/bin/sh".to_string(),
            policy_path("permit_role_policy.sh").display().to_string(),
        ],
        json!({}),
    )
}

fn core_with_permit(badge: &Badge, config: PermitConfig) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book).with_permit_config(config);
    (dir, Arc::new(core))
}

/// Seed an `agent.spawned` carrying `role` so the run — and its role — exist on
/// the log for `decide_permit` to fold. A `None` role omits the key (the honest
/// roleless spawn).
fn seed_run_with_role(core: &McpCore, run: &str, role: Option<&str>) {
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

/// The decision word for an `Edit` (write) request under `run`, through the LIVE
/// `request_permission` call against the role-keyed policy.
async fn decide_edit(core: &McpCore, id: u64, badge: &Badge, run: &str) -> serde_json::Value {
    let result = util::tool_call(
        core,
        id,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": run, "action": "tool.invoke", "tool": "Edit"}),
    )
    .await;
    util::tool_payload(&result)["decision"].clone()
}

/// CRITERION 4 (inject) — the folded role REACHES the verifier input. Proven by
/// the role-keyed policy deciding on it: a `reviewer` run's `Edit` request is
/// DENIED, which the policy can ONLY do by seeing `params.role == "reviewer"`.
/// If the role were not injected, the policy would hit its no-role leg
/// (inconclusive → `ask`), never `deny`.
///
/// COMPILE-RED (fold field absent) then ASSERT-RED (no role injected → `ask`).
#[tokio::test]
async fn folded_role_reaches_the_verifier_reviewer_deny() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::from_specs(vec![role_policy()]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SP4AR0LEINJREVIEWER0R01";
    seed_run_with_role(&core, RUN, Some("reviewer"));

    let decision = decide_edit(&core, 1, &badge, RUN).await;
    assert_eq!(
        decision,
        json!("deny"),
        "the reviewer role reached the policy (params.role) and it denied the \
         write — role IS injected into the permit input (CRITERION 4). A missing \
         role would have escalated (ask), never denied."
    );
}

/// CRITERION 5 (HEADLINE — the SP4a acceptance) — the SAME policy + the SAME
/// `Edit` request yields a DIFFERENT decision purely because the run's role
/// differs. Reviewer → DENY the write; contributor → ALLOW it. This is the
/// load-bearing test: it proves role actually changes the outcome end-to-end
/// through the live PDP (DR-016 §Decision 2 acceptance: "a role-keyed policy
/// decides a permit differently by role").
///
/// COMPILE-RED (fold field absent) then ASSERT-RED (no role injected → both runs
/// hit the no-role leg → both `ask` → NO difference by role).
#[tokio::test]
async fn same_policy_decides_differently_by_role() {
    let badge = Badge::mint().expect("mint badge");
    // ONE policy, ONE config — shared across both runs. Only the role differs.
    let config = PermitConfig::from_specs(vec![role_policy()]);
    let (_dir, core) = core_with_permit(&badge, config);

    const REVIEWER_RUN: &str = "01SP4AHEADREVIEWER0000R01";
    const CONTRIB_RUN: &str = "01SP4AHEADCONTRIB00000R01";
    seed_run_with_role(&core, REVIEWER_RUN, Some("reviewer"));
    seed_run_with_role(&core, CONTRIB_RUN, Some("contributor"));

    let reviewer_decision = decide_edit(&core, 1, &badge, REVIEWER_RUN).await;
    let contrib_decision = decide_edit(&core, 2, &badge, CONTRIB_RUN).await;

    // The acceptance: the decisions DIFFER, driven only by role.
    assert_ne!(
        reviewer_decision, contrib_decision,
        "the SAME policy + SAME Edit request decided DIFFERENTLY by role — this \
         is the SP4a acceptance (DR-016 §Decision 2). reviewer={reviewer_decision}, \
         contributor={contrib_decision}"
    );
    // And the specific decisions the reference policy pins, so the difference is
    // the RIGHT one (reviewer denied a write, contributor allowed it), not two
    // arbitrary non-equal values.
    assert_eq!(
        reviewer_decision,
        json!("deny"),
        "role reviewer → the policy DENIES the write (CRITERION 5)"
    );
    assert_eq!(
        contrib_decision,
        json!("allow"),
        "role contributor → the policy ALLOWS the write (CRITERION 5)"
    );
}

/// CRITERION 4/5 (the honesty corner) — a ROLELESS run (no `role` on its spawn)
/// under the same policy is ESCALATED (`ask`), NEVER coerced to allow. Absence
/// of a role is honest: the policy sees no role axis and cannot decide, so it
/// escalates (I6, DR-012 declared-vs-absent — a role-less agent is not silently
/// treated as a contributor).
///
/// COMPILE-RED then ASSERT-RED. NOTE: today (no injection) EVERY run hits this
/// no-role leg — this test passes for the WRONG reason now. It becomes
/// load-bearing (and honestly green) only once injection exists and the
/// reviewer/contributor runs stop hitting it; it is here to pin that a roleless
/// run STILL escalates after the wiring lands.
#[tokio::test]
async fn roleless_run_escalates_never_coerced_to_allow() {
    let badge = Badge::mint().expect("mint badge");
    let config = PermitConfig::from_specs(vec![role_policy()]);
    let (_dir, core) = core_with_permit(&badge, config);
    const RUN: &str = "01SP4AR0LELESS00000000R01";
    seed_run_with_role(&core, RUN, None);

    let decision = decide_edit(&core, 1, &badge, RUN).await;
    assert_ne!(
        decision,
        json!("allow"),
        "a roleless run is NEVER coerced to allow (I6) — absence of a role is not \
         a synthesized contributor (DR-012)"
    );
    assert_eq!(
        decision,
        json!("ask"),
        "no role axis → the policy escalates (inconclusive → ask), routed to a \
         human, never a synthesized grant (I6)"
    );
}
