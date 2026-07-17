//! S4 oracle — the native pack v1 (§8 BINDING kind 1): diff-scope,
//! forbidden-path, and the three vet natives (bare-mode, pinned-version,
//! allowed-tools). Inputs are CAS refs, never mutable paths; evidence blobs
//! land in the CAS and ride as refs (I2); same content-hashed inputs ⇒ same
//! verdict and same evidence (I6 determinism).
//!
//! RED MODE: assert-red. Every `verify` is `todo!()`-stubbed; each test
//! panics until the natives exist.
//!
//! Pinned input formats (oracle decisions, stated in the work order):
//! - `refs["diff"]` blob: the S2 `diff.ready` summary — one
//!   `<status>\t<path>` line per touched file (the s2/s4 fixture preimages).
//! - `refs["spec"]` blob: the agent-spec TOML, an `[agent]` table (the §13
//!   `[[agent]]` entry serialized standalone).

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{
    AllowedTools, BareMode, DiffScope, ForbiddenPath, NativeVerifier, PinnedVersion, Verdict,
    VerifierInput,
};
use serde_json::{Value, json};

/// Temp CAS with a blob planted; returns the cas plus the blob's
/// `cas:blake3:<hex>` ref string.
fn cas_with(bytes: &[u8]) -> (tempfile::TempDir, Cas, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let cas_ref = cas.put(bytes, "text/plain").expect("put blob");
    let ref_str = format!("cas:blake3:{}", cas_ref.hash);
    (dir, cas, ref_str)
}

fn input(gate: &str, ref_name: &str, ref_str: &str, params: Value) -> VerifierInput {
    VerifierInput {
        gate: gate.to_string(),
        workspace: None,
        refs: BTreeMap::from([(ref_name.to_string(), ref_str.to_string())]),
        params,
        timeout_ms: 120_000,
    }
}

/// A diff touching only allowed paths passes.
#[test]
fn diff_scope_passes_in_scope_diff() {
    let (_dir, cas, diff) = cas_with(b"M\tsrc/checkout/cart.rs\n");
    let out = DiffScope
        .verify(
            &input(
                "pre_merge",
                "diff",
                &diff,
                json!({"allow": ["src/checkout/**"]}),
            ),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass);
}

/// An out-of-scope touch FAILS, and the evidence names the offending path —
/// as a CAS-backed blob the ref resolves to (I2 + interrogability: the
/// blocked agent reads WHAT was out of scope).
#[test]
fn diff_scope_fails_out_of_scope_and_evidence_names_the_path() {
    let (_dir, cas, diff) = cas_with(b"M\tsrc/checkout/cart.rs\nM\tsrc/payments/mod.rs\n");
    let out = DiffScope
        .verify(
            &input(
                "pre_merge",
                "diff",
                &diff,
                json!({"allow": ["src/checkout/**"]}),
            ),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);
    assert!(!out.evidence.is_empty(), "a fail carries evidence");
    assert!(
        out.evidence[0].msg.contains("src/payments/mod.rs"),
        "evidence msg names the offending path; got {:?}",
        out.evidence[0].msg
    );
    let ev_ref = out.evidence[0]
        .cas_ref
        .as_deref()
        .expect("evidence blob lands in the CAS");
    let hash = ev_ref.strip_prefix("cas:blake3:").expect("cas ref shape");
    let blob = cas
        .get(&rezidnt_types::refs::CasRef {
            hash: hash.to_string(),
            bytes: 0,
            mime: "text/plain".to_string(),
        })
        .expect("evidence ref resolves");
    assert!(
        String::from_utf8_lossy(&blob).contains("src/payments/mod.rs"),
        "the evidence blob itself names the path"
    );
}

/// forbidden-path: touching a forbidden glob fails and names the path;
/// a clean diff passes.
#[test]
fn forbidden_path_fails_on_forbidden_touch_and_passes_clean() {
    let params = json!({"forbid": [".env", "secrets/**"]});
    let (_d1, cas1, dirty) = cas_with(b"M\t.env\n");
    let out = ForbiddenPath
        .verify(&input("pre_merge", "diff", &dirty, params.clone()), &cas1)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);
    assert!(
        out.evidence[0].msg.contains(".env"),
        "evidence names the forbidden path"
    );

    let (_d2, cas2, clean) = cas_with(b"M\tsrc/checkout/cart.rs\n");
    let out = ForbiddenPath
        .verify(&input("pre_merge", "diff", &clean, params), &cas2)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass);
}

