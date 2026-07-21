//! c3-egress-fold oracle (DR-029) — CRITERION 5 (HOST-provable, the EMITTER half):
//! the degrade/spawn + per-connection + credential facts ride the REAL minted
//! `egress.*`/`credential.*` subjects (warden-minted, committed `f2c7fc9`, in
//! `spec/ontology.md` + `crates/rezidnt-types/src/taxonomy.rs`), NOT the old
//! placeholder strings. Asserts:
//!   - the composed degrade facts ride `egress.mediated` / `egress.unavailable`
//!     (NOT `"sandbox.mediated"` / `"sandbox.unavailable"`);
//!   - a NEW `egress.denied` fact on the off-allowlist path;
//!   - a NEW `credential.dropped` fact on the unresolvable-secret path;
//!   - `credential.injected` carries `secret_ref` + `dest` + `policy_ref` and NEVER
//!     the value (the value literally cannot be found in the fact payload).
//!
//! The reducer FOLD of these five subjects onto `AgentRunState` is the companion
//! state suite (`crates/rezidnt-state/tests/egress_credential_fold.rs`).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure fact-shape inspection; no netns). The
//! subject-name + payload-shape property is a property of the emitters + the minted
//! taxonomy, so it needs no live injection. Runs on every host, Windows /vet
//! included.
//!
//! ## RED MODE — mixed.
//!   - `degrade_fact` EXISTS but returns PLACEHOLDER subjects (`"sandbox.mediated"`
//!     / `"sandbox.unavailable"`, `compose.rs:193,221`) — so the subject-swap arms
//!     are BEHAVIOR-RED until the implementer applies the DR-029 placeholder-swap map
//!     (ontology §"Placeholder-replacement map").
//!   - the `egress.denied` + `credential.dropped` emitters do NOT exist yet — so
//!     those arms are COMPILE-RED (the honest S4-skeleton signal) against the
//!     `denied_fact` / `dropped_fact` constructors the implementer must add beside
//!     `injected_fact` in `crates/rezidnt-run/src/egress.rs`.
//!   - `injected_fact` EXISTS; the value-absence scan holds GREEN today and STAYS the
//!     forcing function against a careless `.expose()` inline.

use rezidnt_run::compose::{ComposedDegrade, degrade_fact};
use rezidnt_run::egress::BrokeredSecret;
use rezidnt_types::refs::CasRef;

const RUN: &str = "01C3EGRESSFOLDFACTS00RN001";

/// CRITERION 5 (posture subjects) — the Mediated composed state rides
/// `egress.mediated` (NOT the `"sandbox.mediated"` placeholder). The DR-029 taxonomy
/// folds the sandbox posture into the `egress.*` fact as a FIELD; the enforcing
/// state's subject carries no `*.unavailable` marker.
///
/// BEHAVIOR-RED until `degrade_fact`'s Mediated arm returns the ratified
/// `egress.mediated` (ontology §"Placeholder-replacement map").
#[test]
fn mediated_degrade_rides_egress_mediated_not_the_placeholder() {
    let (subject, payload) = degrade_fact(&ComposedDegrade::Mediated, RUN);
    assert_eq!(
        subject, "egress.mediated",
        "CRITERION 5 VIOLATION: the Mediated composed state still rides the placeholder \
         {subject:?} — it must ride the ratified `egress.mediated` (DR-029 §Decision 6; ontology \
         line 494). The old `\"sandbox.mediated\"` string is retired"
    );
    assert_ne!(
        subject, "sandbox.mediated",
        "the retired placeholder `sandbox.mediated` must NOT survive the mint"
    );
    // The sandbox posture rode UP as a FIELD on the egress fact (the taxonomy call).
    assert_eq!(payload["network"].as_str(), Some("mediated"));
    assert_eq!(payload["sandbox"].as_str(), Some("available"));
    assert_eq!(payload["egress_enforceable"].as_bool(), Some(true));
}

