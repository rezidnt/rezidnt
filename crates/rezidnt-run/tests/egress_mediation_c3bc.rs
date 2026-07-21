//! C3b+c oracle (DR-026 — the L7 egress-MITM + credential-brokering slice) — the
//! REAL mediated-egress behavior: an allowlisted host reached THROUGH the proxy
//! (criterion 1), inescapability — direct-egress attempts reach nothing but the
//! proxy (criterion 3), a brokered credential injected upstream the agent never
//! sees (criterion 4), and the connector/CA-PRESENT arm of degrade-closed
//! (criterion 7).
//!
//! ## SUITE PLACEMENT — WSL-ONLY, #[cfg(unix)] + requires `pasta` (or
//! `slirp4netns`) + net-namespace support ON THE BOX. This whole file is
//! `#[cfg(unix)]`, so the HOST (Windows) compiles it to ZERO tests — host /vet
//! neither runs nor is satisfied by it ([[vet-is-host-side-wsl-insufficient]]).
//! The pure egress-DECISION-LOGIC, no-widening, secret-never-in-log,
//! degrade-CLOSED-decision, dep-scan, and connector-argv suites are the
//! HOST-runnable oracles for the same criteria:
//!   - `crates/rezidnt-gate/tests/egress_scope_native_c3bc.rs`          (crit 1, 2)
//!   - `crates/rezidnt-run/tests/egress_no_widening_c3bc.rs`            (crit 6, 3-argv)
//!   - `crates/rezidnt-run/tests/egress_secret_never_in_log_c3bc.rs`    (crit 5)
//!   - `crates/rezidnt-run/tests/egress_degrade_and_deps_c3bc.rs`       (crit 7, 8)
//!   - `crates/rezidnt-fabric/tests/egress_unavailable_fold_c3bc.rs`    (crit 7 log)
//!
//! ## STATUS — #[ignore]-GATED (impl is a `todo!()` stub AND the netns/connector
//! runtime is not guaranteed installable non-interactively). Two reasons these are
//! ignored, both honest:
//!   1. The two LOAD-BEARING real-integration arms — `direct_egress_attempts_*`
//!      (crit 3, inescapability) and `credential_injected_upstream_*` (crit 4,
//!      the agent never sees the token) — are oracle `unimplemented!()` bodies:
//!      they require a netns direct-egress PROBE (a confined child that opens raw
//!      sockets + an alternate resolver and proves NONE reaches the open internet)
//!      and an agent-side request/env CAPTURE (proving the token rides only the
//!      upstream, never the agent). Those two are genuinely-new subsystems beyond
//!      the `EgressProxy` seam (a live pasta-in-netns dataplane + a TLS-terminating
//!      listener + an upstream proxy + a probe binary), NOT built by this slice.
//!      A test that PASSED without them would test nothing (testing-oracles: test
//!      honesty). The C3a `sandbox_bwrap_confinement_c3a.rs` precedent: gated while
//!      the real dataplane is unbuilt; the exit-demo runner builds + un-gates it.
//!   2. `pasta` + net-namespace creation may not be installable/permitted
//!      non-interactively on every WSL box ([[vet-concurrency-flake]] — netns +
//!      raw sockets are privileged-adjacent). If it cannot be provisioned, these
//!      stay ignored with THIS note; the exit-demo runner removes them (same as
//!      C3a's bwrap gating).
//!
//! ## WSL PROVISIONING RESULT (implementer, C3b+c landing — recorded honestly).
//! On the dev box (Ubuntu-24.04 under WSL2) `passt` INSTALLED non-interactively
//! (`sudo -n apt-get install -y passt` → `pasta` at `/usr/bin/pasta`) AND an
//! unprivileged user+net namespace CREATES (`unshare --user --net --map-root-user
//! true` → 0). With that, running THIS suite `--include-ignored` shows the two
//! DECISION arms pass FOR REAL on a backend-present box. Arm crit 1
//! (`allowlisted_host_is_reached_through_the_proxy`) passes: the real
//! `PastaProxy::mediate` returns `Reach` against the folded policy. Arm crit
//! 7-present (`egress_backend_present_reports_available_and_mediates`) passes:
//! `availability()` reports `Available` (pasta present + the rcgen CA builds) and
//! `backend()` is `"pasta+rustls"`.
//!
//! The two LOAD-BEARING arms (crit 3 inescapability, crit 4 agent-never-sees)
//! still `unimplemented!()`-panic: the live netns dataplane + agent-capture are
//! not built. So the WHOLE suite stays `#[ignore]`'d (a file is a unit; the two
//! decision arms are also proven host-side — the mediation predicate is the same
//! `allows()` proven in `egress_no_widening_c3bc.rs`, the availability probe the
//! same one proven in `egress_degrade_and_deps_c3bc.rs`). Faking crit 3/4 green
//! is NOT an acceptable landing; this honest gate + the host criteria green IS
//! (the C3a bwrap precedent). What crit 3/4 still need: a pasta-in-netns dataplane
//! with no default route, a rustls terminating listener + upstream proxy, and a
//! confined probe + agent-request capture — the exit-demo runner's next build.
//!
//! Each test states, inline, the real mechanism it drives and why the host
//! analogue is insufficient (a decision verdict is not a delivered packet; a
//! redacted type is not a terminated TLS session).

