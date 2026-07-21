//! C3 run-loop composition seam (c3-wire — DR-028): splice the C3a sandbox
//! (`bwrap`) INSIDE the c3bc egress connector (`pasta`) on ONE shared netns, so a
//! live governed run is BOTH filesystem-confined AND egress-mediated. This module
//! is the composition the daemon spawn path wires: it renders the pasta-outer
//! wrapper argv (host-inspectable), decides the three composed degrade states as
//! distinct loud facts, owns the composed child as a real `tokio::process::Child`,
//! and (unix) starts the live composed dataplane on the shared netns.
//!
//! ## Why pasta-outer (DR-028 §Decision 1 — the ordering hazard, settled)
//!
//! The composition is `pasta → bwrap → agent`: pasta seals the userspace net and
//! execs `bwrap`-execs-`agent` INSIDE the already-sealed netns. There is no window
//! in which the agent runs before the net route is the proxy-only route, because
//! pasta's exec IS the agent's ancestry. bwrap therefore MUST NOT re-unshare the
//! network (that would drop the agent into a fresh empty netns with no route out);
//! under active egress it uses the unshare-all-MINUS-net posture
//! ([`crate::sandbox::bwrap_argv_shared_netns`]), inheriting pasta's sealed netns.
//!
//! ## No widening (C6/DR-024 preserved end-to-end)
//!
//! Binds/allowlist/secrets reach the wrapper ONLY through the folded
//! [`SandboxPolicy`]/[`EgressPolicy`] (`from_folded_authority` is the sole door);
//! the proxy route is the folded `proxy_addr` ONLY. A run-supplied value in
//! `plan.args`/`plan.env` cannot widen either half — it rides only AFTER the agent
//! handoff as the confined program's OWN args.

use serde_json::{Value, json};

use crate::egress::{EgressAvailability, connector_argv};
use crate::sandbox::{Availability, Bind, SandboxPolicy, bwrap_argv, bwrap_argv_shared_netns};
use crate::spawner::SpawnPlan;

/// The `pasta` connector binary the composed wrapper execs as its outermost
/// program (mirrors `PastaProxy`'s default `connector_bin`). A pure-argv render
/// uses this literal so the host-testable shape names pasta as argv[0]; the live
/// [`start_composed_dataplane`] resolves the real binary through the wired proxy.
const PASTA_BIN: &str = "pasta";

/// The `bwrap` binary the composed wrapper execs as its MIDDLE program (mirrors
/// `BwrapSubstrate`'s default `bin`).
const BWRAP_BIN: &str = "bwrap";

/// Render the pasta-outer composed argv: `pasta … -- bwrap … -- <agent> <args>`
/// (DR-028 §Decision 1). PURE and inspectable — no netns, no spawn — exactly like
/// [`crate::sandbox::bwrap_argv`], so the host suites pin the wrapper shape without
/// pasta or bwrap present.
///
/// - `pasta` is argv[0] (the OUTERMOST program): it seals the userspace net and
///   routes the netns's sole exit to the folded `proxy_addr`, then execs bwrap.
/// - `bwrap` is the MIDDLE program: it confines the filesystem to the folded
///   binds. When `egress_active` it DROPS `--unshare-net` (uses the
///   unshare-all-MINUS-net posture) so the agent inherits pasta's already-sealed
///   netns; when NOT active there is no shared netns to inherit, so it keeps the
///   full `--unshare-all` (net confined the C3a-alone way).
/// - `<agent> <args>` is the INNERMOST program: the confined harness.
///
/// The wrapper directives depend ONLY on the folded `sandbox`/`proxy_addr`; the
/// run-supplied `plan.args`/`plan.env` ride ONLY after the final `--` (the C6
/// no-widening guard — [`bwrap_argv`] and [`connector_argv`] never read the plan
/// for wrapper directives).
pub fn composed_argv(
    plan: &SpawnPlan,
    sandbox: &SandboxPolicy,
    egress_active: bool,
    proxy_addr: &str,
) -> Vec<String> {
    // ---- pasta (outermost): seal the userspace net, route to the folded proxy.
    // argv[0]=pasta, then configure the tap interface (netns addr + the gateway the
    // proxy maps to), quiet + foreground so the daemon owns the child. Mirrors the
    // live `unix_dataplane::PastaHandle::run_confined` invocation.
    let mut argv: Vec<String> = vec![
        PASTA_BIN.to_string(),
        "--config-net".to_string(),
        "-q".to_string(),
        "-f".to_string(),
    ];
    // The routing directives (all outbound TCP+DNS to the folded proxy). Sourced
    // from the folded `proxy_addr` ONLY — never `plan` (the C6 no-widening route).
    argv.extend(connector_argv(plan, proxy_addr));
    // Hand off to the confined program (bwrap).
    argv.push("--".to_string());

    // ---- bwrap (middle): confine the filesystem to the folded binds. Under active
    // egress DROP the net unshare so the agent inherits pasta's sealed netns.
    argv.push(BWRAP_BIN.to_string());
    let bwrap = if egress_active {
        bwrap_argv_shared_netns(plan, sandbox)
    } else {
        bwrap_argv(plan, sandbox)
    };
    argv.extend(bwrap);
    // Hand off to the confined program (the agent) — this is the LAST `--`; the
    // run-supplied argv rides only after it, as the agent's OWN args.
    argv.push("--".to_string());

    // ---- agent (innermost): the confined harness + its run-supplied args.
    argv.push(plan.bin.to_string_lossy().into_owned());
    argv.extend(plan.args.iter().cloned());

    argv
}

