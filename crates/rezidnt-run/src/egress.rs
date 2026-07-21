//! Egress-proxy substrate seam (C3b+c — DR-026; design
//! `permit-egress-proxy-c3b.md`). The `EgressProxy` (I4) is the TLS-terminating
//! chokepoint that makes the deterministic egress permit verdict (`EgressScope`,
//! an `EgressScope` sibling of `PathConfinement` in `rezidnt-gate`) the ONLY
//! route out of C3a's sealed netns — and brokers a secret the agent never holds.
//!
//! ## Scope: decide + enforce both landed (DR-027 split), NOT YET run-loop-wired
//!
//! DR-026's folded C3b+c was split by DR-027 into **c3bc-decide** and
//! **c3bc-enforce**, both now landed in this module:
//! - **c3bc-decide** (host-provable): the `EgressScope` decision (allow/deny/
//!   escalate from folded policy), the [`EgressPolicy`] no-widening guard, the
//!   redacted [`BrokeredSecret`] type, the connector-argv renderer, the
//!   availability/degrade-CLOSED probe, and the [`RezidntCa`] + `rustls`
//!   terminating-config.
//! - **c3bc-enforce** (the live dataplane, unix, `EgressDataplane`/`PastaHandle`
//!   below): a real `pasta` netns→`rustls`-terminating-proxy→upstream byte-path
//!   with NO route out except the proxy (inescapability, DR-026 crit 3), live TLS
//!   termination, and **real credential injection** at the one `.expose()`
//!   upstream-write (crit 4). Proven by the `#[cfg(unix)]` WSL mediation suite
//!   (real netns + real TLS; 4/4 green).
//!
//! **Still NOT wired into a live daemon run loop.** The dataplane exists and is
//! test-proven behind the `EgressDataplane` seam, but nothing in the daemon/spawn
//! path calls it yet — `start()` with unset wiring returns an honest error, and
//! the `#[cfg(not(unix))]` path is an honest unsupported-platform error. Wiring
//! the enforced spawn into the run loop is its own integration step; until then,
//! product copy must not claim egress is enforced in a shipped run (the DR-027
//! honesty guard, narrowed: the mechanism is real, the run-loop integration is
//! not yet done).
//!
//! ## The load-bearing shape (DR-026 §Decision, mirroring the C3a C6 guard)
//!
//! [`EgressPolicy`] is the egress authority. BOTH its `allowlist` AND its
//! `injection_map` fields are PRIVATE and settable ONLY through
//! [`EgressPolicy::from_folded_authority`] — there is deliberately NO constructor
//! that takes a [`SpawnPlan`] arg, an env var, a request destination, or any
//! run-supplied value. This is the DR-024/DR-016 privilege-escalation guard (the
//! same one `SandboxPolicy` enforces for `binds`) expressed in the type system:
//! a run-supplied value can never WIDEN the allowlist OR add a secret mapping
//! (criterion 6). If a future change adds a `SpawnPlan`-sourced allowlist/map
//! constructor, the criterion-6 test must fail first.
//!
//! ## The secret seam (DR-026 §Decision — credential non-exposure, criterion 5)
//!
//! [`BrokeredSecret`] makes leakage STRUCTURALLY hard: its `Debug`/`Display` are
//! REDACTED (`"<redacted>"`), so a stray `{:?}`/`{}` in a fact, an evidence blob,
//! or a trace line prints the redaction, NEVER the bytes. The value is reachable
//! ONLY through the explicit [`BrokeredSecret::expose`], used solely at the
//! upstream-injection point after TLS termination. The durable injection FACT
//! carries a [`BrokeredSecret::secret_ref`] (a label/hash), never the value.

use std::collections::BTreeMap;

use rezidnt_types::refs::CasRef;
use serde_json::{Value, json};

use crate::RunError;
use crate::spawner::SpawnPlan;

/// A destination the egress allowlist names — a host (and optionally a port).
/// The connector redirects ALL outbound TCP+DNS to the proxy (DR-026 §Decision:
/// transparent interception, not proxy-aware-only); this type is the POLICY axis
/// the `EgressScope` verdict and the proxy agree on, not a per-connection arg.
///
/// A destination is minted only inside [`EgressPolicy::from_folded_authority`]
/// from folded state — never from a spawn arg / request destination (the C6
/// lesson, DR-026 §Decision).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// The allowlisted host (e.g. `"github.com"`). Matched against the SNI/Host
    /// the proxy reads after accepting the netns connection.
    pub host: String,
    /// An optional port constraint; `None` allows any port to `host`.
    pub port: Option<u16>,
}

impl Destination {
    /// An allowlisted host on any port.
    pub fn host(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: None,
        }
    }

    /// An allowlisted host constrained to one port.
    pub fn host_port(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port: Some(port),
        }
    }
}

/// A brokered secret whose bytes are STRUCTURALLY hard to leak (DR-026 §Decision;
/// design §4). The daemon holds it process-lifetime, never on the fabric; the
/// agent's environment carries NONE of it.
///
/// Leakage discipline enforced by the type:
/// - `Debug` and `Display` print `"<redacted>"` — a stray `{:?}`/`{}` in a fact,
///   evidence blob, or trace line NEVER prints the value (criterion 5).
/// - the value is reachable ONLY through [`Self::expose`], used solely at the
///   upstream-injection point (post-TLS-termination, on the plaintext the agent
///   never sees).
/// - the durable injection FACT carries [`Self::secret_ref`] (a label/hash),
///   never the value (I2/I3).
#[derive(Clone, PartialEq, Eq)]
pub struct BrokeredSecret {
    /// The reference LABEL/HASH the injection fact records (e.g.
    /// `"github-token"` or a blake3 of the value) — never the value itself. This
    /// is the `secret_ref` on `credential.injected` (I2/I3).
    secret_ref: String,
    /// The secret bytes — PRIVATE. Reachable only via [`Self::expose`]; the
    /// redacted `Debug`/`Display` never touch it.
    value: String,
}

impl BrokeredSecret {
    /// Broker a secret under a reference label. The `secret_ref` is the ONLY
    /// thing that ever rides a fact/evidence/trace; the `value` is exposed only
    /// at upstream injection.
    pub fn new(secret_ref: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            secret_ref: secret_ref.into(),
            value: value.into(),
        }
    }

    /// The reference label/hash the `credential.injected` fact carries (never the
    /// value) — the `secret_ref` contract (DR-026 §Decision, criterion 5).
    pub fn secret_ref(&self) -> &str {
        &self.secret_ref
    }

    /// Expose the secret bytes — the ONE sanctioned reachability point, used
    /// SOLELY at upstream injection (post-termination, on the plaintext the agent
    /// never sees). Named `expose` so every use is an audit grep target: a secret
    /// value can only leave this type through a call the auditor can find.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

/// The redaction the leak-guard rests on: `Debug` NEVER prints the value. A
/// `{:?}` of a `BrokeredSecret` (or a struct containing one) prints the
/// redaction, so an accidental debug-format into a fact/trace cannot leak the
/// bytes (criterion 5).
impl std::fmt::Debug for BrokeredSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show the ref label (safe — it is what facts carry) but REDACT the value.
        f.debug_struct("BrokeredSecret")
            .field("secret_ref", &self.secret_ref)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// `Display` is likewise redacted — a `{}` of a secret prints `"<redacted>"`,
/// never the bytes (criterion 5).
impl std::fmt::Display for BrokeredSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The egress policy for one confined run: the destination ALLOWLIST and the
/// which-secret-for-which-destination INJECTION MAP, folded from the project
/// spec `[gates.permit]`/role layer (DR-026 §Decision — "folded authority, never
/// a self-declared arg").
///
/// BOTH the `allowlist` and the `injection_map` are PRIVATE so they can be set
/// ONLY through [`EgressPolicy::from_folded_authority`]. A [`SpawnPlan`] (which
/// carries run-supplied argv/env) and a request destination can NEVER contribute
/// an allowlisted host OR a secret mapping — the C6 escalation guard, mirrored
/// from `SandboxPolicy` (criterion 6).
#[derive(Debug, Clone)]
pub struct EgressPolicy {
    /// The allowlisted destinations — the ONLY hosts the confined process may
    /// reach through the proxy. PRIVATE: the type-system half of the no-widening
    /// guard. Read via [`Self::allowlist`].
    allowlist: Vec<Destination>,
    /// The which-secret-for-which-destination map: host → the brokered secret the
    /// proxy injects into the UPSTREAM request on an approved egress. PRIVATE:
    /// the same no-widening guard — a run-supplied value cannot add a mapping,
    /// so the agent can never route itself a secret it should not receive
    /// (criterion 4, criterion 6). Read via [`Self::secret_for`].
    injection_map: BTreeMap<String, BrokeredSecret>,
}

impl EgressPolicy {
    /// Build a policy FROM FOLDED AUTHORITY — the ONLY constructor (DR-026
    /// §Decision; the C6/DR-024 lesson mirrored from `SandboxPolicy`). Callers
    /// pass the allowlist AND the injection map derived from the folded
    /// project-spec/role layer; there is intentionally no `SpawnPlan` parameter
    /// and no request-destination parameter here, so a run-supplied value cannot
    /// reach `allowlist` OR `injection_map`.
    ///
    /// The daemon is the sole caller in production (it holds the folded state +
    /// the daemon-side secret store); tests construct a policy this way to STAND
    /// IN for that fold, exactly as the C3a tests feed the folded binds directly.
    pub fn from_folded_authority(
        allowlist: Vec<Destination>,
        injection_map: BTreeMap<String, BrokeredSecret>,
    ) -> Self {
        Self {
            allowlist,
            injection_map,
        }
    }

    /// The allowlisted destinations (read-only view — the field is private so it
    /// is never widened after construction).
    pub fn allowlist(&self) -> &[Destination] {
        &self.allowlist
    }

    /// Is `host` on the allowlist (any matching destination)? The pure
    /// deterministic predicate the `EgressScope` verifier and the proxy agree on:
    /// an exact host match on an allowlisted destination (port-agnostic when the
    /// destination names no port). A host on NO destination is DENIED.
    pub fn allows(&self, host: &str) -> bool {
        self.allowlist.iter().any(|d| d.host == host)
    }