#![cfg(unix)]

use std::collections::BTreeMap;

use rezidnt_run::egress::{
    BrokeredSecret, Destination, EgressAvailability, EgressPolicy, EgressProxy, Mediation,
    PastaProxy,
};

/// A folded egress policy: `github.com` allowlisted, mapped to a brokered token
/// the agent never holds. Stands in for the daemon's fold of the project
/// spec/role layer + the daemon-side secret store (DR-026 §Decision).
fn folded_policy() -> EgressPolicy {
    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new("github-token", "ghp_folded_secret_the_agent_never_holds"),
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

/// CRITERION 1 — an agent reaches an ALLOWLISTED host THROUGH the proxy: the
/// mediation decides `Reach` and the connection is terminated + proxied for real.
/// The host analogue (`egress_scope_native_c3bc.rs`) pins the VERDICT; this pins
/// the delivered mediation over a real netns → proxy path (a verdict is not a
/// packet).
///
/// #[ignore]: `mediate`/`availability` are `todo!()` AND netns/connector may be
/// absent. The implementer removes the gate when the pasta+rustls path lands.
#[test]
#[ignore = "needs pasta + net-namespace runtime AND the todo!() PastaProxy impl (DR-026 crit 1); \
            host analogue: egress_scope_native_c3bc.rs::allowlisted_dest_passes"]
fn allowlisted_host_is_reached_through_the_proxy() {
    let proxy = PastaProxy::default();
    if egress_backend_absent(&proxy) {
        return; // no connector/CA on this box — the mediation arm is not applicable
    }
    let policy = folded_policy();
    let decision = proxy.mediate("github.com", &policy);
    assert!(
        matches!(decision, Mediation::Reach { .. }),
        "an allowlisted host is REACHED through the proxy (terminated + proxied), CRITERION 1; \
         got {decision:?}"
    );
}

/// CRITERION 3 (the inescapability centerpiece) — a confined agent's DIRECT-egress
/// attempts reach NOTHING but the proxy. With the netns started with no default
/// route (only a path to the proxy), an agent that: unsets every proxy env var,
/// opens a RAW socket to a public IP, and uses an ALTERNATE DNS resolver — reaches
/// the proxy or is denied, NEVER the open internet. DNS resolves THROUGH the
/// mediator or is denied. No non-proxy path exists out of the netns.
///
/// This is inherently a real-netns test: the property is that the KERNEL routing
/// of the namespace has no non-proxy exit, which a unit test cannot assert (the
/// host analogue is the connector-argv routing pin in `egress_no_widening_c3bc.rs`,
/// which asserts the argv routes ALL outbound TCP+DNS to the proxy with no direct
/// route — necessary but not sufficient; only the live netns proves inescapability).
///
/// #[ignore]: needs a real net namespace + the connector wiring + a probe binary.
/// The implementer/exit-demo runner provisions it and removes the gate.
#[test]
#[ignore = "needs a real net namespace with no default route + pasta connector (DR-026 crit 3, \
            the inescapability centerpiece); host analogue: egress_no_widening_c3bc.rs::\
            connector_routes_only_to_the_folded_proxy_not_a_plan_supplied_one (argv-level, \
            necessary-not-sufficient)"]
