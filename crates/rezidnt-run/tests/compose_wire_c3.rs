//! c3-wire oracle (DR-028 — the C3 run-loop integration slice): the HOST-provable
//! arms of criteria 1 (composed spawn path + argv shape), 4 (the three degrade
//! decisions each a distinct loud fact, decision arm), and 5 (the composed spawn
//! returns a daemon-owned child, handle-shape arm).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure argv / decision inspection, no pasta,
//! no bwrap, no #[cfg(unix)]). The composed-argv rendering and the degrade
//! DECISION are pure functions of the folded policies + the two availability
//! verdicts; they need no live netns to test. This whole file runs on every host
//! that builds rezidnt (Windows host /vet included). The LIVE shared-netns
//! inescapability under composition is the `#[cfg(unix)]` WSL-only suite
//! `compose_shared_netns_c3_wire.rs`; the LIVE spawn-through-confinement + reap
//! arms are the daemon WSL suite `bins/rezidentd/tests/spawn_composed_c3_wire.rs`.
//!
//! ## RED MODE — COMPILE-RED (the S4 gate-skeleton precedent, DR-028 §Acceptance).
//! This suite asserts against a NEW composition seam the implementer must add to
//! `rezidnt-run`: a `compose` module rendering the pasta -> bwrap -> agent argv
//! and deciding the three composed degrade states. That module does not exist
//! yet, so the crate fails to compile THIS test target — the honest RED for the
//! missing wiring, NOT an assert around an already-green claim. The implementer's
//! work order is exactly the seam named in the `use rezidnt_run::compose::{…}`
//! below plus the two pure fns / one degrade enum it references.
//!
//! ## The seam this pins (the interface decision the implementer MUST honor)
//! `rezidnt_run::compose` must expose:
//!   - `composed_argv(plan, &SandboxPolicy, egress_active: bool, proxy_addr) ->
//!     Vec<String>` — the pasta-outer wrapper argv: `pasta … -- bwrap … -- <agent>`.
//!     When `egress_active`, bwrap DROPS `--unshare-net` from its unshare set (the
//!     agent inherits pasta's already-sealed netns — DR-028 §Decision 1); when NOT
//!     active, bwrap keeps a full `--unshare-all` (no shared netns to inherit).
//!   - `enum ComposedDegrade { Mediated, ConfinedClosed, Unsandboxed }` +
//!     `compose_degrade(&Availability, &EgressAvailability) -> ComposedDegrade` —
//!     the three-state product (DR-028 §Decision 4).
//!   - `degrade_fact(&ComposedDegrade, run: &str) -> (&'static str, serde_json::Value)`
//!     — the DISTINCT loud logged fact each degrade state yields.

use std::collections::BTreeMap;

use rezidnt_run::egress::{BrokeredSecret, Destination, EgressAvailability, EgressPolicy};
use rezidnt_run::sandbox::{Availability, Bind, SandboxPolicy};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

// The composition seam the implementer must add (DR-028). Referenced (not defined)
// here so this file pins the entry-point NAMES; until the `compose` module exists,
// THIS target fails to compile — the honest RED for the missing run-loop wiring.
use rezidnt_run::compose::{ComposedDegrade, compose_degrade, composed_argv, degrade_fact};

const RUN: &str = "01C3WIRECOMPOSE0000000R001";
const PROXY_ADDR: &str = "10.0.0.1:9999";

/// A benign folded sandbox policy: one writable worktree bind + read-only
/// toolchain, `unshare_all = true`. Stands in for the daemon's fold of the
/// project spec / toolchain layer (DR-028 §Decision 3, honestly-minimal source).
fn folded_sandbox() -> SandboxPolicy {
    SandboxPolicy::from_folded_authority(
        vec![Bind::writable("/work/wt-abc"), Bind::read_only("/usr")],
        true,
    )
}

/// A benign folded egress policy: `github.com` allowlisted + one host->secret
/// mapping. Egress ACTIVE (a non-empty allowlist).
#[allow(dead_code)]
fn folded_egress() -> EgressPolicy {
    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new("github-token", "ghp_folded_secret_do_not_leak"),
    );
    EgressPolicy::from_folded_authority(vec![Destination::host("github.com")], injection)
}

/// A trivial spawn plan (the agent the composed wrapper ultimately execs).
fn agent_plan() -> SpawnPlan {
    let agent = AgentSpec {
        name: "confined".to_string(),
        harness: "claude-code".to_string(),
        bin_override: Some("/opt/agent/claude".into()),
        ..AgentSpec::default()
    };
    SpawnPlan::for_claude_code(&agent, "badge-wire", std::env::vars())
}