    /// The brokered secret mapped to `host`, if any (read-only). Returns the
    /// secret (whose value is still redacted under `Debug`) so the proxy can
    /// `.expose()` it at upstream injection ONLY. A host with no mapping gets no
    /// injection — a folded map, never a self-declared one (criterion 4).
    pub fn secret_for(&self, host: &str) -> Option<&BrokeredSecret> {
        self.injection_map.get(host)
    }
}

/// The outcome of a mediation decision for one outbound connection: the verdict
/// axis (`Reach`/`Deny`/`Escalate` — mirroring the permit `pass/fail/inconclusive`)
/// plus, on an approved reach, the `secret_ref` that WOULD be injected (never the
/// value). The proxy uses this to drive termination + injection + the durable
/// fact; the tests pin its shape without a live connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mediation {
    /// The destination is allowlisted → terminate + proxy. `secret_ref` is
    /// `Some(label)` when the folded injection map maps this host to a secret
    /// (the value is injected upstream, never surfaced here), else `None`.
    Reach {
        host: String,
        secret_ref: Option<String>,
    },
    /// The destination is NOT allowlisted → refuse (a durable `egress.denied`).
    Deny { host: String },
    /// Undecidable → escalate to a human (never coerced to reach, I6).
    Escalate { host: String },
}

/// Whether the egress backend (the connector + the proxy + the CA) is usable on
/// this host. The degrade contract is the INVERSE of C3a's (DR-026 §Decision, I6,
/// criterion 7): an [`EgressAvailability::Unavailable`] backend degrades CLOSED —
/// it does NOT open unmediated egress and does NOT inject; it keeps C3a's sealed
/// netns (no network), announces itself with a loud `egress.unavailable` fact,
/// and injects nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressAvailability {
    /// The connector + proxy + CA are present and usable.
    Available,
    /// The backend is absent/unusable; `reason` is the loggable degrade cause
    /// (e.g. `"pasta not found on PATH"` or `"CA key unavailable"`). The run
    /// keeps the sealed netns — NO network, NO injection — after a loud
    /// `egress.unavailable` fact lands (never a silent open, criterion 7).
    Unavailable { reason: String },
}

impl EgressAvailability {
    /// Convenience: is the egress backend available?
    pub fn is_available(&self) -> bool {
        matches!(self, EgressAvailability::Available)
    }
}

/// The egress-proxy substrate (I4 — DR-026 §Decision; design §2, §3, §4). The
/// TLS-terminating chokepoint: it decides an outbound connection against the
/// folded [`EgressPolicy`], terminates TLS with a per-destination leaf cert (the
/// sandbox trusts the rezidnt CA), and — on an approved egress — injects the
/// folded brokered secret into the UPSTREAM request only (the agent never holds
/// it). Absent a backend it degrades CLOSED (no network, no injection).
///
/// Selected by platform exactly like the run/git/sandbox substrates (DR-001): the
/// Linux `pasta`+`rustls` backend is C3b+c; macOS/Windows egress backends are
/// later behind the SAME trait (DR-026 §Consequences, design §6).
pub trait EgressProxy {
    /// A stable backend name for the `egress.*` facts (`"pasta+rustls"`,
    /// `"none"` on a closed degrade).
    fn backend(&self) -> &'static str;

    /// Probe whether this backend can mediate egress on this host — the degrade
    /// gate (criterion 7). The connector (`pasta`) OR the CA absent ⇒
    /// [`EgressAvailability::Unavailable`], so the daemon logs `egress.unavailable`
    /// and degrades CLOSED (keeps the sealed netns, injects nothing). NEVER
    /// panics on a missing binary/key (a missing backend is a VERDICT, not a
    /// crash — the C3a could-not-run discipline).
    fn availability(&self) -> EgressAvailability;

    /// Decide one outbound connection to `host` against the folded `policy` — the
    /// mediation the `EgressScope` verdict drives. The allowlist AND the
    /// injection map come from `policy` ONLY (the C6 guard is enforced by
    /// `EgressPolicy`'s private fields, so this signature cannot be handed a
    /// widening allowlist). On a [`Mediation::Reach`] the returned `secret_ref`
    /// names WHICH secret will be injected upstream (never the value).
    fn mediate(&self, host: &str, policy: &EgressPolicy) -> Mediation;

    /// Terminate + proxy an approved connection, injecting the folded secret into
    /// the UPSTREAM request only (post-termination). Returns the durable
    /// `credential.injected` fact's `secret_ref` (never the value) when a secret
    /// was injected, `None` when the destination had no mapping. The daemon holds
    /// the CA + the secret store (S1: the daemon owns the machinery, not the
    /// client).
    ///
    /// Returns [`RunError::Spawn`] when the backend is available but the confined
    /// termination/injection fails; the daemon's CLOSED-degrade path (when
    /// availability is `Unavailable`) is a SEPARATE branch — the substrate never
    /// silently opens unmediated egress inside `inject_and_proxy` (that would
    /// defeat the degrade-closed contract, I6, criterion 7).
    fn inject_and_proxy(
        &self,
        host: &str,
        policy: &EgressPolicy,
    ) -> Result<Option<String>, RunError>;
}

/// Render the userspace-net connector (`pasta`) argv for a confined run whose
/// SOLE outbound target is the rezidnt proxy — the pure, inspectable arg-building
/// seam the tests pin WITHOUT spawning anything (mirrors `bwrap_argv` being pure
/// so `sandbox_no_widening_c3a.rs` pins it host-side). The Linux impl calls this
/// and hands the result to `pasta`; the host-runnable tests assert the argv
/// routes ALL outbound TCP+DNS to `proxy_addr` (transparent interception) and
/// that NO direct-route / proxy-bypass arg is present (criterion 3 host analogue).
///
/// The `proxy_addr` is the daemon-owned mediator the netns's only route reaches;
/// a run-supplied value cannot change it (the `_plan` arg is deliberately unused
/// for routing — the routing target is the folded proxy, never the plan). This
/// is the C6 no-widening posture applied to the egress route.
///
/// Oracle stub: the implementer writes the real renderer (the exact `pasta`
/// flags, the DNS-redirect directive, the `slirp4netns` alternative). It exists
/// as a `pub` pure fn so the all-outbound-routes-to-proxy + no-bypass tests can
/// drive it host-side (no `pasta`/netns needed to inspect the argv it WOULD run).
pub fn connector_argv(_plan: &SpawnPlan, proxy_addr: &str) -> Vec<String> {
    // The proxy address is the folded mediator ONLY — never `_plan.args`/`.env`.
    // ALL outbound TCP+DNS must redirect here (transparent interception, DR-026
    // §Decision); a proxy-aware-only design (some ports direct) is a silent hole
    // and is REJECTED. `_plan` is deliberately unused for routing (the C6
    // no-widening posture applied to the egress route): a run-supplied `--proxy`
    // in `plan.args` cannot change where egress goes, because we never read the
    // plan here. The routing target is a pure function of the folded
    // `proxy_addr`, so two different adversarial plans + the same folded addr
    // yield the same argv (the property `egress_no_widening_c3bc.rs` pins).
    //
    // `pasta` (passt) provides a userspace network to the confined netns. The
    // netns starts with NO default route (C3a sealed it, `--unshare-all`); the
    // only path out we hand it is this connector. We tell `pasta` to redirect
    // ALL outbound TCP AND UDP (DNS is UDP/53 + TCP/53) to a SINGLE destination —
    // the daemon-owned proxy — so even a raw socket or an alternate resolver the
    // agent opens lands on the mediator, never the open internet.
    //
    // `--outbound-if` / the direct-route toggles are deliberately ABSENT: adding
    // one would be a bypass hole (a non-proxy exit). The DNS redirect
    // (`--dns-forward` at the proxy address) forces resolution THROUGH the
    // mediator. The exact live wiring (a `-T`/`-U` port-forward spec per range,
    // or the `slirp4netns` `--outbound-addr` equivalent) is validated on the
    // WSL netns box; this renderer pins the ARGV the substrate WOULD exec so the
    // no-bypass / no-widening property is host-inspectable (no `pasta` needed).
    let mut argv: Vec<String> = Vec::new();

    // Redirect ALL forwarded DNS queries to the proxy (transparent DNS through
    // the mediator — a raw query to 8.8.8.8 is answered by the proxy, never the
    // open resolver). The proxy host is the routing target, sourced ONLY from
    // the folded `proxy_addr`.
    let (proxy_host, _proxy_port) = split_addr(proxy_addr);
    argv.push("--dns-forward".to_string());
    argv.push(proxy_host.to_string());

    // Redirect ALL outbound TCP to the proxy address (transparent interception,
    // not proxy-aware-only): every forwarded TCP connection terminates at the
    // daemon-owned mediator. `-T` is `pasta`'s TCP-forward directive; the spec
    // names the single proxy destination so no port routes direct.
    argv.push("-T".to_string());
    argv.push(format!("all:{proxy_addr}"));

    // Redirect ALL outbound UDP to the proxy address likewise (DNS is UDP/53;
    // a QUIC/UDP exfil path is closed the same way). No direct UDP route exists.
    argv.push("-U".to_string());
    argv.push(format!("all:{proxy_addr}"));

    argv
}

/// Split a `host:port` (or bare `host`) into `(host, Some(port))`. Pure, no DNS.
/// Used only to name the DNS-forward target from the folded proxy address; a
/// bare host (no colon) forwards DNS to that host with no port constraint.
fn split_addr(addr: &str) -> (&str, Option<&str>) {
    match addr.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (addr, None),
    }
}

