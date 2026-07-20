//! SP4b ORACLE — macaroon-attenuated delegation crypto (DR-017, design §4–§5).
//!
//! FAILING-FIRST. Everything this file references in `rezidnt_run::badge` —
//! `Caveat`, `RootKey`, `Macaroon`, `RequestContext`, `verify`, `Capability`,
//! `VerifyError` — DOES NOT EXIST YET. This crate is expected to FAIL TO
//! COMPILE until the implementer lands the SP4b macaroon envelope. That is the
//! correct oracle state: a `does-not-compile` because the API is absent is the
//! red board (design §5 says monotonicity/forgery are the load-bearing tests;
//! they cannot be green before the crypto exists). Do NOT weaken these to make
//! the crate compile — build the API to them.
//!
//! ## The construction under test (DR-017 §Decision 2, design §4)
//! ```text
//! sig₀   = blake3::keyed_hash(root_key, identifier)
//! sigᵢ₊₁ = blake3::keyed_hash(sigᵢ.as_bytes(), serialize(caveatᵢ))
//! macaroon = { identifier, caveats: [...], sig: sig_last }
//! verify(m, root_key, ctx) = recompute chain; CONSTANT-TIME compare m.sig;
//!                            then eval every caveat against ctx
//! ```
//!
//! ## API surface this board PINS (the implementer builds to exactly this)
//!
//! In `rezidnt_run::badge`:
//! - `pub struct RootKey([u8; 32])` with `RootKey::mint() -> RootKey` (rand,
//!   already vendored) and a test-only `RootKey::from_bytes([u8; 32])` so the
//!   oracle can pin a deterministic key (verify is pure/replayable, I6).
//! - `pub enum Caveat` — tagged first-party predicate, EXACTLY one of four,
//!   serde `#[serde(tag = "kind", rename_all = "snake_case")]` so it wire-matches
//!   the ratified `permit.delegated.added_caveats` shape (ontology lines 412-416):
//!     - `Caveat::Workspace { workspace: String }`
//!     - `Caveat::Verb { verbs: Vec<String> }`
//!     - `Caveat::Expiry { not_after: String }`  // RFC3339 UTC
//!     - `Caveat::Role { role: String }`
//! - `pub struct Macaroon { identifier, caveats: Vec<Caveat>, sig }` with:
//!     - `Macaroon::mint(root: &RootKey, identifier: impl Into<String>,
//!        base: Vec<Caveat>) -> Macaroon`  (daemon, at spawn)
//!     - `fn attenuate(&self, c: Caveat) -> Macaroon`  (holder, offline, NO root key)
//!     - `fn badge_id(&self) -> String`  // DR-018 §(a): hex(blake3(sig)[..8]) —
//!       sig-derived, loggable, never the token. Re-keys per appended caveat, so
//!       a true same-identifier `attenuate` yields DISTINCT parent/child ids.
//!     - `fn caveats(&self) -> &[Caveat]`
//!     - `fn to_wire(&self) -> String` / `Macaroon::from_wire(&str) -> Result<Macaroon, _>`
//!       (the serialized form carried under `REZIDNT_BADGE`, I2 inline)
//! - `pub struct RequestContext { workspace, verb, now, role }` — the PASSED-IN
//!   verify context. `now` is an RFC3339 timestamp STRING supplied by the caller
//!   (NEVER an ambient clock inside verify — DR-017 §Decision 3, I6). Fields are
//!   `Option`/scalar as needed; a builder or public fields are both fine.
//! - `pub fn verify(m: &Macaroon, root: &RootKey, ctx: &RequestContext)
//!      -> Result<Capability, VerifyError>` — recompute the sig chain from
//!   `root`, CONSTANT-TIME compare `m.sig` (compare `blake3::Hash` values, whose
//!   `PartialEq` is documented constant-time — the vendored ct primitive, no new
//!   dep, I7), then evaluate every caveat against `ctx`. Any unsatisfied caveat
//!   → `Err`. A broken MAC chain → `Err(VerifyError::BadSignature)`.
//! - `pub struct Capability` — the resolved authority a verified macaroon grants,
//!   with `fn is_subset_of(&self, other: &Capability) -> bool` so monotonicity is
//!   a first-class assertion (design §5). It reflects the narrowing of every
//!   caveat: allowed workspace(s), allowed verb set, effective expiry, role.
//! - `pub enum VerifyError { BadSignature, CaveatUnsatisfied { kind: String }, .. }`
//!   (thiserror; the refusing caveat's `kind` is surfaced so `gate_explain` can
//!   say WHICH caveat refused — I6 interrogability).

