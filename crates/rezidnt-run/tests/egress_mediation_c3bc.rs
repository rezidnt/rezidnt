//! C3b+c-ENFORCE oracle (DR-026 criteria 3 + 4 and the real-traffic arms of
//! 1/7; DR-027 — the enforcement-dataplane slice). The REAL mediated-egress
//! behavior over a LIVE netns→proxy→upstream byte-path:
//!   - an allowlisted host reached THROUGH the live proxy (criterion 1 real-arm),
//!   - INESCAPABILITY — a confined probe's direct-egress attempts reach nothing
//!     but the proxy (criterion 3, THE centerpiece),
//!   - a brokered credential injected UPSTREAM the agent never sees, captured on
//!     both sides (criterion 4), and
//!   - the connector/CA-PRESENT arm mediating for real (criterion 7 real-arm).
//!
//! ## SUITE PLACEMENT — WSL-ONLY, #[cfg(unix)] + requires `pasta` (or
//! `slirp4netns`) + net-namespace support ON THE BOX. This whole file is
//! `#[cfg(unix)]`, so the HOST (Windows) compiles it to ZERO tests — host /vet
//! neither runs nor is satisfied by it ([[vet-is-host-side-wsl-insufficient]]).
//! The pure egress-DECISION-LOGIC, no-widening, secret-never-in-log,
//! degrade-CLOSED-decision, dep-scan, and connector-argv suites are the
//! HOST-runnable oracles for the DECIDE partition (DR-027) of the same criteria:
//!   - `crates/rezidnt-gate/tests/egress_scope_native_c3bc.rs`          (crit 1, 2)
//!   - `crates/rezidnt-run/tests/egress_no_widening_c3bc.rs`            (crit 6, 3-argv)
//!   - `crates/rezidnt-run/tests/egress_secret_never_in_log_c3bc.rs`    (crit 5)
//!   - `crates/rezidnt-run/tests/egress_degrade_and_deps_c3bc.rs`       (crit 7, 8)
//!   - `crates/rezidnt-fabric/tests/egress_unavailable_fold_c3bc.rs`    (crit 7 log)
//!
//! ## WHAT THIS SUITE PROVES THAT THE HOST ANALOGUES CANNOT (the enforce gap)
//! A decision verdict is not a delivered packet; a redacted type is not a
//! terminated TLS session; an argv that routes to the proxy is not a KERNEL
//! routing table with no other exit. The host suites pin the DECISION half
//! (DR-027 c3bc-decide, LANDED). This suite pins the ENFORCEMENT half (DR-027
//! c3bc-enforce): the live `pasta`-in-netns dataplane, the `rustls` terminating
//! listener, the upstream capture, and the confined probe — driven through the
//! [`EgressDataplane`]/[`DataplaneHandle`] seam.
//!
//! ## STATUS — GREEN on WSL (c3bc-enforce landed; the live dataplane mediates for
//! real). These tests were `#[ignore]`-gated while `PastaProxy as
//! EgressDataplane::start` was a `todo!()` impl seam (a real-traffic test that
//! passed before the dataplane existed would test nothing — testing-oracles: test
//! honesty). The implementer built the live dataplane: `pasta` in a sealed netns
//! whose ONLY route is a proxy-only `/32` (no default route — inescapability), a
//! `rustls` terminating listener minting per-SNI leaves from `RezidntCa`, the
//! upstream TLS dial + the ONE `.expose()` upstream-write injection, and the
//! two-sided capture (via a dev-only probe/capture example, `egress_c3bc_probe`).
//! So the `#[ignore]` gates are removed and all four run un-ignored on a WSL box
//! with `pasta` at `/usr/bin/pasta` + unprivileged user/net namespaces. On a box
//! WITHOUT the backend, `egress_backend_absent` early-returns (the absent-arm is
//! host-covered) — the C3a `bwrap` precedent exactly.
//!
//! ## RUNTIME REQUIREMENTS a box needs to run these green:
//!   - `pasta` (passt) at `/usr/bin/pasta`; unprivileged user+net namespaces.
//!   - Outbound reachability to ONE public IP:port for the inescapability probe's
//!     `RawSocketPublicIp`/`AltDnsResolver` attempts to have something to (fail to)
//!     reach — the probe asserts they reach NOTHING, so a box with NO outbound at
//!     all would pass VACUOUSLY. The harness guards against that: the `ViaProxy`
//!     attempt MUST succeed (proving the route works), so a dead-netns box fails
//!     the `proxy_path_reached` guard rather than passing a vacuous escape check.
//!
//! Each test states, inline, the real mechanism it drives and why the host
//! analogue is insufficient.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use rezidnt_run::egress::{
    BrokeredSecret, CapturedExchange, DataplaneHandle, Destination, EgressAvailability,
    EgressDataplane, EgressPolicy, EgressProxy, EnforceWiring, Mediation, PastaProxy, ProbeReport,
};

