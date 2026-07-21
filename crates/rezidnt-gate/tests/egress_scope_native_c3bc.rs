//! C3b+c oracle (DR-026 — the L7 egress-MITM + credential-brokering slice) — the
//! `EgressScope` native permit-verifier: the deterministic "is this destination
//! ALLOWLISTED for egress?" decision, a `PathConfinement`/`PathScope` sibling. The
//! OS egress proxy (`EgressProxy`, in `rezidnt-run`) makes THIS verdict
//! unbypassable; here we pin the VERDICT itself (I6 — pure, three-valued,
//! interrogable), which is host-runnable and needs no `pasta`/netns/TLS.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (native, no connector/TLS, no #[cfg(unix)]).
//! This whole file is the pure egress-DECISION-LOGIC oracle: it runs on every
//! host that builds rezidnt (Windows host /vet included). The REAL mediated
//! egress / TLS termination / injection behavior lives in the `#[cfg(unix)]`
//! WSL-only suite (`crates/rezidnt-run/tests/egress_mediation_c3bc.rs`), which
//! compiles to 0 tests on host ([[vet-is-host-side-wsl-insufficient]]).
//!
//! GREEN (c3bc-decide, DR-027): `rezidnt_gate::EgressScope::verify` is implemented
//! and registered in `builtin_natives`, so these tests pin the live allowlist-match
//! verdict. They were authored assert-red (panicking on the pre-impl `todo!()`) and
//! now pass against the decision layer; the mechanism that makes this verdict
//! unbypassable (the live proxy) is c3bc-enforce, not this file.
//!
//! ## Params shape pinned by this file (mirrors the `PathConfinement` C3a shape)
//! The native reads its params AFTER the PDP merged the request axis (the
//! requested destination) with the FOLDED egress policy (the allowlist) — so
//! `allow` is ALREADY on the axis here, injected from folded authority (DR-026
//! §Decision, the C6 lesson), never self-declared:
//!
//! ```text
//! { "dest": str,          // the destination host the agent is trying to reach
//!   "allow": [str] }      // FOLDED egress allowlist (host strings)
//! ```
//!
//! At the UNIT level this file feeds `allow` directly into `params` (standing in
//! for the PDP's fold-and-inject); the LIVE folded-authority path (a request
//! value that tries to WIDEN `allow` is ignored) is pinned in
//! `crates/rezidnt-run/tests/egress_no_widening_c3bc.rs` (criterion 6).

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{EgressScope, NativeVerifier, Verdict, VerifierInput};
use serde_json::{Value, json};

fn permit_input(params: Value) -> VerifierInput {
    VerifierInput {
        gate: "permit".to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params,
        timeout_ms: 120_000,
    }
}

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

/// A folded egress allowlist naming the two hosts a governed run may reach —
/// the allowlist the daemon folds from the project spec `[gates.permit]`/role
/// layer (DR-026 §Decision).
fn allowlist() -> Value {
    json!(["github.com", "api.anthropic.com"])
}

/// CRITERION 1 (decision leg) — a request to an ALLOWLISTED host is egress-OK:
/// the destination is on the folded allowlist → Pass (the proxy would terminate +
/// proxy it; the verdict says so). This is the "reaches an allowlisted host
/// through the proxy" arm's DECISION half (the real reach-through-proxy is the
/// WSL integration test).
///
/// RED: `verify` is `todo!()` → panic. Green once the allowlist-match scan lands.
#[test]
fn allowlisted_dest_passes() {
    let (_dir, cas) = empty_cas();
    let out = EgressScope
        .verify(
            &permit_input(json!({ "dest": "github.com", "allow": allowlist() })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "an allowlisted destination is egress-OK → Pass (CRITERION 1 decision leg)"
    );
}

/// CRITERION 2 — a request to a NON-allowlisted host is DENIED, and the evidence
/// NAMES the refused destination (interrogable, I6). This is the "a request to a
/// non-allowlisted host is DENIED and logged — not a silent success and not a
/// crash" arm: the native returns a Fail VERDICT (which the PDP logs as
/// `permit.denied`/`egress.denied`), never a Pass and never an `Err`/panic.
///
/// RED: `verify` is `todo!()` → panic. Green once the deny-by-default scan lands.
#[test]
fn non_allowlisted_dest_denies_and_evidence_names_it() {
    let (_dir, cas) = empty_cas();
    let out = EgressScope
        .verify(
            &permit_input(json!({ "dest": "evil.example.com", "allow": allowlist() })),
            &cas,
        )
        .expect("engine ok — a can't-reach is a VERDICT, never an Err (I6)");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "a destination outside the allowlist is DENIED → Fail (CRITERION 2)"
    );
    assert!(!out.evidence.is_empty(), "a deny carries evidence (I6)");
    assert!(
        out.evidence[0].msg.contains("evil.example.com"),
        "the deny evidence NAMES the refused destination so the denial is interrogable \
         (I6, CRITERION 2); got {:?}",
        out.evidence[0].msg
    );
    // I2: the evidence blob rides as a CAS ref, never inline bytes (the
    // ForbiddenPath/PathConfinement house pattern).
    assert!(
        out.evidence[0].cas_ref.is_some(),
        "the deny evidence blob lands in the CAS and rides as a ref (I2)"
    );
}

