//! DR-058 ORACLE — §Decision 4's two OTHER `path_for` callers map the new
//! `CasError::InvalidAddress` HONESTLY (I6): can't-run / no-ref, NEVER a
//! fabricated pass and never a synthesized evidence ref.
//!
//! DR-058 Context names the callers:
//!
//! - `resolve_ref` (`crates/rezidnt-gate/src/lib.rs:202`) — native-verifier
//!   ref resolution. Today it maps `NotFound` to `Ok(None)` (can't-run →
//!   `inconclusive`) but bubbles EVERY other `CasError` as a `GateError`. Once
//!   the crate guard lands, a non-address ref surfaces `InvalidAddress`, which
//!   MUST map like `NotFound`: the verifier says "I could not run", it does
//!   not error the engine and it does not decide.
//! - `honest_evidence_ref` (`crates/rezidnt-gate/src/permit.rs:394`) — the
//!   permit emit path's metadata-honest companion ref. Today it passes a
//!   subprocess-supplied string to `path_for` + `fs::metadata` UNCHECKED: a
//!   verifier's stdout naming `cas:blake3:../<file>` gets `Some(CasRef)` whose
//!   `bytes` is an UNRELATED file's size — a fabricated evidence ref on a
//!   durable permit-decision fact (the DR-058 Context finding: "the replay
//!   property is defeated by data, not tampering"). It MUST map an invalid
//!   address the way it maps an unstat-able blob: `None`, no ref pinned.
//!
//! ## RED MODE (against the tree at cut time)
//!
//! ASSERT-RED, not compile-red: both defects are observable through public
//! surfaces today. `DiffScope::verify` returns `Err(Cas(Corrupt))` for a
//! planted non-address ref (the `.expect` panics); `aggregate_async` pins a
//! fabricated `deciding_evidence_ref` (the `is_none` fails). The traversal
//! legs PLANT real files so the pre-guard behaviour has something to find —
//! and the planted diff summary is deliberately PASS-SHAPED, so a mutant that
//! "fixes" the mapping by reading through the invalid ref is caught as a
//! fabricated pass, not just a wrong error.
//!
//! ## Windows note, disclosed
//!
//! The uppercase leg is host-red / linux-green today: a case-insensitive
//! filesystem resolves the uppercased address to the real blob (→ `Corrupt`),
//! a case-sensitive one does not (→ honest can't-run already). Post-guard the
//! answer is the same can't-run on both. The exec-driven evidence test runs
//! the platform's own file-printer (`/bin/cat` / `cmd /c type`) so the judge
//! is host-visible, not `#[cfg(unix)]`-vaulted.

use std::collections::BTreeMap;
use std::path::Path;

use rezidnt_cas::Cas;
use rezidnt_gate::permit::{PermitVerifierSpec, aggregate_async};
use rezidnt_gate::{DiffScope, Evidence, NativeVerifier, Verdict, VerifierInput, VerifierOutput};
use serde_json::json;

/// Store rooted one level below the tempdir so `../` targets stay inside it.
fn temp_store() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(&dir.path().join("cas")).expect("open cas");
    (dir, cas)
}

/// A diff summary that would PASS `diff-scope` under the params below if any
/// implementation ever read it through an invalid ref. Coercion bait: the
/// honest answer to an unresolvable ref is can't-run, and a mutant that reads
/// anyway produces the exact fabricated pass this board forbids.
const PASS_SHAPED_SUMMARY: &[u8] = b"M\tsrc/lib.rs\n";

fn diff_input(ref_str: &str) -> VerifierInput {
    VerifierInput {
        gate: "vet".to_string(),
        workspace: None,
        refs: BTreeMap::from([("diff".to_string(), ref_str.to_string())]),
        // Everything the bait summary touches is allowed: a read-through IS a Pass.
        params: json!({"allow": ["**", "src/**", "*"]}),
        timeout_ms: 30_000,
    }
}

/// Assert the one honest shape for an unresolvable ref: `Ok`, `Inconclusive`,
/// evidence says can't-run. Anything else — an engine `Err`, a `Pass`, a
/// `Fail` — either breaks the engine on subprocess-reachable data or decides
/// on evidence that was never read (I6).
fn assert_cannot_run(label: &str, result: Result<VerifierOutput, rezidnt_gate::GateError>) {
    let out = result.unwrap_or_else(|e| {
        panic!(
            "{label}: an unresolvable ref is a CAN'T-RUN, never an engine error \
             — resolve_ref must map InvalidAddress the way it maps NotFound \
             (DR-058 §Decision 4). Got Err({e})"
        )
    });
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "{label}: a ref that cannot resolve must NEVER become a passing \
         verdict (I6) — the planted summary is pass-shaped bait, and a Pass \
         here means the invalid ref was READ"
    );
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "{label}: the honest verdict for an unresolvable pinned input is \
         inconclusive (can't-run), same as an absent blob"
    );
    assert_eq!(
        out.evidence.first().map(|e| e.kind.as_str()),
        Some("cannot-run"),
        "{label}: the evidence says WHY — cannot-run, interrogable (I6): {:?}",
        out.evidence
    );
}

