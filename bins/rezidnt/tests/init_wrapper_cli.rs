//! DR-036 sub-slice `init-wrapper` ORACLE — the HOST-runnable contract for
//! `rezidnt init [DIR] [--defaults] [--force]`, the thin wrapper that chains
//! `doctor -> spec init -> open` (DR-036 §Design line 41). Drives the REAL binary
//! (`CARGO_BIN_EXE_rezidnt`) and pins the ORCHESTRATION contract — the doctor gate,
//! the inconclusive-warn-and-proceed posture, the wrapper clobber nuance, and the
//! sub-command DR-004 exit-class PASS-THROUGH — WITHOUT a daemon, by controlling the
//! injectable seams the two prior slices already established. The one criterion that
//! strictly needs a live daemon (the golden-path chain reaching `agent.spawned`)
//! lives in `bins/rezidentd/tests/init_wrapper_e2e.rs` (`#![cfg(unix)]`, WSL).
//!
//! ## The surface this board PINS for the implementer (the smallest honest one)
//!   - Verb: `rezidnt init [DIR]` — a new subcommand of the EXISTING `rezidnt` CLI
//!     (I7, one binary; NOT a new bin). `DIR` is an OPTIONAL positional target
//!     directory (absent = cwd), MIRRORING `spec init`'s positional so the wrapper
//!     writes `<DIR>/rezidnt.toml` and opens it in place.
//!   - `--defaults` — forwarded to the `spec init` step: generate the minimal spec
//!     non-interactively (no prompts). These deterministic tests drive `--defaults`.
//!   - `--force` — forwarded to the `spec init` step: WITH it an existing spec is
//!     regenerated; WITHOUT it the wrapper SKIPS a present spec (leaves it
//!     byte-unchanged) and PROCEEDS to open — the wrapper clobber nuance below.
//!
//! ## The chain and its DR-004 exit-class pass-through (what the wrapper surfaces)
//! `init` runs `doctor` FIRST (gate on fail, warn on inconclusive), then `spec init`
//! (skip a present spec unless `--force`), then `open`. It invents NO new failure
//! code — it surfaces the sub-commands' classes:
//!   - a doctor check that FAILS aborts the chain with doctor's class (exit 3), and
//!     NO `rezidnt.toml` is written and `open` is never reached;
//!   - a doctor check that is INCONCLUSIVE (none failing) is a WARNING, and the
//!     chain PROCEEDS to spec init;
//!   - a daemon-unreachable `open` step surfaces `open`'s class (exit 4);
//!   - a present spec without `--force` does NOT hard-error at `spec init`'s exit 2
//!     (that is the BARE `spec init`'s clobber class) — the wrapper SKIPS and
//!     proceeds to `open`, so the only failure that can appear is the `open` step's
//!     (daemon-unreachable here → 4), never the clobber 2.
//!
//! ## Injectable seams reused (how these tests force each branch deterministically)
//! These are the SAME seams the `spec-init` and `onboarding-doctor` oracles pinned,
//! reused so the wrapper is exercised without a daemon:
//!   - `PATH` (empty / git-only): the doctor `git` check reads `PATH`. An EMPTY
//!     `PATH` forces the git check to FAIL (git unresolvable) — the doctor-gate
//!     lever. A `PATH` that DOES hold git (and NOT the `claude-code` harness) keeps
//!     the git check `pass` while the harness check is `inconclusive` — the
//!     inconclusive-warn-and-proceed lever.
//!   - `REZIDNT_SOCKET` (dead path): the daemon-dial env. Pointed at an
//!     unequivocally-unreachable socket so IF the chain reaches the `open` step it
//!     fails daemon-unreachable (exit 4) — the observable that PROVES `open` was
//!     reached (a wrapper that aborted earlier could never surface 4).
//!   - `REZIDNT_LOCKFILE` (writable temp dir): the doctor socket/lockfile-writable
//!     check reads this; pointed into a writable temp dir so that check does not
//!     itself fail and mask the branch under test.
//!
//! ## RED MODE — no-such-subcommand-red (honesty, mirroring spec_init / doctor)
//! `rezidnt init` did NOT exist when this board was written: `Cmd` carried no `Init`
//! verb, so clap exited with an "unrecognized subcommand" USAGE error (exit 2)
//! BEFORE any wrapper logic ran. Every exit-code assertion here ALSO guards that
//! stderr is NOT clap's `unrecognized subcommand` / `unexpected argument` message
//! (the `assert_not_clap_usage_error` idiom from `doctor_cli.rs` / `spec_init_cli.rs`)
//! — otherwise a bare exit-code check could false-green on clap rejecting an unknown
//! verb rather than on the wrapper's own orchestration. Authoring intent,
//! past-tense-safe: these were written before the `init` verb existed; each assertion
//! states the CONTRACT it pins, so its message stays TRUE once the wrapper is built.

