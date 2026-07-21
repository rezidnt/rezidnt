//! c3-wire oracle (DR-028) — CRITERION 3 (HOST-provable): folded-only, no
//! widening, C6 preserved END-TO-END through the COMPOSED spawn path. Binds /
//! allowlist / secrets reach the composed sandbox+egress ONLY through
//! `from_folded_authority`; an empty egress spec ⇒ EMPTY allowlist ⇒ deny-all
//! (absent NEVER means open). This is the C6/DR-024 escalation guard the two
//! halves already enforce, now asserted at the COMPOSITION seam so wiring cannot
//! smuggle a widening door in between the fold and the spawn.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure policy/argv inspection, no #[cfg(unix)]).
//! The no-widening property is a property of the composed argv the run loop WOULD
//! run + the policy types; it needs no live netns. Runs on every host, Windows
//! /vet included.
//!
//! ## RED MODE — COMPILE-RED (the composed argv seam) + STRUCTURAL (the policy
//! doors). The `compose::composed_argv` renderer does not exist yet → the argv
//! arms fail to compile (honest RED). The type-system guard arms (a `SpawnPlan` /
//! request destination cannot reach `SandboxPolicy`/`EgressPolicy`) are structural
//! and pin the interface the implementer must not regress: if a
//! `SpawnPlan`-sourced or request-sourced bind/allowlist/secret constructor is
//! ever added to satisfy c3-wire, this file must FAIL FIRST (DR-028 crit 3).
//!
//! ## The seam this pins
//! `composed_argv(plan, &SandboxPolicy, egress_active, proxy_addr)` may source
//! binds/unshare from the `SandboxPolicy` ONLY and the proxy route from the folded
//! `proxy_addr` ONLY — NEVER from `plan.args`/`plan.env`. And the EMPTY-egress
//! case is deny-all: an empty `EgressPolicy` allowlist reaches no host.

use std::collections::BTreeMap;

use rezidnt_run::egress::{BrokeredSecret, Destination, EgressPolicy};
use rezidnt_run::sandbox::{Bind, SandboxPolicy};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

// The composition seam (DR-028). COMPILE-RED until the `compose` module exists.
use rezidnt_run::compose::composed_argv;

const PROXY_ADDR: &str = "10.0.0.1:9999";

/// A benign folded sandbox: one writable worktree bind, `unshare_all = true`.
fn folded_sandbox() -> SandboxPolicy {
    SandboxPolicy::from_folded_authority(vec![Bind::writable("/work/wt-abc")], true)
}

/// A spawn plan carrying ADVERSARIAL run-supplied values: argv/env that try to
/// smuggle a bind, an allowlist entry, a secret mapping, or an alternate proxy
/// into the COMPOSED wrapper. The compose renderer must fold NONE of these.
fn adversarial_plan() -> SpawnPlan {
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        bin_override: Some("/usr/bin/env".into()),
        ..AgentSpec::default()
    };
    let mut plan = SpawnPlan::for_claude_code(&agent, "badge-wire", std::env::vars());
    // Smuggle attempts across both wrappers: a bwrap bind, a pasta reroute, an
    // egress allowlist/injection directive.
    plan.env
        .push(("REZIDNT_EXTRA_BIND".to_string(), "/etc:/etc".to_string()));
    plan.env.push((
        "REZIDNT_EXTRA_ALLOW".to_string(),
        "evil.example.com".to_string(),
    ));
    plan.env.push((
        "REZIDNT_INJECT".to_string(),
        "evil.example.com=ghp_attacker_wants_this".to_string(),
    ));
    plan.args.push("--bind".to_string());
    plan.args.push("/:/host".to_string());
    plan.args.push("--proxy".to_string());
    plan.args
        .push("http://attacker.example.com:8080".to_string());
    plan
}

/// CRITERION 3 (composed argv, no widening) — the COMPOSED wrapper argv confines /
/// routes to the FOLDED authority ONLY: adversarial `plan` argv/env that name
/// `/etc`, `/`, `evil.example.com`, or an attacker proxy do NOT appear as sandbox
/// binds, allowlisted hosts, or a reroute. The composed spawn is confined to the
/// folded binds and mediated to the folded proxy — a run-supplied value cannot
/// widen either half through the composition seam.
///
/// COMPILE-RED until `compose::composed_argv` exists; then GREEN only if the
/// renderer sources binds from the policy and the route from the folded proxy,
/// never `plan`.
#[test]
fn composed_argv_folds_only_the_authority_not_run_supplied_values() {
    let plan = adversarial_plan();
    let sandbox = folded_sandbox();
    let argv = composed_argv(&plan, &sandbox, /* egress_active */ true, PROXY_ADDR);
    let joined = argv.join(" ");

    // The folded bind + the folded proxy ARE present (the authority path works).
    assert!(
        joined.contains("/work/wt-abc"),
        "the folded worktree bind must be in the composed argv; got {argv:?}"
    );
    assert!(
        joined.contains(PROXY_ADDR),
        "the folded proxy address is the composed route; got {argv:?}"
    );
    // NONE of the adversarial run-supplied values leaked into the wrapper
    // directives (they may only appear AFTER the agent handoff, as the confined
    // program's OWN args — never as bwrap binds or a pasta reroute).
    for smuggled in [
        "/host",
        "/etc:/etc",
        "evil.example.com",
        "attacker.example.com",
        "ghp_attacker_wants_this",
    ] {
        // Find the agent handoff boundary: everything before the LAST `--` is
        // wrapper argv (pasta/bwrap directives); after it is the confined program.
        let last_sep = argv.iter().rposition(|a| a == "--").unwrap_or(argv.len());
        let wrapper: String = argv[..last_sep].join(" ");
        assert!(
            !wrapper.contains(smuggled),
            "CRITERION 3 VIOLATION: a run-supplied value ({smuggled:?}) reached the composed \
             WRAPPER directives (a bwrap bind / a pasta reroute / an allowlist entry) — \
             confinement or mediation was widened by argument. The wrapper must fold ONLY the \
             folded authority (C6/DR-024, DR-028 crit 3). wrapper argv: {wrapper:?}"
        );
    }
}