/// The read-only bind(s) the confined program's OWN BINARY needs to be exec'able
/// inside bwrap's sealed mount namespace (DR-028 §Decision 1, the
/// confined-program-must-be-reachable property). Under pasta-outer, bwrap seals
/// the mount-ns and `execvp`s the harness; if the harness binary's directory is
/// not bind-mounted, bwrap reports `No such file or directory` even though the file
/// exists on the host. This folds the DIRECTORY containing `plan.bin` READ-ONLY.
///
/// C6/DR-024 HOLDS: the harness identity is DECLARED authority (the spec's
/// `bin_override` / the resolved harness path the daemon computes), NOT a
/// run-supplied/request-time value — it is folded daemon-side, exactly like the
/// toolchain binds, and reaches the policy only through `from_folded_authority`.
/// A run cannot point this at an arbitrary path: it names the very binary the
/// composed spawn is ABOUT to exec, so binding its directory grants no reach the
/// spawn does not already imply. The real fold (`runs.rs::fold_c3_policies`) and
/// the WSL fixture share THIS one definition of "bind what you're about to exec".
///
/// Returns an empty vec when `plan.bin` has no parent (a bare relative name
/// resolved via `PATH` inside the namespace's own `--ro-bind`ed toolchain) — never
/// a widening bind of `/`.
pub fn confined_program_binds(plan: &SpawnPlan) -> Vec<Bind> {
    // Canonicalize so a symlinked/relative harness path resolves to the real
    // on-disk directory bwrap must bind (best-effort: an unresolvable path folds
    // to its lexical parent, never `/`). No filesystem WRITE, no run-supplied
    // widening — the target is the declared harness binary only.
    let resolved = std::fs::canonicalize(&plan.bin).unwrap_or_else(|_| plan.bin.clone());
    match resolved.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => vec![Bind::read_only(dir)],
        // A bare name (no directory component) resolves via the toolchain binds
        // already folded; nothing extra to bind (and never `/`).
        _ => Vec::new(),
    }
}

/// The three composed run states (DR-028 §Decision 4) — the product of C3a's
/// loud-OPEN sandbox degrade and c3bc's CLOSED egress degrade. Each is a DISTINCT
/// loud fact ([`degrade_fact`]); none silently claims enforcement it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposedDegrade {
    /// sandbox-up + egress-up ⇒ confined + mediated (pasta-outer, shared netns).
    Mediated,
    /// sandbox-up + egress-DOWN ⇒ confined + CLOSED: keep the sealed netns, no
    /// network, no injection, loud `egress.unavailable`. NEVER confined + open.
    ConfinedClosed,
    /// sandbox-DOWN ⇒ unsandboxed AND egress un-enforceable (no sealed netns for
    /// egress to be the sole route out of), regardless of the egress backend's own
    /// availability — the load-bearing sandbox-first dependency.
    Unsandboxed,
}

/// Decide the composed degrade state from the two availability verdicts (DR-028
/// §Decision 4). The dependency is sandbox-FIRST: a DOWN sandbox makes egress
/// un-enforceable regardless of the egress backend's own availability (there is no
/// sealed netns to mediate over), so it short-circuits to [`ComposedDegrade::Unsandboxed`].
pub fn compose_degrade(
    sandbox_avail: &Availability,
    egress_avail: &EgressAvailability,
) -> ComposedDegrade {
    match sandbox_avail {
        // Sandbox down ⇒ unsandboxed; egress cannot enforce with no sealed netns.
        Availability::Unavailable { .. } => ComposedDegrade::Unsandboxed,
        Availability::Available => match egress_avail {
            EgressAvailability::Available => ComposedDegrade::Mediated,
            // Sandbox up + egress down ⇒ confined + CLOSED (keep the sealed netns).
            EgressAvailability::Unavailable { .. } => ComposedDegrade::ConfinedClosed,
        },
    }
}

