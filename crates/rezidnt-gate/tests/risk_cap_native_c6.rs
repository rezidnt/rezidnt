//! C6 oracle (DR-024 — running-risk cap) — the `RiskCap` native verifier and
//! the SHARED deterministic `risk_score` pure fn, the RISK analogue of C1's
//! `SpendCap` (`crates/rezidnt-gate/tests/permit_natives.rs`). This pins the
//! GATE-crate half of C6: the scorer's STRUCTURE + the native's soft/hard
//! verdict shape + the contract-free producer seam (Q5).
//!
//! RED MODE: **compile-red**. These reference `rezidnt_gate::RiskCap` (the C6
//! native) and `rezidnt_gate::risk_score` (the shared pure scorer) — neither
//! exists yet, so the crate fails to compile until the implementer adds both,
//! registers `RiskCap` in `builtin_natives()`, and extracts `risk_score`. Then
//! the assertions pin the three-valued behavior + the shared-fn guarantee.
//!
//! ## The STRUCTURE is the contract; the WEIGHTS are config (DR-024 "does NOT
//! decide"). Every test here passes a TEST scorer TABLE with KNOWN weights via
//! params — it never hardcodes production magic numbers as if they were the
//! contract. What is pinned: the score is per-tool base + path modifier + role
//! modifier, SUMMED; it is deterministic (same axis → same score); its evidence
//! NAMES each contributing factor (I6 interrogability); missing caps → cannot-run
//! (I6); and the scorer used for the VERDICT is the SAME fn the emit site stamps
//! (Q5 — asserted by feeding both call sites the identical axis and comparing).
//!
//! ## Params shape pinned by this file (mirrors SpendCap's C1 shape)
//!
//! The native reads its params AFTER the PDP has merged the request axis with the
//! folded per-run state — so `role` is ALREADY on the axis here, having been
//! folded from `agent.spawned.role` and injected by the PDP (DR-016, the AUTHORITY
//! path), never self-declared on the request:
//!
//! ```text
//! { "tool": str, "paths": [str], "role": str,      // the merged permit axis
//!   "cumulative_risk_score": f64,                  // PDP-injected folded state
//!   "soft_cap_risk": f64, "hard_cap_risk": f64,    // RiskCap's own caps
//!   "risk_table": { "base": {tool -> f64},         // the scorer TABLE (config)
//!                   "sensitive_paths": [glob], "path_modifier": f64,
//!                   "role_modifier": {role -> f64} } }
//! ```
//!
//! `cumulative_risk_score` is the PDP-injected folded accumulator (NEVER live
//! state, determinism BINDING); this-action's risk is COMPUTED inside the
//! verifier from `tool`/`paths`/`role` + `risk_table` (DR-024 Q4 — NOT injected).
//! At the UNIT level this file feeds those axis keys directly into `params`
//! (standing in for the PDP's merge); the LIVE folded-role path is exercised
//! end-to-end in `rezidnt-mcp/tests/permit_live_risk_cap_c6.rs`.

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{NativeVerifier, RiskCap, Verdict, VerifierInput, risk_score};
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

/// A TEST scorer table with KNOWN, easy-to-reason weights (NOT production
/// numbers — DR-024 leaves the real weights to tuning; this file pins STRUCTURE).
/// The three factors and their test weights:
///
/// - per-tool base: `Bash` 6.0, `Read` 1.0 (anything unlisted → 0.0)
/// - a target path matching a sensitive glob adds `path_modifier` 4.0 ONCE
/// - a role modifier: `admin` -2.0, `untrusted` +3.0 (unlisted role → 0.0)
///
/// With this table, an axis `{tool: Bash, paths: [secrets/key.pem], role: untrusted}`
/// scores 6.0 (base) + 4.0 (sensitive path) + 3.0 (role) = 13.0 — a value we can
/// hand-derive and assert exactly.
fn test_table() -> Value {
    json!({
        "base": { "Bash": 6.0, "Read": 1.0 },
        "sensitive_paths": ["secrets/**", "/etc/**"],
        "path_modifier": 4.0,
        "role_modifier": { "admin": -2.0, "untrusted": 3.0 }
    })
}