use std::path::Path;
use std::process::Command;

/// An unequivocally-unreachable daemon socket path. IF the wrapper reaches its
/// `open` step, dialing this fails daemon-unreachable (DR-004 exit 4) — that
/// failure is the OBSERVABLE that `open` was reached. Absolute + nonexistent on
/// every platform's filesystem root.
const DEAD_SOCKET: &str = "/nonexistent/rezidnt-init-wrapper-dead.sock";

/// A `PATH` value holding NO real directory, so NO external bin (git, the
/// harness) resolves — the deterministic lever to force the doctor `git` check to
/// FAIL. An empty string is a valid, bin-free `PATH` on every platform.
const EMPTY_PATH: &str = "";

/// The §13 filename the wrapper writes / opens (mirrors `spec init`).
fn generated_spec(dir: &Path) -> std::path::PathBuf {
    dir.join("rezidnt.toml")
}

/// Run `rezidnt init [args…]` with a FULLY CONTROLLED environment (`env_clear`,
/// then only the `(k, v)` pairs given). `env_clear` is the deterministic lever:
/// the doctor `git`/`harness` checks resolve on the `PATH` the test supplies, not
/// the runner's. `REZIDNT_SOCKET` is always pinned to a dead path (unless a caller
/// overrides it) so a reached `open` step fails daemon-unreachable, never touching
/// a live dev daemon. Returns (exit, stdout, stderr).
fn run_init_clean_env(args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("init").args(args);
    cmd.env_clear();
    cmd.env("REZIDNT_SOCKET", DEAD_SOCKET);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt init");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Assert the given stderr is NOT clap's absent-subcommand / bad-arg usage error.
/// The HONEST-RED guard (the exact idiom from `doctor_cli.rs` / `spec_init_cli.rs`):
/// the `init` verb was absent when this was written, so clap exited 2 with this
/// message; without the guard an exit-2/3/4 assertion could false-green on clap
/// rather than on the wrapper's own orchestration. Stays a true invariant once
/// built (the real `init` never emits clap's unrecognized-subcommand text).
fn assert_not_clap_usage_error(stderr: &str) {
    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand") && !lc.contains("unexpected argument"),
        "RED-HONESTY: `rezidnt init` must EXIST and run its own doctor->spec-init->open \
         chain, NOT be clap rejecting an unknown subcommand (both paths can exit non-zero) \
         — stderr: {stderr}"
    );
}

/// Return the directory containing `bin` on the current `PATH`, or None. A pure
/// filesystem walk of `PATH` (no subprocess) — mirrors `doctor_cli.rs::which_dir`,
/// used to build a controlled `PATH` that DOES hold git for the positive branches,
/// and to skip those branches where git is genuinely absent on the runner.
fn which_dir(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat", ".com"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(dir);
            }
        }
    }
    None
}

/// A writable temp dir + a `REZIDNT_LOCKFILE` path inside it, so the doctor
/// socket/lockfile-writable check is satisfiable (does not itself fail and mask the
/// branch under test). Returns (tempdir, lockfile-string). Keep the tempdir alive.
fn satisfiable_lockfile() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let lock = dir.path().join("mcp.lock");
    let lock_s = lock.to_str().expect("utf8 lockfile").to_string();
    (dir, lock_s)
}

