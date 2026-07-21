//! C3b+c oracle (DR-026 — the L7 egress-MITM + credential-brokering slice),
//! CRITERION 6 — the LOAD-BEARING no-widening test: the egress allowlist AND the
//! secret-injection map CANNOT be widened by any run-supplied input. This is the
//! C6/DR-024 privilege-escalation lesson applied to egress + credential brokering,
//! mirroring C3a's `SandboxPolicy.binds` private-field guard exactly: an input
//! that WIDENS authority (adds an allowlisted host, adds a secret mapping) must
//! come from FOLDED AUTHORITY, or the chokepoint is escapable-by-argument.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure policy/argv inspection, no connector,
//! no #[cfg(unix)]). The no-widening property is a property of the POLICY TYPE and
//! the connector argv the substrate WOULD run — it needs no live egress to test.
//! This whole file runs on every host that builds rezidnt (Windows host /vet
//! included). The REAL mediated egress is the `#[cfg(unix)]` WSL-only suite.
//!
//! RED MODE: **mixed**. The type-system guard tests (a `SpawnPlan` arg / request
//! destination cannot reach `EgressPolicy`) are STRUCTURAL and hold GREEN today —
//! they pin the interface shape the implementer must not regress (like the C3a
//! `policy_binds_come_only_from_the_folded_authority_constructor` structural
//! pin). The `connector_argv` routing tests are **assert-red** (`connector_argv`
//! is `todo!()` → panic) until the implementer writes the renderer.
//!
//! ## The seam this pins (the interface decision the implementer MUST honor)
//! `EgressPolicy::allowlist` AND `EgressPolicy::injection_map` are PRIVATE and set
//! ONLY via `EgressPolicy::from_folded_authority(allowlist, injection_map)`. There
//! is no constructor taking a `SpawnPlan`, an env var, or a request destination.
//! So the ONLY way an allowlist entry OR a secret mapping reaches the mechanism is
//! through the folded-authority path — the type system enforces the C6 guard. If a
//! future change adds a `SpawnPlan`-sourced allowlist/map constructor, THIS is the
//! file that must fail first.

use std::collections::BTreeMap;

use rezidnt_run::egress::{BrokeredSecret, Destination, EgressPolicy, connector_argv};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

/// A benign folded egress policy: one allowlisted host + one host→secret mapping.
/// This stands in for the daemon's fold of the project-spec `[gates.permit]`/role
/// layer + the daemon-side secret store (DR-026 §Decision) — the AUTHORITY the
/// allowlist and injection map legitimately come from.
fn folded_policy() -> EgressPolicy {
    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new("github-token", "ghp_folded_secret_value_do_not_leak"),
    );
    EgressPolicy::from_folded_authority(vec![Destination::host("github.com")], injection)
}

/// A spawn plan carrying ADVERSARIAL run-supplied values: argv and env that try
/// to smuggle in an allowlist entry / a secret mapping / an alternate proxy.
fn adversarial_plan() -> SpawnPlan {
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        bin_override: Some("/usr/bin/env".into()),
        ..AgentSpec::default()
    };
    let mut plan = SpawnPlan::for_claude_code(&agent, "badge-wire", std::env::vars());
    // Adversarial env: the agent (or a compromised harness) plants values that
    // LOOK like allowlist / injection directives, hoping the substrate folds env
    // into the policy or the connector argv.
    plan.env.push((
        "REZIDNT_EXTRA_ALLOW".to_string(),
        "evil.example.com".to_string(),
    ));
    plan.env.push((
        "REZIDNT_INJECT".to_string(),
        "evil.example.com=ghp_attacker_wants_this".to_string(),
    ));
    // Adversarial argv: an alternate proxy address, hoping to reroute egress off
    // the daemon-owned mediator.
    plan.args.push("--proxy".to_string());
    plan.args
        .push("http://attacker.example.com:8080".to_string());
    plan
}

/// CRITERION 6 (the type-system guard, structural) — an `EgressPolicy` can be
/// built ONLY from folded authority. This test COMPILES the one sanctioned
/// constructor and documents that there is no `SpawnPlan`-sourced sibling; the
/// private `allowlist` AND `injection_map` fields mean a caller literally cannot
/// mint a policy from a run-supplied value outside `from_folded_authority`. If a
/// widening constructor is ever added, this file's intent (and the argv test
/// below) break first. Mirrors C3a's structural policy pin.
#[test]
fn egress_policy_comes_only_from_the_folded_authority_constructor() {
    let policy = folded_policy();
    // The allowlist is exactly the folded set — read-only view, no mutation seam.
    assert_eq!(policy.allowlist().len(), 1);
    assert!(policy.allows("github.com"));
    // A host NOT folded in is not allowed — deny-by-default at the policy level.
    assert!(
        !policy.allows("evil.example.com"),
        "a host that was never folded in is not allowlisted (deny-by-default)"
    );
    // The injection map is likewise folded-only: github.com maps, an unfolded
    // host does NOT (a run cannot route itself a secret).
    assert!(
        policy.secret_for("github.com").is_some(),
        "the folded host→secret mapping is present"
    );
    assert!(
        policy.secret_for("evil.example.com").is_none(),
        "an unfolded host has NO secret mapping — a run-supplied value cannot add one \
         (CRITERION 6, the credential-brokering half of the no-widening guard)"
    );
    // The confinement axis the mechanism enforces is the policy's, full stop: a
    // `SpawnPlan` is NOT a parameter of any `EgressPolicy` constructor. This is
    // the interface decision the implementer MUST honor (see the file header).
    let _plan_is_never_a_policy_source =
        SpawnPlan::for_claude_code(&AgentSpec::default(), "badge", std::env::vars());
}