/// CRITERION 1 — the composed spawn is pasta -> bwrap -> agent (pasta-outer), and
/// the wrapper shape is correct. The rendered argv must, in order: start with the
/// pasta connector routing all outbound to the folded proxy, then a `--` handing
/// off to `bwrap`, then bwrap's confinement directives, then a `--` handing off
/// to the agent binary. This is the pasta-outer nesting DR-028 §Decision 1
/// settles: pasta seals the netns and execs bwrap-execs-agent INSIDE it.
///
/// COMPILE-RED until `compose::composed_argv` exists.
#[test]
fn composed_argv_nests_pasta_outer_bwrap_inner_agent_innermost() {
    let plan = agent_plan();
    let sandbox = folded_sandbox();
    let argv = composed_argv(&plan, &sandbox, /* egress_active */ true, PROXY_ADDR);
    let joined = argv.join(" ");

    // pasta is the OUTERMOST program (index 0): the connector seals the userspace
    // net and execs its confined program (bwrap) inside the sealed netns.
    assert!(
        argv.first().is_some_and(|a| a.contains("pasta")),
        "CRITERION 1: pasta must be the OUTERMOST program (pasta-outer, DR-028 §Decision 1) — \
         it seals the netns before the agent can emit a packet. argv: {argv:?}"
    );
    // bwrap appears AFTER pasta and BEFORE the agent (the middle wrapper).
    let pasta_pos = argv.iter().position(|a| a.contains("pasta"));
    let bwrap_pos = argv.iter().position(|a| a.contains("bwrap"));
    let agent_pos = argv.iter().position(|a| a.contains("/opt/agent/claude"));
    assert!(
        matches!((pasta_pos, bwrap_pos, agent_pos), (Some(p), Some(b), Some(a)) if p < b && b < a),
        "CRITERION 1: the composition must nest pasta(outer) -> bwrap(middle) -> agent(inner); \
         got pasta@{pasta_pos:?} bwrap@{bwrap_pos:?} agent@{agent_pos:?} in {argv:?}"
    );
    // The folded proxy is the pasta routing target (the sealed netns's sole route).
    assert!(
        joined.contains(PROXY_ADDR),
        "CRITERION 1: pasta routes the sealed netns's sole exit to the folded proxy address; \
         argv: {argv:?}"
    );
    // The folded worktree bind rode through to the bwrap layer (confined to the
    // folded binds — the composed spawn is filesystem-confined, DR-028 crit 1).
    assert!(
        joined.contains("/work/wt-abc"),
        "CRITERION 1: the folded worktree bind must be present in the bwrap layer of the \
         composed argv (confined to the folded binds); argv: {argv:?}"
    );
}

/// CRITERION 1 (the shared-netns argv delta — the load-bearing composition detail)
/// — when egress is ACTIVE, the bwrap layer DROPS `--unshare-net` from its unshare
/// set so the agent INHERITS pasta's already-sealed netns rather than a fresh
/// empty one (DR-028 §Decision 1). When egress is NOT active, there is no shared
/// sealed netns to inherit, so bwrap keeps a full `--unshare-all` (net included).
///
/// This is the ONE argv difference that makes pasta-outer work: if bwrap
/// re-unshared net, the agent would land in a fresh empty netns with NO route out
/// at all — not pasta's proxy-only route — and mediation would be dead, not
/// enforced. A composed argv that keeps `--unshare-net`/`--unshare-all` net-side
/// under active egress is the silent-hole this pins against.
///
/// COMPILE-RED until `compose::composed_argv` exists.
#[test]
fn bwrap_drops_unshare_net_only_when_egress_is_active() {
    let plan = agent_plan();
    let sandbox = folded_sandbox();

    // Egress ACTIVE: the agent inherits pasta's sealed netns, so bwrap must NOT
    // re-unshare the network namespace (no `--unshare-all`, no `--unshare-net`).
    let active = composed_argv(&plan, &sandbox, /* egress_active */ true, PROXY_ADDR);
    let active_joined = active.join(" ");
    assert!(
        !active_joined.contains("--unshare-net"),
        "CRITERION 1: under ACTIVE egress bwrap must DROP --unshare-net so the agent inherits \
         pasta's already-sealed netns (DR-028 §Decision 1); a re-unshared net would drop the \
         agent into a fresh EMPTY netns with no route — mediation dead, not enforced. argv: {active:?}"
    );
    assert!(
        !active_joined.contains("--unshare-all"),
        "CRITERION 1: under ACTIVE egress bwrap uses the unshare_all-MINUS-net posture — a bare \
         --unshare-all re-unshares net and defeats the shared-netns composition. argv: {active:?}"
    );

    // Egress NOT active: no shared sealed netns exists to inherit, so bwrap keeps
    // the full unshare (net included) — the C3a-alone posture. The composed argv
    // must still confine the net when there is no mediated route to share.
    let inactive = composed_argv(&plan, &sandbox, /* egress_active */ false, PROXY_ADDR);
    let inactive_joined = inactive.join(" ");
    assert!(
        inactive_joined.contains("--unshare-all") || inactive_joined.contains("--unshare-net"),
        "CRITERION 1: with egress NOT active there is no sealed netns to inherit, so bwrap must \
         keep the network unshare (full --unshare-all or --unshare-net) — never leave the agent \
         net-unconfined. argv: {inactive:?}"
    );
}