/// The DISTINCT loud logged fact each composed degrade state yields (DR-028
/// §Decision 4). Returns `(subject, payload)`; the run is named so the fact
/// replays (I3).
///
/// ## WARDEN-GATED subjects — PLACEHOLDER, not ratified (DR-028 §Consequences).
/// The `sandbox.*`/`egress.*` subject family is a DEFERRED warden `/subject`
/// question; the subject strings below are PLACEHOLDERS standing in for the
/// implementer's wiring, keyed off the posture fields the c3bc/c3a fold suites
/// already pin (`network`/`egress_enforceable`/`sandbox`), NOT ratified ontology
/// names. TODO(warden, /subject): mint the family WITH its folding reducer and
/// replace these constants.
///
/// The three payloads are fingerprint-distinct by construction:
/// - Mediated: `network="mediated"`.
/// - ConfinedClosed: `network="sealed"`, `injected=false`, a loggable `reason`.
/// - Unsandboxed: `sandbox="unavailable"`, `egress_enforceable=false` (no silent
///   claim of mediation, no fake handle).
pub fn degrade_fact(degrade: &ComposedDegrade, run: &str) -> (&'static str, Value) {
    match degrade {
        ComposedDegrade::Mediated => (
            // Not a degrade — the enforcing state; its subject carries no
            // `*.unavailable` marker.
            "sandbox.mediated",
            json!({
                "run": run,
                // Confined + mediated over the shared netns.
                "network": "mediated",
                "sandbox": "available",
                "egress_enforceable": true,
            }),
        ),
        ComposedDegrade::ConfinedClosed => (
            // DR-026's CLOSED degrade composed: a distinct loud egress.unavailable.
            "egress.unavailable",
            json!({
                "run": run,
                // The sealed netns is KEPT — no unmediated egress.
                "network": "sealed",
                "sandbox": "available",
                // Egress down does NOT mean open — nothing is injected.
                "injected": false,
                "egress_enforceable": false,
                // Interrogable (I6): the loud degrade carries a loggable reason.
                "reason": "egress backend unavailable — confined + CLOSED (sealed netns, no network)",
            }),
        ),
        ComposedDegrade::Unsandboxed => (
            // The loud-OPEN sandbox degrade composed: unsandboxed AND a declaration
            // that egress is un-enforceable in this run (no sealed netns to mediate
            // over) — never a silent claim of mediation it lacks.
            "sandbox.unavailable",
            json!({
                "run": run,
                // No sealed netns exists; egress mediation is un-enforceable.
                "sandbox": "unavailable",
                "egress_enforceable": false,
                // Deliberately NO `network="mediated"` — the overclaim the threat
                // model forbids. The unsandboxed spawn is loud, not silent.
                "reason": "sandbox unavailable — unsandboxed spawn; egress un-enforceable (no sealed netns)",
            }),
        ),
    }
}

/// The daemon-owned composed child (DR-028 §Decision 2 — the seam reshape). The
/// composed spawn returns THIS: a real `tokio::process::Child` (stdout piped) the
/// run loop drains and the daemon reaper adopts — NOT a bare pid plus a detached
/// orphan waiter (the shape `sandbox.rs` deferred). This finally threads the S1
/// "daemon owns the process" contract through the composed spawn.
#[derive(Debug)]
pub struct ComposedChild {
    /// The backend the spawn ran under (`"pasta+bwrap"` composed, `"bwrap"`
    /// sandbox-only, or `"none"` on the unsandboxed degrade) — recorded for replay.
    backend: String,
    child: tokio::process::Child,
}

impl ComposedChild {
    /// Adopt a spawned composed child under a backend label.
    pub fn new(backend: impl Into<String>, child: tokio::process::Child) -> Self {
        Self {
            backend: backend.into(),
            child,
        }
    }

    /// The backend the composed spawn ran under (the `sandbox.*`/`egress.*` fact's
    /// backend field).
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Take OWNERSHIP of the composed child — the run loop moves it to the daemon
    /// reaper (`wait()`), which owns its lifetime (S1). The owned `tokio::process::Child`
    /// is exactly what the reaper adopts; a pid + detached waiter would not satisfy
    /// this seam.
    pub fn into_child(self) -> tokio::process::Child {
        self.child
    }