/// CRITERION 6 (allowlist arm) — the folded allowlist is not widenable through
/// the policy: adversarial run-supplied hosts (`evil.example.com`) are never
/// `allows()`-true. The policy `allows` is a pure function of the FOLDED
/// allowlist; a plan cannot add to it because a plan is not a policy input at
/// all. This is the type-system half of the guard, asserted behaviorally.
#[test]
fn run_supplied_hosts_do_not_widen_the_allowlist() {
    let policy = folded_policy();
    // None of the adversarial run-supplied hosts became allowlisted — they were
    // never a policy input (a plan cannot reach `from_folded_authority`).
    for smuggled in ["evil.example.com", "attacker.example.com", "0.0.0.0"] {
        assert!(
            !policy.allows(smuggled),
            "CRITERION 6 VIOLATION: a run-supplied host ({smuggled:?}) became allowlisted — \
             the allowlist was widened by argument. The allowlist comes ONLY from the folded \
             policy (the C6/DR-024 escalation lesson, mirrored from C3a)"
        );
    }
}

/// CRITERION 6 (injection-map arm) — the folded secret-injection map is not
/// widenable: a run-supplied host→secret mapping does not appear. The agent
/// cannot route itself a brokered secret it should not receive by naming a
/// destination in its argv/env. This is the credential-brokering half of the
/// no-widening guard (DR-026 §Decision — "which-secret-for-which-destination map
/// is folded authority, never a self-declared arg").
#[test]
fn run_supplied_mappings_do_not_widen_the_injection_map() {
    let policy = folded_policy();
    // The adversarial plan tried to plant `evil.example.com=ghp_attacker...` — it
    // has NO mapping (the map came only from the fold).
    assert!(
        policy.secret_for("evil.example.com").is_none(),
        "CRITERION 6 VIOLATION: a run-supplied host→secret mapping appeared — the injection \
         map was widened by argument, letting a run route itself a secret. The map comes ONLY \
         from folded authority (DR-026 §Decision)"
    );
    // The one folded mapping is intact and carries the RIGHT secret_ref (never the
    // value — the secret_ref is what the fact records, criterion 5).
    let secret = policy
        .secret_for("github.com")
        .expect("the folded mapping is present");
    assert_eq!(
        secret.secret_ref(),
        "github-token",
        "the folded mapping carries its secret_ref (the label the injection fact records, \
         never the value — criterion 5 contract)"
    );
}

/// CRITERION 6 + CRITERION 3 (host analogue) — the connector argv routes egress
/// to the FOLDED proxy address ONLY; an adversarial `--proxy` in the plan argv
/// does NOT reroute egress. The proxy target is the daemon-owned mediator passed
/// to `connector_argv`, never sourced from `plan`. Two DIFFERENT adversarial
/// plans + the SAME proxy addr → the same routing (widening-by-argument would
/// make the route vary with the plan).
///
/// RED: `connector_argv` is `todo!()` → panic. Green once the renderer sources the
/// proxy address from the folded arg ONLY and never folds `plan.args`/`plan.env`.
#[test]
fn connector_routes_only_to_the_folded_proxy_not_a_plan_supplied_one() {
    let proxy = "10.0.0.1:9999"; // the daemon-owned mediator (folded)
    let argv_a = connector_argv(&adversarial_plan(), proxy);
    let joined_a = argv_a.join(" ");

    // The folded proxy address IS the routing target.
    assert!(
        joined_a.contains(proxy),
        "the connector routes outbound to the FOLDED proxy address; got {argv_a:?}"
    );
    // The adversarial plan-supplied proxy did NOT reroute egress.
    assert!(
        !joined_a.contains("attacker.example.com"),
        "CRITERION 6/3 VIOLATION: a run-supplied --proxy ({:?}) rerouted egress off the \
         daemon-owned mediator — the route was widened by argument. The proxy target comes \
         ONLY from the folded mediator, never the plan; argv: {argv_a:?}",
        "http://attacker.example.com:8080"
    );

    // The route depends ONLY on the folded proxy addr, not the plan: a second
    // adversarial plan with different smuggled values yields the same routing.
    let mut plan_b = adversarial_plan();
    plan_b.args.push("--proxy".to_string());
    plan_b
        .args
        .push("http://other.attacker.example:1234".to_string());
    let argv_b = connector_argv(&plan_b, proxy);
    assert_eq!(
        argv_a, argv_b,
        "the connector routing depends ONLY on the folded proxy address — a different \
         adversarial plan must not change it (CRITERION 6, no widening by argument)"
    );
}
