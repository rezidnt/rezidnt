//! C3b+c oracle (DR-026 — the L7 egress-MITM + credential-brokering slice),
//! CRITERION 5 — the CATASTROPHIC-FAILURE-MODE test: the brokered secret VALUE
//! never appears in any fact, evidence blob, CAS payload, or trace. "A property
//! test over the WHOLE emitted log asserts only the `secret_ref` appears, never
//! the secret bytes" (DR-026 §Acceptance-criteria 5, §Risk-register). This is the
//! single most important test surface after inescapability — a leaked secret in
//! the log defeats the entire primitive.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure: build the fabric a mediated+injected
//! flow WOULD emit, then scan it; no connector, no TLS, no #[cfg(unix)]). The
//! never-in-log property is a property of the FACT SHAPES + the redacted
//! `BrokeredSecret` type, so it needs no live injection — it runs on every host
//! that builds rezidnt (Windows host /vet included), exactly where the DR wants
//! it. The REAL injection-on-the-wire is the `#[cfg(unix)]` WSL-only suite; this
//! is the host oracle that makes the never-leak property enforceable the way C3a
//! made no-widening a type property.
//!
//! RED MODE: **mixed**.
//!   - The `BrokeredSecret` redaction tests (`Debug`/`Display` print
//!     `"<redacted>"`; `.expose()` is the only value path) hold GREEN today — they
//!     pin the leak-guard the whole criterion rests on (like C3a's structural
//!     policy pin). If the implementer ever adds a value-printing `Debug`, THIS
//!     is where it fails first.
//!   - The whole-fabric scan drives `egress::injected_fact` — the by-reference
//!     `credential.injected` constructor — which does not exist yet, so this file
//!     is **compile-red** for those tests until the implementer adds it. The
//!     forcing function: the fact constructor must carry a `secret_ref`, NEVER the
//!     value; the scan then proves it holds over the serialized envelope.

use rezidnt_run::egress::BrokeredSecret;

/// The seeded secret whose bytes must appear NOWHERE in the fabric. A distinctive
/// sentinel so the scan is adversarial: if any byte-window of the serialized
/// fabric equals this, the primitive is defeated.
const SECRET_VALUE: &str = "ghp_SUPERSECRET_token_value_MUST_NEVER_LEAK_0xDEADBEEF";
/// The reference LABEL/HASH the fact is allowed to carry (and must, for
/// interrogability) — the `secret_ref`, never the value.
const SECRET_REF: &str = "github-token";

// --- The redaction leak-guard (GREEN today — the type-property half) ----------

/// CRITERION 5 (leak-guard) — `Debug` of a `BrokeredSecret` REDACTS the value. A
/// stray `{:?}` in a fact/evidence/trace prints `"<redacted>"`, never the bytes.
/// This is what makes the never-leak property STRUCTURAL rather than a review
/// convention — the same move C3a made turning no-widening into a private field.
#[test]
fn brokered_secret_debug_is_redacted() {
    let secret = BrokeredSecret::new(SECRET_REF, SECRET_VALUE);
    let debug = format!("{secret:?}");
    assert!(
        !debug.contains(SECRET_VALUE),
        "CRITERION 5 VIOLATION: the secret VALUE appeared in a `{{:?}}` debug format \
         ({debug:?}) — a stray debug-format into a fact/trace would leak it. `Debug` must \
         redact the value (the structural leak-guard, DR-026 §Risk-register)"
    );
    assert!(
        debug.contains("<redacted>"),
        "the redacted `Debug` prints the redaction sentinel; got {debug:?}"
    );
    // The ref label is safe to show (it is what facts carry) — and useful.
    assert!(
        debug.contains(SECRET_REF),
        "the ref label is safe in Debug (it is the fact's `secret_ref`); got {debug:?}"
    );
}

/// CRITERION 5 (leak-guard) — `Display` of a `BrokeredSecret` is `"<redacted>"`,
/// never the bytes. A `{}` interpolation into a log line cannot leak the value.
#[test]
fn brokered_secret_display_is_redacted() {
    let secret = BrokeredSecret::new(SECRET_REF, SECRET_VALUE);
    let shown = format!("{secret}");
    assert_eq!(
        shown, "<redacted>",
        "CRITERION 5: `Display` redacts the value — a `{{}}` into a trace line prints the \
         redaction, never the secret bytes"
    );
}

/// CRITERION 5 (leak-guard) — the value is reachable ONLY through the explicit
/// `.expose()`, which is the audit grep target for "where does a secret leave the
/// broker". `.expose()` returns the true bytes (used SOLELY at upstream
/// injection); every OTHER access path is redacted. `.secret_ref()` returns the
/// label, never the value.
#[test]
fn secret_value_reachable_only_through_expose() {
    let secret = BrokeredSecret::new(SECRET_REF, SECRET_VALUE);
    assert_eq!(
        secret.expose(),
        SECRET_VALUE,
        "`.expose()` is the ONE sanctioned value path (upstream injection only)"
    );
    assert_eq!(
        secret.secret_ref(),
        SECRET_REF,
        "`.secret_ref()` returns the LABEL the fact carries, never the value"
    );
    assert!(
        !secret.secret_ref().contains(SECRET_VALUE),
        "the secret_ref must NOT embed the value (it is the by-reference label, criterion 5)"
    );
}

