//! DR-041 slice `verify-subcommand` ORACLE — the `rezidnt verify <name>`
//! subcommand dispatch + the FIRST real exec verifier, `cargo-test`, at the CLI
//! surface. Pins criteria 1, 2, 3: the §8 verdict DOCUMENT shape, the I6 verdict
//! mapping (the three traps — never coerced, always interrogable), and
//! determinism / no-network with `cost_ms` RECORDED-not-asserted.
//!
//! This board drives the REAL `rezidnt` binary (`CARGO_BIN_EXE_rezidnt`) and
//! pins the CONTRACT — the emitted document is the SAME shape
//! `parse_verifier_output` / `VerifierOutput` already accepts (grounded against
//! `crates/rezidnt-gate/src/lib.rs` and the exec contract oracle
//! `crates/rezidnt-gate/tests/exec_contract.rs`) — NOT a new document shape and
//! NOT a specific machine's toolchain. Determinism comes from CONTROLLING THE
//! INPUTS: the cargo-test TARGET is a tiny fixture crate written into the test
//! tempdir (a couple of trivial passing tests, plus fail / compile-error /
//! no-cargo / timeout variants), deterministic and network-free (S1 stub-harness
//! precedent).
//!
//! ## BOARD HISTORY (RED-at-board-time → GREEN once the slice shipped)
//!
//! At board time `rezidnt verify` was an UNKNOWN subcommand: the `Cmd` enum in
//! `bins/rezidnt/src/main.rs` had no `Verify` arm, so clap exited **2** (usage
//! error) writing NOTHING to stdout, and every `--json` document parse below
//! failed BECAUSE THE SUBCOMMAND WAS ABSENT — never because an assertion was
//! malformed. A dispatch SENTINEL asserted that absence explicitly so a reviewer
//! could see the failure was dispatch-absence and nothing subtler.
//!
//! The implementer then added the `Verify { name, dir, --json, --timeout-ms }`
//! arm wiring `cargo-test` through `ExecVerifier`, and the seven contract tests
//! below went green on their own merits. The sentinel was DESIGNED to flip the
//! moment dispatch landed: it is now `verify_subcommand_dispatch_exists`, pinning
//! the true positive property (the subcommand is registered — `--help` exits 0
//! and names the verb) rather than the now-false absence claim. A regression that
//! dropped or renamed the verb turns it red again, so it stays load-bearing.
//!
//! Unix-only shape is fine per the S4 precedent, but this board needs no socket
//! and only shells to `cargo`, so it stays cross-platform (host `/vet` covers it;
//! `cargo` is a host prerequisite the golden path already assumes).
//!
//! ## The surface this board PINS for the implementer (the smallest honest one)
//!
//! Verb: `rezidnt verify <name> [--dir <DIR>] [--json] [--timeout-ms <MS>]`, a new
//! subcommand of the EXISTING `rezidnt` CLI (I7, one binary; NOT a new bin). In v1
//! `<name>` is `cargo-test`. The command runs the named verifier against the
//! target worktree (`--dir`, absent = cwd) and writes a SINGLE §8
//! `VerifierOutput` JSON document to stdout: `{ "verdict", "evidence": [...],
//! "cost_ms" }`. This is exactly the document `parse_verifier_output` accepts and
//! exactly what the daemon's exec runner reads back when this same subcommand is
//! named as an exec argv in `[gates.pre_merge]` (criterion 4, the separate e2e
//! board). The `--json` flag mirrors every other verb; the document IS the
//! machine-readable output.
//!
//! Injectable seams the implementer MUST honor (how the tests force the traps):
//! - `--dir <DIR>`: the cargo-test target. A fixture crate whose tests PASS → the
//!   document verdict is `pass`; a fixture with a failing `#[test]` → `fail`; a
//!   fixture that does not compile → `inconclusive` (could-not-run), NOT `fail`.
//! - `PATH` without `cargo` → `inconclusive` (could-not-run / cannot-run), NOT
//!   `fail` and NOT a crash (I6: a missing toolchain is undecidable, never a
//!   silent pass and never a hidden failure — DR-041 Decision 3/4).
//! - `--timeout-ms <MS>`: a wall-clock budget a runaway build/test exceeds →
//!   `inconclusive` (timeout), NOT coerced (I6, DR-041 Decision 4).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use rezidnt_gate::{Verdict, VerifierOutput, parse_verifier_output};