/// The request axis + caps + folded cumulative + the test table, assembled the
/// way the PDP assembles the merged params (request axis ∪ RiskCap's own spec
/// params). `cumulative` is the PDP-injected folded accumulator.
fn risk_params(
    tool: &str,
    paths: &[&str],
    role: &str,
    cumulative: f64,
    soft: f64,
    hard: f64,
) -> Value {
    json!({
        "tool": tool,
        "paths": paths,
        "role": role,
        "cumulative_risk_score": cumulative,
        "soft_cap_risk": soft,
        "hard_cap_risk": hard,
        "risk_table": test_table(),
    })
}

// --- the shared scorer: STRUCTURE + determinism (CRITERION 5, CRITERION 7) ----

/// CRITERION 5 (structure + determinism) — `risk_score` sums per-tool base +
/// path modifier + role modifier, and is a PURE fn of its content-pinned axis:
/// SAME axis → SAME score, TWICE. The exact expected value is HAND-DERIVED from
/// the test table (6.0 + 4.0 + 3.0 = 13.0), pinning that all three factors sum.
#[test]
fn risk_score_sums_base_path_and_role_factors_deterministically() {
    let axis = json!({
        "tool": "Bash",
        "paths": ["secrets/key.pem"],
        "role": "untrusted",
    });
    let table = test_table();
    let a = risk_score(&axis, &table);
    let b = risk_score(&axis, &table);
    assert_eq!(
        a, b,
        "same axis + same table → same score (determinism BINDING, I6)"
    );
    assert_eq!(
        a, 13.0,
        "base(Bash=6.0) + sensitive-path(+4.0) + role(untrusted=+3.0) = 13.0 — all three \
         factors SUM (DR-024 Q1 structure; weights are the TEST table, not production)"
    );
}

/// CRITERION 5 (each factor is isolable) — dropping the sensitive path drops
/// exactly the path modifier; a benign role zeroes its modifier; an unlisted
/// tool contributes zero base. This pins that the three factors are INDEPENDENT
/// summands, not an opaque blob (the honest, tunable heuristic DR-024 chose).
#[test]
fn risk_score_factors_are_independent_summands() {
    let table = test_table();
    // Benign path, admin role: base(Read=1.0) + path(0.0) + role(admin=-2.0) = -1.0.
    let benign = json!({"tool": "Read", "paths": ["src/app.rs"], "role": "admin"});
    assert_eq!(
        risk_score(&benign, &table),
        -1.0,
        "no sensitive path → no path modifier; admin role → -2.0; Read base 1.0 = -1.0"
    );
    // Unlisted tool, no paths, unlisted role → all three factors zero.
    let unknown = json!({"tool": "Glob", "paths": [], "role": "reviewer"});
    assert_eq!(
        risk_score(&unknown, &table),
        0.0,
        "an unlisted tool + no path + unlisted role sums to 0.0 (each factor isolable)"
    );
}

// --- the RiskCap native: projected-vs-caps behavior (CRITERION 1, 2, 4) -------

/// CRITERION 1 (native leg) — projected risk (`cumulative + this-action`) UNDER
/// the soft cap → Pass. cumulative 2.0 + this-action Read/benign/admin
/// (1.0 - 2.0 = -1.0) = 1.0 < soft 10.0 → allow.
#[test]
fn risk_cap_under_soft_passes() {
    let (_d, cas) = empty_cas();
    let out = RiskCap
        .verify(
            &permit_input(risk_params(
                "Read",
                &["src/app.rs"],
                "admin",
                2.0,
                10.0,
                20.0,
            )),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "projected risk under the soft cap is allowed (CRITERION 1)"
    );
}

/// CRITERION 2 (soft band) — soft ≤ projected < hard → **Inconclusive** (escalate
/// to a human, NEVER coerced, I6, DR-008 §4). cumulative 5.0 + this-action
/// Bash/sensitive/untrusted (13.0) = 18.0, in [soft 10.0, hard 30.0). The
/// evidence says WHY so the escalation is interrogable.
#[test]
fn risk_cap_soft_band_is_inconclusive_never_coerced() {
    let (_d, cas) = empty_cas();
    let out = RiskCap
        .verify(
            &permit_input(risk_params(
                "Bash",
                &["secrets/key.pem"],
                "untrusted",
                5.0,
                10.0,
                30.0,
            )),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "soft-cap crossing → escalate-to-a-human, NEVER a synthesized pass or auto-deny \
         (I6, DR-008 §4, CRITERION 2)"
    );
    assert_ne!(out.verdict, Verdict::Pass, "the load-bearing honesty guard");
    assert!(
        !out.evidence.is_empty(),
        "the escalation is interrogable — evidence says the soft cap was crossed (I6)"
    );
}

