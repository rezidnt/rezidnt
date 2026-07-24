//! DR-041 slice `dependency-audit` ORACLE — the FOURTH verifier of the
//! production-verifier-pack arc (the fast-follow named in DR-041 Decision 1).
//! Adds ONE real §8 EXEC verifier to the `rezidnt verify <name>` dispatch:
//! `dependency-audit`. Per DR-041 Decision 2 this is an EXEC verifier — it
//! consults an EXTERNAL advisory DB (RustSec, via `cargo audit`), a
//! subprocess-natural external tool — NOT a native in-process computation.
//!
//! Pins DR-041 criteria 1/2/3 at the CLI surface, mirroring
//! `verify_lints_cli.rs` — same `parse_verifier_output` / `VerifierOutput`
//! shape, the same three-valued verdict (never coerced, always interrogable,
//! I6), the same `cost_ms` RECORDED-not-asserted rule.
//!
//! ## THE LOAD-BEARING POSTURE (DR-041 slice-3 instruction, I6)
//!
//! `dependency-audit` consults an external advisory DB. If the DB is UNREACHABLE
//! — or the audit TOOL is simply not installed on the host — the verifier CANNOT
//! reach a verdict: that is `inconclusive` (could-not-run), NEVER a silent
//! `pass` and NEVER a `fail`. A gate that silently passed when it could not check
//! for advisories would be an I6 lie (a coerced inconclusive). These tests pin
//! that the could-not-run condition surfaces as `inconclusive`, never coerced.
//!
//! ## RED MODE (RED at board time, for the RIGHT reason)
//!
//! `rezidnt verify` dispatch EXISTS, but at board time its match arm did not
//! know `dependency-audit`, so it fell through to the `other =>` unknown-verifier
//! fallback — which emits `{"verdict":"inconclusive", … "unknown verifier"}`.
//! Because the tool-absent HAPPY-path verdict is ALSO inconclusive, a bare
//! verdict assert would be VACUOUS against the fallback. So each trap test ALSO
//! asserts the evidence does NOT carry the `unknown verifier` fallback text —
//! i.e. the verifier is genuinely DISPATCHED, not falling through. That assert is
//! RED until the `dependency-audit` match arm lands, and stays load-bearing after
//! (a rename/drop turns it red again).
//!
//! ## TOOL-CAPABILITY GATING (test honesty)
//!
//! A REAL `pass` (clean tree) / `fail` (a known advisory) verdict requires
//! `cargo-audit` on the host AND a reachable advisory DB — neither is guaranteed
//! in CI or on a dev box (indeed it is ABSENT here). Rather than FAKE a verdict,
//! the pass/fail legs are CAPABILITY-GATED: they SKIP (early-return, logging why)
//! when `cargo audit` is not resolvable, so the board is honest about what it can
//! and cannot prove on a given box. The always-runnable legs (dispatch, the
//! could-not-run posture, determinism) carry the slice's core contract; the
//! capability-gated legs sharpen the pass/fail mapping WHERE the tool exists.
//!
//! Cross-platform (no socket; shells to `cargo`) — host `/vet` covers it.

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

/// Scaffold a tiny cargo fixture crate under `root/<name>` with the given
/// `lib.rs` body and `deps` block, returning the crate dir. Kept under the TEST
/// tree (a tempdir), never a production path (oracle house rule).
fn fixture_crate(root: &Path, name: &str, deps: &str, lib_rs: &str) -> PathBuf {
    let crate_dir = root.join(name);
    write(
        &crate_dir.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}"
        ),
    );
    write(&crate_dir.join("src/lib.rs"), lib_rs);
    crate_dir
}

