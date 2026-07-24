//! DR-041 slice `verify-lints` ORACLE — the SECOND slice of the
//! production-verifier-pack arc. Adds TWO real §8 exec verifiers to the
//! ALREADY-SHIPPED `rezidnt verify <name>` dispatch (verify-subcommand landed,
//! commit 79382f6): `clippy` (lint) and `fmt-check` (formatting). Pins DR-041
//! criteria 1/2/3 for BOTH names at the CLI surface, exactly mirroring
//! `verify_cargo_test_cli.rs` — same `parse_verifier_output` / `VerifierOutput`
//! shape, the same three inconclusive traps (never coerced, always
//! interrogable, I6), the same `cost_ms` RECORDED-not-asserted rule.
//!
//! This board drives the REAL `rezidnt` binary (`CARGO_BIN_EXE_rezidnt`) and
//! pins the CONTRACT — the emitted document is the SAME shape
//! `parse_verifier_output` already accepts (grounded against
//! `crates/rezidnt-gate/src/lib.rs`), NOT a new document shape. Determinism
//! comes from CONTROLLING THE INPUTS: each fixture is a tiny, dependency-free
//! cargo crate written into the test tempdir (clean / lint-tripping /
//! mis-formatted / type-error / syntax-error variants), so `cargo clippy` /
//! `cargo fmt --check` neither fetch nor resolve anything off-box (no network,
//! criterion 3).
//!
//! ## RED MODE (RED at board time, for the RIGHT reason — verified empirically)
//!
//! `rezidnt verify` dispatch EXISTS (verify-subcommand shipped), but its match
//! arm only knows `cargo-test`. Today `rezidnt verify clippy` and
//! `rezidnt verify fmt-check` fall through to the `other =>` fallback arm in
//! `bins/rezidnt/src/main.rs::verify`, which emits — VERIFIED by running it —
//!
//!   {"verdict":"inconclusive",
//!    "evidence":[{"kind":"cannot-run",
//!                 "msg":"unknown verifier `clippy` (v1 pack: cargo-test)"}],
//!    "cost_ms":0}
//!
//! and exits 0. So the DOCUMENT SHAPE already parses (right shape today) — the
//! RED lever is the VERDICT MAPPING:
//! - a CLEAN crate must map to `pass`, but today maps to `inconclusive`
//!   (unknown-verifier fallback) → the `assert_eq!(verdict, Pass)` tests fail.
//! - a lint / mis-format must map to `fail`, but today maps to `inconclusive`
//!   → the `assert_eq!(verdict, Fail)` tests fail.
//! - the toolchain-absent and timeout traps ALREADY read `inconclusive` today
//!   (the fallback), so a bare verdict assert on them would be VACUOUS. Each of
//!   those tests therefore ALSO asserts the evidence does NOT carry the
//!   `unknown verifier` fallback text — i.e. the verifier is genuinely
//!   DISPATCHED, not falling through. That extra assert is RED today (the
//!   fallback msg IS present) and stays load-bearing after the slice ships.
//!
//! The DISPATCH SENTINELS below assert each verb answers `--help` with exit 0
//! (clap's help path). `verify` IS a registered subcommand today, so those pass
//! now — they are the flipped-positive form guarding against a future rename of
//! the `clippy` / `fmt-check` names, exactly like `verify_cargo_test_cli.rs`'s
//! `verify_subcommand_dispatch_exists`.
//!
//! Unix-only shape is NOT required here (the S4/cargo-test precedent): this
//! board needs no socket and only shells to `cargo`, so it stays cross-platform
//! (host `/vet` covers it; `cargo`/`clippy`/`rustfmt` are host prerequisites the
//! golden path already assumes).
//!
//! ## WIRING NOTES for the implementer (grounded in the real toolchain)
//!
//! The seam is the SAME one cargo-test uses — a new match arm per name in
//! `verify()` (`"clippy" => verify_clippy(...)`, `"fmt-check" =>
//! verify_fmt_check(...)`), each returning a `VerifierOutput`, each mapping the
//! three traps under DR-041 Decision 4. Verified facts the implementer needs:
//!
//! - CLIPPY names the lint. `cargo clippy --message-format=json` carries
//!   `clippy::needless_return` verbatim in its diagnostic JSON; the human output
//!   also carries `#[warn(clippy::needless_return)]`. Either source lets the
//!   `fail` evidence NAME the lint (interrogability). Clippy's default lints are
//!   WARNINGS (exit 0) — "clippy found a lint" is a `fail` DECISION the verifier
//!   makes from the presence of a warning/error diagnostic, NOT from the exit
//!   code. A crate that does not COMPILE (or `cargo`/`clippy` absent) →
//!   inconclusive(could-not-run), like cargo-test.
//!
//! - FMT-CHECK works at the SYNTAX layer. `cargo fmt --check` (or `rustfmt
//!   --check`) exits 1 for BOTH a mis-formatted file AND a genuine syntax error
//!   — the exit code ALONE cannot tell them apart. The DISCRIMINATOR is the
//!   output: a mis-format prints `Diff in <path>:` on stdout (→ `fail`, NAME the
//!   file); a parse failure prints `error: this file contains an unclosed
//!   delimiter` (or similar `error:`) on stderr (→ inconclusive(could-not-run)).
//!   A crate that is well-formatted but does NOT TYPE-CHECK (a semantic/type
//!   error, no syntax error) is a real fmt `pass` — rustfmt does not type-check.
//!   Do NOT copy cargo-test's "any compile error → inconclusive" rule onto
//!   fmt-check (the `fmt_check_type_error_is_a_real_pass_not_inconclusive` test
//!   below exists precisely to forbid that lazy copy). Toolchain absent
//!   (`cargo`/`rustfmt`) → inconclusive(could-not-run).

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
/// given `lib.rs` body and return the crate dir. NO dependencies — clippy / fmt
/// on it neither fetch nor resolve anything external (determinism + no-network,
/// criterion 3). Kept under the TEST tree (a tempdir), never a production
/// `bins/`/`crates/` path (oracle house rule).
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