/// CRITERION 2 (hard cap) — projected AT or over the hard cap → Fail (deny).
/// cumulative 20.0 + this-action Bash/sensitive/untrusted (13.0) = 33.0 ≥ hard
/// 30.0 → deny.
#[test]
fn risk_cap_hard_cap_denies() {
    let (_d, cas) = empty_cas();
    let out = RiskCap
        .verify(
            &permit_input(risk_params(
                "Bash",
                &["secrets/key.pem"],
                "untrusted",
                20.0,
                10.0,
                30.0,
            )),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "projected risk at/over the hard cap is denied (CRITERION 2)"
    );
}

/// CRITERION 4 — params missing the caps cannot be decided → Inconclusive
/// (escalate/cannot-run), NEVER a synthesized pass (I6). A native that could be
/// made to emit `pass` on garbage is broken (testing-oracles). The axis + table
/// are present; ONLY the caps are absent.
#[test]
fn risk_cap_missing_caps_is_inconclusive_not_pass() {
    let (_d, cas) = empty_cas();
    let out = RiskCap
        .verify(
            &permit_input(json!({
                "tool": "Bash",
                "paths": ["secrets/key.pem"],
                "role": "untrusted",
                "cumulative_risk_score": 5.0,
                "risk_table": test_table(),
                // NO soft_cap_risk / hard_cap_risk — undecidable.
            })),
            &cas,
        )
        .expect("cannot-decide is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "no caps in params is undecidable → escalate, NEVER a synthesized pass (I6, CRITERION 4)"
    );
}

// --- CRITERION 3: the evidence NAMES each contributing factor -----------------