/// A dependency-free crate — nothing for the advisory DB to flag. Used for the
/// clean `pass` leg (capability-gated) and the toolchain-absent trap.
fn clean_crate(root: &Path) -> PathBuf {
    fixture_crate(
        root,
        "verify_audit_clean_fixture",
        "",
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
}

/// Whether `cargo audit` is resolvable on the host (the tool AND the cargo
/// subcommand). When false, the pass/fail legs SKIP rather than fake a verdict.
fn cargo_audit_available() -> bool {
    Command::new("cargo")
        .args(["audit", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `rezidnt verify dependency-audit --dir <dir> --json` with optional extra
/// args and env overrides; capture the raw Output. The document IS the
/// machine-readable stdout.
fn run_verify(dir: &Path, extra: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("verify")
        .arg("dependency-audit")
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
fn verdict_doc(out: &std::process::Output) -> VerifierOutput {
    parse_verifier_output(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "`rezidnt verify dependency-audit --json` must print a §8 VerifierOutput \
             document on stdout that parse_verifier_output accepts ({e}); \
             stdout={:?} stderr={:?} status={:?}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            out.status.code(),
        )
    })
}

/// Assert the verifier was genuinely DISPATCHED (not the unknown-verifier
/// fallback). RED until the match arm lands; keeps the trap tests honest.
fn assert_dispatched(doc: &VerifierOutput) {
    assert!(
        !doc.evidence
            .iter()
            .any(|e| e.msg.contains("unknown verifier")),
        "`rezidnt verify dependency-audit` must be a DISPATCHED verifier, not the \
         unknown-verifier fallback — a trap verdict from the fallback tests nothing \
         (RED-until-dispatched); evidence={:?}",
        doc.evidence
    );
}

// ===========================================================================
// DISPATCH SENTINEL (criterion 1 preface) — the verb answers `--help` with exit
// 0 (clap's help path). Guards against a future rename of the verifier name.
// ===========================================================================

#[test]
fn dependency_audit_dispatch_help_exists() {
    let out = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .args(["verify", "dependency-audit", "--help"])
        .output()
        .expect("run rezidnt verify --help");
    assert_eq!(
        out.status.code(),
        Some(0),
        "`rezidnt verify dependency-audit --help` must exit 0 (registered subcommand); \
         stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

// ===========================================================================
// THE CORE POSTURE (always-runnable) — could-not-run → inconclusive, never
// coerced to a silent pass. This is the slice's load-bearing contract (I6).
// ===========================================================================

/// CRITERION 1 — the subcommand emits a valid §8 verdict document. Even on a box
/// WITHOUT `cargo-audit`, the verifier emits a well-formed `VerifierOutput` (the
/// could-not-run verdict rides the document, not an error exit) — the SAME shape
/// the exec runner reads back.
#[test]
fn dependency_audit_emits_a_valid_section8_verdict_document() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(&clean_crate(root.path()), &[], &[]);
    let doc = verdict_doc(&out);
    assert_dispatched(&doc);
    // cost_ms RECORDED, value NOT asserted (wall-clock varies).
    let _cost_is_recorded_not_asserted: u64 = doc.cost_ms;
}

/// THE POSTURE (trap → inconclusive, could-not-run, NEVER a silent pass): the
/// audit toolchain ABSENT from PATH maps to `inconclusive`, NOT `pass` and NOT
/// `fail`. Forced with an EMPTY PATH so `cargo`/`cargo-audit` cannot be resolved —
/// the "DB unreachable / tool absent" case DR-041 slice-3 pins. A gate that
/// silently passed here would be an I6 lie (a coerced inconclusive).
#[test]
fn dependency_audit_toolchain_absent_is_inconclusive_never_silent_pass() {
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(&clean_crate(root.path()), &[], &[("PATH", "")]);
    let doc = verdict_doc(&out);

    assert_dispatched(&doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "the audit toolchain absent from PATH is could-not-run → inconclusive, NEVER a \
         silent pass and never a fail (I6, DR-041 Decision 3/4, slice-3 no-network \
         posture); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        doc.verdict,
        Verdict::Pass,
        "an unreachable advisory DB / absent tool must NOT be coerced to a silent pass"
    );
    assert_ne!(doc.verdict, Verdict::Fail, "could-not-run is not a fail");
}

/// A wall-clock TIMEOUT maps to `inconclusive` (timeout), NEVER coerced. Forced
/// with a `1 ms` budget; the run must return in bounded time (the timeout kills
/// the child). Only meaningful where the tool actually spawns — capability-gated.
#[test]
fn dependency_audit_timeout_is_inconclusive_never_coerced() {
    if !cargo_audit_available() {
        eprintln!(
            "SKIP dependency_audit_timeout: `cargo audit` is not resolvable on this host — \
             the timeout leg needs the tool to spawn (see board TOOL-CAPABILITY GATING)"
        );
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = clean_crate(root.path());

    let started = Instant::now();
    let out = run_verify(&crate_dir, &["--timeout-ms", "1"], &[]);
    let doc = verdict_doc(&out);

    assert_dispatched(&doc);
    assert_eq!(
        doc.verdict,
        Verdict::Inconclusive,
        "a run exceeding its wall-clock budget is inconclusive (timeout), NEVER coerced \
         (I6, DR-041 Decision 4); stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "the 1 ms timeout must actually bound the run (kill the child); waited {:?}",
        started.elapsed()
    );
}

/// CRITERION 3 — determinism: SAME inputs → SAME verdict. Two runs over the same
/// fixture agree (whether the shared verdict is a real pass, a real fail, or the
/// could-not-run inconclusive on a tool-less box). `cost_ms` varies and is NOT
/// compared.
#[test]
fn dependency_audit_same_inputs_same_verdict_determinism() {
    let root = tempfile::tempdir().expect("tempdir");
    let crate_dir = clean_crate(root.path());

    let a = verdict_doc(&run_verify(&crate_dir, &[], &[]));
    let b = verdict_doc(&run_verify(&crate_dir, &[], &[]));
    assert_dispatched(&a);
    assert_eq!(
        a.verdict, b.verdict,
        "same inputs → same verdict (determinism, I6); cost_ms is NOT compared"
    );
}

// ===========================================================================
// PASS / FAIL MAPPING (capability-gated — needs `cargo audit` on the host)
// ===========================================================================

/// CRITERION 2 (trap A → pass): a dependency-free crate has nothing for the
/// advisory DB to flag → `pass`, never coerced. Capability-gated: needs the tool
/// AND a reachable DB, else SKIP (honest — no fake verdict).
#[test]
fn dependency_audit_clean_tree_maps_to_pass() {
    if !cargo_audit_available() {
        eprintln!(
            "SKIP dependency_audit_clean_tree_maps_to_pass: `cargo audit` is not resolvable \
             on this host (see board TOOL-CAPABILITY GATING) — the clean-pass leg needs the \
             tool AND a reachable advisory DB"
        );
        return;
    }
    let root = tempfile::tempdir().expect("tempdir");
    let out = run_verify(&clean_crate(root.path()), &[], &[]);
    let doc = verdict_doc(&out);
    assert_dispatched(&doc);

    // The tool ran. A dependency-free crate is either a clean pass, OR — if THIS
    // box could not reach the advisory DB — an honest could-not-run inconclusive.
    // It must NEVER be a fail (there are no dependencies to have an advisory).
    assert_ne!(
        doc.verdict,
        Verdict::Fail,
        "a dependency-free crate has nothing to flag — never a fail; stderr={:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        matches!(doc.verdict, Verdict::Pass | Verdict::Inconclusive),
        "a clean tree is a pass, or (if the DB was unreachable) an honest \
         inconclusive — never coerced; got {:?}",
        doc.verdict
    );
}
