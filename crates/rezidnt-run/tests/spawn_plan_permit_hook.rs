//! SP2 hook sub-slice oracle — CRITERION 1 (opt-in, the strongest judge).
//! DR-014 §Decision 2 / design §3: a permit-gated agent's `SpawnPlan` wires the
//! PEP; a non-permit agent's plan wires NONE of it. Pure struct — NO spawning,
//! NO daemon, NO IO. This is the cleanest deterministic judge in the slice.
//!
//! WHAT THE SPEC PINS (design §3): when the agent has a `permit` gate,
//! `SpawnPlan::for_claude_code_permit` injects into the scrubbed env
//!
//! - `REZIDNT_RUN` (this run's ULID — deterministic run discovery, never
//!   cwd-guessed),
//! - `REZIDNT_SOCKET` (the daemon UDS the hook dials),
//!
//! and a `PreToolUse` hook config naming `rezidnt permit-hook`. A run WITHOUT a
//! permit gate spawns exactly as today (none of the three present).
//!
//! RED MODE: **compile-red** (at authoring time). `SpawnPlan::for_claude_code`
//! took `(agent, badge, parent_env)` and returned `{bin, args, env}` with NO
//! hook config and no run/socket injection (crates/rezidnt-run/src/spawner.rs).
//! This board calls a NEW `for_claude_code_permit` signature that threads the
//! run id + socket + a way to inspect the injected hook config — which the
//! implementer added as a sibling constructor, leaving `for_claude_code`
//! intact. The judge existed before the feature.
//!
//! NOTE FOR THE IMPLEMENTER (type paths are negotiable, the BEHAVIOR is not):
//! the load-bearing assertions are (a) permit-gated ⇒ REZIDNT_RUN + REZIDNT_SOCKET
//! in env AND a PreToolUse hook config naming `rezidnt permit-hook`; (b)
//! non-permit ⇒ none of those. If you name the new parameters or the
//! hook-config accessor differently, adjust the calls here to match — do NOT
//! weaken an assertion to compile. The gate-detection key is the agent's `gates`
//! list containing `"permit"` (DR-014 §Decision 2: opt-in keyed on the spec
//! declaring a `[gates.permit]` gate).

use rezidnt_run::badge::{BADGE_ENV_VAR, Badge};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

/// A run ULID the daemon would inject (deterministic run discovery, design §3).
const RUN_ID: &str = "01SP2HOOKRUN00000000000PLAN1";
/// A socket path the daemon would inject.
const SOCKET: &str = "/run/user/1000/rezidnt.sock";

fn permit_agent() -> AgentSpec {
    AgentSpec {
        name: "impl".into(),
        harness: "claude-code".into(),
        worktree: "auto".into(),
        // THE opt-in key (DR-014 §Decision 2): the agent declares the permit gate.
        gates: vec!["permit".into()],
        ..AgentSpec::default()
    }
}

fn plain_agent() -> AgentSpec {
    AgentSpec {
        name: "impl".into(),
        harness: "claude-code".into(),
        worktree: "auto".into(),
        // No permit gate → spawns exactly as today (design §3).
        gates: vec![],
        ..AgentSpec::default()
    }
}