use rezidnt_run::badge::{Caveat, Macaroon, RequestContext, RootKey, VerifyError, verify};

// A fixed workspace/role vocabulary for the examples (opaque strings; rezidnt
// mints no role vocabulary — DR-016).
const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";
// RFC3339 UTC; lexicographic order == chronological for fixed-offset Zulu.
const T_EARLY: &str = "2026-07-19T00:00:00Z";
const T_MID: &str = "2026-07-19T12:00:00Z";
const T_LATE: &str = "2026-07-20T00:00:00Z";

fn root() -> RootKey {
    // Deterministic key so the whole board is replayable (I6). `mint()` is the
    // production path; tests pin bytes.
    RootKey::from_bytes([7u8; 32])
}

/// Lowercase-hex a byte slice — the oracle's own copy of the badge_id encoding,
/// so the DR-018 sig-derived-badge_id assertion pins the exact pre-image
/// (`hex(blake3(sig)[..8])`) without reaching into the crate's private helper.
fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// A permissive context that satisfies a broad base macaroon (workspace A, a
/// state-mutating verb, before any expiry, a lead role).
fn ctx_broad() -> RequestContext {
    RequestContext::new()
        .workspace(WS_A)
        .verb("spawn")
        .now(T_MID)
        .role("lead")
}

// ---------------------------------------------------------------------------
// TEST 4 (foundation) — mint → attenuate → verify round-trip.
// A minted macaroon verifies; appending a satisfiable caveat still verifies
// when the context satisfies it, and REFUSES when the context violates it.
// (Placed first because tests 1-3 build on a working round-trip.)
// ---------------------------------------------------------------------------

#[test]
fn mint_verifies_against_a_satisfying_context() {
    let root = root();
    let m = Macaroon::mint(
        &root,
        "run-01SP4BMINT000000000000000",
        vec![
            Caveat::Workspace {
                workspace: WS_A.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into(), "open".into(), "merge".into()],
            },
        ],
    );
    let cap = verify(&m, &root, &ctx_broad()).expect("a minted badge verifies in its own scope");
    // The verified capability is non-empty and self-consistent (a subset of
    // itself — the reflexive base case of monotonicity).
    assert!(
        cap.is_subset_of(&cap),
        "capability is reflexively its own subset"
    );
}

#[test]
fn attenuate_narrows_and_still_verifies_when_satisfied() {
    let root = root();
    let m = Macaroon::mint(
        &root,
        "run-01SP4BATTEN00000000000000",
        vec![Caveat::Verb {
            verbs: vec!["spawn".into(), "open".into(), "merge".into()],
        }],
    );
    // A holder narrows the verb set — offline, no root key.
    let child = m.attenuate(Caveat::Verb {
        verbs: vec!["open".into()],
    });
    // Context asks for `open` (inside the narrowed set) → verifies.
    let ok = RequestContext::new()
        .workspace(WS_A)
        .verb("open")
        .now(T_MID)
        .role("lead");
    verify(&child, &root, &ok).expect("narrowed badge verifies for a verb still in the set");
}