/// Build the by-reference credential-injection fact — the PDP recording THAT a
/// brokered secret was injected on an approved egress, and WHICH (its
/// `secret_ref`), NEVER the value (DR-026 §Decision, criterion 5; design §4).
/// Returns `(subject, payload)`.
///
/// ## WARDEN-GATED subject — PLACEHOLDER, not ratified.
/// The `credential.injected`/`egress.*` subject family is a DEFERRED warden
/// `/subject` question (DR-026 §Consequences, design §5) — NOT minted here. The
/// subject string below is a PLACEHOLDER standing in for the implementer's chosen
/// wiring; it is not a ratified ontology name. TODO(warden, /subject): once the
/// family is minted WITH its folding reducer (no consumer-less subjects, DR-006),
/// replace this constant with the ratified subject.
///
/// Payload shape (design §4 candidate `credential.injected {run, dest, secret_ref,
/// policy_ref}`): the `secret_ref` is the brokered secret's LABEL/HASH
/// ([`BrokeredSecret::secret_ref`]) — the by-reference contract. The secret VALUE
/// is NEVER read here: this constructor takes the secret only to lift its
/// `secret_ref`, and the `whole_emitted_fabric_carries_secret_ref_never_the_value`
/// scan (criterion 5) is what forbids a careless impl from inlining `.expose()`.
/// `policy_ref` is a CAS ref (the deciding egress policy) so the injection is
/// interrogable/replayable (I3/I6); bytes never ride inline (I2).
pub fn injected_fact(
    run: &str,
    dest: &str,
    secret: &BrokeredSecret,
    policy_ref: &CasRef,
) -> (&'static str, Value) {
    // WARDEN-GATED: placeholder subject, not a ratified ontology name.
    let subject = "credential.injected";
    let payload = json!({
        "run": run,
        "dest": dest,
        // BY REFERENCE: the secret's LABEL/HASH, never the value (criterion 5).
        "secret_ref": secret.secret_ref(),
        "policy_ref": policy_ref,
    });
    (subject, payload)
}

/// The Linux `pasta`+`rustls`+`rcgen` egress backend (C3b+c — DR-026 §Decision;
/// design §2–§4): execs `pasta` (rootless, sole outbound = the proxy), terminates
/// TLS with a process-lifetime rezidnt CA (`rcgen` leaf certs, `rustls`
/// termination), and injects the folded brokered secret upstream. Selected by
/// platform like the run/git/sandbox substrates (DR-001).
///
/// DECISION-LAYER impl (c3bc-decide, DR-027): `mediate` decides, `availability`
/// probes + degrades CLOSED, and `inject_and_proxy` builds the CA/leaf/rustls
/// config then returns the `secret_ref` only — it carries NO live traffic and
/// injects nothing. The live `pasta` netns→proxy→upstream byte-path and real
/// injection are c3bc-enforce (the WSL-only `#[cfg(unix)]` `#[ignore]`'d
/// integration suite), not this type.
#[derive(Debug, Default)]
pub struct PastaProxy {
    /// The `pasta` connector binary name/path to exec (defaults to `"pasta"` on
    /// PATH). The availability probe resolves through this so a test can point a
    /// substrate at a missing binary to exercise the CLOSED degrade.
    pub connector_bin: Option<String>,

    /// c3bc-ENFORCE dataplane wiring — the live netns→proxy→upstream byte-path
    /// (whose IMPL is unix-only, though this data carrier is cross-platform).
    /// `None` on the decide-layer default: the honesty guard (DR-027) means the
    /// substrate does NOT enforce until this is set. The enforce integration suite
    /// builds it (the confined probe binary + the capturing upstream = test-support,
    /// DR-023 fixtures-stay-dev-only); the daemon's live run-loop wiring that folds
    /// this from real project state is the downstream slice. A `start` with `None`
    /// (or on a non-unix host) is an honest `RunError`, never a silent no-op that
    /// looks authoritative.
    pub enforce: Option<EnforceWiring>,
}

/// The bwrap-splice wiring the composed dataplane threads into pasta's netns exec
/// (DR-028 §Decision 1/2). Pure data (the bwrap binary + the unshare-all-MINUS-net
/// confinement prefix rendered from the folded `SandboxPolicy` ONLY — the C6
/// no-widening guard). When present, `pasta … -- <bwrap_bin> <bwrap_prefix> -- <probe> …`
/// runs the probe UNDER bwrap confinement inside pasta's sealed netns; the prefix
/// deliberately carries NO net unshare so the probe inherits the shared netns.
///
/// It is NOT a field on [`PastaProxy`] (that struct's literal shape is pinned by
/// the enforce + c3-wire oracle suites, which construct it with `connector_bin` +
/// `enforce` only); the composed start passes it SEPARATELY to
/// [`unix-only PastaHandle::start_composed`]. Cross-platform data carrier; only the
/// unix dataplane consumes it.
#[derive(Debug, Clone)]
pub struct ComposedSpliceWiring {
    /// The `bwrap` binary pasta execs as the confined program (defaults resolved
    /// by the caller; `"bwrap"` on PATH).
    pub bwrap_bin: String,
    /// The bwrap confinement argv (unshare-all-MINUS-net + `--die-with-parent` +
    /// the folded binds), rendered from the folded [`crate::sandbox::SandboxPolicy`]
    /// ONLY. Sits between `<bwrap_bin>` and the `--` that hands off to the probe.
    pub bwrap_prefix: Vec<String>,
}

/// The test-support wiring the enforce dataplane needs to stand up the live
/// byte-path (DR-023 dev-only). The confined program `pasta` execs in the sealed
/// netns, and the upstream the proxy dials on an approved reach. In production
/// these are folded from real state (the harness the run-loop spawns + the real
/// allowlisted upstream); here the enforce suite provides a probe binary and a
/// capturing upstream so criterion 3/4 are FALSIFIABLE and non-vacuous. Pure data
/// (paths/addr/DER) — cross-platform; only the dataplane that CONSUMES it (the
/// `pasta`/netns machinery) is unix-only.
#[derive(Debug, Clone)]
pub struct EnforceWiring {
    /// The confined program `pasta` execs inside the sealed netns — it seals the
    /// route table to the proxy-only `/32`, runs the direct-egress escape
    /// attempts, and reaches the proxy. Invoked as `<probe_bin> <mode> <args…>`;
    /// the enforce suite points this at its dev-only probe example.
    pub probe_bin: std::path::PathBuf,
    /// The upstream `host:port` the proxy dials on an approved reach (the
    /// capturing TLS server, test-support). The proxy terminates the confined
    /// client's TLS, then opens ITS OWN upstream TLS here and injects the folded
    /// secret — so the capture records what the upstream received (criterion 4).
    pub upstream_addr: std::net::SocketAddr,
    /// The upstream (capture server) CA cert DER the proxy's UPSTREAM client
    /// config trusts — so the proxy's own TLS dial to the test upstream verifies
    /// (the capture server is self-signed; the daemon trusts it for the test).
    /// In production the upstream client trusts the webpki roots; here it trusts
    /// the capture server's CA. Never a secret — a public trust anchor.
    pub upstream_ca_der: Vec<u8>,
    /// The file the capture upstream server writes the headers it RECEIVED to —
    /// `drive_injected_egress` reads it as independent proof the injection reached
    /// the upstream (criterion 4). The capture server (test-support) is started by
    /// the enforce suite writing to this same path.
    pub upstream_capture_path: std::path::PathBuf,
}

impl EgressProxy for PastaProxy {
    fn backend(&self) -> &'static str {
        "pasta+rustls"
    }

    fn availability(&self) -> EgressAvailability {
        // The degrade gate (criterion 7), the INVERSE of C3a's loud-open: BOTH
        // the connector binary AND the CA must be usable, or the backend is
        // Unavailable and the daemon degrades CLOSED (sealed netns, no
        // injection). NEVER a panic — a missing tool/key is a VERDICT, not a
        // crash (the C3a could-not-run discipline).
        //
        // 1. The connector (`pasta`): probed by exec'ing `--version` (std::process,
        //    like `bwrap`/the git-CLI — the connector is EXEC'd, never linked).
        let bin = self.connector_bin.as_deref().unwrap_or("pasta");
        if let Some(reason) = probe_connector(bin) {
            return EgressAvailability::Unavailable { reason };
        }
        // 2. The CA: we must be able to mint the process-lifetime rezidnt CA
        //    (rcgen). A CA-build failure (e.g. no crypto entropy) is a CLOSED
        //    degrade, never an open — a missing CA must never mean open egress.
        if let Err(e) = RezidntCa::new() {
            return EgressAvailability::Unavailable {
                reason: format!("rezidnt CA unavailable: {e}"),
            };
        }
        EgressAvailability::Available
    }

    fn mediate(&self, host: &str, policy: &EgressPolicy) -> Mediation {
        // The mediation decision the `EgressScope` verdict drives, read from the
        // folded `policy` ONLY (the C6 guard is enforced by `EgressPolicy`'s
        // private fields — this cannot be handed a widening allowlist). Mirrors
        // the three-valued permit axis: allowlisted → Reach (terminate + proxy),
        // off-list → Deny (a durable refusal), undecidable → Escalate (never
        // coerced to Reach, I6). A present-empty allowlist Denies everything
        // (deny-by-default), matching `EgressScope`.
        if policy.allows(host) {
            // On an approved reach, name WHICH secret WOULD be injected upstream
            // (the ref/label, NEVER the value). `.secret_ref()` is safe — it is
            // exactly what the `credential.injected` fact carries.
            let secret_ref = policy.secret_for(host).map(|s| s.secret_ref().to_string());
            Mediation::Reach {
                host: host.to_string(),
                secret_ref,
            }
        } else {
            Mediation::Deny {
                host: host.to_string(),
            }
        }
    }

    fn inject_and_proxy(
        &self,
        host: &str,
        policy: &EgressPolicy,
    ) -> Result<Option<String>, RunError> {
        // Adapter task span (rust-conventions): every mediated egress is traced.
        // The span carries the backend + host — NEVER a secret field (the
        // redaction discipline extends to tracing: no `.expose()` near a span).
        let span = tracing::info_span!("adapter", kind = "egress", backend = "pasta+rustls", %host);
        let _guard = span.enter();

        // The degrade-CLOSED branch is the daemon's SEPARATE concern (it checks
        // `availability()` and emits `egress.unavailable` before ever calling
        // here). This method NEVER silently opens unmediated egress: an
        // unallowlisted host is refused as an error VERDICT for the caller, not a
        // proxied connection (I6, criterion 7 — the substrate never becomes the
        // silent-open hole).
        match self.mediate(host, policy) {
            Mediation::Deny { host } => Err(RunError::Spawn(format!(
                "egress to {host} refused: destination outside the folded allowlist \
                 (deny-by-default, no unmediated egress)"
            ))),
            Mediation::Escalate { host } => Err(RunError::Spawn(format!(
                "egress to {host} undecidable: escalate to a human (never coerced to reach, I6)"
            ))),
            Mediation::Reach { host, secret_ref } => {
                // On an approved reach the real backend: (1) mints a
                // per-destination leaf cert from the process-lifetime rezidnt CA,
                // (2) terminates the netns's TLS with it (the sandbox trusts the
                // CA — a folded read-only bind), (3) opens its OWN upstream TLS to
                // the real host, and (4) injects the folded brokered secret into
                // the UPSTREAM request only — on the plaintext the agent never
                // sees. The wiring below builds the CA + leaf (host-provable);
                // the live netns→proxy→upstream byte path is the WSL integration
                // suite (`egress_mediation_c3bc.rs`), which the exit-demo runner
                // provisions. Building the leaf here proves the CA lifecycle
                // compiles + runs without the connector.
                let ca = RezidntCa::new()
                    .map_err(|e| RunError::Spawn(format!("rezidnt CA build failed: {e}")))?;
                let (_leaf, _leaf_key) = ca
                    .leaf_for(&host)
                    .map_err(|e| RunError::Spawn(format!("leaf cert for {host} failed: {e}")))?;
                let _server_config = ca.terminating_config(&host)?;

                // The injection is BY REFERENCE at this seam: return the
                // secret_ref (the label the `credential.injected` fact records),
                // NEVER the value. The ONE `.expose()` call-site — writing the
                // token into the upstream request bytes — lives in the live
                // upstream-write path (the WSL integration surface), never here
                // near a fact/log/return value. This method returns only the
                // ref, so a leak cannot originate at this boundary.
                if let Some(secret) = policy.secret_for(&host) {
                    // Re-derive the ref from the folded policy (defensive: the
                    // ref rides the fact; the value stays put). NEVER `.expose()`.
                    debug_assert_eq!(Some(secret.secret_ref().to_string()), secret_ref);
                }
                Ok(secret_ref)
            }
        }
    }
}