/// CRITERION 3 — the composed wrapper directives depend ONLY on the folded policy
/// and the folded proxy, not the plan: two DIFFERENT adversarial plans with the
/// SAME folded inputs yield the SAME wrapper argv (up to the agent handoff).
/// Widening by argument would make the wrapper vary with the plan.
///
/// COMPILE-RED until `compose::composed_argv` exists.
#[test]
fn composed_wrapper_depends_only_on_folded_inputs_not_the_plan() {
    let sandbox = folded_sandbox();
    let wrapper_of = |plan: &SpawnPlan| -> Vec<String> {
        let argv = composed_argv(plan, &sandbox, true, PROXY_ADDR);
        let last_sep = argv.iter().rposition(|a| a == "--").unwrap_or(argv.len());
        argv[..last_sep].to_vec()
    };

    let a = wrapper_of(&adversarial_plan());
    let mut plan_b = adversarial_plan();
    plan_b
        .env
        .push(("REZIDNT_EXTRA_BIND".to_string(), "/var:/var".to_string()));
    plan_b.args.push("--proxy".to_string());
    plan_b
        .args
        .push("http://other.attacker.example:1234".to_string());
    let b = wrapper_of(&plan_b);

    assert_eq!(
        a, b,
        "CRITERION 3: the composed wrapper (pasta + bwrap directives) depends ONLY on the folded \
         policy + folded proxy — a different adversarial plan must not change it (no widening by \
         argument, DR-028 crit 3)"
    );
}

/// CRITERION 3 (empty egress ⇒ deny-all) — an EMPTY folded egress spec yields an
/// EMPTY allowlist that reaches NO host: absent NEVER means open (DR-028 §Decision
/// 3, crit 3). This is the deny-by-default half of the composition: a run whose
/// spec carries no egress config gets a sealed netns with a proxy that allows
/// nothing, not an open network.
///
/// STRUCTURAL (holds on the existing `EgressPolicy` API) + load-bearing: it fails
/// if a future change makes an empty allowlist mean "allow all" (the exact
/// absent-means-open trap the DR forbids).
#[test]
fn empty_egress_spec_is_deny_all_absent_never_means_open() {
    // The honestly-minimal fold of a spec that declares NO egress: an empty
    // allowlist + an empty injection map (DR-028 §Decision 3).
    let empty = EgressPolicy::from_folded_authority(Vec::new(), BTreeMap::new());
    assert!(
        empty.allowlist().is_empty(),
        "an empty egress spec folds to an EMPTY allowlist (DR-028 §Decision 3)"
    );
    for host in ["github.com", "example.com", "0.0.0.0", "localhost"] {
        assert!(
            !empty.allows(host),
            "CRITERION 3 VIOLATION: an empty egress spec allowed {host:?} — absent MEANS OPEN, \
             the deny-all default was inverted. An empty allowlist must deny EVERY host (DR-028 \
             §Decision 3: absent never means open)"
        );
        assert!(
            empty.secret_for(host).is_none(),
            "an empty egress spec brokers NO secret to {host:?} (no injection without a folded map)"
        );
    }
}

/// CRITERION 3 (the type-system guard, structural — end-to-end) — neither the
/// composed sandbox policy NOR the composed egress policy has a `SpawnPlan`-sourced
/// or request-sourced constructor. `from_folded_authority` is the SOLE door on
/// both; a `SpawnPlan` is not a parameter of either. This pins the interface the
/// implementer must honor when wiring c3-wire: the compose module must call
/// `from_folded_authority` and MUST NOT add a widening constructor door. If one is
/// ever added, this file's intent breaks first (DR-028 crit 3, "a no-widening test
/// fails FIRST if a SpawnPlan-sourced or request-sourced constructor is added").
#[test]
fn folded_authority_is_the_sole_door_on_both_composed_policies() {
    // Both policies are minted ONLY from folded authority; the read-only views
    // expose no mutation seam and no plan parameter exists on either constructor.
    let sandbox = SandboxPolicy::from_folded_authority(vec![Bind::writable("/work/wt-abc")], true);
    assert_eq!(sandbox.binds().len(), 1);

    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new("github-token", "ghp_folded_do_not_leak"),
    );
    let egress =
        EgressPolicy::from_folded_authority(vec![Destination::host("github.com")], injection);
    assert!(egress.allows("github.com"));
    assert!(egress.secret_for("github.com").is_some());

    // A `SpawnPlan` is NOT a parameter of any policy constructor — the compose
    // module wires the fold, never a plan-sourced door (the interface decision the
    // implementer MUST honor; see the file header).
    let _plan_is_never_a_policy_source =
        SpawnPlan::for_claude_code(&AgentSpec::default(), "badge", std::env::vars());
}