/// Write `contents` to `path`, creating parent dirs.
fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir -p");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Scaffold a tiny, network-free cargo fixture crate under `root/<name>` with the
/// given `lib.rs` body and return the crate dir. The crate has NO dependencies —
/// `cargo test` on it neither fetches nor builds anything external (determinism +
/// no-network, criterion 3). Kept under the TEST tree (a tempdir), never under a
/// production `bins/`/`crates/` path (oracle house rule).
fn fixture_crate(root: &Path, name: &str, lib_rs: &str) -> PathBuf {
    let crate_dir = root.join(name);
    write(
        &crate_dir.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n"
        ),
    );
    write(&crate_dir.join("src/lib.rs"), lib_rs);
    crate_dir
}

/// A crate whose tests all PASS (a couple of trivial ones — deterministic).
fn passing_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_pass_fixture",
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\
         #[test] fn adds() { assert_eq!(add(2, 2), 4); }\n\
         #[test] fn adds_zero() { assert_eq!(add(0, 7), 7); }\n",
    )
}

/// A crate that COMPILES but has one failing `#[test]`. The failing test's NAME
/// (`multiplies_wrong`) must reach the §8 evidence so `gate_explain` can say WHICH
/// test failed (criterion 2 interrogability).
fn failing_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_fail_fixture",
        "pub fn mul(a: i32, b: i32) -> i32 { a * b }\n\
         #[test] fn multiplies_ok() { assert_eq!(mul(3, 3), 9); }\n\
         #[test] fn multiplies_wrong() { assert_eq!(mul(2, 2), 5); }\n",
    )
}

/// A crate that does NOT compile (a type error). A compile failure is
/// could-not-run → inconclusive, NEVER a test `fail` (criterion 2, DR-041
/// Decision 4).
fn compile_error_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_compile_error_fixture",
        // `let x: u32 = \"not a number\";` — a hard type error, no test even runs.
        "pub fn broken() -> u32 { let x: u32 = \"not a number\"; x }\n\
         #[test] fn never_runs() { assert_eq!(broken(), 0); }\n",
    )
}

/// Run `rezidnt verify <name> --dir <dir> --json` with an optional extra arg set
/// and env overrides; capture the raw Output. The `--json` document IS the
/// machine-readable stdout.
fn run_verify(dir: &Path, extra: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("verify")
        .arg("cargo-test")
        .arg("--dir")
        .arg(dir)
        .arg("--json");
    cmd.args(extra);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("run rezidnt verify")
}

/// Parse the stdout of a `verify` run as a §8 `VerifierOutput` THROUGH the gate
/// crate's own strict parser — the SAME `parse_verifier_output` the exec runner
/// uses (criterion 1: the shape the runner already accepts, never a new one). A
/// parse failure here is red-for-the-right-reason today: stdout is empty because
/// `verify` is an unknown subcommand.
fn verdict_doc(out: &std::process::Output) -> VerifierOutput {
    parse_verifier_output(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "`rezidnt verify cargo-test --json` must print a §8 VerifierOutput document \
             on stdout that parse_verifier_output accepts ({e}); \
             stdout={:?} stderr={:?} status={:?}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            out.status.code(),
        )
    })
}