// ===========================================================================
// CRITERION 2 — doctor GATING (deterministic via the injectable seams).
// A failing doctor check aborts init at the doctor step (exit 3, NO rezidnt.toml
// written, open never attempted). Paired with a positive (a satisfiable env does
// NOT abort at doctor — it proceeds to generate) so the gate is proven to
// DISCRIMINATE, not always-abort.
// ===========================================================================

/// CRITERION 2 (negative — the gate fires). With an EMPTY `PATH` the doctor `git`
/// check FAILS; `init` must ABORT at the doctor step: exit 3 (doctor's class,
/// surfaced unchanged), NO `rezidnt.toml` in the target dir, and `open` never
/// attempted. `REZIDNT_SOCKET` is a dead path — so IF the wrapper wrongly proceeded
/// to `open` the exit would be 4 (daemon-unreachable), NOT 3; asserting exactly 3
/// (and the absence of the file) proves the abort happened AT doctor, before
/// spec-init and before open.
///
/// Written RED before the `init` verb existed: clap exited 2 (neither 3 nor 4),
/// and the stderr guard confirms the failure was the ABSENT subcommand.
#[test]
fn doctor_fail_aborts_before_spec_and_open() {
    let target = tempfile::tempdir().expect("target tempdir");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", target.path().to_str().unwrap()],
        // EMPTY PATH -> the doctor git check fails. Lockfile writable so ONLY the
        // git check is the forced failure (isolating the gate to git).
        &[("PATH", EMPTY_PATH), ("REZIDNT_LOCKFILE", &lock)],
    );
    assert_not_clap_usage_error(&stderr);

    assert_eq!(
        code,
        Some(3),
        "a FAILING doctor check must abort `init` with doctor's exit class (3), not proceed \
         to open (which against the dead socket would be 4) and not invent a new code; \
         stderr: {stderr}"
    );
    assert!(
        !generated_spec(target.path()).exists(),
        "when doctor gates the chain, `init` must NOT reach the spec-init step — no \
         rezidnt.toml may be written to the target dir (open is never attempted either)"
    );
}

/// CRITERION 2 (positive — the gate does NOT always-abort). With a `PATH` that DOES
/// hold git (doctor git check passes) and a writable lockfile, `init --defaults`
/// must NOT abort at the doctor step — it must PROCEED past doctor to the spec-init
/// step (proven by the `rezidnt.toml` being written) and on toward `open`. Because
/// the socket is dead, the wrapper then fails at the OPEN step (exit 4, criterion 5)
/// — but the load-bearing pin here is that doctor did NOT gate: the spec file exists
/// and the exit is the OPEN class (4), never the doctor class (3). Together with the
/// negative this proves the gate DISCRIMINATES.
///
/// Written RED before the `init` verb existed: clap exited 2 and wrote no file, so
/// both the "spec written" and "exit not 3-from-doctor" pins failed.
#[test]
fn satisfiable_doctor_proceeds_to_spec_init() {
    let Some(git_dir) = which_dir("git") else {
        eprintln!(
            "skipping satisfiable_doctor_proceeds_to_spec_init: no git on this runner's PATH"
        );
        return;
    };
    let target = tempfile::tempdir().expect("target tempdir");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", target.path().to_str().unwrap()],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", &lock),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    assert!(
        generated_spec(target.path()).exists(),
        "with a satisfiable doctor env `init` must PROCEED past the doctor step and RUN \
         spec-init — the generated rezidnt.toml must be written (the gate must not \
         always-abort); stderr: {stderr}"
    );
    // The chain went on to `open` and failed daemon-unreachable — the OPEN class (4),
    // NOT the doctor class (3). Asserting it is not 3 fences off "doctor gated anyway".
    assert_ne!(
        code,
        Some(3),
        "a satisfiable doctor must NOT surface the doctor gate class (3) — the chain \
         proceeded past doctor; the only remaining failure is the dead-socket open step \
         (4). exit: {code:?}; stderr: {stderr}"
    );
}

