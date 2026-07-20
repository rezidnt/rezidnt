//! C3a oracle (DR-025 — the Linux OS-sandbox slice), CRITERION 3 — the
//! LOAD-BEARING security test: confinement CANNOT be widened by any run-supplied
//! input. This is the C6/DR-024 privilege-escalation lesson applied to the
//! sandbox: an input that WIDENS confinement (adds a bind, adds an
//! unshare-exception) must come from FOLDED AUTHORITY, or the sandbox is
//! escapable-by-argument (design §5, §8.3).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure argv/policy inspection, no bwrap, no
//! #[cfg(unix)]). The no-widening property is a property of the POLICY and the
//! argv the substrate WOULD run — it needs no confined process to test. This
//! whole file runs on every host that builds rezidnt (Windows host /vet
//! included). The REAL bwrap confinement is the `#[cfg(unix)]` WSL-only suite.
//!
//! RED MODE: **assert-red**. The adversarial tests drive `bwrap_argv` (the pure
//! arg renderer), which is `todo!()` → panic, until the implementer writes it.
//! The type-system guard tests (a `SpawnPlan` arg cannot reach `SandboxPolicy`)
//! are structural and pin the interface shape the implementer must honor.
//!
//! ## The seam this pins (the interface decision the implementer MUST honor)
//! `SandboxPolicy::binds` is PRIVATE and set ONLY via
//! `SandboxPolicy::from_folded_authority(binds, unshare_all)`. There is no
//! constructor taking a `SpawnPlan`, an env var, or a request arg. So the ONLY
//! way a bind reaches the mechanism is through the folded-authority path — the
//! type system enforces the C6 guard. If a future change adds a
//! `SpawnPlan`-sourced bind constructor, THIS is the file that must fail first.

use rezidnt_run::sandbox::{Bind, SandboxPolicy, bwrap_argv};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

/// A benign folded confinement: one writable worktree bind. This stands in for
/// the daemon's fold of the project-spec `[gates.permit]`/role layer (DR-025
/// §Decision) — the AUTHORITY the binds legitimately come from.
fn folded_policy() -> SandboxPolicy {
    SandboxPolicy::from_folded_authority(vec![Bind::writable("/work/wt-abc")], true)
}

/// A spawn plan carrying ADVERSARIAL run-supplied values: argv and env that try
/// to smuggle in a bind / an escape. A non-permit plan is fine — we only inspect
/// what the substrate does with `plan` vs `policy`.
fn adversarial_plan() -> SpawnPlan {
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        // The agent tries to name its OWN binary path — a classic escape lever.
        bin_override: Some("/usr/bin/env".into()),
        ..AgentSpec::default()
    };
    let mut plan = SpawnPlan::for_claude_code(&agent, "badge-wire", std::env::vars());
    // Adversarial env: the agent (or a compromised harness) plants values that
    // LOOK like bind directives, hoping the substrate folds env into the argv.
    plan.env
        .push(("REZIDNT_EXTRA_BIND".to_string(), "/etc:/etc".to_string()));
    plan.env
        .push(("BWRAP_ARGS".to_string(), "--bind /root /root".to_string()));
    // Adversarial argv: a `--bind` string smuggled into the harness args.
    plan.args.push("--bind".to_string());
    plan.args.push("/:/host".to_string());
    plan
}

/// CRITERION 3 (the load-bearing test) — the rendered `bwrap` argv confines to
/// the FOLDED binds ONLY. Adversarial `plan` argv/env that name `/`, `/etc`,
/// `/root` as binds do NOT appear as sandbox binds: the argv binds ONLY
/// `/work/wt-abc` (the folded authority). A run-supplied value cannot widen
/// confinement.
///
/// RED: `bwrap_argv` is `todo!()` → panic. Green once the renderer sources binds
/// from `policy` ONLY and never folds `plan.args`/`plan.env` into bind directives.
#[test]
fn run_supplied_args_do_not_widen_the_bwrap_binds() {
    let plan = adversarial_plan();
    let policy = folded_policy();
    let argv = bwrap_argv(&plan, &policy);
    let joined = argv.join(" ");

    // The folded bind IS present.
    assert!(
        joined.contains("/work/wt-abc"),
        "the folded worktree bind must be in the argv; got {argv:?}"
    );
    // NONE of the adversarial run-supplied paths leaked in as a bind. If the
    // renderer folded plan.env or plan.args into bind directives, one of these
    // would appear as a `--bind`/`--ro-bind` target.
    for smuggled in ["/host", "/root", "/:/host", "/etc:/etc"] {
        assert!(
            !joined.contains(smuggled),
            "CRITERION 3 VIOLATION: a run-supplied value ({smuggled:?}) reached the \
             sandbox binds — confinement was widened by argument. Binds must come ONLY \
             from the folded policy (the C6/DR-024 escalation lesson). argv: {argv:?}"
        );
    }
    // Belt-and-suspenders: the argv binds EXACTLY the folded set (count the
    // bind flags — one folded bind → exactly one bind directive, not three).
    let bind_flags = argv
        .iter()
        .filter(|a| a.as_str() == "--bind" || a.as_str() == "--ro-bind")
        .count();
    assert_eq!(
        bind_flags, 1,
        "exactly the ONE folded bind is rendered — not the folded bind PLUS the \
         smuggled ones (CRITERION 3); argv: {argv:?}"
    );
}