    /// Borrow the composed child mutably — the run loop `.stdout.take()`s the piped
    /// stream WITHOUT taking ownership, so the reaper still owns the child (the S1
    /// dual-accessor shape the capture path needs).
    pub fn child_mut(&mut self) -> &mut tokio::process::Child {
        &mut self.child
    }
}

#[cfg(unix)]
pub use unix_compose::start_composed_dataplane;

#[cfg(not(unix))]
pub use non_unix_compose::start_composed_dataplane;

/// Non-unix stub for the composed dataplane: an honest unsupported-platform error,
/// never a fake handle (mirrors `EgressDataplane`'s non-unix `start`). The host
/// (Windows) build carries this, so the unix netns helpers stay off it
/// ([[vet-is-host-side-wsl-insufficient]]).
#[cfg(not(unix))]
mod non_unix_compose {
    use crate::RunError;
    use crate::egress::{DataplaneHandle, EgressPolicy, PastaProxy};
    use crate::sandbox::{BwrapSubstrate, SandboxPolicy};

    /// See the unix impl for the contract. On a non-unix host the composed
    /// dataplane (pasta netns + bwrap namespaces + kernel routing) has no backend.
    pub fn start_composed_dataplane(
        _proxy: &PastaProxy,
        _sandbox: &BwrapSubstrate,
        _egress: &EgressPolicy,
        _sbx_policy: &SandboxPolicy,
    ) -> Result<Box<dyn DataplaneHandle>, RunError> {
        Err(RunError::Spawn(
            "the c3-wire composed dataplane (pasta netns + bwrap confinement + kernel routing) is \
             unix-only; this platform has no live composed backend (DR-028 §Consequences — \
             macOS/Windows composition is later, behind the same traits)"
                .to_string(),
        ))
    }
}

/// The live composed dataplane (unix-only): pasta seals the netns and execs bwrap
/// (unshare-all-minus-net) which execs the confined probe INSIDE the shared netns.
/// Every item is `#[cfg(unix)]` so the host build carries none of it.
#[cfg(unix)]
mod unix_compose {
    use crate::RunError;
    use crate::egress::{ComposedSpliceWiring, DataplaneHandle, EgressPolicy, PastaProxy};
    use crate::sandbox::{BwrapSubstrate, SandboxPolicy, bwrap_argv_shared_netns};
    use crate::spawner::SpawnPlan;

    /// Start the live COMPOSED dataplane for a confined mediated-egress run (DR-028
    /// §Decision 1/2). Splices `bwrap` between pasta and the confined program on ONE
    /// shared netns: pasta seals the userspace net and execs
    /// `bwrap <unshare-all-minus-net> -- <probe>` inside it, so the probe (the
    /// running AGENT) inherits pasta's already-sealed, proxy-only netns rather than
    /// a fresh empty one. Returns the same enforce [`DataplaneHandle`] the escape /
    /// injection probe drives — now through the composed run-loop hand-off.
    ///
    /// The bwrap confinement prefix is rendered from the folded `sbx_policy` ONLY
    /// (the C6 no-widening guard). A provisioning failure is an honest [`RunError`],
    /// never a fake pass; a box missing pasta/bwrap is the caller's early-return.
    pub fn start_composed_dataplane(
        proxy: &PastaProxy,
        sandbox: &BwrapSubstrate,
        egress: &EgressPolicy,
        sbx_policy: &SandboxPolicy,
    ) -> Result<Box<dyn DataplaneHandle>, RunError> {
        // The bwrap binary + the unshare-all-MINUS-net confinement prefix, folded
        // from the sandbox policy ONLY. The placeholder plan is never read for
        // wrapper directives (the C6 guard); the confined program (the probe) is
        // appended by the dataplane after the bwrap `--`, not from a plan.
        let bwrap_bin = sandbox.bin.clone().unwrap_or_else(|| "bwrap".to_string());
        let bwrap_prefix = bwrap_argv_shared_netns(&SpawnPlan::default_placeholder(), sbx_policy);

        // Hand the splice to the egress dataplane's composed start: pasta execs
        // `bwrap <prefix> -- <probe> …` instead of `<probe> …` directly, so the
        // probe runs inside pasta's sealed netns UNDER bwrap confinement. This
        // reuses the enforce path's CA/availability/honesty guard (start_composed
        // delegates to the same start_dataplane), so the composition adds no second
        // door.
        proxy.start_composed(
            egress,
            ComposedSpliceWiring {
                bwrap_bin,
                bwrap_prefix,
            },
        )
    }
}