/// CRITERION 4 (decision arm) — sandbox-up + egress-up ⇒ Mediated, and the loud
/// fact says confined + mediated. The composed rule's happy path: both halves up,
/// pasta-outer shared netns.
///
/// COMPILE-RED until `compose::{compose_degrade, degrade_fact}` exist.
#[test]
fn sandbox_up_egress_up_is_mediated_with_a_loud_fact() {
    let degrade = compose_degrade(&Availability::Available, &EgressAvailability::Available);
    assert_eq!(
        degrade,
        ComposedDegrade::Mediated,
        "CRITERION 4: sandbox-up + egress-up ⇒ confined + mediated (DR-028 §Decision 4)"
    );
    let (subject, payload) = degrade_fact(&degrade, RUN);
    assert_eq!(
        payload["run"].as_str(),
        Some(RUN),
        "the composed degrade fact names the run (I3 replayable)"
    );
    // The mediated fact must NOT claim a degrade — it is the enforcing state.
    assert!(
        !subject.contains("unavailable"),
        "CRITERION 4: the mediated state is not a degrade — its fact must not carry an \
         `*.unavailable` degrade subject; got {subject:?} / {payload}"
    );
    assert_eq!(
        payload["network"].as_str(),
        Some("mediated"),
        "CRITERION 4: the mediated fact records network=mediated (confined + mediated over the \
         shared netns); got {payload}"
    );
}

/// CRITERION 4 (decision arm, the CLOSED case) — sandbox-up + egress-DOWN ⇒
/// ConfinedClosed, and the loud fact is an `egress.unavailable`-shaped CLOSED
/// degrade: the sealed netns is KEPT, there is NO network, and NOTHING is
/// injected. This is DR-026's CLOSED degrade composed into the run loop: egress
/// down does NOT mean egress open — it means confined + sealed + no traffic, said
/// loudly (DR-028 §Decision 4, the load-bearing subtlety).
///
/// COMPILE-RED until `compose::{compose_degrade, degrade_fact}` exist.
#[test]
fn sandbox_up_egress_down_is_confined_closed_with_a_loud_egress_unavailable_fact() {
    let degrade = compose_degrade(
        &Availability::Available,
        &EgressAvailability::Unavailable {
            reason: "pasta not found on PATH".to_string(),
        },
    );
    assert_eq!(
        degrade,
        ComposedDegrade::ConfinedClosed,
        "CRITERION 4: sandbox-up + egress-down ⇒ confined + CLOSED (keep the sealed netns, no \
         network) — NEVER confined + open (DR-028 §Decision 4)"
    );
    let (subject, payload) = degrade_fact(&degrade, RUN);
    // The distinct loud fact is egress.unavailable-shaped (the placeholder subject
    // the c3bc fold suite already pins; warden-gated, DR-028 §Consequences).
    assert!(
        subject.contains("egress") && subject.contains("unavailable"),
        "CRITERION 4: the CLOSED degrade emits a distinct loud `egress.unavailable`-shaped fact; \
         got subject {subject:?}"
    );
    assert_eq!(
        payload["network"].as_str(),
        Some("sealed"),
        "CRITERION 4: the CLOSED degrade KEEPS the sealed netns — no unmediated egress. {payload}"
    );
    assert_eq!(
        payload["injected"].as_bool(),
        Some(false),
        "CRITERION 4: the CLOSED degrade injects NOTHING — no leaked secret without mediation. {payload}"
    );
    assert!(
        !payload["reason"].as_str().unwrap_or("").trim().is_empty(),
        "CRITERION 4: the CLOSED degrade carries a LOGGABLE reason (interrogable, I6). {payload}"
    );
}

