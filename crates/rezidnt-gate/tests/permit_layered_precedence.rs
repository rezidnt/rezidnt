//! SP4c oracle — C8 layered policy precedence (DR-019, ACCEPTED 2026-07-20).
//!
//! DR-019 ratifies composition (a): three policy layers — **admin → dev →
//! session** — compose by CONCATENATING their `[gates.permit]` verifier specs
//! into the existing flat `Vec<PermitVerifierSpec>`, in that fixed order.
//! Stricter-wins is INHERITED from the existing monotone aggregate
//! (`aggregate_async`: first-`Fail`→Deny short-circuit, any-`Inconclusive`→
//! Escalate, else Grant) — because the aggregate has NO allow-override
//! primitive, a later layer can never un-Fail an earlier layer's deny. Each
//! `PermitVerifierSpec` must carry LAYER PROVENANCE so the emitted decision /
//! `gate_explain` names the deciding LAYER (I6), not just the verifier.
//!
//! This file pins the composition + provenance at the `rezidnt-gate` layer (the
//! deterministic-judge seam): it uses the `tool-allowlist` native as a
//! fixed-verdict lever — a tool NOT in `allow` → Fail, a tool IN `allow` → Pass
//! — so every aggregate outcome is deterministic and no exec subprocess or MCP
//! wiring is involved. The MCP-live three-source merge proof (admin from
//! daemon/host config, dev from `workspace.spec.applied`, session from the run)
//! lives in `crates/rezidnt-mcp/tests/permit_layered_live.rs`.
//!
//! RED MODE: **compile-red** first. Every test references an API SP4c must add
//! and that does not exist yet:
//!   - `permit::PermitLayer { Admin, Dev, Session }` (the provenance carrier),
//!     with `PermitLayer::as_str()` → "admin"/"dev"/"session";
//!   - a per-layer spec constructor `PermitVerifierSpec::native_in_layer(layer,
//!     name, params)` (and, for symmetry, `exec_in_layer`) stamping provenance;
//!   - `PermitVerifierSpec::layer() -> PermitLayer` accessor;
//!   - the three-layer merge entrypoint `permit::compose_layers(admin, dev,
//!     session) -> Vec<PermitVerifierSpec>` that concatenates in admin→dev→
//!     session order and preserves each spec's provenance;
//!   - `PermitOutcome::deciding_layer -> Option<PermitLayer>` — the layer of the
//!     deciding verifier (`None` only for the empty-set escalate, where there is
//!     no deciding verifier).
//!
//! With those in place, the assertions below pin the four DR-019 acceptance
//! criteria.
//!
//! IMPLEMENTER NOTE (minimal target API — keep it small, DR-019 §"What this does
//! NOT decide" leaves the carrier a field-vs-tag detail; a `layer` field on
//! `PermitVerifierSpec` + `deciding_layer` on `PermitOutcome` is the minimal
//! shape these tests assume). The aggregate + verdict→decision table stay
//! UNCHANGED (DR-019 Decision 1) — `compose_layers` only builds the ordered set
//! that the EXISTING `aggregate`/`aggregate_async` already consume. Existing
//! `PermitVerifierSpec::native`/`exec` may default their layer to `Session`
//! (the least-authority layer) so no existing test regresses.
//!
//! ── The LIVE three-source end-to-end (SHIPPED in SP4c-wire / DR-020) ────────
//! DR-019 §"What this does NOT decide" left the daemon-side WIRING of the
//! three sources (admin from host config, dev from `workspace.spec.applied`,
//! session from the run/agent, merged in `permit_config_for`) to a future seam
//! `/dr`. That `/dr` is now DR-020 (ACCEPTED), and the wiring is BUILT:
//! `McpCore::with_layered_permit_config` injects three resolved layers,
//! `permit_config_for` sources admin (host `REZIDNT_ADMIN_PERMIT`, outside the
//! workspace spec) + dev (`workspace.spec.applied`) + session, and the emit path
//! pins `deciding_layer` in the policy blob. SP4c's RATIFIED core is the
//! composition + provenance pinned in THIS file (`compose_layers` / `PermitLayer`
//! / `deciding_layer`), which is transport-agnostic and lives at the
//! `rezidnt-gate` seam. The MCP-live proof (admin deny non-overridable THROUGH
//! `request_permission`; the decision fact / `gate_explain` naming the deciding
//! layer) now lives in `crates/rezidnt-mcp/tests/permit_layered_live.rs`, and the
//! daemon-side authority-boundary proof (admin sourced OUTSIDE the workspace spec)
//! in `bins/rezidentd/tests/permit_admin_layer_sourcing.rs`. This file pins the
//! ratified composition those live tests build on.

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::permit::{self, PermitDecision, PermitLayer, PermitVerifierSpec};
use rezidnt_gate::{Verdict, VerifierInput};
use serde_json::{Value, json};

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