/// DISPATCH SENTINEL (criterion 1 preface): `rezidnt verify cargo-test` is a
/// REGISTERED subcommand — clap exits 0 on `--help` and its help text names the
/// verb. This is the flipped positive form of the board-time sentinel (which
/// asserted dispatch ABSENCE and was designed to flip the moment the `Verify` arm
/// landed): it now pins the true positive property that the dispatch EXISTS, so
/// the contract tests below stand on a registered verb, not an unknown one. A
/// regression that deleted or renamed the `verify cargo-test` subcommand turns
/// this red (clap would exit 2 on the unknown subcommand), so it stays
/// load-bearing rather than vacuous.
#[test]
fn verify_subcommand_dispatch_exists() {
    let out = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .args(["verify", "cargo-test", "--help"])
        .output()
        .expect("run rezidnt verify --help");
    // A registered subcommand answers `--help` with exit 0 (clap's help path);
    // an unknown subcommand would be a usage error (exit 2). Exit 0 IS the
    // dispatch-exists proof.
    assert_eq!(
        out.status.code(),
        Some(0),
        "`rezidnt verify cargo-test` must be a registered subcommand (--help exits 0); \
         a non-zero exit means the dispatch was dropped/renamed. stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // The help text names the verb — the subcommand is `verify cargo-test`, not a
    // near-miss that merely happens to exit 0. (clap prints the command path in
    // its usage line; matching `verify` keeps the sentinel meaningful without
    // over-pinning clap's exact help layout.)
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(
        help.contains("verify"),
        "the help output must name the `verify` verb; got stdout={help:?}"
    );
}

/// CRITERION 1 — the subcommand emits a valid §8 verdict document. A passing
/// fixture crate yields a well-formed `VerifierOutput` on stdout: verdict `pass`,
/// an `evidence` array (possibly empty), and a `cost_ms` u64. The document is the
/// SAME shape the exec runner reads back (parse_verifier_output accepts it) — the
/// wire and the log can never drift (§8 BINDING).
#[test]
fn emits_a_valid_section8_verdict_document_on_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = passing_crate(root.path());

    let out = run_verify(&crate_dir, &[], &[]);
    let doc = verdict_doc(&out);

    assert_eq!(
        doc.verdict,
        Verdict::Pass,
        "a crate whose tests pass verifies pass; stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    // `cost_ms` is RECORDED but its VALUE is not asserted (wall-clock varies —
    // mirroring golden_path.rs). Presence / u64 is the whole of criterion 3's
    // cost pin: the document HAS a cost_ms field (a u64 by type), nothing more.
    // (VerifierOutput.cost_ms is a u64, so its presence is the parse succeeding;
    // this line documents the deliberate non-assertion of a number.)
    let _cost_is_recorded_not_asserted: u64 = doc.cost_ms;
}

/// CRITERION 2 (trap A → pass): a project whose tests PASS maps to `pass`, never
/// coerced away. (The document-shape half is pinned above; this pins the mapping
/// leg alongside the fail/inconclusive legs so the three traps read together.)
#[test]
fn tests_pass_maps_to_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(&passing_crate(root.path()), &[], &[]);
    assert_eq!(verdict_doc(&out).verdict, Verdict::Pass);
}

/// CRITERION 2 (trap B → fail, INTERROGABLE): a real TEST FAILURE maps to `fail`,
/// and the evidence NAMES the failing test (`multiplies_wrong`) — a bare boolean
/// would fail this. `gate_explain` must be able to say WHICH test failed (I6
/// interrogability, DR-041 Decision 4), so the failing test's name appears in the
/// evidence msg (or an evidence blob the msg references), never just a verdict.
#[test]
fn a_real_test_failure_maps_to_fail_and_names_the_failing_test() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(&failing_crate(root.path()), &[], &[]);
    let doc = verdict_doc(&out);

    assert_eq!(
        doc.verdict,
        Verdict::Fail,
        "a compiling crate with a failing #[test] verifies fail (NOT inconclusive — \
         it compiled and the tool reported a real defect); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Interrogability: the failing test's name must be somewhere in the evidence,
    // so `gate why` / `gate_explain` can answer "which test". We do not pin the
    // evidence `kind` or the exact msg wording — only that the NAME is carried.
    let named = doc
        .evidence
        .iter()
        .any(|e| e.msg.contains("multiplies_wrong"));
    assert!(
        named,
        "the fail verdict must NAME the failing test in its evidence \
         (interrogability, I6) — a bare boolean is not enough; evidence={:?}",
        doc.evidence
    );
}