/// CRITERION 2 — determinism: same params → same verdict AND same named refused
/// destination. A deny replays identically from the log (I6, DR-026 §Exit-demo
/// "that denial replays from the log — same recorded facts → same verdict").
#[test]
fn egress_deny_is_deterministic() {
    let (_dir, cas) = empty_cas();
    let params = json!({ "dest": "evil.example.com", "allow": allowlist() });
    let out = EgressScope
        .verify(&permit_input(params.clone()), &cas)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);
    let again = EgressScope
        .verify(&permit_input(params), &cas)
        .expect("engine ok");
    assert_eq!(
        out.evidence[0].msg, again.evidence[0].msg,
        "same params → same named refused destination (determinism BINDING, I6) — the \
         property that makes an egress denial replayable from the log"
    );
}

/// I6 honesty — the native NEVER coerces an undecidable input to a Pass: `allow`
/// ABSENT is cannot-run → Inconclusive (escalate to a human), never Pass. A
/// verifier that could be made to Pass on a missing allowlist is broken
/// (testing-oracles: verifier conformance). Mirrors `PathConfinement`'s
/// missing-binds arm and `SpendCap`'s missing-caps arm exactly.
///
/// RED: `verify` is `todo!()` → panic. Green once the cannot-run branch lands.
#[test]
fn missing_allowlist_is_inconclusive_not_pass() {
    let (_dir, cas) = empty_cas();
    let out = EgressScope
        .verify(&permit_input(json!({ "dest": "github.com" })), &cas)
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "allow ABSENT → cannot-run → Inconclusive (escalate), NEVER a synthesized egress \
         pass (I6); an absent allowlist is undecidable, not open"
    );
}

/// I6 honesty — an EMPTY allowlist (present-but-empty `allow: []`) allows
/// NOTHING: every destination is off the allowlist → any requested dest is a Fail
/// (deny), never a vacuous Pass. A present-empty allowlist is a total lockdown
/// (no egress), not an open gate — the deny-by-default posture DR-026 pins
/// (degrade CLOSED is the strictest degrade; an empty allowlist is its policy
/// analogue). Mirrors `PathConfinement`'s empty-binds arm.
#[test]
fn empty_allowlist_allows_nothing_and_denies() {
    let (_dir, cas) = empty_cas();
    let out = EgressScope
        .verify(
            &permit_input(json!({ "dest": "github.com", "allow": [] })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "an EMPTY allowlist allows nothing → every destination is off-list → Fail \
         (a present-empty allowlist is lockdown, never open — deny-by-default, DR-026)"
    );
}

/// I6 honesty / determinism — a destination that differs from an allowlisted host
/// only by a suffix (a classic bypass lever: `github.com.evil.com`) is NOT
/// allowlisted → Fail. The match is an EXACT host match, never a substring/prefix
/// match — a substring match would be a confinement HOLE (an egress version of
/// "a sandbox with a hole is worse than none", design §8.3). Pins the match
/// semantics the implementer must honor.
#[test]
fn suffix_lookalike_host_is_not_allowlisted() {
    let (_dir, cas) = empty_cas();
    for lookalike in ["github.com.evil.com", "notgithub.com", "github.como"] {
        let out = EgressScope
            .verify(
                &permit_input(json!({ "dest": lookalike, "allow": allowlist() })),
                &cas,
            )
            .expect("engine ok");
        assert_eq!(
            out.verdict,
            Verdict::Fail,
            "a look-alike host ({lookalike:?}) that is not an EXACT allowlist match is DENIED \
             — an exact host match, never substring/prefix (an egress confinement hole is worse \
             than none, design §8.3)"
        );
    }
}