/// CRITERION 3 (interrogability) — the RiskCap verdict's evidence NAMES each
/// contributing factor (per-tool base, path modifier, role modifier) so
/// `gate_explain` can answer "why this risk" (I6, like IntentLock naming BOTH
/// the off-task tool AND the declared intent). Asserted on a soft-band crossing,
/// whose evidence is the interrogable artifact. The factor NAMES are pinned; the
/// exact prose is not (the implementer chooses the wording, the STRUCTURE is the
/// contract).
#[test]
fn risk_cap_evidence_names_each_contributing_factor() {
    let (_d, cas) = empty_cas();
    let out = RiskCap
        .verify(
            &permit_input(risk_params(
                "Bash",
                &["secrets/key.pem"],
                "untrusted",
                5.0,
                10.0,
                30.0,
            )),
            &cas,
        )
        .expect("engine ok");
    let ev = out
        .evidence
        .iter()
        .map(|e| e.msg.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    // The three factors that summed into the score must each be NAMED — the
    // requested tool, the sensitive path it touched, and the role — so the
    // breakdown is interrogable (I6). A bare total is NOT enough.
    assert!(
        ev.contains("Bash"),
        "the per-tool base factor names the tool (Bash); got {ev:?}"
    );
    assert!(
        ev.contains("secrets/key.pem") || ev.to_lowercase().contains("path"),
        "the path-sensitivity factor names the sensitive path (or the path factor); got {ev:?}"
    );
    assert!(
        ev.contains("untrusted") || ev.to_lowercase().contains("role"),
        "the role modifier factor names the role; got {ev:?}"
    );
}

// --- CRITERION 7: the shared-fn guarantee + contract-free seam ----------------

/// CRITERION 7 (shared-fn guarantee) — the score the native uses for its VERDICT
/// equals the score the emit site would stamp: BOTH call the SAME `risk_score`
/// pure fn on the SAME content-pinned axis, so they CANNOT diverge (DR-024 Q5
/// option iii). We prove the seam by construction: compute the delta the emit
/// site would stamp (`risk_score(axis, table)`) and show the native's verdict is
/// exactly the one `cumulative + that delta` vs the caps dictates — no second,
/// divergent scorer exists.
#[test]
fn verdict_and_stamped_delta_use_the_same_scorer() {
    let (_d, cas) = empty_cas();
    let axis = json!({"tool": "Bash", "paths": ["secrets/key.pem"], "role": "untrusted"});
    let table = test_table();
    // The delta the EMIT site stamps onto permit.granted (the shared fn).
    let stamped = risk_score(&axis, &table);
    assert_eq!(stamped, 13.0, "the stamped delta is the shared-fn score");

    // The NATIVE, given cumulative 5.0, must decide on projected 5.0 + stamped
    // = 18.0. With soft 10.0 / hard 30.0 that is the soft band → Inconclusive.
    // If the native used a DIFFERENT scorer, the boundary would move and this
    // verdict would flip — the shared fn is what keeps them locked.
    let out = RiskCap
        .verify(
            &permit_input(risk_params(
                "Bash",
                &["secrets/key.pem"],
                "untrusted",
                5.0,
                10.0,
                30.0,
            )),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "the native decided on projected = cumulative(5.0) + risk_score-delta(13.0) = 18.0, in \
         [soft 10.0, hard 30.0) — the SAME scalar the emit site stamps (Q5 shared fn, CRITERION 7)"
    );
}

/// CRITERION 7 (contract-free seam) — the §8 exec contract is UNCHANGED: neither
/// `VerifierOutput` nor `PermitOutcome` gains a risk field (DR-024 Q5 rejects
/// options i/ii). A native RiskCap never touches the exec STDOUT contract, so no
/// exec golden/replay churns. This is a COMPILE-TIME guard: `VerifierOutput` is
/// constructed with EXACTLY its three existing fields; a `risk_delta` field would
/// have to be added here too, which the DR forbids. If someone adds a risk field
/// to the struct, this exhaustive construction breaks — the seam stays closed.
#[test]
fn verifier_output_carries_no_risk_field_seam_is_contract_free() {
    // Exhaustive struct construction: if `VerifierOutput` grew a `risk_delta`
    // (option i — the §8 STDOUT contract change DR-024 rejects), this would fail
    // to compile for a missing field. The seam is contract-free by construction.
    let out = rezidnt_gate::VerifierOutput {
        verdict: Verdict::Pass,
        evidence: vec![],
        cost_ms: 0,
    };
    assert_eq!(out.verdict, Verdict::Pass);
    // Serialized shape carries no risk key either (no §8 wire churn).
    let wire = serde_json::to_value(&out).expect("VerifierOutput serializes");
    assert!(
        wire.get("risk_delta").is_none() && wire.get("risk").is_none(),
        "the §8 VerifierOutput wire shape carries NO risk field (DR-024 Q5: seam is contract-free)"
    );
}

/// CRITERION 7 (contract-free seam, PermitOutcome leg) — `PermitOutcome` gains NO
/// risk/delta field either (DR-024 Q5 rejects option ii, the aggregator
/// write-back). This exhaustively DESTRUCTURES `PermitOutcome`: if a risk field
/// were added, the `..`-free pattern would fail to compile with a missing-binding
/// error (well — a non-exhaustive struct pattern warns/errors), pinning the seam
/// closed. The shared `risk_score` fn carries the scalar, NOT a new struct field.
#[allow(dead_code)]
fn permit_outcome_has_no_risk_field(o: rezidnt_gate::permit::PermitOutcome) {
    // Bind EVERY field by name — no `..`. Adding a `risk_delta` field to
    // PermitOutcome (option ii) makes this pattern non-exhaustive → compile break.
    let rezidnt_gate::permit::PermitOutcome {
        verdict: _,
        decision: _,
        deciding_verifier: _,
        deciding_layer: _,
        deciding_params: _,
        evidence: _,
        deciding_evidence_ref: _,
        verifiers_run: _,
    } = o;
}

// --- registration -------------------------------------------------------------

/// CRITERION 1 (registration) — `RiskCap` is registered in `builtin_natives()`
/// beside `SpendCap` by its canonical name `risk-cap`, so the aggregator can
/// dispatch it on the live PDP. COMPILE/RUN-RED until the native exists AND is
/// added to `builtin_natives()`.
#[test]
fn risk_cap_native_is_registered_by_name() {
    let names: Vec<&'static str> = rezidnt_gate::builtin_natives()
        .iter()
        .map(|n| n.name())
        .collect();
    assert!(
        names.contains(&"risk-cap"),
        "builtin_natives() must register the C6 native \"risk-cap\" beside \"spend-cap\"; got {names:?}"
    );
}