/// Probe the host for a usable `pasta` connector (the degrade gate, criterion 7).
/// Pointed at a binary NAME/PATH; a missing/unrunnable binary yields `Some(reason)`
/// (a loggable degrade cause), a usable one yields `None`. NEVER panics — a
/// missing connector is the honest CLOSED-degrade signal, not a crash (the C3a
/// `probe_backend` discipline). `std::process` (exec'd like the git-CLI) — no
/// linked connector crate (I7, criterion 8).
fn probe_connector(bin: &str) -> Option<String> {
    match std::process::Command::new(bin)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => None,
        // Ran but `--version` was non-zero: not a safe Available assumption —
        // report the odd exit as the CLOSED-degrade reason (never a silent open).
        Ok(status) => Some(format!("{bin} --version exited with {status}")),
        Err(e) => Some(format!("{bin} connector not runnable on PATH: {e}")),
    }
}

/// The process-lifetime rezidnt CA (DR-026 §Decision; design §3): a self-signed
/// CA minted at daemon start whose PRIVATE KEY is daemon-only and NEVER on the
/// fabric (I2/I3 — the catastrophic-failure surface). It mints per-destination
/// leaf certs (`rcgen`) the proxy terminates TLS with (`rustls`); the sandbox
/// trusts the CA cert via a folded READ-ONLY bind (the CA cert is public; only
/// the key is secret). Sandbox-scoped: injected only into confined trust stores,
/// never the host's (§8 blast-radius boundary — a compromised daemon is out of
/// scope, stated plainly in the DR).
struct RezidntCa {
    /// The CA certificate DER (public — injected read-only into the sandbox
    /// trust store). Safe to surface; it is not the secret. Kept as owned DER so
    /// it can ride the leaf chain without re-borrowing the params.
    ca_cert_der: Vec<u8>,
    /// The CA issuer — owns the CA distinguished name + the SIGNING KEY. The
    /// PRIVATE key half is daemon-only: never serialized onto a fact, a CAS
    /// write, or a trace. Held only to SIGN leaf certs here.
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
}

impl RezidntCa {
    /// Mint the process-lifetime rezidnt CA. Fallible (a crypto/entropy failure
    /// is a CLOSED-degrade cause, never a panic — the could-not-run discipline):
    /// the caller maps `Err` to `EgressAvailability::Unavailable`.
    fn new() -> Result<Self, rcgen::Error> {
        let ca_key = rcgen::KeyPair::generate()?;
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new())?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "rezidnt egress CA");
        // A real CA: it may sign leaf certs. Sandbox-scoped by where its cert is
        // trusted (a confined bind), never the host trust store.
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca_cert = params.self_signed(&ca_key)?;
        let ca_cert_der = ca_cert.der().to_vec();
        // The issuer OWNS the params + signing key ('static): the CA private key
        // lives only here, used solely to sign leaves below.
        let issuer = rcgen::Issuer::new(params, ca_key);
        Ok(Self {
            ca_cert_der,
            issuer,
        })
    }

    /// The CA certificate DER — PUBLIC (it is the trust anchor, not the secret;
    /// the private key never leaves `self.issuer`). The confined client's trust
    /// store is seeded with this so it trusts the per-destination leaves the
    /// proxy mints; the enforce dataplane hands it to the confined probe as a
    /// PEM-encoded read-only file (the folded read-only bind, on the live path).
    #[cfg(unix)]
    fn ca_cert_der(&self) -> &[u8] {
        &self.ca_cert_der
    }

    /// Mint a per-destination leaf cert for `host`, signed by the CA — returning
    /// the leaf DER + its private key. This is the cert the proxy presents to the
    /// confined client when terminating TLS to `host`; the sandbox trusts it
    /// because it trusts the CA (the folded bind). Per-destination (not a
    /// wildcard) so the terminated identity names exactly the mediated host.
    fn leaf_for(&self, host: &str) -> Result<(rcgen::Certificate, rcgen::KeyPair), rcgen::Error> {
        let leaf_key = rcgen::KeyPair::generate()?;
        let mut params = rcgen::CertificateParams::new(vec![host.to_string()])?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, host);
        // Signed BY the CA issuer (the CA private key never leaves `self.issuer`).
        let leaf = params.signed_by(&leaf_key, &self.issuer)?;
        Ok((leaf, leaf_key))
    }

    /// Build the `rustls` server config the proxy terminates the confined client's
    /// TLS with, presenting the per-destination leaf (+ the CA in the chain) and
    /// the leaf's own key. Daemon-side; the private keys never leave the daemon.
    fn terminating_config(&self, host: &str) -> Result<rustls::ServerConfig, RunError> {
        let (leaf, leaf_key) = self
            .leaf_for(host)
            .map_err(|e| RunError::Spawn(format!("leaf sign failed: {e}")))?;

        let cert_chain = vec![
            rustls::pki_types::CertificateDer::from(leaf.der().to_vec()),
            rustls::pki_types::CertificateDer::from(self.ca_cert_der.clone()),
        ];
        let key_der = rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der())
            .map_err(|e| RunError::Spawn(format!("leaf key encode failed: {e}")))?;

        // Explicit `ring` provider (default features off): the config never
        // depends on a process-global default provider being installed, so the
        // termination path is self-contained and deterministic (no ambient
        // crypto-provider state). This is the ONLY crypto provider linked
        // (criterion 8 — no second provider).
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| RunError::Spawn(format!("rustls protocol versions: {e}")))?
            .with_no_client_auth()
            .with_single_cert(cert_chain, key_der)
            .map_err(|e| RunError::Spawn(format!("rustls termination config failed: {e}")))
    }
}

// ============================================================================
// c3bc-ENFORCE — the LIVE DATAPLANE seam (DR-027 §Decision — the enforce slice)
// ============================================================================
//
// Everything ABOVE this line is c3bc-decide (DR-027): it DECIDES and builds
// certs but carries NO live traffic and injects NOTHING — the enforcement-inert
// governance/type/scaffold layer. The section BELOW is the c3bc-enforce
// interface: the LIVE netns→proxy→upstream byte-path that makes DR-026 criteria
// 3 (inescapability) and 4 (agent-never-sees-token) achievable. It is defined
// here (the interface the enforce impl must build) but its live methods are
// `todo!()`-stubbed — so the WSL integration suite `egress_mediation_c3bc.rs`
// FAILS for the right reason (the LIVE dataplane is absent), not because the
// interface is undecided.
//
// ## Why a separate trait, DI'd, driven by the tests (the house pattern)
//
// The `EgressProxy` trait above decides + scaffolds; it never opens a socket.
// The dataplane is a genuinely-new subsystem (a `pasta`-in-netns connector with
// no default route, a `rustls` terminating listener, an upstream TLS dial, and
// the ONE `.expose()` upstream-write) whose integration can only be proven on a
// Linux netns box. Following the `SandboxSubstrate`/`BwrapSubstrate` split, the
// live path is its own trait ([`EgressDataplane`]) returning a handle the tests
// drive ([`DataplaneHandle`]) — so the enforce oracle asserts against a stable
// seam while the impl behind it is `todo!()` until the implementer lands it.
//
// ## The ONE `.expose()` call-site lives HERE (DR-026 §Decision, criterion 5)
//
// The decide layer's `inject_and_proxy` returns only the `secret_ref` and NEVER
// calls `.expose()` — its own comment records that the upstream-write call-site
// "lives in the live upstream-write path (the WSL integration surface), never
// here." THIS is that surface: the enforce impl's upstream-write is the single
// sanctioned `.expose()` reachability point, on the plaintext the agent never
// sees. The [`CapturedExchange`] the handle returns is what lets a test PROVE
// the token rode the upstream request and NOT the agent's own request — the
// criterion-4 capture seam.