// --- fixtures -------------------------------------------------------------

/// A crate that is CLEAN under clippy AND rustfmt: no lints, correct formatting.
/// (`add` is a plain function with no clippy findings; the file is rustfmt-clean.)
fn clean_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_lints_clean_fixture",
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
}

/// A crate that COMPILES and is rustfmt-clean but trips a stable, default-on
/// clippy lint: `clippy::needless_return` (a needless `return` statement). The
/// lint NAME must reach the §8 evidence so `gate why` can say WHICH lint fired
/// (criterion 2 interrogability). rustfmt-clean so it is NOT also an fmt fixture.
fn clippy_lint_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_lints_clippy_fixture",
        "pub fn f(x: i32) -> i32 {\n    return x + 1;\n}\n",
    )
}

/// A crate whose src/lib.rs is deliberately MIS-FORMATTED (no spacing, all on one
/// line) but syntactically valid and type-correct. `cargo fmt --check` reports a
/// diff naming the file → `fail`, and the offending FILE name must reach the
/// evidence (criterion 2 interrogability for fmt-check).
fn misformatted_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_lints_fmt_fixture",
        "pub fn f()->i32{let x=1;x+2}\n",
    )
}

/// A crate that is WELL-FORMATTED and syntactically valid but does NOT TYPE-CHECK
/// (a semantic/type error: `let x: u32 = "not a number";`). This is the fmt-check
/// DISTINCTION fixture: rustfmt works at the SYNTAX layer, so this is a real fmt
/// `pass` — NOT inconclusive (unlike clippy/cargo-test, which need a compiling
/// crate). The body below is already rustfmt-canonical, so `cargo fmt --check`
/// finds no diff.
fn type_error_but_formatted_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_lints_typeerr_fixture",
        "pub fn broken() -> u32 {\n    let x: u32 = \"not a number\";\n    x\n}\n",
    )
}

/// A crate with a GENUINE SYNTAX error (an unclosed delimiter) that rustfmt
/// CANNOT PARSE. This is could-not-run → inconclusive for fmt-check (the ONLY
/// fmt inconclusive-by-content case), distinct from the mis-format `fail` above.
fn syntax_error_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_lints_syntaxerr_fixture",
        // `pub fn broken( -> u32 { let x =` — an unclosed `(`, no closing brace:
        // rustfmt reports "this file contains an unclosed delimiter".
        "pub fn broken( -> u32 { let x =\n",
    )
}

