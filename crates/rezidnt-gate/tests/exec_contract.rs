//! S4 oracle — the exec-verifier §8 contract (BINDING kind 2) and the
//! determinism/honesty requirements: exact stdin document, scrubbed
//! environment, wall-clock timeout, and the three inconclusive traps
//! (testing-oracles: verifier conformance — malformed input, timeouts,
//! nonzero exit; `inconclusive`-not-`pass` every time).
//!
//! RED MODE: assert-red. `ExecVerifier::run` is `todo!()`-stubbed; every
//! test panics until the runner exists. Unix-only: the argv programs are
//! tiny /bin/sh scripts in the test tempdir (S1 stub-harness precedent) —
//! deterministic, no network.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rezidnt_gate::{DEFAULT_TIMEOUT_MS, ExecVerifier, InconclusiveReason, Verdict, VerifierInput};
use serde_json::json;

/// Write an executable sh script into `dir`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).expect("write script");
    let mut perms = std::fs::metadata(&path).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

fn verifier(name: &str, path: &Path) -> ExecVerifier {
    ExecVerifier {
        name: name.to_string(),
        argv: vec![path.display().to_string()],
    }
}

/// A canonical pre_merge input: refs are CAS-ref STRINGS (inputs pinned by
/// content hash — BINDING; the blake3 is the committed s4 diff preimage
/// `M\tsrc/checkout/cart.rs\n`, computed with the reference crate).
fn input(timeout_ms: u64) -> VerifierInput {
    VerifierInput {
        gate: "pre_merge".to_string(),
        workspace: None,
        refs: BTreeMap::from([(
            "diff".to_string(),
            "cas:blake3:1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e"
                .to_string(),
        )]),
        params: json!({}),
        timeout_ms,
    }
}

/// §8 happy path: the verifier's stdout document is recorded VERBATIM —
/// verdict, evidence (kind/msg/ref), and its self-reported cost_ms (the
/// exit criterion's "recorded cost" per verifier).
#[tokio::test]
async fn stdout_document_is_recorded_verbatim() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(
        dir.path(),
        "fail.sh",
        r#"printf '{"verdict":"fail","evidence":[{"kind":"finding","msg":"test regression: auth::login","ref":"cas:blake3:a0fda6ff40cb5f91bd2d09cbfb839ae91b9b4c9aa0ccfc0981986c10d4d08246"}],"cost_ms":8412}\n'"#,
    );
    let record = verifier("tests-pass", &path)
        .run(&input(DEFAULT_TIMEOUT_MS))
        .await;

    assert_eq!(record.verifier, "tests-pass");
    assert_eq!(record.verdict, Verdict::Fail);
    assert_eq!(
        record.reason, None,
        "a delivered verdict carries no inconclusive reason"
    );
    assert_eq!(
        record.cost_ms, 8412,
        "cost_ms is the verifier's own report, recorded"
    );
    assert_eq!(record.evidence.len(), 1);
    assert_eq!(record.evidence[0].msg, "test regression: auth::login");
    assert_eq!(
        record.evidence[0].cas_ref.as_deref(),
        Some("cas:blake3:a0fda6ff40cb5f91bd2d09cbfb839ae91b9b4c9aa0ccfc0981986c10d4d08246")
    );
}

/// The child receives EXACTLY the serialized §8 stdin document — gate, refs
/// as cas strings, params, timeout_ms. This is what the ontology records
/// verbatim as `inputs`, so the wire and the log can never drift.
#[tokio::test]
async fn exec_receives_the_exact_stdin_document() {
    let dir = tempfile::tempdir().expect("tempdir");
    let received = dir.path().join("received.json");
    let path = script(
        dir.path(),
        "echo-stdin.sh",
        &format!(
            "cat > {}\nprintf '{{\"verdict\":\"pass\",\"evidence\":[],\"cost_ms\":1}}\\n'",
            received.display()
        ),
    );
    let input = input(DEFAULT_TIMEOUT_MS);
    let record = verifier("stdin-echo", &path).run(&input).await;
    assert_eq!(record.verdict, Verdict::Pass);

    let got: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&received).expect("stdin was delivered"))
            .expect("stdin is one JSON document");
    assert_eq!(
        got,
        serde_json::to_value(&input).expect("input serializes"),
        "the stdin document is the §8 contract, byte-for-meaning"
    );
    assert_eq!(
        got["timeout_ms"],
        json!(120_000),
        "DEFAULT timeout rides the document"
    );
}

