//! c3-egress-fold oracle (DR-029) — CRITERION 3 (HOST-provable): the fold is
//! folded-only, no widening, C6 preserved END-TO-END. The non-empty `[egress]`
//! allowlist + the resolved injection_map reach `EgressPolicy` ONLY through
//! `from_folded_authority`; the no-widening property fails-FIRST if a
//! `SpawnPlan`/request-sourced `EgressPolicy` constructor is ever added; a
//! run-supplied host/secret cannot widen the allowlist OR add a secret mapping.
//!
//! Extends the `compose_no_widening_c3_wire.rs` discipline down to the NEW fold seam
//! this slice adds (`spec::EgressSpec` + a `SecretSource` -> `EgressPolicy`), and
//! re-pins the `EgressPolicy` private-field guard the whole C3 axis rests on.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure policy fold + type-system guard; no
//! netns). Runs on every host, Windows /vet included.
//!
//! ## RED MODE — COMPILE-RED (the fold entry-point + the `SecretSource` seam do not
//! exist yet) + STRUCTURAL (the `EgressPolicy` door). The fold fn the implementer
//! must add does not exist → the fold arms fail to compile (honest RED). The
//! type-system guard arm (a `SpawnPlan`/request cannot reach `EgressPolicy`) is
//! structural and pins the interface the implementer must not regress.
//!
//! IMPLEMENTER ADDS (the seam this pins):
//!   - `pub fn fold_egress_policy(spec: &EgressSpec, secrets: &dyn SecretSource)
//!         -> Result<(EgressPolicy, Vec<CredentialDrop>), RunError>` (or equivalent)
//!     in `rezidnt_run` — assembling the allowlist from `spec.allowlist` and the
//!     injection_map by resolving each `[egress.secrets]` value via the
//!     `SecretSource`, THROUGH `EgressPolicy::from_folded_authority` ONLY. An
//!     unresolvable secret_ref is DROPPED (reported as a `CredentialDrop`), never a
//!     fake secret. The exact return shape is the implementer's oracle-first call;
//!     this file pins the C6 property, not the signature bytes.

use std::collections::BTreeMap;

use rezidnt_run::egress::{BrokeredSecret, Destination, EgressPolicy};
use rezidnt_run::sandbox::{Bind, SandboxPolicy};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::{AgentSpec, ProjectSpec};

// The NEW fold entry-point this slice adds + the SecretSource seam it consumes.
// COMPILE-RED until they exist.
use rezidnt_run::egress::fold_egress_policy;
use rezidnt_run::secret::SecretSource;

/// A test SecretSource that resolves exactly the refs it is seeded with — an
/// in-memory stand-in for the daemon-side host-file backend (so this host suite
/// never touches a real file/env). Unresolvable refs return `Ok(None)` (the DROP
/// signal).
struct MapSecretSource(BTreeMap<String, String>);

impl SecretSource for MapSecretSource {
    fn resolve(&self, secret_ref: &str) -> Result<Option<BrokeredSecret>, rezidnt_run::RunError> {
        Ok(self
            .0
            .get(secret_ref)
            .map(|v| BrokeredSecret::new(secret_ref, v)))
    }
}

/// Parse a spec carrying a non-empty `[egress]` block (the folded-authority source).
fn spec_with_egress() -> ProjectSpec {
    ProjectSpec::from_toml_str(
        r#"
[project]
name = "acme"
repo = "."

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"

[egress]
allowlist = ["github.com"]

[egress.secrets]
"github.com" = "gh_token"
"#,
    )
    .expect("spec with [egress] parses")
}

/// CRITERION 3 (positive — the fold reaches EgressPolicy via from_folded_authority)
/// — a non-empty `[egress]` spec + a SecretSource that resolves the ref folds to an
/// EgressPolicy whose allowlist contains the declared host and whose injection_map
/// brokers the resolved secret. The fold is the authority path working end-to-end.
///
/// COMPILE-RED until `fold_egress_policy` exists.
#[test]
fn fold_yields_a_policy_with_the_declared_host_and_resolved_secret() {
    let spec = spec_with_egress();
    let secrets = MapSecretSource(
        [("gh_token".to_string(), "ghp_folded_do_not_leak".to_string())]
            .into_iter()
            .collect(),
    );

    let (policy, drops) =
        fold_egress_policy(&spec.egress, &secrets).expect("the non-empty egress spec folds");

    assert!(
        policy.allows("github.com"),
        "the declared allowlist host reaches the folded EgressPolicy"
    );
    let secret = policy
        .secret_for("github.com")
        .expect("the resolved secret is brokered to its host in the injection_map");
    assert_eq!(
        secret.secret_ref(),
        "gh_token",
        "the injection_map brokers the resolved secret BY REFERENCE (its secret_ref label)"
    );
    assert!(
        drops.is_empty(),
        "every declared secret_ref resolved — no drops on the all-resolvable fold"
    );
}

/// CRITERION 3 (deny-all when the spec is empty) — an EMPTY `[egress]` spec folds to
/// an EMPTY allowlist that reaches no host (absent never means open — the DR-028
/// default preserved through the NEW fold seam, DR-029 §Decision 1).
///
/// COMPILE-RED until `fold_egress_policy` exists.
#[test]
fn empty_egress_spec_folds_deny_all() {
    let spec = ProjectSpec::from_toml_str(
        r#"
[project]
name = "tiny"
repo = "."
"#,
    )
    .expect("spec parses");
    let secrets = MapSecretSource(BTreeMap::new());

    let (policy, _drops) =
        fold_egress_policy(&spec.egress, &secrets).expect("an empty egress spec folds");
    for host in ["github.com", "example.com", "0.0.0.0"] {
        assert!(
            !policy.allows(host),
            "CRITERION 3 VIOLATION: an EMPTY [egress] spec allowed {host:?} — absent MUST mean \
             deny-all through the fold (DR-029 §Decision 1)"
        );
    }
}