// ===========================================================================
// CRITERION 3 — inconclusive -> WARN + PROCEED. An environment where a doctor
// check is INCONCLUSIVE (not fail) does NOT abort init — the chain proceeds past
// doctor to the spec-generation step.
// ===========================================================================

/// CRITERION 3 — a doctor check that is INCONCLUSIVE (never a fail) must NOT gate
/// the wrapper. We construct exactly that environment: a `PATH` that HOLDS git (so
/// the `git` check PASSES — no fail) but does NOT hold the `claude-code` harness (so
/// the `harness` check is INCONCLUSIVE, per the `onboarding-doctor` contract:
/// harness-not-on-PATH is inconclusive, never a hard fail). With a writable lockfile
/// and no failing check, `init` must PROCEED past doctor and WRITE the spec (the
/// spec-generation step), rather than abort. The proof is the `rezidnt.toml`
/// existing AND the exit NOT being the doctor class (3): an inconclusive warned and
/// the chain went on (to the dead-socket open, exit 4).
///
/// Isolation note: this test uses a `PATH` of exactly the git directory, chosen
/// because `claude-code` is not a standard tool that dir would also hold — so the
/// harness check is deterministically inconclusive there. If a runner's git dir
/// somehow also held a `claude-code` bin the harness check would pass instead; the
/// assertion (proceeds past doctor, not gated) still holds, since all-pass ALSO
/// proceeds — the branch under test is only "no fail => not gated".
///
/// Written RED before the `init` verb existed: clap exited 2, wrote no file.
#[test]
fn doctor_inconclusive_warns_and_proceeds() {
    let Some(git_dir) = which_dir("git") else {
        eprintln!("skipping doctor_inconclusive_warns_and_proceeds: no git on this runner's PATH");
        return;
    };
    // Guard the premise: the harness bin must NOT be resolvable on this PATH, else
    // the harness check would pass rather than be inconclusive. (It is a git-only
    // dir; `claude-code` living there would be extraordinary — skip if so.)
    if which_dir("claude-code").as_deref() == Some(git_dir.as_path()) {
        eprintln!(
            "skipping doctor_inconclusive_warns_and_proceeds: claude-code resolves in the git \
             dir, so the harness check would pass, not be inconclusive"
        );
        return;
    }

    let target = tempfile::tempdir().expect("target tempdir");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", target.path().to_str().unwrap()],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", &lock),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    assert!(
        generated_spec(target.path()).exists(),
        "an INCONCLUSIVE doctor check (harness not on PATH) is a WARNING, not a gate — \
         `init` must PROCEED to the spec-generation step and write rezidnt.toml; stderr: {stderr}"
    );
    assert_ne!(
        code,
        Some(3),
        "an inconclusive-only doctor must NOT abort with the doctor gate class (3) — the \
         chain proceeded (its later failure, if any, is the dead-socket open step, 4). \
         exit: {code:?}; stderr: {stderr}"
    );
}

// ===========================================================================
// CRITERION 4 — existing-spec SKIP (the wrapper clobber nuance). An existing
// rezidnt.toml with NO --force must NOT hard-error at spec init's exit 2 (unlike
// BARE `spec init`) — the wrapper leaves the file BYTE-UNCHANGED and PROCEEDS to
// the open step. With --force it regenerates. (The proceed-to-open half is proven
// host-side here via the dead socket: getting PAST spec-init toward open surfaces
// the OPEN failure class (4), not the clobber class (2). The daemon-backed spawn
// half is the e2e's job.)
// ===========================================================================