/// CRITERION 5 (posture subjects) — BOTH degrade floors ride the ONE ratified
/// `egress.unavailable` subject, disambiguated by the `sandbox` field (DR-029
/// taxonomy judgment: one compose_degrade decision → one subject). The
/// ConfinedClosed floor is `sandbox="available"` (the egress backend was down); the
/// Unsandboxed floor is `sandbox="unavailable"` — and the retired
/// `"sandbox.unavailable"` placeholder must NOT survive.
///
/// BEHAVIOR-RED until the Unsandboxed arm rides `egress.unavailable` with the
/// `sandbox` discriminator (ontology line 503-506).
#[test]
fn both_degrade_floors_ride_egress_unavailable_disambiguated_by_sandbox_field() {
    let (closed_subject, closed) = degrade_fact(&ComposedDegrade::ConfinedClosed, RUN);
    assert_eq!(
        closed_subject, "egress.unavailable",
        "the confined+CLOSED floor rides `egress.unavailable` (name ratified, DR-029)"
    );
    assert_eq!(
        closed["sandbox"].as_str(),
        Some("available"),
        "the ConfinedClosed floor's discriminator is sandbox=available (the sandbox held)"
    );
    assert_eq!(closed["egress_enforceable"].as_bool(), Some(false));
    assert_eq!(closed["injected"].as_bool(), Some(false));

    let (unsbx_subject, unsbx) = degrade_fact(&ComposedDegrade::Unsandboxed, RUN);
    assert_eq!(
        unsbx_subject, "egress.unavailable",
        "CRITERION 5 VIOLATION: the Unsandboxed floor still rides the placeholder \
         {unsbx_subject:?} — DR-029 MERGES both floors onto `egress.unavailable`, disambiguated \
         by the `sandbox` field (ontology line 503). The old `\"sandbox.unavailable\"` is retired"
    );
    assert_eq!(
        unsbx["sandbox"].as_str(),
        Some("unavailable"),
        "the Unsandboxed floor's discriminator is sandbox=unavailable (no sealed netns)"
    );
    assert_eq!(
        unsbx["egress_enforceable"].as_bool(),
        Some(false),
        "the honesty anchor — no silent claim of mediation on the unsandboxed floor"
    );
    // The two floors are the SAME subject but distinguishable by `sandbox`.
    assert_eq!(closed_subject, unsbx_subject);
    assert_ne!(closed["sandbox"], unsbx["sandbox"]);
}

/// CRITERION 5 (`credential.injected` carries secret_ref + dest + policy_ref, NEVER
/// the value) — the by-reference injection fact rides `credential.injected` and its
/// payload carries the ref/dest/policy_ref; the value literally cannot be found in
/// the serialized fact. This re-pins the DR-026 crit-5 leak-discipline at the
/// ratified-subject level.
#[test]
fn credential_injected_carries_ref_dest_policy_never_the_value() {
    use rezidnt_run::egress::injected_fact;

    const SECRET_VALUE: &str = "ghp_injected_value_MUST_NEVER_RIDE_THE_FACT_0xC3FOLD";
    const SECRET_REF: &str = "gh_token";
    let policy_ref = CasRef {
        hash: "po11c3egressfold000000000000000000000000000000000000000000eg".to_string(),
        bytes: 128,
        mime: "application/octet-stream".to_string(),
    };
    let secret = BrokeredSecret::new(SECRET_REF, SECRET_VALUE);

    let (subject, payload) = injected_fact(RUN, "github.com", &secret, &policy_ref);
    assert_eq!(
        subject, "credential.injected",
        "the injection fact rides the ratified `credential.injected` subject (DR-029)"
    );
    assert_eq!(payload["secret_ref"].as_str(), Some(SECRET_REF));
    assert_eq!(payload["dest"].as_str(), Some("github.com"));
    assert_eq!(
        payload["policy_ref"]["hash"].as_str(),
        Some(policy_ref.hash.as_str())
    );

    // THE value literally cannot be found anywhere in the serialized fact.
    let serialized = serde_json::to_string(&payload).expect("payload serializes");
    assert!(
        !serialized.contains(SECRET_VALUE),
        "CRITERION 5 VIOLATION (CATASTROPHIC): the secret VALUE appeared in the \
         `credential.injected` payload — only secret_ref/dest/policy_ref may ride it, NEVER the \
         value (DR-026 crit 5, ontology line 520)"
    );
    assert!(
        serialized.contains(SECRET_REF),
        "non-vacuous: the secret_ref LABEL rides the fact (only the value is absent)"
    );
}