/// The seeded token the agent must NEVER see (criterion 4). A distinctive
/// sentinel so the agent-side/upstream capture assertions are adversarial: if the
/// agent's own request or env carries these bytes, the primitive is defeated.
const FOLDED_TOKEN: &str = "ghp_folded_secret_the_agent_never_holds_0xC3BC";
/// The reference LABEL the injection fact is allowed to carry (never the value).
const TOKEN_REF: &str = "github-token";

/// A folded egress policy: `github.com` allowlisted, mapped to a brokered token
/// the agent never holds. Stands in for the daemon's fold of the project
/// spec/role layer + the daemon-side secret store (DR-026 §Decision).
fn folded_policy() -> EgressPolicy {
    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new(TOKEN_REF, FOLDED_TOKEN),
    );
    EgressPolicy::from_folded_authority(vec![Destination::host("github.com")], injection)
}

/// Skip the body when the egress backend is not usable on this box — the real
/// mediation arms need the connector + CA. (The backend-ABSENT degrade arm is the
/// host-runnable probe suite, not here.) Returns true when the caller should
/// early-return.
fn egress_backend_absent(proxy: &PastaProxy) -> bool {
    !matches!(proxy.availability(), EgressAvailability::Available)
}

/// Locate the dev-only probe/capture example binary (`egress_c3bc_probe`) built
/// alongside this test. The test binary lives in `<target>/debug/deps/`; the
/// example sits at `<target>/debug/examples/egress_c3bc_probe`. Dev-only
/// test-support (DR-023) — NEVER the shipped daemon binary (I7).
fn probe_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
    // .../debug/deps/egress_mediation_c3bc-HASH  ->  .../debug/examples/<bin>
    let debug = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("target/debug dir");
    debug.join("examples").join("egress_c3bc_probe")
}

/// A live dataplane handle PLUS the test-support upstream capture server it drives.
/// Owns the capture child + its temp files so they outlive the run; delegates the
/// `DataplaneHandle` methods to the real `PastaHandle`. Dropping tears the capture
/// server down.
struct TestDataplane {
    inner: Box<dyn DataplaneHandle>,
    capture_child: Child,
    _tmp: tempfile::TempDir,
}

impl DataplaneHandle for TestDataplane {
    fn proxy_addr(&self) -> &str {
        self.inner.proxy_addr()
    }
    fn run_escape_probe(&self) -> Result<ProbeReport, rezidnt_run::RunError> {
        self.inner.run_escape_probe()
    }
    fn drive_injected_egress(
        &self,
        host: &str,
    ) -> Result<(CapturedExchange, Option<String>), rezidnt_run::RunError> {
        self.inner.drive_injected_egress(host)
    }
}

impl Drop for TestDataplane {
    fn drop(&mut self) {
        let _ = self.capture_child.kill();
        let _ = self.capture_child.wait();
    }
}