/// A missing input blob means the check CANNOT RUN: `inconclusive`, never
/// pass, never fail, never an engine error (I6 honesty).
#[test]
fn missing_cas_blob_is_inconclusive_not_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let absent = "cas:blake3:0000000000000000000000000000000000000000000000000000000000000000";
    let out = DiffScope
        .verify(
            &input("pre_merge", "diff", absent, json!({"allow": ["**"]})),
            &cas,
        )
        .expect("cannot-run is a verdict, not an error");
    assert_eq!(out.verdict, Verdict::Inconclusive);
}

/// I6 determinism: same content-hashed inputs, same verdict AND same
/// evidence refs — run twice, compare everything but cost.
#[test]
fn same_refs_same_verdict_and_same_evidence() {
    let (_dir, cas, diff) = cas_with(b"M\tsrc/payments/mod.rs\n");
    let inp = input(
        "pre_merge",
        "diff",
        &diff,
        json!({"allow": ["src/checkout/**"]}),
    );
    let first = DiffScope.verify(&inp, &cas).expect("engine ok");
    let second = DiffScope.verify(&inp, &cas).expect("engine ok");
    assert_eq!(first.verdict, second.verdict);
    assert_eq!(
        first.evidence, second.evidence,
        "evidence is deterministic, refs included"
    );
}

// --- vet natives (the pre-spawn policy: bare-mode / pinned-version /
// --- allowed-tools). The spec blobs are the committed fixture preimages.

const SPEC_CONFORMING: &[u8] = b"[agent]\nname = \"impl\"\nharness = \"claude-code\"\nbare = true\nharness_version = \"2.1.191\"\nallowed_tools = [\"Read\", \"Edit\"]\n";
const SPEC_UNBARED: &[u8] = b"[agent]\nname = \"impl\"\nharness = \"claude-code\"\nbare = false\n";

/// DR-001: `--bare` is the determinism knob a vet gate can require. A spec
/// without `bare = true` FAILS bare-mode; the conforming spec passes.
#[test]
fn bare_mode_requires_bare_true() {
    let (_d1, cas1, unbared) = cas_with(SPEC_UNBARED);
    let out = BareMode
        .verify(&input("vet", "spec", &unbared, json!({})), &cas1)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);
    assert!(
        out.evidence[0].msg.contains("bare"),
        "the refusal is interrogable: evidence names the missing knob"
    );

    let (_d2, cas2, conforming) = cas_with(SPEC_CONFORMING);
    let out = BareMode
        .verify(&input("vet", "spec", &conforming, json!({})), &cas2)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass);
}

/// Risk register (DR-001): harness CLI churn — governed runs pin the harness
/// version. A spec without `harness_version` fails pinned-version.
#[test]
fn pinned_version_requires_harness_version() {
    let (_d1, cas1, unpinned) = cas_with(SPEC_UNBARED);
    let out = PinnedVersion
        .verify(&input("vet", "spec", &unpinned, json!({})), &cas1)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);

    let (_d2, cas2, pinned) = cas_with(SPEC_CONFORMING);
    let out = PinnedVersion
        .verify(&input("vet", "spec", &pinned, json!({})), &cas2)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass);
}

/// DR-001: permission composition (`--allowedTools`) constrains the harness
/// per AgentSpec. A spec without an explicit non-empty `allowed_tools` list
/// fails allowed-tools.
#[test]
fn allowed_tools_requires_explicit_list() {
    let (_d1, cas1, missing) = cas_with(SPEC_UNBARED);
    let out = AllowedTools
        .verify(&input("vet", "spec", &missing, json!({})), &cas1)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Fail);

    let (_d2, cas2, listed) = cas_with(SPEC_CONFORMING);
    let out = AllowedTools
        .verify(&input("vet", "spec", &listed, json!({})), &cas2)
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Pass);
}