/// CRITERION 4 (decision arm, the UNSANDBOXED case) — sandbox-DOWN ⇒ Unsandboxed,
/// and the loud fact declares egress is UN-ENFORCEABLE in this degraded run: there
/// is no sealed netns for egress to be the sole route out of, so the run makes NO
/// silent claim of mediation and stands up NO fake handle (DR-028 §Decision 4).
/// The load-bearing composed subtlety: egress mediation is only meaningful when
/// the sandbox netns exists.
///
/// The three states must be DISTINCT facts — this asserts the sandbox-down fact is
/// neither the mediated fact nor a plain egress-CLOSED fact: it must say egress is
/// un-enforceable (mediation impossible), not merely unavailable.
///
/// COMPILE-RED until `compose::{compose_degrade, degrade_fact}` exist.
#[test]
fn sandbox_down_is_unsandboxed_with_a_loud_egress_unenforceable_fact() {
    // Even if the egress backend itself is "Available", a DOWN sandbox makes
    // mediation un-enforceable — there is no sealed netns to be the sole route
    // out of. The composed decision must reflect the sandbox-first dependency.
    let degrade = compose_degrade(
        &Availability::Unavailable {
            reason: "bwrap not found on PATH".to_string(),
        },
        &EgressAvailability::Available,
    );
    assert_eq!(
        degrade,
        ComposedDegrade::Unsandboxed,
        "CRITERION 4: sandbox-down ⇒ unsandboxed AND egress un-enforceable (no sealed netns to \
         mediate over), regardless of the egress backend's own availability (DR-028 §Decision 4)"
    );
    let (subject, payload) = degrade_fact(&degrade, RUN);
    // Distinct from BOTH other states: the fact must state egress is
    // un-enforceable (mediation impossible), not merely unavailable, and must NOT
    // silently claim mediation.
    assert_ne!(
        payload["network"].as_str(),
        Some("mediated"),
        "CRITERION 4: a sandbox-down run must NEVER silently claim network=mediated — that is the \
         overclaim DR-028's threat model forbids. {payload}"
    );
    assert_eq!(
        payload["sandbox"].as_str(),
        Some("unavailable"),
        "CRITERION 4: the sandbox-down fact records the sandbox as unavailable (the unsandboxed \
         spawn is loud, not silent — DR-025 loud-OPEN degrade composed). {payload}"
    );
    assert_eq!(
        payload["egress_enforceable"].as_bool(),
        Some(false),
        "CRITERION 4: the sandbox-down fact declares egress UN-ENFORCEABLE — no fake handle, no \
         silent claim of mediation it lacks (DR-028 §Decision 4, the load-bearing subtlety). {payload}"
    );
    assert!(
        !subject.is_empty(),
        "CRITERION 4: the unsandboxed state emits a non-empty loud subject"
    );
}

/// CRITERION 4 (the three-are-DISTINCT guard) — the three composed degrade states
/// yield THREE distinct loud facts; none is silently equal to another. A design
/// that collapsed two states into one fact would let a run claim (or fail to
/// disclaim) enforcement it does not have — the exact honesty failure DR-028
/// §Decision 4 forbids ("none silently claiming enforcement it lacks").
///
/// COMPILE-RED until `compose::{compose_degrade, degrade_fact}` exist.
#[test]
fn the_three_degrade_states_emit_three_distinct_facts() {
    let mediated = degrade_fact(&ComposedDegrade::Mediated, RUN);
    let closed = degrade_fact(&ComposedDegrade::ConfinedClosed, RUN);
    let unsandboxed = degrade_fact(&ComposedDegrade::Unsandboxed, RUN);

    // Fingerprint each fact by (subject, network, egress_enforceable/injected) — the
    // distinguishing shape. All three must differ pairwise.
    let fingerprint = |(subject, payload): &(&'static str, serde_json::Value)| -> String {
        format!(
            "{subject}|net={}|enf={}|inj={}",
            payload["network"].as_str().unwrap_or("?"),
            payload["egress_enforceable"]
                .as_bool()
                .map(|b| b.to_string())
                .unwrap_or_else(|| "?".into()),
            payload["injected"]
                .as_bool()
                .map(|b| b.to_string())
                .unwrap_or_else(|| "?".into()),
        )
    };
    let fm = fingerprint(&mediated);
    let fc = fingerprint(&closed);
    let fu = fingerprint(&unsandboxed);
    assert_ne!(
        fm, fc,
        "CRITERION 4: mediated and confined-CLOSED must be DISTINCT facts"
    );
    assert_ne!(
        fc, fu,
        "CRITERION 4: confined-CLOSED and unsandboxed must be DISTINCT facts"
    );
    assert_ne!(
        fm, fu,
        "CRITERION 4: mediated and unsandboxed must be DISTINCT facts"
    );
}
