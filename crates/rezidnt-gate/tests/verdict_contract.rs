//! S4 oracle — the §8 verdict contract (BINDING) and the BINDING defaults.
//!
//! RED MODE: assert-red. The skeleton types are real; `parse_verifier_output`
//! and `GateDef::default` are `todo!()`-stubbed, so every test here panics
//! until the implementer lands them. No test in this file passes against the
//! skeleton (verified at board time).

use proptest::prelude::*;
use rezidnt_gate::{DEFAULT_TIMEOUT_MS, GateDef, Verdict, parse_verifier_output};
use serde_json::json;

/// The three canonical verdict strings parse to the three variants —
/// and carry evidence + cost_ms through verbatim.
#[test]
fn canonical_stdout_documents_parse() {
    let out = parse_verifier_output(
        br#"{"verdict":"fail","evidence":[{"kind":"finding","msg":"test regression: auth::login","ref":"cas:blake3:a0fda6ff40cb5f91bd2d09cbfb839ae91b9b4c9aa0ccfc0981986c10d4d08246"}],"cost_ms":8412}"#,
    )
    .expect("the doc §8 example is the contract");
    assert_eq!(out.verdict, Verdict::Fail);
    assert_eq!(
        out.cost_ms, 8412,
        "cost_ms recorded verbatim (the exit demands recorded cost)"
    );
    assert_eq!(out.evidence.len(), 1);
    assert_eq!(out.evidence[0].kind, "finding");
    assert_eq!(
        out.evidence[0].cas_ref.as_deref(),
        Some("cas:blake3:a0fda6ff40cb5f91bd2d09cbfb839ae91b9b4c9aa0ccfc0981986c10d4d08246"),
        "evidence rides as CAS refs (I2)"
    );

    for (text, verdict) in [
        (
            r#"{"verdict":"pass","evidence":[],"cost_ms":1}"#,
            Verdict::Pass,
        ),
        (
            r#"{"verdict":"inconclusive","evidence":[],"cost_ms":1}"#,
            Verdict::Inconclusive,
        ),
    ] {
        assert_eq!(
            parse_verifier_output(text.as_bytes())
                .expect("canonical verdict string")
                .verdict,
            verdict
        );
    }
}

/// I6: a verdict is NEVER a bare boolean and never a near-miss string.
/// `{"verdict": true}` and `{"verdict": "passed"}` are malformed — Err, so
/// the engine's only honest mapping is `inconclusive`, never pass.
#[test]
fn boolean_and_unknown_verdicts_are_malformed() {
    for text in [
        r#"{"verdict":true,"evidence":[],"cost_ms":1}"#,
        r#"{"verdict":false,"evidence":[],"cost_ms":1}"#,
        r#"{"verdict":"passed","evidence":[],"cost_ms":1}"#,
        r#"{"verdict":"PASS","evidence":[],"cost_ms":1}"#,
        r#"{"verdict":"ok","evidence":[],"cost_ms":1}"#,
        r#"LGTM ship it"#,
        r#""#,
    ] {
        assert!(
            parse_verifier_output(text.as_bytes()).is_err(),
            "must be malformed (I6: never coerced toward a verdict): {text:?}"
        );
    }
}

/// BINDING defaults: 120 s wall-clock timeout, network off.
#[test]
fn gate_def_defaults_pin_120s_timeout_and_network_off() {
    let def = GateDef::default();
    assert_eq!(def.timeout_ms, DEFAULT_TIMEOUT_MS);
    assert_eq!(
        DEFAULT_TIMEOUT_MS, 120_000,
        "doc §8: wall-clock timeout 120 s DEFAULT"
    );
    assert!(
        !def.network,
        "no network by default (BINDING); opt-in is recorded"
    );
}

/// The stdin document a gate def assembles carries the def's timeout and the
/// CAS-ref strings untouched — inputs pinned by content hash, not paths.
#[test]
fn input_for_carries_default_timeout_and_cas_refs_verbatim() {
    let def = GateDef {
        name: "pre_merge".to_string(),
        network: false,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    };
    let diff_ref = "cas:blake3:1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e";
    let input = def.input_for(
        None,
        [("diff".to_string(), diff_ref.to_string())].into(),
        json!({"allow": ["src/checkout/**"]}),
    );
    assert_eq!(input.gate, "pre_merge");
    assert_eq!(input.timeout_ms, 120_000);
    assert_eq!(input.refs["diff"], diff_ref);
    assert_eq!(input.params, json!({"allow": ["src/checkout/**"]}));
}

proptest! {
    /// Verifier conformance (testing-oracles): if garbage can parse to
    /// `pass`, the contract is broken. For ARBITRARY byte strings, a parse
    /// either fails or yields a document whose verdict came from the three
    /// canonical strings — and a `pass` requires the literal `"verdict":"pass"`
    /// member to be present in the input.
    #[test]
    fn garbage_never_parses_to_pass(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        if let Ok(out) = parse_verifier_output(&bytes) {
            if out.verdict == Verdict::Pass {
                let text = String::from_utf8(bytes.clone()).map_err(|_| {
                    TestCaseError::fail("non-UTF8 input parsed to a pass verdict")
                })?;
                prop_assert!(
                    text.contains(r#""verdict""#) && text.contains(r#""pass""#),
                    "a pass verdict must be explicit in the input, never synthesized: {text:?}"
                );
            }
        }
    }
}