/// A crate that does NOT COMPILE (a hard type error, but here used for CLIPPY:
/// clippy needs a compiling crate, so a build break is could-not-run →
/// inconclusive, NOT a lint `fail`). Same body as the cargo-test compile-error
/// fixture, reused for the clippy leg.
fn compile_error_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_lints_compile_error_fixture",
        "pub fn broken() -> u32 { let x: u32 = \"not a number\"; x }\n",
    )
}

// --- run + parse helpers --------------------------------------------------

/// Run `rezidnt verify <name> --dir <dir> --json` with optional extra args and
/// env overrides; capture the raw Output. The document IS the machine-readable
/// stdout (the `--json` flag mirrors every other verb).
fn run_verify(
    name: &str,
    dir: &Path,
    extra: &[&str],
    envs: &[(&str, &str)],
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("verify")
        .arg(name)
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
/// uses (criterion 1: the shape the runner already accepts, never a new one).
fn verdict_doc(name: &str, out: &std::process::Output) -> VerifierOutput {
    parse_verifier_output(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "`rezidnt verify {name} --json` must print a §8 VerifierOutput document \
             on stdout that parse_verifier_output accepts ({e}); \
             stdout={:?} stderr={:?} status={:?}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            out.status.code(),
        )
    })
}

/// The board-time fallback text a NOT-YET-DISPATCHED verifier name emits (see the
/// RED MODE header). Any evidence carrying this proves the name fell through to
/// the `other =>` arm rather than being genuinely dispatched — so the
/// dispatched-trap tests (toolchain-absent, timeout) assert its ABSENCE, which is
/// RED today and load-bearing after.
fn is_unknown_verifier_fallback(doc: &VerifierOutput) -> bool {
    doc.evidence
        .iter()
        .any(|e| e.msg.contains("unknown verifier"))
}

/// Assert the verifier was genuinely DISPATCHED (not the unknown-verifier
/// fallback). RED today; the discriminator that keeps the trap tests honest.
fn assert_dispatched(name: &str, doc: &VerifierOutput) {
    assert!(
        !is_unknown_verifier_fallback(doc),
        "`rezidnt verify {name}` must be a DISPATCHED verifier, not the \
         unknown-verifier fallback — a trap verdict that came from the fallback \
         tests nothing (RED-until-dispatched); evidence={:?}",
        doc.evidence
    );
}

// ===========================================================================
// DISPATCH SENTINELS (criterion 1 preface) — each verb name answers `--help`
// with exit 0 (clap's help path). `verify` dispatch already exists, so these
// pass today; they are the flipped-positive guard against a future rename of
// the `clippy` / `fmt-check` verifier names.
// ===========================================================================

fn assert_verify_help_exit0(name: &str) {
    let out = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .args(["verify", name, "--help"])
        .output()
        .expect("run rezidnt verify --help");
    assert_eq!(
        out.status.code(),
        Some(0),
        "`rezidnt verify {name} --help` must exit 0 (registered subcommand); \
         stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn verify_clippy_dispatch_help_exists() {
    assert_verify_help_exit0("clippy");
}

#[test]
fn verify_fmt_check_dispatch_help_exists() {
    assert_verify_help_exit0("fmt-check");
}

// ===========================================================================
// CLIPPY
// ===========================================================================

/// CRITERION 1 (clippy) — the subcommand emits a valid §8 verdict document. A
/// clean crate yields a well-formed `VerifierOutput`: verdict `pass`, an
/// `evidence` array, and a `cost_ms` u64. Same shape the exec runner reads back.
#[test]
fn clippy_emits_a_valid_section8_verdict_document_on_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("clippy", &clean_crate(root.path()), &[], &[]);
    let doc = verdict_doc("clippy", &out);

    assert_eq!(
        doc.verdict,
        Verdict::Pass,
        "a clippy-clean crate verifies pass; stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    // cost_ms RECORDED, value NOT asserted (wall-clock varies — mirrors
    // golden_path.rs / verify_cargo_test_cli.rs). Presence is the parse
    // succeeding; this line documents the deliberate non-assertion.
    let _cost_is_recorded_not_asserted: u64 = doc.cost_ms;
}

/// CRITERION 2 (clippy, trap A → pass): a clean crate maps to `pass`, never
/// coerced away.
#[test]
fn clippy_clean_crate_maps_to_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("clippy", &clean_crate(root.path()), &[], &[]);
    assert_eq!(verdict_doc("clippy", &out).verdict, Verdict::Pass);
}