/// Start the live dataplane for a confined mediated-egress run — the seam every
/// real-traffic arm drives. Stands up the test-support capturing upstream TLS
/// server (host namespace), wires the `PastaProxy` to it + the confined probe
/// binary, and calls the real `EgressDataplane::start`. The returned handle
/// carries a genuine netns→proxy→upstream byte-path (no fake pass).
fn handle(_proxy: &PastaProxy, policy: &EgressPolicy) -> Box<dyn DataplaneHandle> {
    let tmp = tempfile::tempdir().expect("dataplane tempdir");
    let ca_der_out = tmp.path().join("upstream-ca.der");
    let capture_out = tmp.path().join("upstream-capture.json");

    // Reserve an ephemeral port for the capture upstream, then hand it the exact
    // addr (the probe/proxy dial `upstream_addr`). Bind+drop to reserve.
    let upstream_addr = {
        let l = TcpListener::bind("127.0.0.1:0").expect("reserve upstream port");
        l.local_addr().expect("upstream addr")
    };

    // Start the capturing upstream TLS server (dev-only). It writes its CA DER to
    // `ca_der_out` (for the proxy to trust) and the received headers to
    // `capture_out` (independent criterion-4 proof).
    let mut capture_child = Command::new(probe_bin())
        .arg("upstream-capture")
        .arg(upstream_addr.to_string())
        .arg(&ca_der_out)
        .arg(&capture_out)
        .arg("github.com")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn upstream capture server");

    // Wait for the UPSTREAM_READY line so the bind is up before the proxy dials.
    let stdout = capture_child.stdout.take().expect("capture stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read capture ready line");
    assert!(
        line.starts_with("UPSTREAM_READY"),
        "capture server signalled ready (got {line:?})"
    );

    let upstream_ca_der = std::fs::read(&ca_der_out).expect("read upstream CA der");

    let proxy = PastaProxy {
        connector_bin: None,
        enforce: Some(EnforceWiring {
            probe_bin: probe_bin(),
            upstream_addr,
            upstream_ca_der,
            upstream_capture_path: capture_out,
        }),
    };
    let inner = proxy.start(policy).expect(
        "the live egress dataplane starts (pasta netns + rustls listener + upstream capture)",
    );
    Box::new(TestDataplane {
        inner,
        capture_child,
        _tmp: tmp,
    })
}

/// CRITERION 1 (real-arm) — an agent reaches an ALLOWLISTED host THROUGH the live
/// proxy: not just a `Reach` verdict, but a delivered mediation over a real netns
/// → proxy path. The host analogue (`egress_scope_native_c3bc.rs`) pins the
/// VERDICT; this pins the DELIVERED reach over a live dataplane (a verdict is not
/// a packet). The `ViaProxy` probe attempt reaching the proxy IS the delivered
/// reach.
///
/// UN-IGNORED (c3bc-enforce landed): `EgressDataplane::start` now builds the live
/// pasta netns + rustls proxy, so this runs green on a box with `pasta` + user/net
/// namespaces (WSL Ubuntu-24.04). On a box WITHOUT the backend, `egress_backend_absent`
/// early-returns (the absent-arm is host-covered) — the C3a bwrap precedent.
#[test]
fn allowlisted_host_is_reached_through_the_proxy() {
    let proxy = PastaProxy::default();
    if egress_backend_absent(&proxy) {
        return; // no connector/CA on this box — the mediation arm is not applicable
    }
    let policy = folded_policy();

    // The decision half (green in the decide layer) still holds: the folded policy
    // decides Reach for the allowlisted host.
    assert!(
        matches!(
            proxy.mediate("github.com", &policy),
            Mediation::Reach { .. }
        ),
        "the folded allowlist decides Reach for github.com (the decision precondition)"
    );

    // The ENFORCEMENT half this suite adds: the confined agent actually REACHES
    // github.com THROUGH the live proxy. The `ViaProxy` probe attempt landing on
    // the proxy is the delivered reach — a verdict turned into a carried packet.
    let h = handle(&proxy, &policy);
    let report: ProbeReport = h
        .run_escape_probe()
        .expect("the confined probe runs inside the netns");
    assert!(
        report.proxy_path_reached(),
        "CRITERION 1 real-arm: the confined agent must actually REACH the allowlisted host \
         THROUGH the live proxy (the ViaProxy path carries traffic) — a Reach verdict that \
         delivers no packet is not enforcement. Report: {report:?}"
    );
}

