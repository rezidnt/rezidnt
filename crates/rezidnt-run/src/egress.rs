//! Egress-proxy substrate seam (C3b+c — DR-026; design
//! `permit-egress-proxy-c3b.md`). The `EgressProxy` (I4) is the TLS-terminating
//! chokepoint that makes the deterministic egress permit verdict (`EgressScope`,
//! an `EgressScope` sibling of `PathConfinement` in `rezidnt-gate`) the ONLY
//! route out of C3a's sealed netns — and brokers a secret the agent never holds.
//!
//! ## Scope: this is the DECISION layer — ENFORCEMENT-INERT (DR-027 split)
//!
//! DR-026's folded C3b+c was split by DR-027 into **c3bc-decide** (this module,
//! landed) and **c3bc-enforce** (the dataplane, next slice). What lives here and
//! is proven green: the `EgressScope` decision (allow/deny/escalate from folded
//! policy), the [`EgressPolicy`] no-widening guard, the redacted
//! [`BrokeredSecret`] type, the connector-argv renderer, the availability/
//! degrade-CLOSED probe, and the [`RezidntCa`] + `rustls` terminating-config
//! **scaffolding** (proves the CA/leaf lifecycle compiles and runs). What is NOT
//! here and is c3bc-enforce's job: the live `pasta` netns→proxy→upstream
//! **byte-path** (inescapability, DR-026 crit 3), live TLS termination of real
//! traffic, and **real credential injection** into the upstream request
//! (crit 4). `mediate`/`inject_and_proxy` therefore DECIDE and build certs but
//! carry no live traffic and inject nothing — they return the `secret_ref` only.
//! **This substrate MUST NOT be wired into a live run loop as if it enforced
//! egress or brokered a credential until c3bc-enforce ships** (the DR-027 honesty
//! guard — the decision layer announces itself inert, it does not look
//! authoritative).
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
