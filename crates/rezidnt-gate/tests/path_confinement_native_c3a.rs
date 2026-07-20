//! C3a oracle (DR-025 — the Linux OS-sandbox slice) — the `PathConfinement`
//! native permit-verifier: the deterministic "is this path INSIDE the sandbox
//! confinement?" decision, a `PathScope`/`ForbiddenPath` sibling. The OS sandbox
//! (`SandboxSubstrate`, in `rezidnt-run`) makes THIS verdict unbypassable; here
//! we pin the VERDICT itself (I6 — pure, three-valued, interrogable), which is
//! host-runnable and needs no `bwrap`.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (native, no bwrap, no #[cfg(unix)]).
//! This whole file is the pure confinement-LOGIC oracle: it runs on every host
//! that builds rezidnt (Windows host /vet included). The REAL bwrap confinement
//! behavior lives in `crates/rezidnt-run/tests/sandbox_bwrap_confinement_c3a.rs`
//! (`#[cfg(unix)]`, WSL-only, compiles to 0 tests on host —
//! [[vet-is-host-side-wsl-insufficient]]).
//!
//! RED MODE: **assert-red**. `rezidnt_gate::PathConfinement` EXISTS (a stub type)
//! but its `verify` is `todo!()`, so every test here PANICS until the implementer
//! writes the containment scan. A new test that passed before the impl exists
//! would be testing nothing (testing-oracles: test honesty).
//!
//! ## Params shape pinned by this file (mirrors the `PathScope` C1 shape)
//! The native reads its params AFTER the PDP merged the request axis with the
//! FOLDED confinement policy — so `binds` is ALREADY on the axis here, injected
//! from folded authority (DR-025 §Decision, the C6 lesson), never self-declared:
//!
//! ```text
//! { "paths": [str],                 // the action's touched paths (request axis)
//!   "binds": [ { "host_path": str, "writable": bool } ] }  // FOLDED confinement
//! ```
//!
//! At the UNIT level this file feeds `binds` directly into `params` (standing in
//! for the PDP's fold-and-inject); the LIVE folded-authority path (a request arg
//! that tries to WIDEN `binds` is ignored) is pinned in
//! `crates/rezidnt-run/tests/sandbox_no_widening_c3a.rs`.

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{NativeVerifier, PathConfinement, Verdict, VerifierInput};
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

/// A confinement policy axis with a writable worktree bind + a read-only
/// toolchain bind — the two binds a governed spawn folds (DR-025 §Decision).
fn worktree_binds() -> Value {
    json!([
        { "host_path": "/work/wt-abc", "writable": true },
        { "host_path": "/opt/toolchain", "writable": false }
    ])
}