/// CRITERION 2 (clippy, trap B → fail, INTERROGABLE): a crate that trips a
/// default-on clippy lint maps to `fail`, and the evidence NAMES the lint
/// (`clippy::needless_return`) — a bare boolean fails this. `gate why` must be
/// able to say WHICH lint fired (I6 interrogability, DR-041 Decision 4).
#[test]
fn clippy_lint_maps_to_fail_and_names_the_lint() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("clippy", &clippy_lint_crate(root.path()), &[], &[]);
    let doc = verdict_doc("clippy", &out);

    assert_eq!(
        doc.verdict,
        Verdict::Fail,
        "a compiling crate with a clippy lint verifies fail (the tool ran and \
         reported a real defect); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let named = doc
        .evidence
        .iter()
        .any(|e| e.msg.contains("clippy::needless_return"));
    assert!(
        named,
        "the fail verdict must NAME the lint (`clippy::needless_return`) in its \
         evidence (interrogability, I6) — a bare boolean is not enough; evidence={:?}",
        doc.evidence
    );
}

/// CRITERION 2 (clippy, trap C → inconclusive, could-not-run): a COMPILE ERROR
/// maps to `inconclusive`, NOT `fail`. Clippy needs a compiling crate; a build
/// break means no lint pass ran (DR-041 Decision 4 — a compile error is not a
/// lint failure).
#[test]
fn clippy_compile_error_maps_to_inconclusive_not_fail() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("clippy", &compile_error_crate(root.path()), &[], &[]);
    let doc = verdict_doc("clippy", &out);

    assert_dispatched("clippy", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a crate that does not COMPILE is could-not-run → inconclusive, NEVER fail \
         (I6, DR-041 Decision 4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        doc.verdict,
        Verdict::Fail,
        "a compile error must not be coerced to a lint failure"
    );
}

/// CRITERION 2 (clippy, trap C' → inconclusive, cannot-run): `cargo`/`clippy`
/// ABSENT from PATH maps to `inconclusive` (could-not-run), NOT `fail` and NOT a
/// crash. Forced with an EMPTY PATH so the toolchain cannot be resolved. The
/// cannot-run condition is a VERDICT on stdout, not an error exit.
///
/// NOTE: today this reads inconclusive via the unknown-verifier fallback, so the
/// `assert_dispatched` guard is the RED lever here (the verifier is not yet
/// dispatched); the verdict assert stays meaningful once it is.
#[test]
fn clippy_toolchain_absent_maps_to_inconclusive_not_fail() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("clippy", &clean_crate(root.path()), &[], &[("PATH", "")]);
    let doc = verdict_doc("clippy", &out);

    assert_dispatched("clippy", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "cargo/clippy absent from PATH is cannot-run → inconclusive, NEVER fail and \
         never a silent pass (I6, DR-041 Decision 3/4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(doc.verdict, Verdict::Pass, "absent toolchain is not a pass");
    assert_ne!(doc.verdict, Verdict::Fail, "absent toolchain is not a fail");
}

/// CRITERION 2 (clippy, trap D → inconclusive, timeout): a wall-clock TIMEOUT
/// maps to `inconclusive` (timeout), NEVER coerced. Forced with a `1 ms`
/// `--timeout-ms` against the clean crate — a budget no real `cargo clippy`
/// (spawn + compile + lint) can beat, so the timeout trap fires DETERMINISTICALLY
/// (a warm-cache clippy can finish under 50 ms, so a tight-but-nonzero 1 ms is the
/// robust forced-timeout, matching the sibling fmt-check timeout test). The run
/// must ALSO return in bounded time — the timeout actually kills the child.
#[test]
fn clippy_timeout_maps_to_inconclusive_never_coerced() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = clean_crate(root.path());

    let started = Instant::now();
    let out = run_verify("clippy", &crate_dir, &["--timeout-ms", "1"], &[]);
    let doc = verdict_doc("clippy", &out);

    assert_dispatched("clippy", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a run exceeding its wall-clock budget is inconclusive (timeout), NEVER \
         coerced to pass or fail (I6, DR-041 Decision 4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "the 1 ms timeout must actually bound the run (kill the child); waited {:?}",
        started.elapsed()
    );
}