/// A permit `VerifierInput`: the request's `tool` plus the per-verifier params
/// ride `params`; there is no CAS blob (the descriptor is inline). Mirrors
/// `permit_aggregate.rs::permit_input`.
fn permit_input(params: Value) -> VerifierInput {
    VerifierInput {
        gate: permit::LIFECYCLE_POINT.to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params,
        timeout_ms: rezidnt_gate::DEFAULT_TIMEOUT_MS,
    }
}

/// A native permit entry STAMPED with its source layer (the SP4c dispatch unit).
/// `tool-allowlist` is the fixed-verdict lever: `allow` decides Pass vs Fail
/// deterministically, so the aggregate outcome is a pure function of the merged
/// order — the deterministic-judge seam (testing-oracles).
fn native_in(layer: PermitLayer, name: &str, params: Value) -> PermitVerifierSpec {
    PermitVerifierSpec::native_in_layer(layer, name, params)
}

/// A layer that DENIES the request tool: `tool-allowlist` with an `allow` set
/// that excludes it → Fail. (Used to build an admin-layer deny.)
fn deny_layer(layer: PermitLayer) -> Vec<PermitVerifierSpec> {
    vec![native_in(
        layer,
        "tool-allowlist",
        json!({ "allow": ["Read"] }), // "Bash"/"Edit" excluded → Fail
    )]
}

/// A layer that GRANTS the request tool: `tool-allowlist` whose `allow` includes
/// it → Pass. (Used to build a session-layer "would allow".)
fn grant_layer(layer: PermitLayer) -> Vec<PermitVerifierSpec> {
    vec![native_in(
        layer,
        "tool-allowlist",
        json!({ "allow": ["Read", "Edit", "Bash"] }),
    )]
}

// ---------------------------------------------------------------------------
// CRITERION 1 — Admin deny is NON-OVERRIDABLE by a session allow.
// ---------------------------------------------------------------------------

/// CRITERION 1 (HEADLINE) — the admin layer contributes a verifier that FAILS,
/// the session layer would GRANT; the COMPOSED aggregate verdict is DENY. The
/// session allow cannot un-Fail the admin deny because the aggregate has no
/// allow-override primitive and admin's spec runs FIRST in the concat order
/// (DR-019 Decision 1). This is C8 stricter-wins.
///
/// COMPILE-RED until `compose_layers` / `PermitLayer` exist.
#[test]
fn admin_deny_is_not_overridable_by_session_allow() {
    let (_dir, cas) = empty_cas();
    // The request: tool "Bash" — admin denies it, session would allow it.
    let input = permit_input(json!({ "tool": "Bash" }));

    let admin = deny_layer(PermitLayer::Admin); // Bash NOT allowed → Fail
    let dev: Vec<PermitVerifierSpec> = vec![]; // empty dev layer
    let session = grant_layer(PermitLayer::Session); // Bash allowed → would Pass

    let set = permit::compose_layers(admin, dev, session);
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the admin-layer deny is NON-OVERRIDABLE by a later session allow — the \
         composed verdict is Deny (C8 stricter-wins, DR-019 criterion 1). \
         outcome={outcome:?}"
    );
    assert_eq!(
        outcome.verdict,
        Verdict::Fail,
        "the aggregate three-valued verdict is Fail (admin's first Fail \
         short-circuits before session runs)"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 2 — Layer provenance surfaced (the deciding LAYER is named).
// ---------------------------------------------------------------------------

/// CRITERION 2 — the outcome names the DECIDING LAYER, not merely the verifier.
/// The admin deny above must surface `deciding_layer == Admin` so `gate_explain`
/// / the decision fact answers "why blocked" with *"admin layer"* (I6
/// interrogability, DR-019 Decision 3). Two layers can hold the SAME verifier
/// name (`tool-allowlist`), so the verifier name ALONE cannot disambiguate which
/// authority denied — the layer provenance is what makes an admin deny
/// *auditably* non-overridable.
///
/// COMPILE-RED (no `deciding_layer` / `PermitLayer`) then ASSERT on provenance.
#[test]
fn deciding_layer_provenance_is_surfaced_admin() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Bash" }));

    // BOTH admin and session carry a `tool-allowlist` — the NAME is ambiguous;
    // only the layer distinguishes which authority decided.
    let admin = deny_layer(PermitLayer::Admin); // denies Bash → decides
    let dev: Vec<PermitVerifierSpec> = vec![];
    let session = grant_layer(PermitLayer::Session);

    let set = permit::compose_layers(admin, dev, session);
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the admin layer decides Deny"
    );
    assert_eq!(
        outcome.deciding_verifier, "tool-allowlist",
        "the deciding verifier's NAME is recorded (unchanged behavior)"
    );
    assert_eq!(
        outcome.deciding_layer,
        Some(PermitLayer::Admin),
        "the DECIDING LAYER is surfaced as `admin` — `gate_explain` answers 'why \
         blocked' with the layer, not merely the verifier name (I6, DR-019 \
         criterion 2). outcome={outcome:?}"
    );
    assert_eq!(
        PermitLayer::Admin.as_str(),
        "admin",
        "the layer renders as the stable string the decision fact carries"
    );
}