/// CRITERION 1 — an agent action reading/writing INSIDE the allowed binds is
/// confined-OK: a path under the writable worktree bind → Pass (the sandbox
/// would permit it; the verdict says so). This is the "runs, and reads/writes
/// inside the allowed binds succeed" arm of criterion 1.
///
/// RED: `verify` is `todo!()` → panic. Green once the containment scan lands.
#[test]
fn path_inside_writable_bind_passes() {
    let (_dir, cas) = empty_cas();
    let out = PathConfinement
        .verify(
            &permit_input(json!({
                "paths": ["/work/wt-abc/src/main.rs"],
                "binds": worktree_binds(),
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "a path inside the writable worktree bind is confined-OK → Pass (CRITERION 1)"
    );
}

/// CRITERION 1 — a READ inside a read-only toolchain bind is also confined-OK.
/// The confinement decision is "is this path covered by a bind at all"; a
/// read-only bind covers reads. (The read/write SPLIT — a write to a read-only
/// bind — is a mechanism concern the bwrap suite pins; the native's pass here is
/// that the path is inside confinement.)
#[test]
fn path_inside_readonly_bind_passes() {
    let (_dir, cas) = empty_cas();
    let out = PathConfinement
        .verify(
            &permit_input(json!({
                "paths": ["/opt/toolchain/bin/rustc"],
                "binds": worktree_binds(),
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "a path inside a read-only bind is inside confinement → Pass (CRITERION 1)"
    );
}

/// CRITERION 2 — a path OUTSIDE every bind is DENIED, and the evidence NAMES the
/// out-of-bounds path (interrogable, I6). This is the "a read/write outside the
/// binds is DENIED and logged — not a silent success and not a crash" arm: the
/// native returns a Fail VERDICT (which the PDP logs as `permit.denied`), never
/// a Pass and never an `Err`/panic.
///
/// RED: `verify` is `todo!()` → panic. Green once the containment scan lands.
#[test]
fn path_outside_binds_denies_and_evidence_names_it() {
    let (_dir, cas) = empty_cas();
    let out = PathConfinement
        .verify(
            &permit_input(json!({
                "paths": ["/etc/shadow"],
                "binds": worktree_binds(),
            })),
            &cas,
        )
        .expect("engine ok — a can't-confine is a VERDICT, never an Err (I6)");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "a path outside every bind is DENIED → Fail (CRITERION 2)"
    );
    assert!(!out.evidence.is_empty(), "a deny carries evidence (I6)");
    assert!(
        out.evidence[0].msg.contains("/etc/shadow"),
        "the deny evidence NAMES the out-of-bounds path so the denial is interrogable \
         (I6, CRITERION 2); got {:?}",
        out.evidence[0].msg
    );
    // I2: the evidence blob rides as a CAS ref, never inline bytes (the
    // ForbiddenPath/DiffScope house pattern).
    assert!(
        out.evidence[0].cas_ref.is_some(),
        "the deny evidence blob lands in the CAS and rides as a ref (I2)"
    );
}

/// CRITERION 2 — the FIRST out-of-bounds path in list order is the named
/// offender (deterministic evidence, I6): same params → same verdict AND same
/// named offender. Two out-of-bounds paths; the scan names the first.
#[test]
fn path_confinement_names_first_offender_deterministically() {
    let (_dir, cas) = empty_cas();
    let params = json!({
        "paths": ["/work/wt-abc/ok.rs", "/root/.ssh/id_rsa", "/etc/passwd"],
        "binds": worktree_binds(),
    });
    let out = PathConfinement
        .verify(&permit_input(params.clone()), &cas)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);
    assert!(
        out.evidence[0].msg.contains("/root/.ssh/id_rsa"),
        "the FIRST out-of-bounds path in list order is the named offender \
         (deterministic, I6); got {:?}",
        out.evidence[0].msg
    );
    // Determinism: a second identical run yields the same named offender.
    let again = PathConfinement
        .verify(&permit_input(params), &cas)
        .expect("engine ok");
    assert_eq!(
        out.evidence[0].msg, again.evidence[0].msg,
        "same params → same named offender (determinism BINDING, I6)"
    );
}

/// I6 honesty — the native NEVER coerces an undecidable input to a Pass: `binds`
/// ABSENT is cannot-run → Inconclusive (escalate to a human), never Pass. A
/// verifier that could be made to Pass on missing confinement is broken
/// (testing-oracles: verifier conformance). Mirrors `SpendCap`'s missing-caps
/// arm exactly.
///
/// RED: `verify` is `todo!()` → panic. Green once the cannot-run branch lands.
#[test]
fn missing_binds_is_inconclusive_not_pass() {
    let (_dir, cas) = empty_cas();
    let out = PathConfinement
        .verify(
            &permit_input(json!({ "paths": ["/work/wt-abc/src/main.rs"] })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "binds ABSENT → cannot-run → Inconclusive (escalate), NEVER a synthesized \
         confinement pass (I6); an empty/absent policy is undecidable, not open"
    );
}

/// I6 honesty — an EMPTY bind set (present-but-empty `binds: []`) confines
/// NOTHING: every path is out of bounds → any requested path is a Fail (deny),
/// never a vacuous Pass. A present-empty policy is a total lockdown, not an open
/// gate — the "a sandbox with a hole is worse than none" property (design §8.3).
#[test]
fn empty_binds_confines_nothing_and_denies() {
    let (_dir, cas) = empty_cas();
    let out = PathConfinement
        .verify(
            &permit_input(json!({
                "paths": ["/work/wt-abc/src/main.rs"],
                "binds": [],
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "an EMPTY bind set confines nothing → every path is out of bounds → Fail \
         (a present-empty policy is lockdown, never open — design §8.3)"
    );
}