/// CRITERION 3 (clippy) — determinism: SAME inputs → SAME verdict. Two runs over
/// the same clean fixture agree. `cost_ms` varies (wall-clock) and is NOT
/// compared. No-network is structural (the fixture has zero dependencies).
#[test]
fn clippy_same_inputs_same_verdict_determinism() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = clean_crate(root.path());

    let a = verdict_doc("clippy", &run_verify("clippy", &crate_dir, &[], &[]));
    let b = verdict_doc("clippy", &run_verify("clippy", &crate_dir, &[], &[]));
    assert_eq!(
        a.verdict, b.verdict,
        "same inputs → same verdict (determinism, I6); cost_ms is NOT compared"
    );
    assert_eq!(a.verdict, Verdict::Pass);
}

// ===========================================================================
// FMT-CHECK
// ===========================================================================

/// CRITERION 1 (fmt-check) — a valid §8 verdict document on a well-formatted
/// crate: verdict `pass`, `evidence` array, `cost_ms` u64.
#[test]
fn fmt_check_emits_a_valid_section8_verdict_document_on_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("fmt-check", &clean_crate(root.path()), &[], &[]);
    let doc = verdict_doc("fmt-check", &out);

    assert_eq!(
        doc.verdict,
        Verdict::Pass,
        "a rustfmt-clean crate verifies pass; stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _cost_is_recorded_not_asserted: u64 = doc.cost_ms;
}

/// CRITERION 2 (fmt-check, trap A → pass): a correctly-formatted crate maps to
/// `pass`, never coerced away.
#[test]
fn fmt_check_formatted_crate_maps_to_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("fmt-check", &clean_crate(root.path()), &[], &[]);
    assert_eq!(verdict_doc("fmt-check", &out).verdict, Verdict::Pass);
}

/// CRITERION 2 (fmt-check, trap B → fail, INTERROGABLE): a mis-formatted file
/// maps to `fail`, and the evidence NAMES the offending file (`lib.rs`). `gate
/// why` must be able to say WHICH file is mis-formatted (I6 interrogability).
#[test]
fn fmt_check_misformatted_maps_to_fail_and_names_the_file() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("fmt-check", &misformatted_crate(root.path()), &[], &[]);
    let doc = verdict_doc("fmt-check", &out);

    assert_eq!(
        doc.verdict,
        Verdict::Fail,
        "a mis-formatted crate verifies fail (rustfmt ran and reported a real \
         formatting defect); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The offending FILE must be named. We pin the file stem `lib.rs` (the path
    // rendering — absolute vs relative — is the implementer's choice; the NAME
    // is the interrogability contract).
    let named = doc.evidence.iter().any(|e| e.msg.contains("lib.rs"));
    assert!(
        named,
        "the fail verdict must NAME the mis-formatted file (`lib.rs`) in its \
         evidence (interrogability, I6) — a bare boolean is not enough; evidence={:?}",
        doc.evidence
    );
}

/// CRITERION 2 (fmt-check, THE DISTINCTION → real pass, NOT inconclusive): a
/// crate that is well-formatted and syntactically valid but does NOT TYPE-CHECK
/// (a semantic/type error, no syntax error) yields a real fmt `pass` — rustfmt
/// works at the SYNTAX layer and does not type-check. This is the explicit test
/// that FORBIDS lazily copying cargo-test's "any compile error → inconclusive"
/// rule onto fmt-check (DR-041 slice scope; the load-bearing distinction).
#[test]
fn fmt_check_type_error_is_a_real_pass_not_inconclusive() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(
        "fmt-check",
        &type_error_but_formatted_crate(root.path()),
        &[],
        &[],
    );
    let doc = verdict_doc("fmt-check", &out);

    assert_dispatched("fmt-check", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Pass,
        "a well-formatted, syntactically-valid crate that fails to TYPE-CHECK is a \
         real fmt PASS — rustfmt does not type-check, so a type error is NOT a fmt \
         inconclusive (DR-041 slice distinction); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        doc.verdict,
        Verdict::Inconclusive,
        "fmt-check must NOT coerce a semantic/type error into inconclusive — only a \
         genuine SYNTAX error (rustfmt cannot parse) is could-not-run"
    );
}