/// A `cas:blake3:` ref whose hash part is a TRAVERSAL, with a real pass-shaped
/// file planted at the target. Today: `Cas::get` reads the planted bytes,
/// re-hashes, and `resolve_ref` bubbles `Corrupt` as an engine error — a
/// subprocess-suppliable string breaks the engine. Post-guard: can't-run.
#[test]
fn a_traversal_diff_ref_is_a_cannot_run_never_an_engine_error_or_a_pass() {
    let (dir, cas) = temp_store();
    std::fs::write(dir.path().join("outside_summary.txt"), PASS_SHAPED_SUMMARY)
        .expect("plant traversal target");

    let result = DiffScope.verify(&diff_input("cas:blake3:../outside_summary.txt"), &cas);
    assert_cannot_run("traversal ref", result);
}

/// A non-address name planted INSIDE the CAS root — no traversal, still not an
/// address. Today this also reads and errors `Corrupt`; post-guard it is the
/// same can't-run as every other unresolvable ref.
#[test]
fn an_in_root_non_address_ref_is_a_cannot_run() {
    let (dir, cas) = temp_store();
    std::fs::write(dir.path().join("cas").join("evil"), PASS_SHAPED_SUMMARY)
        .expect("plant in-root non-address file");

    let result = DiffScope.verify(&diff_input("cas:blake3:evil"), &cas);
    assert_cannot_run("in-root non-address ref", result);
}

/// The UPPERCASE variant of a real blob's address (the Windows hazard: the
/// case-insensitive join finds the lowercase blob, the hash compare then
/// reports Corrupt → engine error). Post-guard: InvalidAddress → can't-run on
/// every platform.
#[test]
fn an_uppercased_real_address_ref_is_a_cannot_run() {
    let (_dir, cas) = temp_store();
    let stored = cas
        .put(PASS_SHAPED_SUMMARY, "text/plain")
        .expect("put real summary");
    let upper = stored.hash.to_ascii_uppercase();
    assert_ne!(upper, stored.hash);

    let result = DiffScope.verify(&diff_input(&format!("cas:blake3:{upper}")), &cas);
    assert_cannot_run("uppercased real address", result);
}

/// PARITY CONTROL (green today, the mapping being matched): a WELL-FORMED
/// address the store does not hold is already an honest can't-run. DR-058
/// §Decision 4's obligation is "map InvalidAddress the SAME way" — this pins
/// the way.
#[test]
fn an_absent_well_formed_ref_is_the_cannot_run_being_matched() {
    let (_dir, cas) = temp_store();
    let absent = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";
    let result = DiffScope.verify(&diff_input(&format!("cas:blake3:{absent}")), &cas);
    assert_cannot_run("absent well-formed ref", result);
}

// ---------------------------------------------------------------------------
// honest_evidence_ref — driven through `aggregate_async` + an exec permit
// verifier, the exact vector DR-058 Context names: Evidence deserialized from
// a subprocess's STDOUT reaches `path_for` + `fs::metadata`.
// ---------------------------------------------------------------------------

/// argv that prints a file to stdout on this platform. The exec runner
/// `env_clear`s the child, so argv[0] is absolute.
#[cfg(unix)]
fn print_file_argv(path: &Path) -> Vec<String> {
    vec!["/bin/cat".to_string(), path.display().to_string()]
}
#[cfg(windows)]
fn print_file_argv(path: &Path) -> Vec<String> {
    let comspec =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_string());
    vec![
        comspec,
        "/c".to_string(),
        "type".to_string(),
        path.display().to_string(),
    ]
}

/// One exec permit verifier whose stdout is exactly `out`, dispatched through
/// the real aggregation. Returns the aggregated outcome.
async fn aggregate_planted_stdout(
    dir: &Path,
    cas: &Cas,
    out: &VerifierOutput,
) -> rezidnt_gate::permit::PermitOutcome {
    let doc = dir.join("verifier_stdout.json");
    std::fs::write(
        &doc,
        serde_json::to_vec(out).expect("stdout doc serializes"),
    )
    .expect("write stdout doc");
    let spec = PermitVerifierSpec::exec("planted-stdout", print_file_argv(&doc), json!({}));
    let input = VerifierInput {
        gate: "permit".to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params: json!({}),
        timeout_ms: 30_000,
    };
    aggregate_async(&[spec], &input, cas)
        .await
        .expect("aggregation itself must not error")
}

