//! SP1 oracle — the native permit-verifier pack (design
//! `docs/design/permit-engine.md` §6; DR-009 C1). The `permit` gate's native
//! kind: deterministic Rust policy checks that decide an agent ACTION (not a
//! diff) before it runs. Three natives land in SP1:
//!
//!   - `tool-allowlist` — the action's `tool` must be in `params.allow`
//!     (allow → Pass; not-allowed → Fail); the harness `--allowedTools` axis
//!     moved mid-run (design §2: the permit gate fills the enforcement middle).
//!   - `path-scope`  — the action's target path(s) must be inside
//!     `params.allow` globs (reuses the two-star matcher the diff natives use);
//!     out-of-scope → Fail, named in evidence (I6).
//!   - `spend-cap` (DR-009 C1) — the running per-session spend (a PINNED input,
//!     never live state) vs. a soft/hard cap + a rate limit: under soft → Pass;
//!     soft ≤ spend < hard → **Inconclusive** (escalate, NEVER coerced, I6);
//!     spend ≥ hard → Fail (deny); rate exceeded → Fail.
//!
//! RED MODE: **compile-red** — these reference `ToolAllowlist`, `PathScope`,
//! and `SpendCap` (the SP1 natives) which do not exist yet, so the crate fails
//! to compile until the implementer adds them and registers them in
//! `builtin_natives()`. Then the assertions pin the three-valued behavior.
//!
//! ## Determinism BINDING — the C1 accumulator-input shape (pinned here)
//! A native that read MUTABLE live accumulator state would break the
//! content-hash-pinned-inputs BINDING (doc §8; same rule as `refs["diff"]`).
//! So the running per-session totals arrive as a PINNED INPUT: the spend-cap
//! native reads the current spend from `params` (the §8 stdin `params`, which
//! is recorded VERBATIM in the `inputs` document on the verdict fact and thus
//! content-hash-pinned and replayable). Params shape pinned by this file:
//!   `{ "cumulative_spend_usd": f64,   // the pinned accumulator snapshot
//!      "action_cost_usd": f64,        // this action's incremental charge
//!      "soft_cap_usd": f64, "hard_cap_usd": f64,
//!      "window_action_count": u64, "rate_limit": u64 }`
//! Same params ⇒ same verdict, every time (I6). The producer of the snapshot is
//! the daemon folding `PermitAccumulators` from the log (I3) and passing it in;
//! the native never touches mutable state.

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{NativeVerifier, PathScope, SpendCap, ToolAllowlist, Verdict, VerifierInput};
use serde_json::{Value, json};

/// A permit-gate `VerifierInput` with `params` (no CAS blob needed for the
/// tool/spend natives — the descriptor is inline, per ontology
/// `permit.requested.target` line 320). A fresh empty CAS is passed since these
/// natives decide from params + (for path-scope) a pinned target-path list.
fn permit_input(refs: BTreeMap<String, String>, params: Value) -> VerifierInput {
    VerifierInput {
        gate: "permit".to_string(),
        workspace: None,
        refs,
        params,
        timeout_ms: 120_000,
    }
}

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

// --- tool-allowlist ---------------------------------------------------------

/// The requested tool is on the allowlist → Pass (allow).
#[test]
fn tool_allowlist_allows_listed_tool() {
    let (_d, cas) = empty_cas();
    let out = ToolAllowlist
        .verify(
            &permit_input(
                BTreeMap::new(),
                json!({"tool": "Read", "allow": ["Read", "Edit"]}),
            ),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass, "a listed tool is allowed");
}

/// A tool NOT on the allowlist → Fail (deny), and the evidence names the
/// offending tool so the blocked agent can read WHY (I6).
#[test]
fn tool_allowlist_denies_unlisted_tool_with_evidence() {
    let (_d, cas) = empty_cas();
    let out = ToolAllowlist
        .verify(
            &permit_input(
                BTreeMap::new(),
                json!({"tool": "Bash", "allow": ["Read", "Edit"]}),
            ),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail, "an unlisted tool is denied");
    assert!(
        !out.evidence.is_empty() && out.evidence[0].msg.contains("Bash"),
        "the denial names the offending tool (I6); got {:?}",
        out.evidence
    );
}

/// The native never SYNTHESIZES a pass: a request with NO `tool` field and no
/// allowlist cannot be decided → Inconclusive (escalate), never coerced to
/// Pass (I6). This is the honesty guard on the allowlist native.
#[test]
fn tool_allowlist_undecidable_is_inconclusive_not_pass() {
    let (_d, cas) = empty_cas();
    let out = ToolAllowlist
        .verify(&permit_input(BTreeMap::new(), json!({})), &cas)
        .expect("cannot-decide is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "no tool + no allowlist is undecidable → escalate, NEVER a synthesized pass (I6)"
    );
}

// --- path-scope -------------------------------------------------------------

/// A target path inside the allowed glob scope → Pass.
#[test]
fn path_scope_allows_in_scope_path() {
    let (_d, cas) = empty_cas();
    let out = PathScope
        .verify(
            &permit_input(
                BTreeMap::new(),
                json!({"paths": ["src/checkout/cart.rs"], "allow": ["src/checkout/**"]}),
            ),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass);
}

/// An out-of-scope target path → Fail, evidence names the path (I6).
#[test]
fn path_scope_denies_out_of_scope_path_with_evidence() {
    let (_d, cas) = empty_cas();
    let out = PathScope
        .verify(
            &permit_input(
                BTreeMap::new(),
                json!({"paths": ["src/checkout/cart.rs", "/etc/passwd"], "allow": ["src/checkout/**"]}),
            ),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);
    assert!(
        out.evidence[0].msg.contains("/etc/passwd"),
        "the denial names the out-of-scope path (I6); got {:?}",
        out.evidence
    );
}