/// CRITERION 2 (trap C → inconclusive, could-not-run): a COMPILE ERROR maps to
/// `inconclusive`, NOT `fail`. The crate never built, so no test ran — the
/// verifier could not reach a verdict (DR-041 Decision 4: compile error /
/// toolchain absent → could-not-run, never a test failure). The document verdict
/// is `inconclusive`; it is emphatically not `fail` and not `pass`.
#[test]
fn a_compile_error_maps_to_inconclusive_not_fail() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(&compile_error_crate(root.path()), &[], &[]);
    let doc = verdict_doc(&out);

    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a crate that does not COMPILE is could-not-run → inconclusive, NEVER fail \
         (I6, DR-041 Decision 4 — a compile error is not a test failure); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        doc.verdict,
        Verdict::Fail,
        "a compile error must not be coerced to a test failure"
    );
}

/// CRITERION 2 (trap C' → inconclusive, cannot-run): `cargo` ABSENT from PATH maps
/// to `inconclusive` (could-not-run / cannot-run), NOT `fail` and NOT a crash. The
/// verifier wrapper is in-binary but its toolchain is a host prerequisite (DR-041
/// Decision 3 honesty note): a machine without `cargo` cannot decide, so it says
/// so — never a silent pass, never a hidden failure. Forced by running with an
/// EMPTY PATH so `cargo` cannot be resolved.
///
/// The verifier still emits a §8 document (verdict `inconclusive`) on stdout: the
/// cannot-run condition is a VERDICT, not an error exit — mirroring the exec
/// runner's `CouldNotRun` mapping in `crates/rezidnt-gate/src/lib.rs`.
#[test]
fn cargo_absent_from_path_maps_to_inconclusive_not_fail() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = passing_crate(root.path());

    // Empty PATH: `cargo` is unresolvable, so the verifier cannot run the suite.
    let out = run_verify(&crate_dir, &[], &[("PATH", "")]);
    let doc = verdict_doc(&out);

    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "cargo absent from PATH is cannot-run → inconclusive, NEVER fail and never a \
         silent pass (I6, DR-041 Decision 3/4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(doc.verdict, Verdict::Pass, "absent toolchain is not a pass");
    assert_ne!(doc.verdict, Verdict::Fail, "absent toolchain is not a fail");
}

/// CRITERION 2 (trap D → inconclusive, timeout): a wall-clock TIMEOUT maps to
/// `inconclusive` (timeout reason), NEVER coerced to pass/fail. Forced with a
/// tiny `--timeout-ms` against a passing crate: even a trivial `cargo test`
/// exceeds a sub-100ms budget (compile + link + run), so the timeout trap fires
/// deterministically without a sleeping fixture. The run must also RETURN in
/// bounded time — the timeout actually kills the child (mirrors the exec
/// contract's `timeout_is_inconclusive_with_reason_timeout`).
#[test]
fn a_wall_clock_timeout_maps_to_inconclusive_never_coerced() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = passing_crate(root.path());

    let started = Instant::now();
    let out = run_verify(&crate_dir, &["--timeout-ms", "50"], &[]);
    let doc = verdict_doc(&out);

    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a run exceeding its wall-clock budget is inconclusive (timeout), NEVER \
         coerced to pass or fail (I6, DR-041 Decision 4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "the 50 ms timeout must actually bound the run (kill the child); waited {:?}",
        started.elapsed()
    );
}

/// CRITERION 3 — determinism: the SAME inputs yield the SAME verdict. Two runs of
/// `cargo-test` over the same passing fixture agree on the verdict. `cost_ms`
/// varies (wall-clock) and is deliberately NOT compared — only the verdict, which
/// is the deterministic axis (I6: same content-hashed inputs → same verdict). The
/// no-network leg is structural: the fixture crate has zero dependencies, so
/// `cargo test` neither fetches nor resolves anything off-box.
#[test]
fn same_inputs_same_verdict_determinism() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = passing_crate(root.path());

    let a = verdict_doc(&run_verify(&crate_dir, &[], &[]));
    let b = verdict_doc(&run_verify(&crate_dir, &[], &[]));
    assert_eq!(
        a.verdict, b.verdict,
        "same inputs → same verdict (determinism, I6); cost_ms is NOT compared \
         (wall-clock varies — recorded, not asserted)"
    );
    assert_eq!(a.verdict, Verdict::Pass);
}