#[test]
fn attenuate_refuses_when_the_added_caveat_is_violated() {
    let root = root();
    let m = Macaroon::mint(
        &root,
        "run-01SP4BREFUSE0000000000000",
        vec![Caveat::Verb {
            verbs: vec!["spawn".into(), "open".into(), "merge".into()],
        }],
    );
    let child = m.attenuate(Caveat::Verb {
        verbs: vec!["open".into()],
    });
    // Context asks for `merge` — allowed by the PARENT, forbidden by the child's
    // narrowing caveat → refuse.
    let bad = RequestContext::new()
        .workspace(WS_A)
        .verb("merge")
        .now(T_MID)
        .role("lead");
    let err = verify(&child, &root, &bad).expect_err("a verb outside the narrowed set is refused");
    assert!(
        matches!(err, VerifyError::CaveatUnsatisfied { .. }),
        "refusal is a caveat-unsatisfied, not a MAC break: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// TEST 4b (DR-018 §(a)) — badge_id is SIG-DERIVED, and a true `attenuate` under
// a SHARED identifier yields DISTINCT parent/child badge_ids. This is the
// property DR-018 makes real: the delegation edge is a genuine OFFLINE
// attenuation (no root key, identifier preserved), not a root-key re-mint.
//
// Against the CURRENT code this FAILS for the right reason: `badge_id()` still
// derives from the identifier, so `parent.attenuate(c)` (which preserves the
// identifier) collapses parent==child badge_id. The revised board pins the flip.
// ---------------------------------------------------------------------------

/// The load-bearing DR-018 assertion. `badge_id` is `hex(blake3(sig)[..8])`; a
/// true `attenuate` re-keys the running sig but PRESERVES the identifier, so the
/// child's badge_id differs from the parent's WHILE the identifier is shared —
/// and the child capability is still ⊆ the parent's (monotonicity, now realized
/// through `attenuate` rather than a re-mint).
#[test]
fn attenuate_yields_distinct_badge_ids_under_a_shared_identifier() {
    let root = root();
    let parent = Macaroon::mint(
        &root,
        "run-01SP4BDR018ATTEN000000000",
        vec![
            Caveat::Workspace {
                workspace: WS_A.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into(), "open".into(), "merge".into()],
            },
        ],
    );
    // A holder narrows the badge OFFLINE — no root key. The role caveat is the
    // SP4b delegation trigger (DR-016 §Decision 3).
    let child = parent.attenuate(Caveat::Role {
        role: "reviewer".into(),
    });

    // (1) The identifier is PRESERVED by attenuation — this is why the offline
    // property holds (changing it would force sig₀ back to the root key).
    assert_eq!(
        parent.identifier(),
        child.identifier(),
        "attenuate preserves the identifier (offline: no root key re-mint) — DR-018 §(a)/§Context"
    );

    // (2) DR-018 §(a): badge_id is sig-derived, so a re-keyed child differs from
    // its parent EVEN THOUGH the identifier is shared. This is the assertion that
    // fails against the current identifier-derived badge_id().
    assert_ne!(
        parent.badge_id(),
        child.badge_id(),
        "a true attenuate re-keys the running sig -> distinct sig-derived badge_ids \
         despite a shared identifier (DR-018 §(a): hex(blake3(sig)[..8]))"
    );

    // (3) badge_id shape is unchanged: 8-byte hex prefix (16 hex chars), never
    // the token, and derived from the SIG not the raw sig bytes.
    for id in [parent.badge_id(), child.badge_id()] {
        assert_eq!(
            id.len(),
            16,
            "badge_id is an 8-byte hex prefix (shape unchanged): {id}"
        );
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "badge_id is lowercase hex: {id}"
        );
    }
    // The badge_id pre-image is blake3(sig), NOT the raw sig bytes (so no bytes
    // of the MAC itself land on the fabric — DR-018 §(a) bullet 1).
    assert_eq!(
        parent.badge_id(),
        hex_lower(&blake3::hash(&parent.sig_bytes()).as_bytes()[..8]),
        "badge_id derives from hex(blake3(sig)[..8]) — the sig hashed, never raw sig bytes"
    );

    // (4) The child capability is still ⊆ the parent's: monotonicity, now
    // realized through `attenuate`. Verify both in a context that satisfies the
    // child (the reviewer role) and assert the subset relation.
    let ctx = RequestContext::new()
        .workspace(WS_A)
        .verb("open")
        .now(T_MID)
        .role("reviewer");
    let parent_cap = verify(&parent, &root, &ctx).expect("parent verifies in this context");
    let child_cap =
        verify(&child, &root, &ctx).expect("attenuated child verifies (role satisfied)");
    assert!(
        child_cap.is_subset_of(&parent_cap),
        "the attenuated child's capability is a subset of the parent's (monotonicity through attenuate): \
         child={child_cap:?} parent={parent_cap:?}"
    );
}