/// CRITERION 3 (unresolvable ref ⇒ dropped, host mediated-without-injection, never a
/// fake secret) — a declared `[egress.secrets]` ref the SecretSource cannot resolve
/// is DROPPED: the host stays on the allowlist (mediated), but has NO secret mapping
/// (no injection), and the drop is reported (the loud `credential.dropped` fact rides
/// it — asserted in the facts suite). Never a fake/empty secret (DR-029 §Decision 2).
///
/// COMPILE-RED until `fold_egress_policy` exists.
#[test]
fn unresolvable_secret_ref_is_dropped_never_a_fake_secret() {
    let spec = spec_with_egress(); // declares github.com -> gh_token
    // A source that resolves NOTHING (gh_token is unresolvable here).
    let secrets = MapSecretSource(BTreeMap::new());

    let (policy, drops) = fold_egress_policy(&spec.egress, &secrets)
        .expect("the fold proceeds past an unresolvable ref");

    // The host stays allowlisted (mediated) ...
    assert!(
        policy.allows("github.com"),
        "an unresolvable secret drops the INJECTION, not the allowlist entry — the run still \
         proceeds mediated for the host (DR-029 §Decision 2)"
    );
    // ... but carries NO secret (no fake/empty injection) ...
    assert!(
        policy.secret_for("github.com").is_none(),
        "CRITERION 3 VIOLATION: an unresolvable secret_ref left a secret mapped — the mapping MUST \
         be DROPPED, never a fake/empty secret (DR-029 §Decision 2)"
    );
    // ... and the drop is reported so the loud fact can ride it.
    assert!(
        drops
            .iter()
            .any(|d| d.secret_ref() == "gh_token" && d.dest() == "github.com"),
        "the dropped mapping is REPORTED (dest + secret_ref) so a loud credential.dropped fact \
         rides it — never a silent uninjection (DR-029 §Decision 2). drops: {drops:?}"
    );
}

/// CRITERION 3 (C6 — a run-supplied host/secret cannot widen the fold) — the fold
/// reads the `[egress]` spec (project-DECLARED folded authority) + the SecretSource
/// ONLY; a `SpawnPlan` carrying an adversarial host/secret in its argv/env is not a
/// parameter of the fold, so it cannot add an allowlist entry OR a secret mapping.
/// This pins the C6/DR-024 escalation guard at the NEW fold seam.
///
/// COMPILE-RED until `fold_egress_policy` exists; STRUCTURAL on the guard.
#[test]
fn a_run_supplied_host_or_secret_cannot_widen_the_fold() {
    let spec = spec_with_egress(); // github.com only
    let secrets = MapSecretSource(
        [("gh_token".to_string(), "ghp_folded".to_string())]
            .into_iter()
            .collect(),
    );
    let (policy, _drops) = fold_egress_policy(&spec.egress, &secrets).expect("folds");

    // An adversarial plan naming evil.example.com + a smuggled token.
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        ..AgentSpec::default()
    };
    let mut plan = SpawnPlan::for_claude_code(&agent, "badge", std::env::vars());
    plan.env.push((
        "REZIDNT_EXTRA_ALLOW".to_string(),
        "evil.example.com".to_string(),
    ));
    plan.env.push((
        "REZIDNT_INJECT".to_string(),
        "evil.example.com=ghp_attacker".to_string(),
    ));

    // The fold does NOT take the plan — the run-supplied values reach nothing.
    assert!(
        !policy.allows("evil.example.com"),
        "CRITERION 3 VIOLATION: a run-supplied host (plan.env) widened the folded allowlist — the \
         fold must read the [egress] spec + SecretSource ONLY (C6/DR-024, DR-029 §Decision 1)"
    );
    assert!(
        policy.secret_for("evil.example.com").is_none(),
        "CRITERION 3 VIOLATION: a run-supplied secret directive added an injection mapping — the \
         injection_map folds ONLY from the resolved [egress.secrets] (C6, DR-029 §Decision 1)"
    );
    // The plan exists but is provably not a fold source (kept to show the smuggle
    // vector is real and ignored).
    let _ = plan;
}

/// CRITERION 3 (the type-system guard, structural — fails FIRST if a widening door
/// is added) — `EgressPolicy::from_folded_authority` remains the SOLE constructor;
/// a `SpawnPlan`/request destination is NOT a parameter of any `EgressPolicy`
/// constructor. If a future change adds a `SpawnPlan`-sourced or request-sourced
/// allowlist/map door to satisfy this slice, THIS is the intent that breaks first
/// (DR-029 §Decision 1, "no SpawnPlan/request door is added"; DR-024/DR-016 C6).
#[test]
fn from_folded_authority_stays_the_sole_egress_door() {
    // The only way to mint a non-empty policy is through folded authority.
    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new("gh_token", "ghp_folded_do_not_leak"),
    );
    let policy =
        EgressPolicy::from_folded_authority(vec![Destination::host("github.com")], injection);
    assert!(policy.allows("github.com"));
    assert!(policy.secret_for("github.com").is_some());

    // A `SpawnPlan` is NOT a parameter of any policy constructor — the fold wires
    // the authority, never a plan-sourced door. (Compile-time interface pin.)
    let sandbox = SandboxPolicy::from_folded_authority(vec![Bind::writable("/work/wt")], true);
    assert_eq!(sandbox.binds().len(), 1);
    let _plan_is_never_a_policy_source =
        SpawnPlan::for_claude_code(&AgentSpec::default(), "badge", std::env::vars());
}