/// One direct-egress escape attempt a confined probe makes, and whether it
/// reached the open internet (DR-026 criterion 3 — inescapability). The confined
/// probe tries each of these from INSIDE the sealed netns, whose ONLY route out
/// is the rezidnt proxy; a well-formed dataplane makes every non-proxy attempt
/// reach NOTHING (fail/timeout) and the proxy attempt succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeAttempt {
    /// Unset every proxy env var (`HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`) and try
    /// to reach a known public host by name — proving the route, not env, is what
    /// confines (an agent that unsets proxy env still cannot escape).
    UnsetProxyEnv { target_host: String },
    /// Open a RAW TCP socket to a hard-coded public IP:port (e.g. `1.1.1.1:80`) —
    /// bypassing any proxy-aware client. With no default route this reaches
    /// nothing; a delivered byte is an inescapability HOLE.
    RawSocketPublicIp { ip: String, port: u16 },
    /// Query an ALTERNATE DNS resolver directly (e.g. `8.8.8.8:53`) — proving DNS
    /// resolves THROUGH the mediator or is denied, never via the open resolver.
    AltDnsResolver { resolver_ip: String },
    /// The sanctioned path: reach an allowlisted host THROUGH the proxy. This is
    /// the ONE attempt that MUST succeed (else the netns is not merely sealed, it
    /// is dead — the proxy route must actually carry traffic).
    ViaProxy { target_host: String },
}

/// Whether one [`EscapeAttempt`] reached the open internet. The inescapability
/// property (criterion 3) is: EVERY non-`ViaProxy` attempt is `Blocked`, and the
/// `ViaProxy` attempt is `ReachedProxy`. A non-proxy attempt that is
/// `ReachedOpenInternet` is a confinement HOLE — the test that observes it FAILS
/// loudly (theater, DR-026 §8.2 — "a bypass makes it theater").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeReach {
    /// The attempt reached NOTHING — connection refused, timed out, or DNS denied.
    /// This is the REQUIRED outcome for every direct-egress (non-proxy) attempt.
    Blocked { how: String },
    /// The attempt landed on the rezidnt proxy (the sole route out) — the
    /// REQUIRED outcome for the `ViaProxy` attempt, and an ACCEPTABLE outcome for
    /// a raw-socket/alt-DNS attempt that pasta transparently redirected to the
    /// mediator (it reached the proxy, NOT the open internet — still no escape).
    ReachedProxy { evidence: String },
    /// CATASTROPHIC — the attempt reached the OPEN INTERNET, bypassing the proxy.
    /// A single observation of this on a non-proxy attempt is an inescapability
    /// failure; the criterion-3 test asserts this NEVER occurs.
    ReachedOpenInternet { evidence: String },
}

/// The result of running the confined direct-egress probe (criterion 3). Pairs
/// each attempt with what it reached, so the test asserts the whole set: no
/// non-proxy attempt reached the open internet, and the proxy attempt worked.
#[derive(Debug, Clone)]
pub struct ProbeReport {
    /// Each escape attempt and its observed reach, in the order the probe ran.
    pub outcomes: Vec<(EscapeAttempt, ProbeReach)>,
}

impl ProbeReport {
    /// Did ANY direct-egress (non-`ViaProxy`) attempt reach the open internet?
    /// `true` is an inescapability HOLE — the criterion-3 test fails on it.
    pub fn any_direct_escape_reached_internet(&self) -> bool {
        self.outcomes.iter().any(|(attempt, reach)| {
            !matches!(attempt, EscapeAttempt::ViaProxy { .. })
                && matches!(reach, ProbeReach::ReachedOpenInternet { .. })
        })
    }

    /// Did the sanctioned `ViaProxy` attempt reach the proxy? `false` means the
    /// netns is not merely sealed but has NO working route at all — the proxy
    /// path must actually carry traffic (else the test is vacuous).
    pub fn proxy_path_reached(&self) -> bool {
        self.outcomes.iter().any(|(attempt, reach)| {
            matches!(attempt, EscapeAttempt::ViaProxy { .. })
                && matches!(reach, ProbeReach::ReachedProxy { .. })
        })
    }
}

/// The two sides of a mediated exchange, captured so criterion 4 is FALSIFIABLE:
/// what the AGENT's own request carried (captured at the proxy INGRESS, before
/// injection) vs what the UPSTREAM test server RECEIVED (post-termination, post-
/// injection). The token must be ABSENT from `agent_request_headers` and
/// PRESENT in `upstream_received_headers`. This is the capture seam the enforce
/// impl must expose; without it criterion 4 cannot be proven, only asserted-
/// around (theater the oracle refuses).
#[derive(Debug, Clone)]
pub struct CapturedExchange {
    /// The AGENT's own request as the proxy read it at ingress (before any
    /// injection) — header name → value. The brokered token MUST NOT appear here:
    /// the agent never held it, so its own request cannot carry it.
    pub agent_request_headers: BTreeMap<String, String>,
    /// The environment the confined agent ran with (name → value). The brokered
    /// token MUST NOT appear here either (criterion 4 — "absent from the agent's
    /// environment").
    pub agent_env: BTreeMap<String, String>,
    /// What the UPSTREAM test server actually RECEIVED (post-termination, post-
    /// injection) — header name → value. The brokered token (e.g. an
    /// `Authorization` header) MUST appear here: the upstream received it, the
    /// agent never did.
    pub upstream_received_headers: BTreeMap<String, String>,
}

impl CapturedExchange {
    /// Does the AGENT side (its own request headers OR its environment) carry the
    /// secret VALUE anywhere? `true` is a criterion-4 failure — the agent held or
    /// transmitted a secret it must never see.
    pub fn agent_side_contains(&self, secret_value: &str) -> bool {
        self.agent_request_headers
            .values()
            .chain(self.agent_env.values())
            .any(|v| v.contains(secret_value))
    }

    /// Does the UPSTREAM-received request carry the secret VALUE? `true` is the
    /// REQUIRED criterion-4 outcome — the injection reached the upstream (else the
    /// non-exposure assertion is vacuous: the token must land SOMEWHERE, and the
    /// only legitimate somewhere is the upstream the agent never sees).
    pub fn upstream_contains(&self, secret_value: &str) -> bool {
        self.upstream_received_headers
            .values()
            .any(|v| v.contains(secret_value))
    }
}

/// A live dataplane started for one confined mediated egress — the handle the
/// enforce tests drive (DR-026 criteria 3 + 4). It owns the running `pasta`
/// connector (sole route = the proxy), the `rustls` terminating listener, and
/// the upstream capture server for the duration of the test, and tears them all
/// down on drop. The tests never touch the sockets directly; they ask the handle
/// to run the confined probe and to surface the captured exchange.
///
/// This trait is the CAPTURE SEAM criterion 4 needs and the PROBE SEAM criterion
/// 3 needs. The implementer builds the concrete handle behind
/// [`EgressDataplane::start`]; until then `start` is `todo!()` and the WSL suite
/// fails for the missing live impl.
pub trait DataplaneHandle {
    /// The proxy address the confined netns's SOLE route reaches (`host:port`).
    /// A test uses this to point an allowlisted host's upstream at a capture
    /// server and to name the `ViaProxy` probe target.
    fn proxy_addr(&self) -> &str;

    /// Run the confined direct-egress probe INSIDE the sealed netns and return
    /// what each attempt reached (criterion 3). The probe unsets proxy env, opens
    /// a raw socket to a public IP, queries an alternate resolver, and reaches an
    /// allowlisted host via the proxy — the report pairs each attempt with its
    /// [`ProbeReach`]. NEVER panics on a blocked attempt (a blocked escape is the
    /// EXPECTED result, reported as [`ProbeReach::Blocked`], not an error).
    fn run_escape_probe(&self) -> Result<ProbeReport, RunError>;

    /// Drive ONE approved+mapped mediated egress to `host` end-to-end (criterion
    /// 4): the confined agent issues its request, the proxy terminates TLS,
    /// injects the folded secret into the UPSTREAM request only (the ONE
    /// `.expose()` call-site), and the capture server records what it received.
    /// Returns the captured two-sided [`CapturedExchange`] PLUS the durable
    /// injection `secret_ref` (never the value) the daemon would fold onto the
    /// log — so the test asserts both the non-exposure AND the by-reference fact.
    fn drive_injected_egress(
        &self,
        host: &str,
    ) -> Result<(CapturedExchange, Option<String>), RunError>;
}

/// The LIVE egress dataplane substrate (c3bc-enforce — DR-026 criteria 3, 4;
/// DR-027 §Decision). Where [`EgressProxy`] DECIDES, this ENFORCES: it stands up
/// the netns connector + terminating listener + upstream capture and returns a
/// [`DataplaneHandle`] the tests drive. Selected by platform like the other
/// substrates (DR-001); the Linux `pasta`+`rustls` backend is the enforce slice's
/// job.
pub trait EgressDataplane {
    /// Start a live dataplane for one confined mediated-egress run under `policy`.
    /// Sets up the `pasta` netns connector whose SOLE outbound target is the
    /// proxy (no default route — the inescapability precondition), a `rustls`
    /// terminating listener presenting per-destination leaves from the folded CA,
    /// the upstream TLS dial + capture, and the injection path. Returns a handle
    /// the test drives, or [`RunError`] if the box cannot provision the netns/
    /// connector (a provisioning failure is an honest error, never a fake pass).
    ///
    /// IMPLEMENTER (c3bc-enforce): this is the enforcement dataplane the enforce
    /// slice builds — the live netns→proxy→upstream byte-path DR-027 split out.
    /// It is `todo!()` here so the WSL integration suite fails for the missing
    /// LIVE impl, not for an undecided interface.
    fn start(&self, policy: &EgressPolicy) -> Result<Box<dyn DataplaneHandle>, RunError>;
}

// The live enforce dataplane is unix-only (netns + `pasta` + kernel routing). On
// non-unix hosts `start` is an honest unsupported-platform error — NEVER a fake
// handle. Splitting the impl keeps the host build free of dead unix-only helpers
// ([[vet-is-host-side-wsl-insufficient]]): every dataplane helper below is
// `#[cfg(unix)]`, and the non-unix `start` references none of them.
#[cfg(not(unix))]
impl EgressDataplane for PastaProxy {
    fn start(&self, _policy: &EgressPolicy) -> Result<Box<dyn DataplaneHandle>, RunError> {
        Err(RunError::Spawn(
            "the c3bc-enforce egress dataplane (pasta netns + kernel routing) is unix-only; \
             this platform has no live dataplane backend (DR-026 §Consequences — macOS/Windows \
             egress backends are later, behind the same trait)"
                .to_string(),
        ))
    }
}