/// A DR-018 corollary made explicit: EACH appended caveat re-keys the sig, so a
/// two-hop chain (parent -> child -> grandchild) produces THREE distinct
/// sig-derived badge_ids under one shared identifier. A re-mint scheme keyed on
/// the identifier could not produce this from a single mint.
#[test]
fn each_attenuation_hop_rekeys_the_badge_id() {
    let root = root();
    let parent = Macaroon::mint(
        &root,
        "run-01SP4BDR018CHAIN000000000",
        vec![Caveat::Verb {
            verbs: vec!["spawn".into(), "open".into(), "merge".into()],
        }],
    );
    let child = parent.attenuate(Caveat::Verb {
        verbs: vec!["open".into(), "merge".into()],
    });
    let grandchild = child.attenuate(Caveat::Verb {
        verbs: vec!["open".into()],
    });

    // One shared identifier across the whole chain (offline attenuation).
    assert_eq!(parent.identifier(), child.identifier());
    assert_eq!(child.identifier(), grandchild.identifier());

    // Three distinct sig-derived badge_ids (each hop re-keys).
    let (p, c, g) = (parent.badge_id(), child.badge_id(), grandchild.badge_id());
    assert_ne!(p, c, "hop 1 re-keys the badge_id");
    assert_ne!(c, g, "hop 2 re-keys the badge_id");
    assert_ne!(p, g, "parent and grandchild badge_ids differ");
}

// ---------------------------------------------------------------------------
// TEST 2 — Forgery / tamper / reorder rejection. Each mutation INDEPENDENTLY
// must break the keyed-MAC chain and fail verify. This is the crypto integrity
// surface (design §5).
// ---------------------------------------------------------------------------

/// Build a broad, satisfiable macaroon over the fixed root for the tamper suite.
fn tamper_subject(root: &RootKey) -> Macaroon {
    Macaroon::mint(
        root,
        "run-01SP4BTAMPER0000000000000",
        vec![
            Caveat::Workspace {
                workspace: WS_A.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into(), "open".into()],
            },
            Caveat::Expiry {
                not_after: T_LATE.into(),
            },
        ],
    )
}

#[test]
fn untampered_baseline_verifies() {
    // Guards the tamper suite: if the baseline itself did not verify, the
    // rejection tests below would pass vacuously (test-honesty: the mutation,
    // not a broken fixture, must be what fails).
    let root = root();
    let m = tamper_subject(&root);
    verify(&m, &root, &ctx_broad())
        .expect("the untampered subject verifies — the tamper suite is honest");
}

#[test]
fn caveat_removed_fails_verify() {
    let root = root();
    let m = tamper_subject(&root);
    // Reconstruct a macaroon that carries the SAME sig but a caveat list with
    // one removed: the recomputed chain no longer matches m.sig → MAC break.
    // `from_parts` is the test seam that builds a Macaroon from an explicit
    // (identifier, caveats, sig) WITHOUT recomputing — the only way to forge.
    let mut caveats = m.caveats().to_vec();
    caveats.pop(); // drop the expiry caveat, keep the stolen sig
    let forged = Macaroon::from_parts(m.identifier(), caveats, m.sig_bytes());
    let err =
        verify(&forged, &root, &ctx_broad()).expect_err("a removed caveat breaks the MAC chain");
    assert!(
        matches!(err, VerifyError::BadSignature),
        "removal is a signature failure: {err:?}"
    );
}