// --- The whole-fabric scan (COMPILE-RED — the property half) ------------------
//
// Build the fabric a mediated+injected flow WOULD emit — a `permit.requested`,
// an `egress.allowed`/`permit.granted`, and a `credential.injected` fact — feed
// each through the emit path, serialize the WHOLE set (envelopes + payloads +
// any evidence), and assert the seeded secret bytes appear NOWHERE while the
// `secret_ref` DOES. This is the adversarial "over the whole emitted log"
// property DR-026 §5 demands.

/// CRITERION 5 (the centerpiece) — a mediated+injected flow's WHOLE emitted
/// fabric contains the `secret_ref` but NEVER the secret value. Build the
/// injection fact by reference (`egress::injected_fact`), assemble it into a real
/// `Event` envelope, serialize everything, and scan every byte.
///
/// COMPILE-RED until `rezidnt_run::egress::injected_fact` exists (the by-reference
/// `credential.injected {run, dest, secret_ref, policy_ref}` constructor — NEVER
/// the value). The forcing function: the constructor cannot be written to carry
/// the value and still pass this scan.
///
/// ## WARDEN-GATED ONTOLOGY — DO NOT MINT HERE.
/// The `credential.injected` subject is a DEFERRED warden `/subject` question
/// (DR-026 §Consequences / design §5) — NOT decided in the DR and NOT minted here.
/// The `injected_fact` constructor uses a PLACEHOLDER subject string; the SUBJECT
/// NAME is not ratified. TODO(warden, /subject): once the `credential.*`/`egress.*`
/// family is minted WITH its folding reducer (no consumer-less subjects, DR-006),
/// pin the ratified subject and STRENGTHEN this to assert the by-ref injection
/// folds onto a run-state field. The never-leak scan holds regardless of the name.
#[test]
fn whole_emitted_fabric_carries_secret_ref_never_the_value() {
    use rezidnt_run::egress::injected_fact;
    use rezidnt_types::refs::CasRef;
    use rezidnt_types::{Event, SourceId, Subject};
    use ulid::Ulid;

    const RUN: &str = "01C3BCEGRESSINJECT00R0001";
    let policy_ref = CasRef {
        hash: "po11c3bc00000000000000000000000000000000000000000000000000eg".to_string(),
        bytes: 128,
        mime: "application/octet-stream".to_string(),
    };
    let secret = BrokeredSecret::new(SECRET_REF, SECRET_VALUE);

    // The by-reference injection fact: it records THAT a secret was injected and
    // WHICH (the ref/label), never the bytes. The constructor takes the SECRET so
    // the type system could (wrongly) let a careless impl inline `.expose()`; the
    // scan below is what forbids that.
    let (subject, payload) = injected_fact(RUN, "github.com", &secret, &policy_ref);

    let event = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("the injection fact is a legal ≤32KiB envelope (I2 — ref only, never bytes)");

    // The fact DOES carry the secret_ref (interrogability, I6) ...
    assert_eq!(
        event.payload()["secret_ref"].as_str(),
        Some(SECRET_REF),
        "the injection fact carries the `secret_ref` so the injection is interrogable (I6)"
    );
    // ... and the fact does NOT carry the dest as a secret, etc. The dest is safe.
    assert_eq!(event.payload()["dest"].as_str(), Some("github.com"));

    // THE SCAN: serialize the WHOLE envelope (id, ts, subject, payload, refs) and
    // assert the seeded secret bytes appear NOWHERE. This is the adversarial
    // whole-fabric property — a byte-window equality over the serialized event.
    let serialized = event
        .to_json_line()
        .expect("event serializes to a JSONL frame");
    assert!(
        !serialized
            .as_bytes()
            .windows(SECRET_VALUE.len())
            .any(|w| w == SECRET_VALUE.as_bytes()),
        "CRITERION 5 VIOLATION (CATASTROPHIC): the secret VALUE appears in the emitted \
         `credential.injected` fabric — the secret leaked into the log, defeating the entire \
         primitive. Only the `secret_ref` may ride the fact, never the bytes (I2/I3, DR-026 §5)"
    );
    // The secret_ref DOES appear (so the assertion above is non-vacuous — the
    // secret's LABEL is present, only its VALUE is absent).
    assert!(
        serialized.contains(SECRET_REF),
        "the secret_ref appears in the serialized fact (non-vacuous: the label rides, the \
         value does not)"
    );
    // And the policy_ref rides for replayable interrogability (I3).
    assert!(
        serialized.contains(&policy_ref.hash),
        "the deciding policy_ref rides the injection fact so the injection replays (I3)"
    );
}