/// CRITERION 2 (companion) — the deciding layer is the ESCALATING verifier's
/// layer when a dev-layer verifier escalates (Inconclusive). Proves provenance
/// tracks the actual deciding verifier across layers, not a fixed guess. Here
/// admin passes, dev escalates (unknown native → honest can't-run Inconclusive),
/// session would pass — the outcome is Escalate carrying `deciding_layer == Dev`.
///
/// COMPILE-RED then ASSERT on provenance.
#[test]
fn deciding_layer_provenance_tracks_escalating_dev_layer() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" })); // allowlisted everywhere

    let admin = grant_layer(PermitLayer::Admin); // Read allowed → Pass
    // an UNKNOWN native name → honest can't-run → Inconclusive (never a pass, I6).
    let dev = vec![native_in(
        PermitLayer::Dev,
        "no-such-native-verifier",
        json!({}),
    )];
    let session = grant_layer(PermitLayer::Session); // Read allowed → Pass

    let set = permit::compose_layers(admin, dev, session);
    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "no Fail but a dev-layer Inconclusive → Escalate (never coerced, I6)"
    );
    assert_eq!(
        outcome.deciding_layer,
        Some(PermitLayer::Dev),
        "the deciding layer is the DEV layer that escalated — provenance tracks \
         the real deciding verifier across layers (DR-019 criterion 2). \
         outcome={outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 3 — Concat order verified: admin → dev → session; a later layer
// cannot un-Fail an earlier-layer Fail (ordering + monotonicity → stricter-wins).
// ---------------------------------------------------------------------------

/// CRITERION 3 — `compose_layers` merges in EXACTLY admin → dev → session order,
/// stamping each spec's provenance. Asserted structurally: the merged vector's
/// layers appear in that fixed order, and swapping which layer denies changes
/// which layer decides. This pins that ordering is not incidental.
///
/// COMPILE-RED until `compose_layers` / `layer()` / `PermitLayer` exist.
#[test]
fn compose_layers_concatenates_admin_then_dev_then_session_in_order() {
    let admin = vec![native_in(PermitLayer::Admin, "tool-allowlist", json!({}))];
    let dev = vec![native_in(PermitLayer::Dev, "tool-allowlist", json!({}))];
    let session = vec![native_in(PermitLayer::Session, "tool-allowlist", json!({}))];

    let set = permit::compose_layers(admin, dev, session);

    let layers: Vec<PermitLayer> = set.iter().map(|s| s.layer()).collect();
    assert_eq!(
        layers,
        vec![PermitLayer::Admin, PermitLayer::Dev, PermitLayer::Session],
        "layers concatenate in the fixed admin→dev→session order, provenance \
         preserved per spec (DR-019 criterion 3)"
    );
}

/// CRITERION 3 (the monotonicity leg) — a later-layer verifier CANNOT un-Fail an
/// earlier-layer Fail. Because admin's Fail short-circuits BEFORE dev/session
/// run, and the aggregate has no allow-override primitive, adding permissive
/// later layers leaves the verdict Deny. Proven by composing the admin deny with
/// increasingly permissive dev + session layers and asserting the verdict stays
/// Deny and the deciding layer stays Admin.
///
/// COMPILE-RED then ASSERT.
#[test]
fn later_layer_cannot_un_fail_an_earlier_layer_fail() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Bash" }));

    let admin = deny_layer(PermitLayer::Admin); // Bash denied at admin

    // Every combination of permissive-or-empty dev/session must NOT rescue it.
    let dev_options: Vec<Vec<PermitVerifierSpec>> = vec![vec![], grant_layer(PermitLayer::Dev)];
    let session_options: Vec<Vec<PermitVerifierSpec>> =
        vec![vec![], grant_layer(PermitLayer::Session)];

    for dev in &dev_options {
        for session in &session_options {
            let set = permit::compose_layers(admin.clone(), dev.clone(), session.clone());
            let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
            assert_eq!(
                outcome.decision,
                PermitDecision::Deny,
                "no later layer un-Fails the admin deny — monotone aggregate \
                 (no allow-override) makes admin non-overridable (DR-019 \
                 criterion 3). dev_empty={} session_empty={}",
                dev.is_empty(),
                session.is_empty()
            );
            assert_eq!(
                outcome.deciding_layer,
                Some(PermitLayer::Admin),
                "the admin layer stays the deciding authority regardless of \
                 later layers"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// CRITERION 4 — Empty / absent layer degrades to escalate, never allow.
// ---------------------------------------------------------------------------

/// CRITERION 4 (all-empty) — a three-layer resolution where ALL layers are empty
/// composes to an EMPTY set, which the aggregate ESCALATES (undecidable → route
/// to a human), NEVER a synthesized allow (DR-011 §3 discipline preserved per
/// layer, DR-019 criterion 4). No layer's absence manufactures a permission.
///
/// COMPILE-RED until `compose_layers` exists.
#[test]
fn all_empty_layers_escalate_never_allow() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" }));

    let set = permit::compose_layers(vec![], vec![], vec![]);
    assert!(
        set.is_empty(),
        "three empty layers concat to the empty set (zero verifiers)"
    );

    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_ne!(
        outcome.decision,
        PermitDecision::Grant,
        "an all-empty three-layer resolution NEVER synthesizes an allow (I6, \
         DR-019 criterion 4)"
    );
    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "all-empty → undecidable → escalate to a human (DR-011 §3 per layer)"
    );
    assert_eq!(
        outcome.deciding_layer, None,
        "an empty set has NO deciding verifier and therefore no deciding layer"
    );
}

/// CRITERION 4 (present-but-empty layer) — a PRESENT but empty layer contributes
/// ZERO verifiers and never manufactures a Grant. Here admin and dev are empty
/// and only the session layer has a single passing verifier: the outcome is a
/// Grant DECIDED BY THE SESSION layer — the empty admin/dev layers add nothing,
/// they neither block nor synthesize. This pins that "empty layer == zero
/// verifiers", not "empty layer == allow".
///
/// COMPILE-RED then ASSERT.
#[test]
fn present_but_empty_layers_contribute_zero_verifiers() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" })); // session allowlists Read

    let admin: Vec<PermitVerifierSpec> = vec![]; // present but empty
    let dev: Vec<PermitVerifierSpec> = vec![]; // present but empty
    let session = grant_layer(PermitLayer::Session); // one passing verifier

    let set = permit::compose_layers(admin, dev, session);
    assert_eq!(
        set.len(),
        1,
        "the two empty layers contribute zero verifiers — only session's one \
         verifier is in the merged set (DR-019 criterion 4)"
    );

    let outcome = permit::aggregate(&set, &input, &cas).expect("aggregate runs");
    assert_eq!(
        outcome.decision,
        PermitDecision::Grant,
        "the single session verifier passes → Grant; the empty admin/dev layers \
         neither blocked nor manufactured the allow"
    );
    assert_eq!(
        outcome.deciding_layer,
        Some(PermitLayer::Session),
        "the grant is decided by the session layer (the only layer with a \
         verifier), and provenance names it"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 3 (property) — monotonicity: composing an ADDITIONAL later layer can
// only make the verdict STRICTER or EQUAL, never more permissive.
// ---------------------------------------------------------------------------

/// The strictness rank of a decision: Grant (most permissive) < Escalate < Deny
/// (strictest). C8's whole guarantee is that adding a layer can only move the
/// verdict UP this ladder or leave it, never DOWN toward Grant (DR-019 Decision
/// 1: the aggregate has no allow-override primitive).
fn strictness(decision: PermitDecision) -> u8 {
    match decision {
        PermitDecision::Grant => 0,
        PermitDecision::Escalate => 1,
        PermitDecision::Deny => 2,
    }
}

proptest::proptest! {
    /// CRITERION 3 (HEADLINE property) — for an arbitrary base admin+dev
    /// composition and an arbitrary session layer, adding the session layer to
    /// the tail NEVER makes the composed verdict MORE PERMISSIVE than the base.
    /// `strictness(with_session) >= strictness(base)`. This is stricter-wins
    /// stated as a monotonicity law: a session layer can tighten (add a Fail or
    /// an Inconclusive) or leave the verdict, but it can never un-Fail an
    /// earlier layer's deny or downgrade an escalate to a grant.
    ///
    /// The layers are drawn from a small deterministic alphabet of
    /// `tool-allowlist` specs (allow-Bash → Pass, deny-Bash → Fail) plus an
    /// unknown-native spec (→ Inconclusive), so every aggregate outcome is a
    /// pure function of the merged order — no exec, no I/O, fully replayable.
    ///
    /// COMPILE-RED until `compose_layers` / `PermitLayer` exist.
    #[test]
    fn adding_a_session_layer_is_monotone_never_more_permissive(
        admin_kinds in proptest::collection::vec(0u8..3, 0..3),
        dev_kinds in proptest::collection::vec(0u8..3, 0..3),
        session_kinds in proptest::collection::vec(0u8..3, 0..3),
    ) {
        let (_dir, cas) = empty_cas();
        // Request tool "Bash": kind 0 allows it (Pass), kind 1 denies it (Fail),
        // kind 2 is an unknown native (Inconclusive).
        let input = permit_input(json!({ "tool": "Bash" }));

        let spec = |layer: PermitLayer, kind: u8| -> PermitVerifierSpec {
            match kind {
                0 => native_in(layer, "tool-allowlist", json!({ "allow": ["Bash"] })),
                1 => native_in(layer, "tool-allowlist", json!({ "allow": ["Read"] })),
                _ => native_in(layer, "no-such-native-verifier", json!({})),
            }
        };
        let build = |layer: PermitLayer, kinds: &[u8]| -> Vec<PermitVerifierSpec> {
            kinds.iter().map(|k| spec(layer, *k)).collect()
        };

        let admin = build(PermitLayer::Admin, &admin_kinds);
        let dev = build(PermitLayer::Dev, &dev_kinds);
        let session = build(PermitLayer::Session, &session_kinds);

        // Base = admin + dev, no session (session slot empty).
        let base_set = permit::compose_layers(admin.clone(), dev.clone(), vec![]);

        // PRECONDITION: the monotonicity law is over a RESOLVED base. The EMPTY
        // base is a ratified EXCEPTION, not a violation — an empty configured set
        // is honest-undecidable → Escalate (DR-019 Decision 1: aggregate
        // unchanged; `permit_aggregate::empty_configured_set_escalates_never_synthesizes_allow`),
        // and adding a passing session verifier legitimately RESOLVES that
        // Escalate to a Grant. On the strictness ladder that reads as "more
        // permissive", yet it is exactly the ratified "present-but-empty admin/dev
        // contribute zero verifiers, session decides" behavior this file ALSO
        // pins in `present_but_empty_layers_contribute_zero_verifiers`. The
        // property's real claim is "adding a session layer to a RESOLVED admin+dev
        // base only tightens", so its domain excludes the empty base. This is a
        // precondition CORRECTION (the empty base is out of the property's
        // domain), NOT a weakening — no assertion is relaxed for any resolved
        // base, and the empty-base→Escalate rule is pinned by its own test above.
        proptest::prop_assume!(!base_set.is_empty());

        let base = permit::aggregate(&base_set, &input, &cas).expect("aggregate base");

        // With the session layer appended.
        let full_set = permit::compose_layers(admin, dev, session);
        let full = permit::aggregate(&full_set, &input, &cas).expect("aggregate full");

        proptest::prop_assert!(
            strictness(full.decision) >= strictness(base.decision),
            "adding the session layer made the verdict MORE PERMISSIVE — C8 \
             monotonicity violated. base={:?} with_session={:?}",
            base.decision,
            full.decision,
        );
    }
}