/// CRITERION 2 (fmt-check, trap C → inconclusive, could-not-run): a GENUINE
/// SYNTAX error (rustfmt cannot parse) maps to `inconclusive`, NOT `fail` and NOT
/// `pass`. This is the ONLY fmt inconclusive-by-content case (the syntax layer):
/// distinct from a mis-format `fail`, distinct from a type-error `pass`.
#[test]
fn fmt_check_syntax_error_maps_to_inconclusive_not_fail() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("fmt-check", &syntax_error_crate(root.path()), &[], &[]);
    let doc = verdict_doc("fmt-check", &out);

    assert_dispatched("fmt-check", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a crate rustfmt CANNOT PARSE (genuine syntax error) is could-not-run → \
         inconclusive, NEVER fail (I6, DR-041 Decision 4 — a parse failure is not a \
         formatting defect); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        doc.verdict,
        Verdict::Fail,
        "a syntax error is not a fmt fail"
    );
    assert_ne!(
        doc.verdict,
        Verdict::Pass,
        "a syntax error is not a fmt pass"
    );
}

/// CRITERION 2 (fmt-check, trap C' → inconclusive, cannot-run): the toolchain
/// (`cargo`/`rustfmt`) ABSENT from PATH maps to `inconclusive` (could-not-run),
/// NOT `fail` and NOT a crash. Forced with an EMPTY PATH. RED lever today is the
/// `assert_dispatched` guard (not yet dispatched).
#[test]
fn fmt_check_toolchain_absent_maps_to_inconclusive_not_fail() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify("fmt-check", &clean_crate(root.path()), &[], &[("PATH", "")]);
    let doc = verdict_doc("fmt-check", &out);

    assert_dispatched("fmt-check", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "cargo/rustfmt absent from PATH is cannot-run → inconclusive, NEVER fail and \
         never a silent pass (I6, DR-041 Decision 3/4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(doc.verdict, Verdict::Pass, "absent toolchain is not a pass");
    assert_ne!(doc.verdict, Verdict::Fail, "absent toolchain is not a fail");
}

/// CRITERION 2 (fmt-check, trap D → inconclusive, timeout): a wall-clock TIMEOUT
/// maps to `inconclusive` (timeout), NEVER coerced. Forced with a tiny
/// `--timeout-ms`; the run must return in bounded time (the timeout kills the
/// child).
#[test]
fn fmt_check_timeout_maps_to_inconclusive_never_coerced() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = clean_crate(root.path());

    let started = Instant::now();
    let out = run_verify("fmt-check", &crate_dir, &["--timeout-ms", "1"], &[]);
    let doc = verdict_doc("fmt-check", &out);

    assert_dispatched("fmt-check", &doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a run exceeding its wall-clock budget is inconclusive (timeout), NEVER \
         coerced (I6, DR-041 Decision 4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "the timeout must actually bound the run (kill the child); waited {:?}",
        started.elapsed()
    );
}

/// CRITERION 3 (fmt-check) — determinism: SAME inputs → SAME verdict. Two runs
/// over the same mis-formatted fixture agree on `fail`. `cost_ms` varies and is
/// NOT compared. No-network is structural (zero dependencies).
#[test]
fn fmt_check_same_inputs_same_verdict_determinism() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = misformatted_crate(root.path());

    let a = verdict_doc("fmt-check", &run_verify("fmt-check", &crate_dir, &[], &[]));
    let b = verdict_doc("fmt-check", &run_verify("fmt-check", &crate_dir, &[], &[]));
    assert_eq!(
        a.verdict, b.verdict,
        "same inputs → same verdict (determinism, I6); cost_ms is NOT compared"
    );
    assert_eq!(a.verdict, Verdict::Fail);
}