/// CRITERION 5 (`egress.denied` — the NEW off-allowlist emitter) — a per-connection
/// off-allowlist denial rides the ratified `egress.denied` subject, carrying `dest`
/// (the denied host) + `policy_ref` (the deciding allowlist, for `gate_explain`), so
/// *what was denied* and *why* are facts, not re-derivations (ontology line 513).
///
/// COMPILE-RED until the `denied_fact` constructor exists beside `injected_fact` in
/// `crates/rezidnt-run/src/egress.rs` — the `Mediation::Deny` verdict exists today,
/// the durable-fact emitter is this slice's build (ontology §"Placeholder-
/// replacement map", line 539).
#[test]
fn egress_denied_fact_names_the_off_allowlist_dest_and_policy() {
    // COMPILE-RED: `denied_fact` does not exist yet.
    use rezidnt_run::egress::denied_fact;

    let policy_ref = CasRef {
        hash: "po22c3egressfold000000000000000000000000000000000000000000eg".to_string(),
        bytes: 96,
        mime: "application/octet-stream".to_string(),
    };
    let (subject, payload) = denied_fact(RUN, "evil.example.com", &policy_ref);
    assert_eq!(
        subject, "egress.denied",
        "an off-allowlist denial rides the ratified `egress.denied` subject (DR-029, ontology 513)"
    );
    assert_eq!(payload["run"].as_str(), Some(RUN));
    assert_eq!(
        payload["dest"].as_str(),
        Some("evil.example.com"),
        "the denied (off-allowlist) destination is recorded so WHAT was denied is a fact"
    );
    assert_eq!(
        payload["policy_ref"]["hash"].as_str(),
        Some(policy_ref.hash.as_str()),
        "the deciding policy_ref rides so `gate_explain` can answer WHY denied (I6/I3)"
    );
}

/// CRITERION 5 (`credential.dropped` — the NEW unresolvable-secret emitter) — a
/// `secret_ref` the SecretSource could not resolve rides the ratified
/// `credential.dropped` subject, carrying `dest` + `secret_ref` (the unresolvable
/// label) + a loggable `reason`, and carries NO value (there is none — that is the
/// point, ontology line 527).
///
/// COMPILE-RED until the `dropped_fact` constructor exists beside `injected_fact` —
/// the drop path is this slice's build (ontology §"Placeholder-replacement map",
/// line 540).
#[test]
fn credential_dropped_fact_names_the_unresolvable_ref_and_carries_no_value() {
    // COMPILE-RED: `dropped_fact` does not exist yet.
    use rezidnt_run::egress::dropped_fact;

    let (subject, payload) = dropped_fact(
        RUN,
        "github.com",
        "gh_token",
        "secret_ref unresolvable by the configured SecretSource",
    );
    assert_eq!(
        subject, "credential.dropped",
        "an unresolvable secret_ref rides the ratified `credential.dropped` subject (DR-029)"
    );
    assert_eq!(payload["run"].as_str(), Some(RUN));
    assert_eq!(
        payload["dest"].as_str(),
        Some("github.com"),
        "the destination that LOST its injection is recorded (ontology line 529)"
    );
    assert_eq!(
        payload["secret_ref"].as_str(),
        Some("gh_token"),
        "the unresolvable LABEL is recorded — honest about the absence, never a fake token"
    );
    assert!(
        payload["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "the drop carries a loggable reason so WHY uninjected is interrogable (I6, ontology 531)"
    );
    // By construction the drop can carry no value — there was none to resolve.
    let serialized = serde_json::to_string(&payload).expect("payload serializes");
    assert!(
        !serialized.contains("<redacted>"),
        "a credential.dropped fact carries no BrokeredSecret at all — no value, no redaction \
         sentinel; only labels (ontology line 532)"
    );
}