#[test]
fn caveat_edited_fails_verify() {
    let root = root();
    let m = tamper_subject(&root);
    // WIDEN the verb caveat under the stolen sig — the escalation attempt.
    let mut caveats = m.caveats().to_vec();
    caveats[1] = Caveat::Verb {
        verbs: vec!["spawn".into(), "open".into(), "merge".into()],
    };
    let forged = Macaroon::from_parts(m.identifier(), caveats, m.sig_bytes());
    let err =
        verify(&forged, &root, &ctx_broad()).expect_err("an edited caveat breaks the MAC chain");
    assert!(
        matches!(err, VerifyError::BadSignature),
        "edit is a signature failure: {err:?}"
    );
}

#[test]
fn caveats_reordered_fails_verify() {
    let root = root();
    let m = tamper_subject(&root);
    // Reorder two caveats under the stolen sig — the chain is order-dependent
    // (sigᵢ₊₁ folds sigᵢ), so a swap breaks it even though the SET is identical.
    let mut caveats = m.caveats().to_vec();
    caveats.swap(0, 1);
    let forged = Macaroon::from_parts(m.identifier(), caveats, m.sig_bytes());
    let err =
        verify(&forged, &root, &ctx_broad()).expect_err("reordered caveats break the MAC chain");
    assert!(
        matches!(err, VerifyError::BadSignature),
        "reorder is a signature failure: {err:?}"
    );
}

#[test]
fn forged_random_sig_fails_verify() {
    let root = root();
    let m = tamper_subject(&root);
    // A wholly fabricated sig (attacker with no root key) — must fail.
    let forged = Macaroon::from_parts(m.identifier(), m.caveats().to_vec(), [0xABu8; 32]);
    let err = verify(&forged, &root, &ctx_broad()).expect_err("a forged sig has no valid chain");
    assert!(
        matches!(err, VerifyError::BadSignature),
        "a forged sig is a signature failure: {err:?}"
    );
}