/// A sentinel an operator would recognize if it were clobbered — a VALID §13 spec
/// (so the wrapper's open step accepts it) marked as hand-authored, distinct from
/// anything the generator emits (the `operator-authored` name proves preservation).
const EXISTING_SENTINEL: &str = "# DO-NOT-CLOBBER sentinel spec (operator-authored)\n\
[project]\nname = \"operator-authored\"\nrepo = \".\"\n\n[[agent]]\nname = \"hand\"\n\
harness = \"claude-code\"\nworktree = \"auto\"\n";

/// CRITERION 4 (skip, byte-unchanged, proceeds — NOT the clobber-2). With an
/// existing `rezidnt.toml` and NO `--force`, the WRAPPER must NOT reproduce bare
/// `spec init`'s exit-2 clobber refusal: it SKIPS the present spec (leaving its bytes
/// EXACTLY as they were) and PROCEEDS to `open`. Against the dead socket the open
/// step then fails daemon-unreachable — so the observed exit is 4 (the OPEN class),
/// NOT 2 (the clobber class). The two teeth: (a) the sentinel bytes are UNCHANGED
/// (the wrapper did not regenerate); (b) the exit is NOT 2 — proving it got PAST
/// spec-init toward open, rather than hard-erroring on the present file.
///
/// `PATH` holds git so the doctor step passes (isolating the assertion to the
/// clobber nuance, not a doctor gate). Skipped where the runner has no git.
///
/// Written RED before the `init` verb existed: clap exited 2 with the
/// unrecognized-subcommand message — the stderr guard distinguishes that absent-verb
/// 2 from the (wrong) clobber 2 this test forbids, so the RED is honest.
#[test]
fn existing_spec_without_force_skips_and_proceeds_not_clobber_two() {
    let Some(git_dir) = which_dir("git") else {
        eprintln!(
            "skipping existing_spec_without_force_skips_and_proceeds_not_clobber_two: no git \
             on this runner's PATH"
        );
        return;
    };
    let target = tempfile::tempdir().expect("target tempdir");
    let path = generated_spec(target.path());
    std::fs::write(&path, EXISTING_SENTINEL).expect("write existing spec");
    let before = std::fs::read(&path).expect("read before");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", target.path().to_str().unwrap()],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", &lock),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    let after = std::fs::read(&path).expect("read after");
    assert_eq!(
        before, after,
        "the wrapper must SKIP a present rezidnt.toml (no --force) and leave it \
         BYTE-UNCHANGED — it must not regenerate/truncate the operator's file"
    );
    assert_ne!(
        code,
        Some(2),
        "the WRAPPER must NOT hard-error at bare `spec init`'s clobber class (exit 2) when a \
         spec already exists without --force — it SKIPS and proceeds to open (whose \
         dead-socket failure is 4, proving it got past spec-init). exit: {code:?}; \
         stderr: {stderr}"
    );
}

/// CRITERION 4 (--force regenerates). With `--force`, the wrapper's spec-init step
/// REGENERATES the present spec: the sentinel bytes are REPLACED by a freshly
/// generated §13 spec. We assert the file CHANGED (the sentinel is gone) — the
/// regenerate half of the nuance. The chain then proceeds to `open` (dead socket →
/// the open failure class), so we do not pin a success exit here; the load-bearing
/// pin is that `--force` overwrote (bytes changed, sentinel name gone), unlike the
/// skip branch above.
///
/// `PATH` holds git so doctor passes. Written RED before the `init` verb existed:
/// clap exited 2 and wrote nothing, so the sentinel survived and the "bytes changed"
/// pin failed.
#[test]
fn existing_spec_with_force_regenerates() {
    let Some(git_dir) = which_dir("git") else {
        eprintln!("skipping existing_spec_with_force_regenerates: no git on this runner's PATH");
        return;
    };
    let target = tempfile::tempdir().expect("target tempdir");
    let path = generated_spec(target.path());
    std::fs::write(&path, EXISTING_SENTINEL).expect("write existing spec");
    let before = std::fs::read_to_string(&path).expect("read before");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (_code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", "--force", target.path().to_str().unwrap()],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", &lock),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    let after = std::fs::read_to_string(&path).expect("read after");
    assert_ne!(
        before, after,
        "with --force the wrapper's spec-init step must REGENERATE the present spec — the \
         hand-authored sentinel must be REPLACED (bytes change); stderr: {stderr}"
    );
    assert!(
        !after.contains("operator-authored"),
        "a regenerated spec must not retain the sentinel's operator-authored name — the \
         generator wrote a fresh §13 spec over it"
    );
}