/// CRITERION 3 (the inescapability centerpiece) — a confined agent's DIRECT-egress
/// attempts reach NOTHING but the proxy. With the netns started with NO default
/// route (only a path to the proxy), an agent that: unsets every proxy env var,
/// opens a RAW socket to a public IP, and uses an ALTERNATE DNS resolver — reaches
/// the proxy or is denied, NEVER the open internet. DNS resolves THROUGH the
/// mediator or is denied. No non-proxy path exists out of the netns.
///
/// This is inherently a real-netns test: the property is that the KERNEL routing
/// of the namespace has no non-proxy exit, which a unit test cannot assert (the
/// host analogue is the connector-argv routing pin in `egress_no_widening_c3bc.rs`,
/// which asserts the argv routes ALL outbound TCP+DNS to the proxy with no direct
/// route — necessary but NOT sufficient; only the live netns proves inescapability).
///
/// NON-VACUOUS guard: the `ViaProxy` attempt MUST succeed (`proxy_path_reached`) —
/// so a box with NO outbound at all cannot pass this vacuously (every attempt
/// blocked including the proxy would mean a dead netns, not an inescapable one).
///
/// UN-IGNORED (c3bc-enforce landed): the live pasta netns starts with the host
/// routes, then the confined probe SEALS them to a proxy-only `/32` (no default
/// route). A raw socket to a public IP then gets ENETUNREACH — proven here. Runs
/// green on WSL with `pasta` + user/net namespaces; `egress_backend_absent`
/// early-returns on a box without the backend.
#[test]
fn direct_egress_attempts_reach_nothing_but_the_proxy() {
    let proxy = PastaProxy::default();
    if egress_backend_absent(&proxy) {
        return;
    }
    let policy = folded_policy();
    let h = handle(&proxy, &policy);

    // Run the confined direct-egress probe INSIDE the netns: it (a) unsets
    // HTTP_PROXY/HTTPS_PROXY/ALL_PROXY and tries a public host by name, (b) opens
    // a raw TCP socket to a known-public IP, (c) queries an alternate DNS resolver
    // directly, and (d) reaches the allowlisted host via the proxy.
    let report: ProbeReport = h
        .run_escape_probe()
        .expect("the confined direct-egress probe runs inside the netns");

    // THE inescapability assertion: NO direct-egress (non-proxy) attempt reached
    // the open internet. A single byte delivered to a non-proxy destination is an
    // inescapability HOLE (theater, DR-026 §8.2 — "a bypass makes it theater").
    assert!(
        !report.any_direct_escape_reached_internet(),
        "CRITERION 3 VIOLATION (inescapability HOLE): a direct-egress attempt (unset-proxy-env, \
         raw socket, or alternate DNS) reached the OPEN INTERNET — the netns has a non-proxy \
         exit and the chokepoint is escapable. Every direct attempt must reach NOTHING (blocked) \
         or land on the proxy; none may reach the open internet. Report: {report:?}"
    );

    // NON-VACUOUS: the proxy path itself MUST work — a dead netns (everything
    // blocked, including the proxy) would pass the escape check vacuously. The
    // sole route out must actually carry traffic.
    assert!(
        report.proxy_path_reached(),
        "the ViaProxy path must REACH the proxy (the sole route works) — otherwise the escape \
         check is vacuous (a netns with no route at all is not the same as an inescapable one). \
         Report: {report:?}"
    );
}