#[cfg(unix)]
impl PastaProxy {
    /// The c3-wire COMPOSED dataplane start (DR-028 §Decision 1/2): identical to
    /// [`EgressDataplane::start`] but pasta execs the confined probe THROUGH the
    /// `splice`'s `bwrap` inside the sealed netns (pasta-outer -> bwrap -> probe),
    /// so the probe is filesystem-confined AND inherits pasta's sealed netns. The
    /// splice's bwrap prefix is folded from the sandbox policy ONLY (C6 guard); the
    /// composed run-loop (`compose::start_composed_dataplane`) is the sole caller.
    /// Kept beside `start` so both share the CA/availability/honesty-guard path.
    pub fn start_composed(
        &self,
        policy: &EgressPolicy,
        splice: ComposedSpliceWiring,
    ) -> Result<Box<dyn DataplaneHandle>, RunError> {
        self.start_dataplane(policy, Some(splice))
    }

    /// The shared dataplane-start path (DR-026 enforce + DR-028 compose). `splice`
    /// selects the confined exec: `None` → pasta execs the probe directly
    /// (c3bc-enforce), `Some` → pasta execs `bwrap <prefix> -- <probe>` inside the
    /// sealed netns (c3-wire composition). The CA mint, the honesty guard (unwired
    /// enforce is an error), and the CLOSED-on-unavailable degrade are identical.
    fn start_dataplane(
        &self,
        policy: &EgressPolicy,
        splice: Option<ComposedSpliceWiring>,
    ) -> Result<Box<dyn DataplaneHandle>, RunError> {
        // The enforce dataplane needs its wiring (the confined probe program + the
        // upstream to dial). Absent it, this is the DR-027 honesty guard, not a
        // fake pass: the decide-layer default does NOT enforce, so an unwired
        // `start` is an explicit error — never a silent handle that looks
        // authoritative.
        let wiring = self.enforce.clone().ok_or_else(|| {
            RunError::Spawn(
                "the c3bc-enforce dataplane is not wired (PastaProxy.enforce is None): the \
                 decision layer does not enforce until the live run-loop (or the enforce \
                 integration suite) provides the confined probe + upstream (DR-027 honesty guard)"
                    .to_string(),
            )
        })?;

        // The connector must be present (a missing `pasta` is a CLOSED degrade,
        // never a crash — the availability contract).
        if !self.availability().is_available() {
            return Err(RunError::Spawn(
                "egress backend unavailable at dataplane start (pasta/CA absent) — the daemon \
                 degrades CLOSED here; never opens (criterion 7)"
                    .to_string(),
            ));
        }

        // The process-lifetime CA (its private key is daemon-only, never leaves
        // this process). Its cert (public) is written read-only for the confined
        // probe's trust store — the folded read-only bind, on the live path.
        let ca = std::sync::Arc::new(
            RezidntCa::new()
                .map_err(|e| RunError::Spawn(format!("rezidnt CA build failed: {e}")))?,
        );

        let handle = match splice {
            Some(splice) => unix_dataplane::PastaHandle::start_composed(
                self.connector_bin.clone(),
                ca,
                policy.clone(),
                wiring,
                splice,
            )?,
            None => unix_dataplane::PastaHandle::start(
                self.connector_bin.clone(),
                ca,
                policy.clone(),
                wiring,
            )?,
        };
        Ok(Box::new(handle))
    }
}

#[cfg(unix)]
impl EgressDataplane for PastaProxy {
    fn start(&self, policy: &EgressPolicy) -> Result<Box<dyn DataplaneHandle>, RunError> {
        // The c3bc-enforce shape: pasta execs the confined probe DIRECTLY (no bwrap
        // splice — the enforce suite's self-made netns). The composed run-loop uses
        // `start_composed` to splice bwrap inside the sealed netns (DR-028).
        self.start_dataplane(policy, None)
    }
}