// ===========================================================================
// CRITERION 5 — surfaces sub-command exit classes (no wrapper-invented code).
// Pins the two required pass-throughs: doctor-fail -> 3, and open-daemon-unreachable
// -> 4. (The doctor-fail 3 pass-through is also exercised, from the abort angle, by
// `doctor_fail_aborts_before_spec_and_open`; this states it as the explicit
// exit-class contract. The open-unreachable 4 is the reached-open observable.)
// ===========================================================================

/// CRITERION 5 (doctor-fail surfaces exactly 3). The wrapper introduces no new
/// failure code: a FAILING doctor check surfaces DOCTOR's class — exit 3 — not a
/// wrapper-invented value and not the open class (4). Empty `PATH` forces the git
/// check to fail; the dead socket would give 4 only if the chain wrongly reached
/// open, so asserting exactly 3 pins the pass-through.
///
/// Written RED before the `init` verb existed: clap exited 2; the stderr guard
/// confirms the 2 was the absent verb, not a real surfaced class.
#[test]
fn surfaces_doctor_fail_class_three() {
    let target = tempfile::tempdir().expect("target tempdir");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", target.path().to_str().unwrap()],
        &[("PATH", EMPTY_PATH), ("REZIDNT_LOCKFILE", &lock)],
    );
    assert_not_clap_usage_error(&stderr);
    assert_eq!(
        code,
        Some(3),
        "init must SURFACE doctor's DR-004 class (3) on a failing check — not invent a code, \
         not remap it to open's 4; stderr: {stderr}"
    );
}

/// CRITERION 5 (open-daemon-unreachable surfaces exactly 4). When the chain reaches
/// the `open` step and the daemon is unreachable, the wrapper surfaces OPEN's class
/// — exit 4 (daemon-unreachable) — unchanged. We satisfy doctor (git on PATH,
/// writable lockfile) so the chain reaches open, and point `REZIDNT_SOCKET` at a
/// dead path. The exit must be 4: NOT the doctor class (3, which would mean the gate
/// wrongly fired), NOT the clobber class (2), and NOT a wrapper-invented code. This
/// is the host-observable proof that the chain reached `open`.
///
/// Written RED before the `init` verb existed: clap exited 2; the stderr guard
/// confirms the 2 was the absent verb, so "exit 4" failing is the honest red.
#[test]
fn surfaces_open_daemon_unreachable_class_four() {
    let Some(git_dir) = which_dir("git") else {
        eprintln!(
            "skipping surfaces_open_daemon_unreachable_class_four: no git on this runner's PATH"
        );
        return;
    };
    let target = tempfile::tempdir().expect("target tempdir");
    let (_lockdir, lock) = satisfiable_lockfile();

    let (code, _stdout, stderr) = run_init_clean_env(
        &["--defaults", target.path().to_str().unwrap()],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", &lock),
            // Redundant with run_init_clean_env's default, stated explicitly: the
            // open step dials THIS, and it is unreachable.
            ("REZIDNT_SOCKET", DEAD_SOCKET),
        ],
    );
    assert_not_clap_usage_error(&stderr);
    assert_eq!(
        code,
        Some(4),
        "a daemon-unreachable OPEN step must surface open's DR-004 class (4) unchanged — the \
         wrapper reached open (doctor passed, spec was generated) and adds no new failure \
         semantics. Not 3 (doctor did not gate), not 2 (no clobber), not a wrapper code. \
         exit: {code:?}; stderr: {stderr}"
    );
}