/// CRITERION 4 — a brokered credential is injected on approved egress and the
/// agent NEVER sees the secret: it is absent from the agent's ENVIRONMENT and from
/// the agent's OWN request; only the UPSTREAM request (post-termination) carries
/// it. Drive a real mediated+injected flow through the live dataplane; capture
/// BOTH sides (agent ingress vs upstream received); assert the token is absent
/// agent-side and present upstream, and that only the `secret_ref` (never the
/// value) rides the durable fact.
///
/// The host analogue (`egress_secret_never_in_log_c3bc.rs`) proves the secret
/// never hits the LOG; this proves the secret never hits the AGENT — a different
/// surface (the agent's env/request vs the fabric). Both are needed.
///
/// UN-IGNORED (c3bc-enforce landed): the proxy terminates the confined client's
/// TLS, injects the folded token into the UPSTREAM request only (the ONE
/// `.expose()`), and the capturing upstream records what it received. Green on WSL;
/// `egress_backend_absent` early-returns without the backend.
#[test]
fn credential_injected_upstream_agent_never_sees_it() {
    let proxy = PastaProxy::default();
    if egress_backend_absent(&proxy) {
        return;
    }
    let policy = folded_policy();
    let h = handle(&proxy, &policy);

    // Drive ONE approved+mapped mediated egress end-to-end: the confined agent
    // issues its request, the proxy terminates TLS + injects the folded token into
    // the UPSTREAM request only (the ONE `.expose()` call-site), and the capture
    // server records what it received. Returns both sides + the by-ref secret_ref.
    let (exchange, secret_ref): (CapturedExchange, Option<String>) = h
        .drive_injected_egress("github.com")
        .expect("an approved+mapped egress injects upstream and captures both sides");

    // (1) The upstream RECEIVED the token — the injection reached its only
    // legitimate destination. Non-vacuous: the token must land SOMEWHERE, and the
    // only allowed somewhere is the upstream the agent never sees.
    assert!(
        exchange.upstream_contains(FOLDED_TOKEN),
        "CRITERION 4: the UPSTREAM request (post-termination) must carry the injected token — \
         the injection reached the upstream the agent never sees. Upstream headers: {:?}",
        exchange.upstream_received_headers
    );

    // (2) THE load-bearing half — the AGENT's own request AND its environment
    // carry NO token bytes. The agent cannot exfiltrate what it never held.
    assert!(
        !exchange.agent_side_contains(FOLDED_TOKEN),
        "CRITERION 4 VIOLATION: the brokered token appeared on the AGENT side — in the agent's \
         own request headers or its environment. The agent must NEVER hold or transmit the \
         secret; only the upstream (which the agent never sees) may carry it. Agent request: \
         {:?}, agent env keys: {:?}",
        exchange.agent_request_headers,
        exchange.agent_env.keys().collect::<Vec<_>>()
    );

    // (3) The durable injection fact rides BY REFERENCE: the returned secret_ref
    // names WHICH secret was injected (criterion 5's contract, on the real path),
    // never the value.
    assert_eq!(
        secret_ref.as_deref(),
        Some(TOKEN_REF),
        "the injection is recorded BY REFERENCE — the secret_ref (not the value) names it \
         (CRITERION 4/5 contract on the live path)"
    );

    // (4) Belt-and-braces on the by-reference contract: the returned ref is the
    // LABEL, never the value bytes (a careless impl returning `.expose()` here
    // would leak — this catches it on the live path, complementing the host
    // whole-fabric scan).
    assert!(
        !secret_ref
            .as_deref()
            .is_some_and(|r| r.contains(FOLDED_TOKEN)),
        "CRITERION 5 (live): the returned secret_ref must be the LABEL, never the value bytes"
    );
}

/// CRITERION 7 (connector/CA-PRESENT real-arm) — when the backend IS available,
/// the substrate reports `Available` AND stands up a live dataplane that mediates
/// for real (no CLOSED degrade). The complement of the host-runnable
/// backend-ABSENT probe: here availability drives the LIVE MEDIATED path, not the
/// degrade. (The degrade-CLOSED absent arm is already host-covered by
/// `egress_degrade_and_deps_c3bc.rs` — the DR-026 crit-7 real-arm delta is that
/// PRESENT actually mediates over a live path, not merely reports Available.)
///
/// UN-IGNORED (c3bc-enforce landed): a PRESENT backend now stands up a live
/// mediating dataplane (the ViaProxy path carries traffic), not merely reports
/// Available. Green on WSL; without the backend the present-arm is not applicable
/// (the absent-arm is host-covered by `egress_degrade_and_deps_c3bc.rs`).
#[test]
fn egress_backend_present_reports_available_and_mediates() {
    let proxy = PastaProxy::default();
    if !matches!(proxy.availability(), EgressAvailability::Available) {
        eprintln!(
            "egress backend absent on this box; the present real-arm is not applicable here \
             (the absent-arm is host-runnable in egress_degrade_and_deps_c3bc.rs)"
        );
        return;
    }
    // Available: name + backend as before ...
    assert_eq!(proxy.backend(), "pasta+rustls");

    // ... AND the real-arm delta — availability drives a LIVE dataplane that
    // actually mediates (the ViaProxy path carries traffic), not merely a reported
    // Available with no delivered egress.
    let policy = folded_policy();
    let h = handle(&proxy, &policy);
    let report = h
        .run_escape_probe()
        .expect("the present backend stands up a live mediating dataplane");
    assert!(
        report.proxy_path_reached(),
        "CRITERION 7 real-arm: a PRESENT backend must actually MEDIATE over a live path (the \
         proxy route carries traffic), not merely report Available. Report: {report:?}"
    );
}