fn failing_output_with_ref(cas_ref: Option<String>) -> VerifierOutput {
    VerifierOutput {
        verdict: Verdict::Fail,
        evidence: vec![Evidence {
            kind: "probe".to_string(),
            msg: "planted by the dr058 oracle".to_string(),
            cas_ref,
        }],
        cost_ms: 1,
    }
}

/// THE FABRICATION JUDGE — a deciding evidence ref whose hash part is a
/// traversal, with a real file (of a DISTINCTIVE size) planted at the target.
/// Today `honest_evidence_ref` stats the planted file and pins
/// `Some(CasRef{hash: "../…", bytes: 12345, …})` onto the outcome — a
/// synthesized evidence ref addressing nothing in the store, exactly what a
/// `gate_explain`/`debrief` replay would then try to resolve. The honest
/// answer is `None`: no ref, never a fabricated one (I6).
///
/// The `verdict == Fail` assertion is the exec-plumbing canary: it proves the
/// crafted stdout was delivered and decided, so a broken subprocess cannot
/// make the `is_none` leg pass vacuously (a could-not-run exec yields
/// Inconclusive with its own ref-less evidence, and this test fails LOUDLY on
/// the verdict instead).
#[tokio::test]
async fn a_traversal_evidence_ref_is_never_pinned_as_deciding_evidence() {
    let (dir, cas) = temp_store();
    let planted: Vec<u8> = vec![0x5a; 12_345];
    std::fs::write(dir.path().join("outside_evidence.bin"), &planted)
        .expect("plant traversal target");

    let out = failing_output_with_ref(Some("cas:blake3:../outside_evidence.bin".to_string()));
    let outcome = aggregate_planted_stdout(dir.path(), &cas, &out).await;

    assert_eq!(
        outcome.verdict,
        Verdict::Fail,
        "canary: the planted Fail must arrive through the exec seam — an \
         Inconclusive here means the subprocess did not run, and the judge \
         below would be vacuous: {outcome:?}"
    );
    assert!(
        outcome.deciding_evidence_ref.is_none(),
        "a non-address evidence ref pins NO deciding_evidence_ref — \
         honest_evidence_ref must map InvalidAddress like an unstat-able blob \
         (None), never fabricate a CasRef whose bytes is an unrelated file's \
         size (I6, DR-058 §Decision 4): got {:?}",
        outcome.deciding_evidence_ref
    );
}

/// PARITY CONTROL (green today): a WELL-FORMED absent ref already yields
/// `None` — the mapping InvalidAddress must match.
#[tokio::test]
async fn an_absent_well_formed_evidence_ref_pins_no_ref() {
    let (dir, cas) = temp_store();
    let absent = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";
    let out = failing_output_with_ref(Some(format!("cas:blake3:{absent}")));
    let outcome = aggregate_planted_stdout(dir.path(), &cas, &out).await;

    assert_eq!(outcome.verdict, Verdict::Fail, "canary: the exec ran");
    assert!(
        outcome.deciding_evidence_ref.is_none(),
        "an absent blob yields no honest metadata to pin: {:?}",
        outcome.deciding_evidence_ref
    );
}

/// NON-VACUITY CONTROL (green today): a REAL evidence blob's ref IS pinned,
/// with the blob's true byte length. Without this, an implementation could
/// satisfy the fabrication judge by mapping EVERYTHING to `None` — retiring
/// the metadata-honesty the field exists for.
#[tokio::test]
async fn a_real_evidence_ref_is_still_pinned_with_true_bytes() {
    let (dir, cas) = temp_store();
    let blob = b"real evidence bytes for the dr058 oracle";
    let stored = cas.put(blob, "text/plain").expect("put evidence blob");

    let out = failing_output_with_ref(Some(format!("cas:blake3:{}", stored.hash)));
    let outcome = aggregate_planted_stdout(dir.path(), &cas, &out).await;

    assert_eq!(outcome.verdict, Verdict::Fail, "canary: the exec ran");
    let r = outcome
        .deciding_evidence_ref
        .expect("a resolvable evidence ref is pinned — always-None is the inverted defect");
    assert_eq!(r.hash, stored.hash, "the pinned ref is the evidence's own");
    assert_eq!(
        r.bytes,
        blob.len() as u64,
        "bytes is the blob's TRUE length from the store, never the claim"
    );
}