/// CRITERION 1 (opt-in, positive leg): a permit-gated agent's plan carries
/// REZIDNT_RUN and REZIDNT_SOCKET in the scrubbed env, injected at spawn so the
/// hook discovers its run deterministically and dials the right daemon (DR-014
/// §Decision 2; design §3(1)).
///
/// COMPILE-RED until `for_claude_code_permit` provides the run-id + socket
/// injection seam.
#[test]
fn permit_gated_plan_injects_run_and_socket_env() {
    let badge = Badge::mint().expect("mint");
    // NEW seam: the daemon threads the run id + socket into the pure plan. The
    // implementer chooses the exact signature; the behavior below is the pin.
    let plan = SpawnPlan::for_claude_code_permit(
        &permit_agent(),
        &badge,
        std::iter::empty(),
        RUN_ID,
        SOCKET,
    );

    assert!(
        plan.env
            .iter()
            .any(|(k, v)| k == "REZIDNT_RUN" && v == RUN_ID),
        "a permit-gated plan injects REZIDNT_RUN so the hook discovers its run \
         deterministically, never cwd-guessed (design §3(1)); env = {:?}",
        plan.env
    );
    assert!(
        plan.env
            .iter()
            .any(|(k, v)| k == "REZIDNT_SOCKET" && v == SOCKET),
        "a permit-gated plan injects REZIDNT_SOCKET so the hook dials the right \
         daemon (design §3(1)); env = {:?}",
        plan.env
    );
    // The badge injection (existing S1 contract) is NOT regressed by the
    // additive env — the plan is still scrubbed + badged.
    assert!(
        plan.env
            .iter()
            .any(|(k, v)| k == BADGE_ENV_VAR && *v == badge.token_hex()),
        "the permit plan still injects the badge (S1 contract unchanged); env = {:?}",
        plan.env
    );
}

/// CRITERION 1 (opt-in, the hook-config leg): a permit-gated plan carries a
/// `PreToolUse` hook config naming `rezidnt permit-hook` (design §3(2)) — the
/// thing that actually points claude-code at the PEP. This is the load-bearing
/// half: env alone does not intercept a tool call; the hook config does.
///
/// COMPILE-RED until the plan exposes the injected hook config.
#[test]
fn permit_gated_plan_carries_a_pretooluse_hook_config_naming_permit_hook() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_claude_code_permit(
        &permit_agent(),
        &badge,
        std::iter::empty(),
        RUN_ID,
        SOCKET,
    );

    // NEW accessor: the injected claude-code settings the daemon writes into the
    // worktree (`.claude/settings.json`, design §3(2)). Rendered to a string so
    // the assertions do not couple to the settings struct shape — the pin is
    // that it is a PreToolUse hook invoking `rezidnt permit-hook`.
    let hook_config = plan
        .permit_hook_config()
        .expect("a permit-gated plan carries a PreToolUse hook config (design §3(2))");

    assert!(
        hook_config.contains("PreToolUse"),
        "the injected hook config is a PreToolUse hook (design §3(2)): {hook_config}"
    );
    assert!(
        hook_config.contains("rezidnt permit-hook"),
        "the PreToolUse hook invokes the `rezidnt permit-hook` subcommand — the \
         PEP is a CLI subcommand, not a new binary (DR-014 §Decision 1): {hook_config}"
    );
}

/// CRITERION 1 (opt-in, negative leg — the honesty half): a NON-permit agent's
/// plan carries NONE of the PEP wiring: no REZIDNT_RUN, no REZIDNT_SOCKET, no
/// hook config. A run without a permit gate spawns exactly as today (DR-014
/// §Decision 2; design §3). This is what keeps `pep?` absence honest downstream
/// (criterion 2/6): the daemon does not synthesize enforcement it did not wire.
///
/// COMPILE-RED until the permit seam exists.
#[test]
fn non_permit_plan_carries_no_pep_wiring() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_claude_code_permit(
        &plain_agent(),
        &badge,
        std::iter::empty(),
        RUN_ID,
        SOCKET,
    );

    assert!(
        !plan.env.iter().any(|(k, _)| k == "REZIDNT_RUN"),
        "a non-permit plan injects NO REZIDNT_RUN — no PEP wired (design §3); env = {:?}",
        plan.env
    );
    assert!(
        !plan.env.iter().any(|(k, _)| k == "REZIDNT_SOCKET"),
        "a non-permit plan injects NO REZIDNT_SOCKET — no PEP wired (design §3); env = {:?}",
        plan.env
    );
    assert!(
        plan.permit_hook_config().is_none(),
        "a non-permit plan carries NO PreToolUse hook config — edge-gated-only, \
         the honest absence `pep?` records (DR-014; design §6)"
    );
}