/// I6 determinism: same params ⇒ same verdict AND same evidence (refs
/// included). The permit natives are pinned to the same determinism BINDING as
/// the diff natives.
#[test]
fn path_scope_is_deterministic() {
    let (_d, cas) = empty_cas();
    let input = permit_input(
        BTreeMap::new(),
        json!({"paths": ["/etc/passwd"], "allow": ["src/**"]}),
    );
    let a = PathScope.verify(&input, &cas).expect("engine ok");
    let b = PathScope.verify(&input, &cas).expect("engine ok");
    assert_eq!(a.verdict, b.verdict);
    assert_eq!(a.evidence, b.evidence, "evidence is deterministic");
}

// --- spend-cap (DR-009 C1) --------------------------------------------------
//
// The load-bearing honesty native: the soft-cap → Inconclusive/escalate path
// is the whole reason C1 folds into SP1. A soft-cap crossing is NOT a deny and
// NOT an allow — it is an ask-a-human, and the mapping (`permit::decision_for`)
// turns that Inconclusive into `permit.escalated`, never coerced (I6).

fn spend_params(cumulative: f64, cost: f64, soft: f64, hard: f64) -> Value {
    json!({
        "cumulative_spend_usd": cumulative,
        "action_cost_usd": cost,
        "soft_cap_usd": soft,
        "hard_cap_usd": hard,
        "window_action_count": 0u64,
        "rate_limit": 1000u64,
    })
}

/// Projected spend well UNDER the soft cap → Pass (allow).
#[test]
fn spend_cap_under_soft_cap_passes() {
    let (_d, cas) = empty_cas();
    let out = SpendCap
        .verify(
            &permit_input(BTreeMap::new(), spend_params(1.0, 0.5, 10.0, 20.0)),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "spend under the soft cap is allowed"
    );
}

/// Projected spend in the SOFT band (soft ≤ spend < hard) → **Inconclusive**.
/// This is the C1 honesty test: a soft-cap crossing escalates to a human, it is
/// NEVER coerced to a pass and NEVER auto-denied (I6, DR-008 §4). The evidence
/// says why so the escalation is interrogable.
#[test]
fn spend_cap_soft_band_is_inconclusive_never_coerced() {
    let (_d, cas) = empty_cas();
    // cumulative 9.5 + cost 1.0 = 10.5, which is ≥ soft(10) and < hard(20).
    let out = SpendCap
        .verify(
            &permit_input(BTreeMap::new(), spend_params(9.5, 1.0, 10.0, 20.0)),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "soft-cap crossing → escalate-to-a-human, NEVER a synthesized pass or auto-deny (I6, DR-008 §4)"
    );
    assert_ne!(out.verdict, Verdict::Pass, "the load-bearing honesty guard");
    assert!(
        !out.evidence.is_empty(),
        "the escalation is interrogable — evidence says the soft cap was crossed (I6)"
    );
}

/// Projected spend AT or over the HARD cap → Fail (deny).
#[test]
fn spend_cap_hard_cap_denies() {
    let (_d, cas) = empty_cas();
    // cumulative 19.5 + cost 1.0 = 20.5 ≥ hard(20).
    let out = SpendCap
        .verify(
            &permit_input(BTreeMap::new(), spend_params(19.5, 1.0, 10.0, 20.0)),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "spend at/over the hard cap is denied"
    );
}

/// Rate limit: the per-window action count at/over the limit → Fail (deny),
/// independent of spend (spend is comfortably under both caps here).
#[test]
fn spend_cap_rate_limit_denies() {
    let (_d, cas) = empty_cas();
    let params = json!({
        "cumulative_spend_usd": 0.0,
        "action_cost_usd": 0.0,
        "soft_cap_usd": 10.0,
        "hard_cap_usd": 20.0,
        "window_action_count": 100u64,
        "rate_limit": 100u64,
    });
    let out = SpendCap
        .verify(&permit_input(BTreeMap::new(), params), &cas)
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "at/over the rate limit is denied regardless of spend headroom"
    );
}

/// The spend-cap native NEVER coerces garbage to a pass: params missing the
/// caps cannot be decided → Inconclusive (escalate), never Pass (I6). A native
/// that could be made to emit `pass` on garbage is broken (testing-oracles).
#[test]
fn spend_cap_missing_caps_is_inconclusive_not_pass() {
    let (_d, cas) = empty_cas();
    let out = SpendCap
        .verify(
            &permit_input(BTreeMap::new(), json!({"cumulative_spend_usd": 5.0})),
            &cas,
        )
        .expect("cannot-decide is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "no caps in params is undecidable → escalate, NEVER a synthesized pass (I6)"
    );
}

// --- registration -----------------------------------------------------------

/// The three SP1 permit natives are registered in `builtin_natives()` by their
/// canonical names — the same registry the engine dispatches on and `replay`
/// re-executes against. Names are the ontology/design spellings
/// (`tool-allowlist`, `path-scope`, `spend-cap`).
///
/// COMPILE-RED until the natives exist AND are added to `builtin_natives()`.
#[test]
fn sp1_permit_natives_are_registered_by_name() {
    let names: Vec<&'static str> = rezidnt_gate::builtin_natives()
        .iter()
        .map(|n| n.name())
        .collect();
    for expected in ["tool-allowlist", "path-scope", "spend-cap"] {
        assert!(
            names.contains(&expected),
            "builtin_natives() must register the SP1 permit native {expected:?}; got {names:?}"
        );
    }
}