fn direct_egress_attempts_reach_nothing_but_the_proxy() {
    let proxy = PastaProxy::default();
    if egress_backend_absent(&proxy) {
        return;
    }
    // The real test (implementer): spawn a confined probe inside the netns that
    // (a) unsets HTTP_PROXY/HTTPS_PROXY/ALL_PROXY, (b) opens a raw TCP socket to a
    // known-public IP (e.g. 1.1.1.1:80), (c) queries an alternate DNS resolver
    // (e.g. 8.8.8.8) directly. ASSERT: every attempt either lands on the rezidnt
    // proxy or fails — NONE reaches the open internet. A single byte delivered to
    // a non-proxy destination is an inescapability HOLE (theater, DR-026 §8.2 —
    // "a bypass makes it theater"). The mechanism + probe are the implementer's.
    let _policy = folded_policy();
    unimplemented!(
        "implementer: netns direct-egress probe — unset-proxy-env + raw-socket + alt-DNS all \
         reach ONLY the proxy or are denied (CRITERION 3, inescapability)"
    );
}

/// CRITERION 4 — a brokered credential is injected on approved egress and the
/// agent NEVER sees the secret: it is absent from the agent's environment and from
/// the agent's own request; only the UPSTREAM request (post-termination) carries
/// it. Drive a real mediated+injected flow; assert the injection happened (the
/// returned `secret_ref` names it) AND that the agent-side request/env carried no
/// token.
///
/// The host analogue (`egress_secret_never_in_log_c3bc.rs`) proves the secret
/// never hits the LOG; this proves the secret never hits the AGENT — a different
/// surface (the agent's env/request vs the fabric). Both are needed.
///
/// #[ignore]: `inject_and_proxy` is `todo!()` AND needs the real termination path.
#[test]
#[ignore = "needs pasta + rustls termination + a real upstream (DR-026 crit 4, credential \
            non-exposure); host analogue: egress_secret_never_in_log_c3bc.rs (log surface, not \
            the agent surface)"]
fn credential_injected_upstream_agent_never_sees_it() {
    let proxy = PastaProxy::default();
    if egress_backend_absent(&proxy) {
        return;
    }
    let policy = folded_policy();
    // The injection returns the secret_ref (never the value) — proof THAT a secret
    // was injected and WHICH, by reference (criterion 5's contract, exercised on
    // the real path).
    let secret_ref = proxy
        .inject_and_proxy("github.com", &policy)
        .expect("an approved+mapped egress injects and returns its secret_ref");
    assert_eq!(
        secret_ref.as_deref(),
        Some("github-token"),
        "the upstream request carried the folded token BY REFERENCE (secret_ref), CRITERION 4"
    );
    // The real test (implementer): assert the AGENT's captured request headers +
    // the agent's environment contain NO token bytes — only the upstream (which
    // the agent never sees) does. The capture seam is the implementer's; the
    // never-in-agent assertion is the load-bearing half.
    unimplemented!(
        "implementer: assert the agent's env + own request carried NO token — only the upstream \
         post-termination request does (CRITERION 4, the agent cannot exfiltrate what it never held)"
    );
}

/// CRITERION 7 (connector/CA-PRESENT arm) — when the backend IS available, the
/// substrate reports `Available` and mediates for real (no CLOSED degrade). The
/// complement of the host-runnable backend-ABSENT probe: here availability drives
/// the MEDIATED path, not the degrade.
///
/// #[ignore]: `availability` is `todo!()`; un-ignored once the probe lands and a
/// box with pasta+CA runs it.
#[test]
#[ignore = "needs pasta + CA present + the todo!() availability probe (DR-026 crit 7 present-arm); \
            host analogue: egress_degrade_and_deps_c3bc.rs (the absent-arm)"]
fn egress_backend_present_reports_available_and_mediates() {
    let proxy = PastaProxy::default();
    if let EgressAvailability::Available = proxy.availability() {
        assert_eq!(proxy.backend(), "pasta+rustls");
    } else {
        eprintln!(
            "egress backend absent on this box; the present-arm is not applicable here \
             (the absent-arm is host-runnable in egress_degrade_and_deps_c3bc.rs)"
        );
    }
}