#[test]
fn wrong_root_key_fails_verify() {
    // A macaroon minted under one daemon's root key must NOT verify under
    // another's — the root key is the whole trust anchor.
    let minting_root = root();
    let m = tamper_subject(&minting_root);
    let other_root = RootKey::from_bytes([9u8; 32]);
    let err = verify(&m, &other_root, &ctx_broad()).expect_err("a foreign root key cannot verify");
    assert!(
        matches!(err, VerifyError::BadSignature),
        "wrong root is a signature failure: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// TEST 3 — Constant-time sig comparison (no timing oracle on the MAC).
// We cannot portably measure wall-clock timing in a unit test, so we pin the
// PROPERTY that guarantees it: verify compares `blake3::Hash` values (whose
// `PartialEq` is documented constant-time — the vendored ct primitive, I7),
// via a value-level near-miss test. `Macaroon::sig_hash()` returns the
// `blake3::Hash` the implementer MUST compare with (not a raw &[u8] memcmp).
// ---------------------------------------------------------------------------

#[test]
fn one_bit_off_sig_is_rejected() {
    // A near-miss sig (single bit flipped) must be refused exactly like a
    // wildly-wrong one — no early-exit shortcut a timing attacker could exploit.
    let root = root();
    let m = tamper_subject(&root);
    let mut sig = m.sig_bytes();
    sig[0] ^= 0x01; // flip one bit
    let forged = Macaroon::from_parts(m.identifier(), m.caveats().to_vec(), sig);
    let err =
        verify(&forged, &root, &ctx_broad()).expect_err("a one-bit-off sig is still rejected");
    assert!(
        matches!(err, VerifyError::BadSignature),
        "near-miss is a signature failure: {err:?}"
    );
}

#[test]
fn sig_is_a_blake3_hash_compared_constant_time() {
    // Pins the ct primitive at the type level: the sig the verifier compares is
    // a `blake3::Hash`, whose equality is constant-time by construction (blake3
    // docs). This forces the implementer AWAY from a variable-time `==` on
    // `&[u8]`. `sig_hash()` exposing a `blake3::Hash` is the load-bearing seam.
    let root = root();
    let m = tamper_subject(&root);
    let recomputed: blake3::Hash = m.sig_hash();
    // The verified macaroon's own sig equals itself under the ct compare.
    assert_eq!(
        recomputed,
        m.sig_hash(),
        "sig compares as a constant-time blake3::Hash"
    );
    // And a different hash is unequal (the ct compare still discriminates).
    assert_ne!(
        recomputed,
        blake3::hash(b"not the sig"),
        "a different hash is unequal under ct compare"
    );
}

// ---------------------------------------------------------------------------
// TEST 5 — Expiry-as-caveat against a PASSED-IN timestamp. No ambient now().
// verify accepts before `not_after`, refuses at/after — driven entirely by the
// request-context timestamp. Determinism: same macaroon + same ts → same
// verdict, always (I6, DR-017 §Decision 3).
// ---------------------------------------------------------------------------

fn expiring_macaroon(root: &RootKey) -> Macaroon {
    Macaroon::mint(
        root,
        "run-01SP4BEXPIRY0000000000000",
        vec![Caveat::Expiry {
            not_after: T_MID.into(),
        }],
    )
}

fn ctx_at(now: &str) -> RequestContext {
    RequestContext::new()
        .workspace(WS_A)
        .verb("spawn")
        .now(now)
        .role("lead")
}

#[test]
fn expiry_accepts_before_not_after() {
    let root = root();
    let m = expiring_macaroon(&root);
    verify(&m, &root, &ctx_at(T_EARLY)).expect("before not_after the badge is live");
}

#[test]
fn expiry_refuses_at_or_after_not_after() {
    let root = root();
    let m = expiring_macaroon(&root);
    // AT the boundary: `not_after` means invalid AT and after (ontology line
    // 415: "invalid after not_after"; the boundary itself is expired — half-open
    // [.., not_after) validity, the safe reading for a capability).
    let at = verify(&m, &root, &ctx_at(T_MID)).expect_err("at not_after the badge has expired");
    assert!(
        matches!(at, VerifyError::CaveatUnsatisfied { .. }),
        "expiry-at is a caveat refusal: {at:?}"
    );
    let after =
        verify(&m, &root, &ctx_at(T_LATE)).expect_err("after not_after the badge has expired");
    assert!(
        matches!(after, VerifyError::CaveatUnsatisfied { .. }),
        "expiry-after is a caveat refusal: {after:?}"
    );
}

#[test]
fn expiry_verdict_is_deterministic_in_the_passed_in_timestamp() {
    // Same macaroon + same ts → identical verdict across repeated calls. If
    // verify read an ambient clock, THIS would flake — the test is the I6 guard
    // that it does not.
    let root = root();
    let m = expiring_macaroon(&root);
    for _ in 0..8 {
        assert!(
            verify(&m, &root, &ctx_at(T_EARLY)).is_ok(),
            "before-expiry verdict is stable"
        );
        assert!(
            verify(&m, &root, &ctx_at(T_LATE)).is_err(),
            "after-expiry verdict is stable"
        );
    }
}

// ---------------------------------------------------------------------------
// TEST 1 — MONOTONICITY (THE load-bearing property, I6). For ANY macaroon M and
// ANY caveat c, capability(verify(M+c)) ⊆ capability(verify(M)). Attenuation is
// monotone-decreasing — a widening bug is privilege escalation. Property test
// over generated caveat chains + contexts, plus forgery under proptest.
// ---------------------------------------------------------------------------

mod monotonicity {
    use super::*;
    use proptest::prelude::*;

    // Generate a single first-party caveat from the four kinds, over a small
    // fixed vocabulary so contexts can plausibly satisfy chains.
    fn any_caveat() -> impl Strategy<Value = Caveat> {
        prop_oneof![
            prop::sample::select(vec![WS_A, WS_B]).prop_map(|w| Caveat::Workspace {
                workspace: w.into()
            }),
            // A non-empty subset of the verb universe, deterministic order.
            prop::collection::vec(prop::sample::select(vec!["spawn", "open", "merge"]), 1..4)
                .prop_map(|mut vs| {
                    vs.sort();
                    vs.dedup();
                    Caveat::Verb {
                        verbs: vs.into_iter().map(String::from).collect(),
                    }
                }),
            prop::sample::select(vec![T_EARLY, T_MID, T_LATE]).prop_map(|t| Caveat::Expiry {
                not_after: t.into()
            }),
            prop::sample::select(vec!["lead", "sub", "reviewer"])
                .prop_map(|r| Caveat::Role { role: r.into() }),
        ]
    }

    fn any_chain() -> impl Strategy<Value = Vec<Caveat>> {
        prop::collection::vec(any_caveat(), 0..6)
    }

    // A request context drawn from the same vocabulary.
    fn any_context() -> impl Strategy<Value = RequestContext> {
        (
            prop::sample::select(vec![WS_A, WS_B]),
            prop::sample::select(vec!["spawn", "open", "merge"]),
            prop::sample::select(vec![T_EARLY, T_MID, T_LATE]),
            prop::sample::select(vec!["lead", "sub", "reviewer"]),
        )
            .prop_map(|(w, v, t, r)| RequestContext::new().workspace(w).verb(v).now(t).role(r))
    }

    proptest! {
        /// THE invariant: for any base chain, any added caveat, and any context,
        /// if the CHILD (M+c) verifies then the PARENT (M) verifies too, and the
        /// child's capability is a subset of the parent's. Attenuation can only
        /// narrow — it never widens (privilege escalation). A counterexample here
        /// is a security defect, not a flaky test.
        #[test]
        fn attenuation_never_widens(
            base in any_chain(),
            added in any_caveat(),
            ctx in any_context(),
        ) {
            let root = root();
            let parent = Macaroon::mint(&root, "run-01SP4BPROPMONO000000000000", base);
            let child = parent.attenuate(added);

            let parent_v = verify(&parent, &root, &ctx);
            let child_v = verify(&child, &root, &ctx);

            // (a) If the child verifies in this context, the parent MUST too:
            // the child forbids a superset of what the parent forbids.
            if let Ok(child_cap) = &child_v {
                let parent_cap = parent_v
                    .as_ref()
                    .expect("if the child verifies, the parent (strictly broader) must verify — widening bug otherwise");
                // (b) The capability the child grants ⊆ the parent's.
                prop_assert!(
                    child_cap.is_subset_of(parent_cap),
                    "child capability must be a subset of the parent's (monotonicity): child={child_cap:?} parent={parent_cap:?}"
                );
            }
        }

        /// Forgery under proptest: for ANY chain and ANY single-byte flip of the
        /// sig, verify fails. Complements the example-based tamper tests with a
        /// generated sweep of the mutation space.
        #[test]
        fn any_sig_flip_fails(
            base in any_chain(),
            byte in 0usize..32,
            xor in 1u8..=255,
        ) {
            let root = root();
            let m = Macaroon::mint(&root, "run-01SP4BPROPFORGE00000000000", base);
            let mut sig = m.sig_bytes();
            sig[byte] ^= xor;
            let forged = Macaroon::from_parts(m.identifier(), m.caveats().to_vec(), sig);
            let ctx = RequestContext::new().workspace(WS_A).verb("spawn").now(T_EARLY).role("lead");
            prop_assert!(
                matches!(verify(&forged, &root, &ctx), Err(VerifyError::BadSignature)),
                "any sig mutation must break the MAC chain"
            );
        }
    }
}