/// CRITERION 3 — the argv is a pure function of (plan, folded policy): the SAME
/// folded policy yields the SAME binds REGARDLESS of what the adversarial plan
/// carries. Two DIFFERENT adversarial plans + the SAME folded policy → the same
/// bind directives. Widening-by-argument would make the binds vary with the plan.
#[test]
fn binds_depend_only_on_folded_policy_not_the_plan() {
    let policy = folded_policy();

    let argv_a = bwrap_argv(&adversarial_plan(), &policy);
    // A second plan with DIFFERENT smuggled values.
    let mut plan_b = adversarial_plan();
    plan_b
        .env
        .push(("REZIDNT_EXTRA_BIND".to_string(), "/var:/var".to_string()));
    plan_b.args.push("/proc:/proc".to_string());
    let argv_b = bwrap_argv(&plan_b, &policy);

    let binds = |argv: &[String]| -> Vec<String> {
        argv.windows(2)
            .filter(|w| w[0] == "--bind" || w[0] == "--ro-bind")
            .map(|w| format!("{} {}", w[0], w[1]))
            .collect()
    };
    assert_eq!(
        binds(&argv_a),
        binds(&argv_b),
        "the bind directives depend ONLY on the folded policy — a different \
         adversarial plan must not change them (CRITERION 3, no widening by argument)"
    );
}

/// CRITERION 3 (unshare-exception arm) — a run-supplied value cannot add an
/// unshare-EXCEPTION either: the folded policy says `unshare_all = true`, so the
/// argv must carry `--unshare-all` and MUST NOT carry any un-share exception
/// (e.g. `--share-net`) sourced from the plan. The network-namespace unshare is
/// the foundation C3b (egress proxy) requires; a plan that could re-share the
/// net would defeat it (design §4).
///
/// RED: `bwrap_argv` is `todo!()` → panic. Green once the renderer takes unshare
/// from `policy` and never from the plan.
#[test]
fn run_supplied_args_cannot_add_an_unshare_exception() {
    let mut plan = adversarial_plan();
    // Adversarial: try to re-share the network namespace via argv/env.
    plan.args.push("--share-net".to_string());
    plan.env
        .push(("BWRAP_SHARE".to_string(), "net".to_string()));
    let policy = folded_policy(); // unshare_all = true

    let argv = bwrap_argv(&plan, &policy);
    let joined = argv.join(" ");
    assert!(
        joined.contains("--unshare-all"),
        "the folded unshare_all=true renders --unshare-all; got {argv:?}"
    );
    assert!(
        !joined.contains("--share-net"),
        "CRITERION 3 VIOLATION: a run-supplied --share-net re-opened the network \
         namespace — an unshare-exception was added by argument. The unshare set \
         comes ONLY from the folded policy (design §4/§8.3); argv: {argv:?}"
    );
}

/// CRITERION 3 (the type-system guard, structural) — a `SandboxPolicy` can be
/// built ONLY from folded authority. This test COMPILES the one sanctioned
/// constructor and documents that there is no `SpawnPlan`-sourced sibling; the
/// private `binds` field means a caller literally cannot mint a policy from a
/// run-supplied value outside `from_folded_authority`. If a widening constructor
/// is ever added, this file's intent (and the arg tests above) break first.
///
/// This one is not RED (it is a structural pin, like the exec no-vendored-engine
/// guard) — it asserts the SHAPE the implementer must not regress.
#[test]
fn policy_binds_come_only_from_the_folded_authority_constructor() {
    let policy = SandboxPolicy::from_folded_authority(
        vec![
            Bind::writable("/work/wt-abc"),
            Bind::read_only("/opt/toolchain"),
        ],
        true,
    );
    // The binds are exactly the folded ones — read-only view, no mutation seam.
    assert_eq!(policy.binds().len(), 2);
    assert!(policy.unshare_all());
    // The confinement axis the mechanism enforces is the policy's, full stop:
    // a `SpawnPlan` is NOT a parameter of any `SandboxPolicy` constructor. This
    // is the interface decision the implementer MUST honor (see the file header).
    let _plan_is_never_a_policy_source =
        SpawnPlan::for_claude_code(&AgentSpec::default(), "badge", std::env::vars());
}