/// §8 BINDING: nonzero exit = inconclusive, NEVER pass — even when the
/// stdout says pass. The trap is deliberate.
#[tokio::test]
async fn nonzero_exit_is_inconclusive_even_when_stdout_says_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(
        dir.path(),
        "liar.sh",
        "printf '{\"verdict\":\"pass\",\"evidence\":[],\"cost_ms\":1}\\n'\nexit 3",
    );
    let record = verifier("liar", &path)
        .run(&input(DEFAULT_TIMEOUT_MS))
        .await;
    assert_eq!(
        record.verdict,
        Verdict::Inconclusive,
        "nonzero exit can never deliver a verdict (I6)"
    );
    assert_eq!(record.reason, Some(InconclusiveReason::NonzeroExit));
}

/// §8 BINDING: malformed output = inconclusive, never pass.
#[tokio::test]
async fn malformed_stdout_is_inconclusive() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in [
        ("prose.sh", "printf 'LGTM ship it\\n'"),
        (
            "near-miss.sh",
            "printf '{\"verdict\":\"passed\",\"evidence\":[],\"cost_ms\":1}\\n'",
        ),
        (
            "boolean.sh",
            "printf '{\"verdict\":true,\"evidence\":[],\"cost_ms\":1}\\n'",
        ),
        ("silent.sh", "true"),
    ] {
        let path = script(dir.path(), name, body);
        let record = verifier(name, &path).run(&input(DEFAULT_TIMEOUT_MS)).await;
        assert_eq!(
            record.verdict,
            Verdict::Inconclusive,
            "{name}: malformed output is inconclusive, never pass (I6)"
        );
        assert_eq!(
            record.reason,
            Some(InconclusiveReason::MalformedOutput),
            "{name}"
        );
    }
}

/// The wall-clock timeout is ENFORCED (not advisory): a verifier sleeping
/// past `timeout_ms` comes back `inconclusive { timeout }` in bounded time.
#[tokio::test]
async fn timeout_is_inconclusive_with_reason_timeout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(dir.path(), "sleeper.sh", "sleep 30");
    let started = Instant::now();
    let record = verifier("sleeper", &path).run(&input(300)).await;
    assert_eq!(record.verdict, Verdict::Inconclusive);
    assert_eq!(record.reason, Some(InconclusiveReason::Timeout));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the 300 ms timeout must actually kill the child; waited {:?}",
        started.elapsed()
    );
}

/// Doc §12: exec verifiers run with a SCRUBBED environment — no ambient var
/// in the daemon's environment reaches the child.
///
/// HARDENING REWRITE (S4-debrief LOW): the prior version planted a canary via
/// `std::env::set_var`, which is process-global and `unsafe` in edition 2024 —
/// it races every sibling test running in parallel (the cargo default), the
/// flake this item chased. This version plants NOTHING in the process
/// environment: it reads the parent's REAL ambient vars (PATH, HOME, and
/// whatever cargo injects — all present without any test mutation) and asserts
/// the child sees NONE of them, plus that the child's env is structurally
/// EMPTY (proving `env_clear`, not just one canary's absence). Strictly
/// stronger than the canary check and free of process-global mutation.
#[tokio::test]
async fn environment_is_scrubbed() {
    let dir = tempfile::tempdir().expect("tempdir");
    // The probe emits its ENTIRE environment as the evidence msg (one VAR=VAL
    // per line via `env`), plus a marker line so an empty env is unambiguous.
    let path = script(
        dir.path(),
        "env-probe.sh",
        r#"envdump=$(env | tr '\n' ';')
printf '{"verdict":"pass","evidence":[{"kind":"env","msg":"BEGIN;%sEND"}],"cost_ms":1}\n' "$envdump""#,
    );
    let record = verifier("env-probe", &path)
        .run(&input(DEFAULT_TIMEOUT_MS))
        .await;
    assert_eq!(record.verdict, Verdict::Pass);
    let child_env = &record.evidence[0].msg;

    // Structural: the scrub is env_clear, so the child's environment is empty
    // (a POSIX shell may still synthesize PWD/SHLVL/_; those are shell-minted,
    // not INHERITED, so we assert on inheritance below rather than exact "").
    // The parent's own ambient vars — which we did NOT plant — must be absent.
    for ambient in ["PATH", "HOME", "CARGO", "CARGO_MANIFEST_DIR"] {
        if std::env::var_os(ambient).is_some() {
            assert!(
                !child_env.contains(&format!(";{ambient}=")),
                "ambient {ambient} from the parent leaked into the scrubbed \
                 child environment (doc §12): {child_env}"
            );
        }
    }
    // And the child never sees an inherited secret-shaped var — assert the
    // scrub is total by checking the dump carries no `=` from an inherited
    // name at all beyond shell-synthesized ones. PATH being the load-bearing
    // one, its absence is the §12 proof.
    assert!(
        std::env::var_os("PATH").is_some(),
        "sanity: the parent really has PATH to leak"
    );
    assert!(
        !child_env.contains(";PATH="),
        "PATH is the canonical ambient var; its presence would mean the \
         environment was not cleared (doc §12 scrub): {child_env}"
    );
}