/// The live enforce dataplane (unix-only): the `pasta` netns connector (sole
/// route = the proxy, no default route), the `rustls` terminating listener, the
/// upstream TLS dial + the ONE `.expose()` injection, and the two-sided capture.
/// Every item here is `#[cfg(unix)]` so the host build carries none of it
/// ([[vet-is-host-side-wsl-insufficient]]).
///
/// ## The wire protocol the confined probe speaks (dev-only, DR-023)
///
/// `pasta` execs the confined probe (test-support) inside the sealed netns. The
/// probe seals its own route table to the proxy-only `/32`, runs the direct-egress
/// escape attempts, reaches the proxy, and reports each attempt as one JSON line
/// on stdout. The parent parses those lines into a [`ProbeReport`]. The probe is
/// dev-only test-support (a compiled example), NEVER shipped in the daemon binary
/// (I7) — the same fixtures-stay-dev-only rule as the golden seeders (DR-023).
#[cfg(unix)]
mod unix_dataplane {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        CapturedExchange, ComposedSpliceWiring, DataplaneHandle, EgressPolicy, EnforceWiring,
        EscapeAttempt, ProbeReach, ProbeReport, RezidntCa,
    };
    use crate::RunError;

    /// The probe-mode selectors the confined program dispatches on (argv[1]). The
    /// dev-only probe example matches these exact strings — they are the wire
    /// contract between the dataplane and its test-support probe.
    pub const MODE_ESCAPE_PROBE: &str = "escape-probe";
    pub const MODE_INJECTED: &str = "injected-egress";

    /// Bound on the HTTP request head the proxy reads at ingress — a control-plane
    /// read is bounded so a malicious confined client cannot exhaust proxy memory
    /// (the I2 discipline applied to the terminated request head).
    const REQUEST_HEAD_LIMIT: usize = 64 * 1024;

    /// A live `pasta` netns + `rustls` terminating proxy for one confined
    /// mediated-egress run. Owns the proxy listener thread (host namespace, which
    /// has internet), the CA, the folded policy, and the enforce wiring; tears the
    /// listener down on drop.
    pub struct PastaHandle {
        connector_bin: String,
        proxy_addr: String,
        ca: Arc<RezidntCa>,
        policy: EgressPolicy,
        wiring: EnforceWiring,
        /// c3-wire composition splice (DR-028): when `Some`, pasta execs the
        /// confined probe THROUGH `bwrap` inside the sealed netns (pasta-outer ->
        /// bwrap -> probe), so the probe is filesystem-confined AND inherits pasta's
        /// sealed netns. `None` for the c3bc-enforce self-made shape (pasta execs
        /// the probe directly).
        splice: Option<ComposedSpliceWiring>,
        /// Signals the accept loop to stop; the listener drops with the handle.
        shutdown: Arc<AtomicBool>,
        /// The last ingress (agent) request headers the proxy read — the capture
        /// seam for `drive_injected_egress` (agent side).
        agent_headers: Arc<Mutex<BTreeMap<String, String>>>,
        listener_thread: Option<std::thread::JoinHandle<()>>,
    }

    impl PastaHandle {
        pub fn start(
            connector_bin: Option<String>,
            ca: Arc<RezidntCa>,
            policy: EgressPolicy,
            wiring: EnforceWiring,
        ) -> Result<Self, RunError> {
            // The c3bc-enforce shape: pasta execs the probe DIRECTLY, no bwrap
            // splice (the enforce suite's self-made netns).
            Self::start_inner(connector_bin, ca, policy, wiring, None)
        }

        /// The c3-wire COMPOSED start (DR-028 §Decision 1/2): pasta seals the netns
        /// and execs `bwrap <prefix> -- <probe>` inside it, so the probe (the
        /// running agent) inherits pasta's sealed netns UNDER bwrap confinement.
        /// The `splice` carries the bwrap binary + the unshare-all-MINUS-net
        /// confinement prefix (folded from the sandbox policy ONLY — C6 guard).
        pub fn start_composed(
            connector_bin: Option<String>,
            ca: Arc<RezidntCa>,
            policy: EgressPolicy,
            wiring: EnforceWiring,
            splice: ComposedSpliceWiring,
        ) -> Result<Self, RunError> {
            Self::start_inner(connector_bin, ca, policy, wiring, Some(splice))
        }

        fn start_inner(
            connector_bin: Option<String>,
            ca: Arc<RezidntCa>,
            policy: EgressPolicy,
            wiring: EnforceWiring,
            splice: Option<ComposedSpliceWiring>,
        ) -> Result<Self, RunError> {
            let connector_bin = connector_bin.unwrap_or_else(|| "pasta".to_string());
            // Bind the proxy on host loopback (the host namespace HAS internet; the
            // proxy is the sole route the sealed netns reaches, mapped in by pasta
            // `--map-gw`). Ephemeral port so parallel runs don't collide.
            let listener = TcpListener::bind("127.0.0.1:0")
                .map_err(|e| RunError::Spawn(format!("proxy listener bind failed: {e}")))?;
            let local = listener
                .local_addr()
                .map_err(|e| RunError::Spawn(format!("proxy listener addr: {e}")))?;
            let proxy_addr = format!("127.0.0.1:{}", local.port());

            let shutdown = Arc::new(AtomicBool::new(false));
            let agent_headers = Arc::new(Mutex::new(BTreeMap::new()));

            let loop_ca = Arc::clone(&ca);
            let loop_policy = policy.clone();
            let loop_wiring = wiring.clone();
            let loop_shutdown = Arc::clone(&shutdown);
            let loop_agent_headers = Arc::clone(&agent_headers);
            // A dedicated std thread runs the blocking accept loop — this is a
            // substrate helper thread, NOT an async context (rust-conventions:
            // no blocking in async; this is deliberately off the async runtime).
            listener
                .set_nonblocking(false)
                .map_err(|e| RunError::Spawn(format!("proxy listener nonblocking: {e}")))?;
            let listener_thread = std::thread::spawn(move || {
                // The adapter task span (rust-conventions) — carries the backend +
                // proxy addr, NEVER a secret field (the redaction discipline
                // extends to tracing: no `.expose()` near a span).
                let span = tracing::info_span!(
                    "adapter",
                    kind = "egress-dataplane",
                    backend = "pasta+rustls"
                );
                let _guard = span.enter();
                proxy_accept_loop(
                    listener,
                    loop_ca,
                    loop_policy,
                    loop_wiring,
                    loop_shutdown,
                    loop_agent_headers,
                );
            });

            Ok(Self {
                connector_bin,
                proxy_addr,
                ca,
                policy,
                wiring,
                splice,
                shutdown,
                agent_headers,
                listener_thread: Some(listener_thread),
            })
        }

        /// Exec the confined probe inside a `pasta` sealed netns, passing the mode,
        /// the proxy address, and the CA-cert PEM path. The netns starts with the
        /// host routes copied; the probe SEALS them to the proxy-only `/32` before
        /// probing (the inescapability precondition — a raw socket to a public IP
        /// then reaches NOTHING). Returns the probe's stdout for the caller to
        /// parse.
        fn run_confined(
            &self,
            mode: &str,
            extra: &[String],
        ) -> Result<std::process::Output, RunError> {
            // Write the CA cert (PUBLIC) to a temp PEM the confined probe trusts —
            // the folded read-only trust anchor on the live path. A std-only unique
            // path (no `tempfile` dep — criterion 8 keeps rustls+rcgen the ONLY new
            // linked deps); cleaned up on drop of the guard below.
            let ca_path = unique_temp_path("rezidnt-egress-ca", "pem");
            std::fs::write(&ca_path, pem_encode_cert(self.ca.ca_cert_der()))
                .map_err(|e| RunError::Spawn(format!("write ca pem: {e}")))?;
            let _ca_guard = TempPathGuard(ca_path.clone());

            // `pasta --config-net -q -f -- <probe> <mode> <proxy_addr> <ca_pem> …`
            //   --config-net: configure the tap interface (give the netns an addr +
            //                 the gateway the proxy maps to).
            //   -q -f:        quiet, foreground (we own the child).
            // The probe seals routes then runs; the connector is EXEC'd, never
            // linked (I7 — no new linked crate for the netns).
            let mut cmd = std::process::Command::new(&self.connector_bin);
            cmd.arg("--config-net").arg("-q").arg("-f").arg("--");
            // c3-wire COMPOSITION (DR-028 §Decision 1): when a splice is present,
            // pasta execs `bwrap <prefix> --` BEFORE the probe, so the probe runs
            // filesystem-confined by bwrap INSIDE pasta's sealed netns. The prefix
            // carries the unshare-all-MINUS-net posture (no `--unshare-net`), so the
            // probe INHERITS pasta's already-sealed, proxy-only netns rather than a
            // fresh empty one. Absent a splice (c3bc-enforce), pasta execs the probe
            // directly. The prefix is folded from the sandbox policy ONLY (C6 guard).
            if let Some(splice) = &self.splice {
                cmd.arg(&splice.bwrap_bin);
                for a in &splice.bwrap_prefix {
                    cmd.arg(a);
                }
                // The CA-cert PEM is the folded READ-ONLY trust anchor the confined
                // probe reads to trust the proxy's per-destination leaf (DR-026
                // §Decision — the sandbox trusts the rezidnt CA via a read-only
                // bind). Under composition bwrap seals the mount-ns, so this
                // daemon-written temp PEM must be bind-mounted or the probe cannot
                // read it (`read CA pem: No such file or directory`). It is a PUBLIC
                // trust anchor (never the CA private key, which never leaves the
                // daemon) — a read-only bind of the exact file the probe was told to
                // read, not a policy widening.
                let ca_str = ca_path.to_string_lossy().into_owned();
                cmd.arg("--ro-bind").arg(&ca_str).arg(&ca_str);
                cmd.arg("--");
            }
            cmd.arg(&self.wiring.probe_bin)
                .arg(mode)
                .arg(&self.proxy_addr)
                .arg(&ca_path);
            for e in extra {
                cmd.arg(e);
            }
            // The probe inherits a clean, marked env; the enforce suite asserts the
            // brokered token is absent from it (criterion 4 — the agent env).
            cmd.env_clear();
            cmd.env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
            let output = cmd
                .output()
                .map_err(|e| RunError::Spawn(format!("pasta confined probe exec failed: {e}")))?;
            Ok(output)
        }
    }

    impl DataplaneHandle for PastaHandle {
        fn proxy_addr(&self) -> &str {
            &self.proxy_addr
        }

        fn run_escape_probe(&self) -> Result<ProbeReport, RunError> {
            let host = self
                .policy
                .allowlist()
                .first()
                .map(|d| d.host.clone())
                .ok_or_else(|| {
                    RunError::Spawn("no allowlisted host to drive the ViaProxy probe".to_string())
                })?;
            let output = self.run_confined(MODE_ESCAPE_PROBE, std::slice::from_ref(&host))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let outcomes = parse_probe_lines(&stdout).map_err(|e| {
                RunError::Spawn(format!(
                    "probe report parse failed ({e}); stdout={stdout:?} stderr={stderr:?}"
                ))
            })?;
            if outcomes.is_empty() {
                return Err(RunError::Spawn(format!(
                    "confined probe produced no outcomes (netns/pasta setup likely failed); \
                     stdout={stdout:?} stderr={stderr:?}"
                )));
            }
            Ok(ProbeReport { outcomes })
        }

        fn drive_injected_egress(
            &self,
            host: &str,
        ) -> Result<(CapturedExchange, Option<String>), RunError> {
            // The upstream capture server (test-support, started by the caller)
            // writes the headers it RECEIVED to `wiring.upstream_capture_path` —
            // independent proof the injection reached the upstream (the capture
            // server records it, not the proxy asserting about itself).
            //
            // The confined probe issues ONE request to `host` through the proxy and
            // reports its OWN env (the agent env) on stdout.
            let extra = vec![host.to_string()];
            let output = self.run_confined(MODE_INJECTED, &extra)?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                return Err(RunError::Spawn(format!(
                    "confined injected-egress probe failed: status={:?} stdout={stdout:?} \
                     stderr={stderr:?}",
                    output.status
                )));
            }

            // The agent's OWN request headers, captured by the proxy at ingress
            // (before injection). The agent never held the token, so its own
            // request cannot carry it.
            let agent_request_headers = self
                .agent_headers
                .lock()
                .map_err(|_| RunError::Spawn("agent-headers mutex poisoned".to_string()))?
                .clone();

            // The agent env: the probe reports its own environment as a JSON line
            // prefixed `AGENT_ENV `, so criterion 4 can assert the token is absent.
            let agent_env = parse_agent_env(&stdout);

            // What the UPSTREAM received (post-injection) — read from the capture
            // server's file (independent of the proxy).
            let upstream_received_headers =
                read_upstream_capture(&self.wiring.upstream_capture_path)?;

            // The by-reference secret_ref for the durable fact (never the value).
            let secret_ref = self
                .policy
                .secret_for(host)
                .map(|s| s.secret_ref().to_string());

            Ok((
                CapturedExchange {
                    agent_request_headers,
                    agent_env,
                    upstream_received_headers,
                },
                secret_ref,
            ))
        }
    }

    impl Drop for PastaHandle {
        fn drop(&mut self) {
            // Signal the accept loop and unblock it with a throwaway connection so
            // the thread observes the flag and exits (the listener drops with it).
            self.shutdown.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(&self.proxy_addr);
            if let Some(t) = self.listener_thread.take() {
                let _ = t.join();
            }
        }
    }

    /// The blocking accept loop (a dedicated std thread, never async). Each
    /// connection: read the ClientHello SNI, mediate against the folded policy,
    /// present the per-SNI leaf, terminate TLS, read the confined client's request
    /// headers (captured as the AGENT side), inject the folded secret into the
    /// UPSTREAM request only (the ONE `.expose()`), dial the upstream, and relay.
    fn proxy_accept_loop(
        listener: TcpListener,
        ca: Arc<RezidntCa>,
        policy: EgressPolicy,
        wiring: EnforceWiring,
        shutdown: Arc<AtomicBool>,
        agent_headers: Arc<Mutex<BTreeMap<String, String>>>,
    ) {
        for stream in listener.incoming() {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            let stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            // One connection at a time is sufficient for the confined probe (it
            // issues serial requests); errors on a single connection are logged and
            // never crash the loop (a broken client is not a proxy failure).
            if let Err(e) = handle_connection(stream, &ca, &policy, &wiring, &agent_headers) {
                // A single broken/denied connection is logged (redacted span — never
                // a secret field) and never crashes the loop or the daemon.
                tracing::warn!(error = %e, "egress proxy connection handling failed");
            }
        }
    }

    fn handle_connection(
        stream: TcpStream,
        ca: &RezidntCa,
        policy: &EgressPolicy,
        wiring: &EnforceWiring,
        agent_headers: &Arc<Mutex<BTreeMap<String, String>>>,
    ) -> Result<(), RunError> {
        stream
            .set_nodelay(true)
            .map_err(|e| RunError::Spawn(format!("proxy nodelay: {e}")))?;
        // Read the ClientHello to learn the SNI (which host the confined client is
        // reaching) BEFORE choosing a leaf — the mediation input.
        let mut acceptor = rustls::server::Acceptor::default();
        let mut stream = stream;
        let accepted = loop {
            let n = acceptor
                .read_tls(&mut stream)
                .map_err(|e| RunError::Spawn(format!("read ClientHello: {e}")))?;
            // EOF before a full ClientHello (a bare probe/close, e.g. the drop
            // wake-up connection) — bail instead of busy-looping on `Ok(None)`.
            if n == 0 {
                return Err(RunError::Spawn(
                    "connection closed before ClientHello completed".to_string(),
                ));
            }
            match acceptor.accept() {
                Ok(Some(a)) => break a,
                Ok(None) => continue,
                // rustls 0.23 returns `(Error, AcceptedAlert)` on accept failure;
                // the alert is best-effort back to the client, the Error is ours.
                Err((e, _alert)) => return Err(RunError::Spawn(format!("tls accept: {e}"))),
            }
        };
        let sni = accepted
            .client_hello()
            .server_name()
            .map(|s| s.to_string())
            .ok_or_else(|| RunError::Spawn("client hello carried no SNI".to_string()))?;

        // MEDIATE — the folded decision (deny-by-default). Off-allowlist → refuse
        // (drop): the confined client's TLS fails to complete, which is a denied
        // egress, never a silent proxy-through. Reuses the SAME allowlist predicate
        // the decide-layer `mediate` uses (the folded policy's private fields).
        if !policy.allows(&sni) {
            return Err(RunError::Spawn(format!(
                "egress to {sni} not approved — connection refused (deny-by-default)"
            )));
        }

        // Present the per-SNI leaf and terminate the confined client's TLS. Use
        // `into_connection` so the ServerConnection CONTINUES from the ClientHello
        // the Acceptor already read — a fresh `ServerConnection::new` would discard
        // that consumed handshake and stall waiting for a ClientHello that already
        // arrived (the confined client sees EAGAIN).
        let server_config = ca.terminating_config(&sni)?;
        let conn = accepted
            .into_connection(Arc::new(server_config))
            .map_err(|(e, _alert)| RunError::Spawn(format!("into connection: {e}")))?;
        let mut tls = rustls::StreamOwned::new(conn, stream);

        // Read the confined client's request line + headers (HTTP/1.1). This is the
        // AGENT side — captured verbatim. The agent never held the token, so this
        // must carry NONE of it (criterion 4).
        let (request_line, headers) = read_http_head(&mut tls)?;
        {
            let mut guard = agent_headers
                .lock()
                .map_err(|_| RunError::Spawn("agent-headers mutex poisoned".to_string()))?;
            *guard = headers.clone();
        }

        // Open the proxy's OWN upstream TLS to the capture server (its host is the
        // mediated `sni`; the dial target is the folded upstream). Trust the
        // upstream CA (public trust anchor, never a secret).
        let upstream = dial_upstream(&sni, wiring)?;
        let mut upstream = upstream;

        // Rebuild the request UPSTREAM and inject the folded secret HERE — the ONE
        // `.expose()` call-site, on the plaintext the agent never sees. The token
        // rides ONLY the upstream request; it never touches a fact, a log line, a
        // trace, or a return value (the redaction discipline).
        let mut upstream_req = String::new();
        upstream_req.push_str(&request_line);
        upstream_req.push_str("\r\n");
        for (k, v) in &headers {
            // Skip any client-supplied Authorization — the broker owns it.
            if k.eq_ignore_ascii_case("authorization") {
                continue;
            }
            upstream_req.push_str(k);
            upstream_req.push_str(": ");
            upstream_req.push_str(v);
            upstream_req.push_str("\r\n");
        }
        if let Some(secret) = policy.secret_for(&sni) {
            // === THE ONE `.expose()` CALL-SITE (DR-026 §Decision, criterion 5). ===
            // The brokered secret bytes leave `BrokeredSecret` HERE and only here,
            // written into the UPSTREAM request bytes. Never near a fact/log/trace/
            // return value; the `secret_ref` (label) is what the durable fact
            // carries. An auditor greps `.expose(` and finds exactly this line.
            upstream_req.push_str("Authorization: Bearer ");
            upstream_req.push_str(secret.expose());
            upstream_req.push_str("\r\n");
        }
        upstream_req.push_str("\r\n");
        upstream
            .write_all(upstream_req.as_bytes())
            .map_err(|e| RunError::Spawn(format!("upstream write: {e}")))?;
        upstream
            .flush()
            .map_err(|e| RunError::Spawn(format!("upstream flush: {e}")))?;

        // Relay the upstream response back to the confined client (so its request
        // completes and the ViaProxy probe observes a real reach — non-vacuity).
        // Many upstreams (incl. our capture server) close the TCP connection
        // WITHOUT a TLS close_notify after `Connection: close`; rustls surfaces
        // that as `UnexpectedEof`. That is a graceful end-of-response here (we
        // already hold the full body), NOT a proxy failure — tolerate it rather
        // than dropping the response and leaving the client with an empty read.
        let resp = read_to_end_tolerant(&mut upstream)?;
        tls.write_all(&resp)
            .map_err(|e| RunError::Spawn(format!("client relay write: {e}")))?;
        let _ = tls.flush();
        Ok(())
    }

    /// Dial the proxy's OWN upstream TLS. The upstream is the folded capture server
    /// (test-support); its self-signed CA is trusted here (a public trust anchor).
    fn dial_upstream(
        sni: &str,
        wiring: &EnforceWiring,
    ) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, RunError> {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(
                wiring.upstream_ca_der.clone(),
            ))
            .map_err(|e| RunError::Spawn(format!("upstream root add: {e}")))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| RunError::Spawn(format!("upstream protocol versions: {e}")))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from(sni.to_string())
            .map_err(|e| RunError::Spawn(format!("upstream server name {sni}: {e}")))?;
        let conn = rustls::ClientConnection::new(Arc::new(config), server_name)
            .map_err(|e| RunError::Spawn(format!("upstream client connection: {e}")))?;
        let sock = TcpStream::connect(wiring.upstream_addr).map_err(|e| {
            RunError::Spawn(format!("upstream connect {}: {e}", wiring.upstream_addr))
        })?;
        Ok(rustls::StreamOwned::new(conn, sock))
    }

    /// Read a TLS stream to EOF, tolerating a peer close WITHOUT a TLS
    /// close_notify (rustls surfaces that as `io::ErrorKind::UnexpectedEof`). For
    /// a proxied response that is a graceful end-of-body — the upstream sent
    /// `Connection: close` and closed — not a failure. Any OTHER error propagates.
    fn read_to_end_tolerant<S: Read>(stream: &mut S) -> Result<Vec<u8>, RunError> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(RunError::Spawn(format!("upstream read: {e}"))),
            }
        }
        Ok(out)
    }

    /// Read an HTTP/1.1 request head (request line + headers) from a terminated
    /// stream. Returns `(request_line, headers)`. Bounded so a malicious client
    /// cannot exhaust memory (I2 discipline: control-plane reads are bounded).
    fn read_http_head<S: Read>(
        stream: &mut S,
    ) -> Result<(String, BTreeMap<String, String>), RunError> {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = stream
                .read(&mut byte)
                .map_err(|e| RunError::Spawn(format!("request head read: {e}")))?;
            if n == 0 {
                break;
            }
            buf.push(byte[0]);
            if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                break;
            }
            if buf.len() > REQUEST_HEAD_LIMIT {
                return Err(RunError::Spawn("request head exceeded bound".to_string()));
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let mut lines = text.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| RunError::Spawn("empty request".to_string()))?
            .to_string();
        let mut headers = BTreeMap::new();
        for line in lines {
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        Ok((request_line, headers))
    }

    /// Parse the confined probe's stdout (one JSON line per escape attempt) into
    /// the `(EscapeAttempt, ProbeReach)` pairs the [`ProbeReport`] carries.
    fn parse_probe_lines(stdout: &str) -> Result<Vec<(EscapeAttempt, ProbeReach)>, String> {
        let mut out = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if !line.starts_with('{') {
                continue;
            }
            let v: serde_json::Value =
                serde_json::from_str(line).map_err(|e| format!("json {line:?}: {e}"))?;
            let kind = v.get("attempt").and_then(|x| x.as_str()).unwrap_or("");
            let reach = v.get("reach").and_then(|x| x.as_str()).unwrap_or("");
            let detail = v
                .get("detail")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let attempt = match kind {
                "unset_proxy_env" => EscapeAttempt::UnsetProxyEnv {
                    target_host: v
                        .get("target")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                },
                "raw_socket_public_ip" => EscapeAttempt::RawSocketPublicIp {
                    ip: v
                        .get("ip")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    port: v.get("port").and_then(|x| x.as_u64()).unwrap_or(0) as u16,
                },
                "alt_dns_resolver" => EscapeAttempt::AltDnsResolver {
                    resolver_ip: v
                        .get("resolver")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                },
                "via_proxy" => EscapeAttempt::ViaProxy {
                    target_host: v
                        .get("target")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                },
                other => return Err(format!("unknown attempt kind {other:?}")),
            };
            let reach = match reach {
                "blocked" => ProbeReach::Blocked { how: detail },
                "reached_proxy" => ProbeReach::ReachedProxy { evidence: detail },
                "reached_open_internet" => ProbeReach::ReachedOpenInternet { evidence: detail },
                other => return Err(format!("unknown reach {other:?}")),
            };
            out.push((attempt, reach));
        }
        Ok(out)
    }

    /// Parse the probe's `AGENT_ENV {json}` line (its own environment) so criterion
    /// 4 can assert the brokered token is absent from the agent env.
    fn parse_agent_env(stdout: &str) -> BTreeMap<String, String> {
        for line in stdout.lines() {
            if let Some(rest) = line.trim().strip_prefix("AGENT_ENV ")
                && let Ok(m) = serde_json::from_str::<BTreeMap<String, String>>(rest)
            {
                return m;
            }
        }
        BTreeMap::new()
    }

    /// Read the upstream capture server's recorded headers (independent proof the
    /// injection reached the upstream).
    fn read_upstream_capture(path: &std::path::Path) -> Result<BTreeMap<String, String>, RunError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| RunError::Spawn(format!("read upstream capture {path:?}: {e}")))?;
        serde_json::from_str(&text)
            .map_err(|e| RunError::Spawn(format!("parse upstream capture: {e}")))
    }

    /// PEM-encode a DER certificate (BEGIN/END CERTIFICATE) — the confined probe
    /// reads this as its trust anchor. Pure, no new dep (base64 via a tiny local
    /// encoder to avoid a linked crate — I7).
    fn pem_encode_cert(der: &[u8]) -> String {
        let b64 = base64_encode(der);
        let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            out.push('\n');
        }
        out.push_str("-----END CERTIFICATE-----\n");
        out
    }

    /// Minimal standard base64 encoder (no linked crate — I7: 20 lines beats a
    /// dependency for a PEM wrapper).
    fn base64_encode(input: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 {
                T[((n >> 6) & 63) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                T[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    /// A unique path under the system temp dir — std-only (no `tempfile` dep, so
    /// rustls+rcgen stay the ONLY new linked deps, criterion 8). Uniqueness from a
    /// fresh `Ulid` (already a crate dep) + the pid.
    fn unique_temp_path(prefix: &str, ext: &str) -> std::path::PathBuf {
        let name = format!(
            "{prefix}-{}-{}.{ext}",
            std::process::id(),
            ulid::Ulid::new()
        );
        std::env::temp_dir().join(name)
    }

    /// Removes its path on drop (best-effort) — the std-only stand-in for the
    /// `tempfile` auto-cleanup we deliberately avoid linking.
    struct TempPathGuard(std::path::PathBuf);
    impl Drop for TempPathGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
