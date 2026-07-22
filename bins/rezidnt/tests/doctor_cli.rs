//! DR-036 sub-slice `onboarding-doctor` ORACLE — the HOST-runnable environment-
//! preflight contract for `rezidnt doctor` (criteria 1, 2, 3). Drives the REAL
//! binary (`CARGO_BIN_EXE_rezidnt`) and pins the CONTRACT — the report shape, the
//! per-check pass/inconclusive honesty (I6), the DR-004 exit-code classes, and the
//! read-only / fact-free / no-daemon posture (I3/I7) — NOT a specific machine's
//! environment. Determinism comes from CONTROLLING THE INPUTS through env/path
//! seams the implementer must honor (below), so no test passes only on one box.
//!
//! Cross-platform on purpose (no socket, no `#![cfg(unix)]`): host `/vet` covers
//! these. The one check whose *positive* is inherently unix (a UDS-path-writable
//! probe) is fenced into `doctor_socket_unix.rs`; here the socket/lockfile-writable
//! seam is exercised only through the cross-platform PATH-directory-writable form
//! (a writable temp dir → pass; a non-existent parent → non-pass), which holds on
//! host and WSL alike.
//!
//! ## The surface this board PINS for the implementer (the smallest honest one)
//!
//! Verb `rezidnt doctor` is a new subcommand of the EXISTING `rezidnt` CLI (I7,
//! one binary; NOT a new bin): a READ-ONLY environment preflight over the §11
//! golden-path assumptions. It prints per-check findings, emits NO fabric fact,
//! opens NO socket, makes NO network call, and needs NO running daemon (DR-036
//! §Design line 40; I3/I7).
//!
//! `--json` is a machine-readable findings mode (every other verb carries
//! `--json`: Vet/Debrief/Gate). The JSON is an object with a `checks` array; each
//! element is `{ "name": <str>, "status": "pass"|"inconclusive"|"fail" }` (a
//! `detail`/message field is the implementer's to add — not pinned). The human
//! (non-`--json`) mode prints one findings line per check; these tests read STATUS
//! from `--json` so they never brittle-match prose.
//!
//! ## The check set (derived from §11 topology + DR-036 §Design line 40)
//!
//! §11 (Cross-platform topology, line 252) names the golden-path substrate
//! assumptions: the daemon+substrates run in WSL2; git worktrees are a substrate;
//! agents (the chosen harness) run under capture; §11 line 240 / §9 name the
//! daemon socket/lockfile transport path. DR-036 §Design line 40 pins the `doctor`
//! set explicitly: "WSL2 reachable, git present, the chosen harness resolvable, the
//! daemon socket/lockfile path writable". The pinned check names (matched
//! case-insensitively, as substrings, so the implementer keeps naming latitude)
//! are: `git` (git present/resolvable on PATH, §11 line 252 worktrees); `harness`
//! (the chosen agent harness resolvable on PATH, §11 agents); `socket`/`lockfile`
//! (the daemon socket/lockfile path is WRITABLE, §11 line 240); and `wsl` (WSL2
//! reachable, §11 line 252 — inherently environment-dependent, so the honest
//! inconclusive-capable check). This board REQUIRES the `git` and
//! socket/lockfile-writable checks by name (the deterministically forceable pair).
//! The `harness`/`wsl` checks are named here but only their HONESTY (never coerced
//! to pass) is asserted generically, so the implementer's exact naming/count is
//! not over-constrained.
//!
//! ## Injectable seams the implementer MUST honor (how the tests force pass/fail)
//!
//! `PATH`: the git-present and harness-resolvable checks resolve their bins on
//! `PATH`. A `PATH` that LACKS `git` forces the git check non-pass; a `PATH` that
//! lacks the harness bin forces the harness check non-pass. (These tests force the
//! git check by emptying PATH.)
//!
//! `REZIDNT_SOCKET` / `REZIDNT_LOCKFILE`: the socket/lockfile-writable check reads
//! these EXISTING env vars (defined in `bins/rezidnt/src/main.rs` — `lockfile_path()`
//! already honors `REZIDNT_LOCKFILE`; the daemon honors `REZIDNT_SOCKET`). Point
//! them INTO a writable temp dir and the writable check PASSES; point them at a
//! path whose PARENT does not exist or is not writable and the check is non-pass.
//! No new seam is minted; the check must READ these, not probe a hardcoded XDG path.
//!
//! ## DR-004 exit-code mapping for `doctor` (the mapping this board PINS)
//!
//! `doctor` is NOT a gate (it runs pre-daemon, emits no verdict fact), so exit 5
//! is WRONG for it (5 is reserved for `vet`/`debrief`/`pre_merge` gate-fail, §9).
//! The honest mapping mirrors how the other NON-gate verbs map (`rebuild`'s failure
//! class is 3, a substrate fault, in `main()`'s table): all checks pass gives exit
//! 0 (clean environment); any check that FAILS (unsatisfiable — git missing, socket
//! path unwritable) gives exit 3 (substrate fault: the environment substrate the
//! golden path assumes is not present, the same class `rebuild` uses for a
//! misbehaving store); any check INCONCLUSIVE (genuinely unknown, e.g. WSL2
//! unprobeable) with none failing also gives exit 3 (DR-004: `inconclusive` is 3,
//! NEVER coerced to 0/pass — I6; §9: "inconclusive is NOT 5 — it is 3, never
//! coerced toward pass or fail"). So a NON-clean preflight is exit 3 whether the
//! worst finding is fail or inconclusive; a clean one is 0. Exit 2 stays clap's
//! usage-error class. The implementer maps in `main()`'s per-verb table exactly as
//! the other verbs do; the tests assert only the CLASS (0 clean, 3 non-clean,
//! never 5, never a coerced 0).
//!
//! ## RED MODE — no-such-subcommand-red (honesty, mirroring spec_init / resolve)
//! These were authored RED, before the `Doctor` verb existed: with `Cmd` carrying
//! no `Doctor`, clap exited with an "unrecognized subcommand" USAGE error (exit 2)
//! BEFORE any check logic ran. Every exit-code assertion here ALSO guards that stderr is NOT clap's
//! `unrecognized subcommand` / `unexpected argument` message — otherwise a bare
//! code check would false-green while the subcommand is absent (clap's own usage
//! error is exit 2, and a doctor that never ran would trivially "not emit pass").
//! The stderr guard is the exact idiom from `operator_resolve_permit_cli.rs` and
//! `spec_init_cli.rs`. Authoring intent, past-tense-safe: these were written RED,
//! before `doctor` existed; each assertion states the CONTRACT it pins, so its
//! message stays true once the check logic is built.

use std::process::Command;

use serde_json::Value;

/// Run `rezidnt doctor [extra…]` with the given env OVERRIDES (each `(k, v)` is
/// set; the child otherwise inherits the runner's env). Returns (exit, stdout,
/// stderr). Callers that must force a check non-pass by removing something from
/// `PATH` use [`run_doctor_clean_env`] instead (a fully controlled env).
fn run_doctor(extra: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("doctor").args(extra);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt doctor");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Run `rezidnt doctor [extra…]` with a FULLY CONTROLLED environment: the child
/// inherits NOTHING but the `(k, v)` pairs given, so a test can force the git /
/// harness checks non-pass by supplying a `PATH` that lacks those bins (or omitting
/// `PATH` entirely). `env_clear` is the deterministic lever for the "genuinely
/// unsatisfiable check" honesty assertion — the environment is what the test says
/// it is, not the runner's.
fn run_doctor_clean_env(extra: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("doctor").args(extra);
    cmd.env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt doctor (clean env)");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Assert the given stderr is NOT clap's absent-subcommand / bad-arg usage error.
/// The HONEST-RED guard: `doctor` is absent today, so clap exits 2 with exactly
/// this message; without this guard an exit-2/exit-3 assertion could false-green on
/// clap rather than on `doctor`'s own logic. Stays a true invariant once built (the
/// real `doctor` never emits clap's unrecognized-subcommand text).
fn assert_not_clap_usage_error(stderr: &str) {
    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand") && !lc.contains("unexpected argument"),
        "RED-HONESTY: `rezidnt doctor` must EXIST and run its own preflight, NOT be clap \
         rejecting an unknown subcommand (both paths can exit non-zero) — stderr: {stderr}"
    );
}

/// Parse `doctor --json` stdout into the pinned `{ checks: [ {name, status}, … ] }`
/// shape, returning the `checks` array. Fails loudly (with the stderr) when the
/// output is absent/unparseable — the correct RED while `doctor` is unbuilt (clap
/// wrote no JSON), and a real contract violation once built.
fn parse_checks(stdout: &str, stderr: &str, code: Option<i32>) -> Vec<Value> {
    let doc: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`rezidnt doctor --json` must print a JSON findings object on stdout \
             (doctor absent / not yet emitting --json?): parse error {e}; exit {code:?}; \
             stderr: {stderr}; stdout: {stdout:?}"
        )
    });
    doc["checks"].as_array().cloned().unwrap_or_else(|| {
        panic!(
            "the `doctor --json` object must carry a `checks` ARRAY (each element \
                 {{name, status}}) — got: {doc:#}"
        )
    })
}

/// The status string of a check element, lowercased; panics if absent (the status
/// field is load-bearing — a check with no status cannot be honestly reported).
fn status_of(check: &Value) -> String {
    check["status"]
        .as_str()
        .unwrap_or_else(|| panic!("each doctor check must carry a string `status`: {check:#}"))
        .to_lowercase()
}

/// The (lowercased) name of a check element, or empty string if absent.
fn name_of(check: &Value) -> String {
    check["name"].as_str().unwrap_or_default().to_lowercase()
}

/// Find the first check whose name CONTAINS `needle` (case-insensitive), so the
/// implementer keeps latitude on the exact label (`git` vs `git-present`).
fn find_check<'a>(checks: &'a [Value], needle: &str) -> Option<&'a Value> {
    checks.iter().find(|c| name_of(c).contains(needle))
}

/// A `PATH` value that contains NO real directories, so NO external bin (git,
/// the harness) resolves on it — the deterministic lever to force the git /
/// harness checks non-pass regardless of the host. An empty string is a valid,
/// deterministically-bin-free PATH on every platform.
const EMPTY_PATH: &str = "";

// ===========================================================================
// CRITERION 1 — `doctor` runs the §11 read-only checks and prints per-check
// findings with a pass/inconclusive status, NEVER coercing an unknown/unsatisfiable
// check to `pass` (I6). Pinned via `--json` so the assertion reads STATUS, not prose.
// ===========================================================================

/// CRITERION 1 (report shape) — `doctor --json` emits a findings object with a
/// `checks` array; every check carries a `name` and a `status` drawn from the
/// closed set {pass, inconclusive, fail}. Also pins that the §11 golden-path
/// assumptions are actually CHECKED: the `git` and socket/lockfile-writable checks
/// are present by name (the deterministically forceable pair this board owns).
///
/// Authored RED: with `doctor` not yet built, no JSON was printed and
/// `parse_checks` panicked on the empty/clap-usage stdout. Built, it pins the
/// machine-readable findings contract.
#[test]
fn doctor_json_reports_named_checks_with_closed_status_set() {
    // A writable temp dir so the socket/lockfile-writable check has a satisfiable
    // input (this test only inspects the SHAPE, but a satisfiable env keeps it from
    // depending on the runner's real XDG paths).
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let (code, stdout, stderr) = run_doctor(
        &["--json"],
        &[
            ("REZIDNT_SOCKET", sock.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    assert!(
        !checks.is_empty(),
        "`doctor --json` must report at least one §11 preflight check; got none. stderr: {stderr}"
    );

    // Every status is in the closed set — the honesty backbone: no fourth value,
    // and (below) no coercion of unknowns to pass.
    for c in &checks {
        let s = status_of(c);
        assert!(
            matches!(s.as_str(), "pass" | "inconclusive" | "fail"),
            "each doctor check status must be one of pass|inconclusive|fail (I6 — no \
             coerced fourth value): {c:#}"
        );
    }

    // The §11 golden-path assumptions this board deterministically forces MUST be
    // among the checks (by name substring — implementer keeps label latitude).
    assert!(
        find_check(&checks, "git").is_some(),
        "the §11 golden-path assumes git (worktree substrate, §11 line 252) — a `git` \
         check must be present. checks: {checks:#?}"
    );
    assert!(
        find_check(&checks, "socket").is_some() || find_check(&checks, "lockfile").is_some(),
        "the §11 golden-path assumes a writable daemon socket/lockfile path (§11 line 240) — \
         a socket/lockfile-writable check must be present. checks: {checks:#?}"
    );
}

/// CRITERION 1 (I6 — the load-bearing honesty pin: an unsatisfiable check is NEVER
/// coerced to pass). Force the git-present check to be UNSATISFIABLE by running
/// `doctor` with an EMPTY `PATH` (no directory holds `git`), in a fully-controlled
/// environment. The git check MUST report a NON-pass status (fail or inconclusive)
/// — it must NOT print `pass` for a git it cannot resolve. Paired with the positive
/// (`doctor_git_present_env_passes_git_check`) so this can't false-green on a doctor
/// that reports EVERYTHING inconclusive.
///
/// Written RED: `doctor` absent → no JSON → `parse_checks` panics. Once built, this
/// is the I6 never-coerce proof for a genuinely-missing substrate.
#[test]
fn doctor_missing_git_is_not_coerced_to_pass() {
    // A writable temp dir keeps the socket/lockfile check satisfiable so the ONLY
    // forced-unsatisfiable input is git (isolating the assertion to the git check).
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let (code, stdout, stderr) = run_doctor_clean_env(
        &["--json"],
        &[
            ("PATH", EMPTY_PATH),
            ("REZIDNT_SOCKET", sock.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    let git = find_check(&checks, "git").unwrap_or_else(|| {
        panic!("the git check must be reported even (especially) when git is absent: {checks:#?}")
    });
    let s = status_of(git);
    assert_ne!(
        s, "pass",
        "with an EMPTY PATH git cannot be resolved — the git check must NOT be `pass` \
         (I6: an unknown/unsatisfiable check is NEVER coerced to pass). It must be `fail` \
         or `inconclusive`. git check: {git:#}"
    );
}

/// CRITERION 1 (the positive that fences off "everything is always inconclusive").
/// Give `doctor` a `PATH` that DOES contain a real `git` (the runner's — resolved
/// once at test time and skipped if this box has no git) and assert the git check
/// reports `pass`. Together with `doctor_missing_git_is_not_coerced_to_pass` this
/// proves the git check DISCRIMINATES: pass when satisfiable, non-pass when not —
/// so the never-coerce assertion is not vacuously satisfied by a doctor that reports
/// every check inconclusive.
///
/// Written RED: `doctor` absent → no JSON → `parse_checks` panics.
#[test]
fn doctor_git_present_env_passes_git_check() {
    // Resolve the runner's real git directory; if this box genuinely has no git,
    // the positive is not assertable here — skip (the negative still holds the line).
    let Some(git_dir) = which_dir("git") else {
        eprintln!("skipping doctor_git_present_env_passes_git_check: no git on this runner's PATH");
        return;
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let (code, stdout, stderr) = run_doctor_clean_env(
        &["--json"],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_SOCKET", sock.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    let git = find_check(&checks, "git")
        .unwrap_or_else(|| panic!("the git check must be reported: {checks:#?}"));
    assert_eq!(
        status_of(git),
        "pass",
        "with a PATH that DOES contain git, the git check must be `pass` — proving the \
         check discriminates (not always-inconclusive). git check: {git:#}"
    );
}

// ===========================================================================
// CRITERION 2 — exit-code honesty (DR-004): clean env → 0; a non-pass check
// surfaces exit 3 (substrate fault / never-coerced inconclusive), NEVER 5 (doctor
// is not a gate), NEVER a coerced 0.
// ===========================================================================

/// CRITERION 2 (clean environment → exit 0). Give `doctor` a satisfiable env for
/// the checks this board forces (a `PATH` with git, a writable temp dir for the
/// socket/lockfile paths). If EVERY check passes, exit is 0. Because a check this
/// board does not control (e.g. `wsl` on a non-WSL host) may legitimately be
/// inconclusive, this test asserts the IMPLICATION rather than an unconditional 0:
/// when the JSON shows all-pass, the exit MUST be 0 (and it must never be 5). This
/// keeps the exit-0 pin honest on any box without demanding WSL2 be present.
///
/// Written RED: `doctor` absent → clap exit 2 + no JSON → the guards/parse fail.
#[test]
fn doctor_clean_environment_exits_zero() {
    let Some(git_dir) = which_dir("git") else {
        eprintln!("skipping doctor_clean_environment_exits_zero: no git on this runner's PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let (code, stdout, stderr) = run_doctor_clean_env(
        &["--json"],
        &[
            ("PATH", git_dir.to_str().unwrap()),
            ("REZIDNT_SOCKET", sock.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    let all_pass = checks.iter().all(|c| status_of(c) == "pass");

    // doctor is NOT a gate: exit 5 is wrong for it under ANY environment.
    assert_ne!(
        code,
        Some(5),
        "`doctor` is not a gate (§9) — it must NEVER exit 5 (that class is vet/debrief/\
         pre_merge gate-fail). exit: {code:?}; stderr: {stderr}"
    );
    if all_pass {
        assert_eq!(
            code,
            Some(0),
            "when every §11 check passes, the preflight is clean and `doctor` exits 0 \
             (DR-004). checks: {checks:#?}; stderr: {stderr}"
        );
    } else {
        // Not all-pass on this box (e.g. WSL2 unprobeable) — then it must NOT be a
        // coerced 0 (that is criterion 2's other half, asserted directly below in
        // the forced-fail test); here we only require the all-pass⇒0 implication.
        eprintln!(
            "doctor_clean_environment_exits_zero: not all checks passed on this box \
             (exit {code:?}) — the all-pass⇒exit-0 implication is vacuously satisfied; the \
             forced-fail test pins the non-pass⇒exit-3 side."
        );
    }
}

/// CRITERION 2 (a check that cannot be satisfied → exit 3, NEVER a coerced 0, NEVER
/// 5). Force the git check unsatisfiable (empty `PATH`) and assert the process exit
/// is 3 (DR-004 substrate-fault class — the environment substrate the golden path
/// assumes is not present), NOT 0 (a coerced pass) and NOT 5 (doctor is not a gate).
/// This is the exit-code twin of `doctor_missing_git_is_not_coerced_to_pass`: the
/// per-check status is non-pass there; here the PROCESS exit reflects it honestly.
///
/// Written RED: `doctor` absent → clap exits 2, which is neither 3 nor 0 nor 5, so
/// the "exit 3" assertion fails — AND the stderr guard confirms it failed because
/// the subcommand is absent (clap usage), not for a spurious reason.
#[test]
fn doctor_unsatisfiable_check_exits_three_not_zero_not_five() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let (code, _stdout, stderr) = run_doctor_clean_env(
        &["--json"],
        &[
            ("PATH", EMPTY_PATH),
            ("REZIDNT_SOCKET", sock.to_str().unwrap()),
            ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
        ],
    );
    assert_not_clap_usage_error(&stderr);

    assert_ne!(
        code,
        Some(0),
        "a failed preflight (git unresolvable on an empty PATH) must NOT exit 0 — I6: an \
         unsatisfiable check is never coerced to a clean pass. stderr: {stderr}"
    );
    assert_ne!(
        code,
        Some(5),
        "`doctor` is not a gate — a failed check must NOT exit 5 (§9 reserves 5 for \
         vet/debrief/pre_merge). stderr: {stderr}"
    );
    assert_eq!(
        code,
        Some(3),
        "a §11 check that cannot be satisfied is a substrate fault → exit 3 (DR-004; the \
         same class `rebuild` uses, and the class `inconclusive` maps to, never coerced). \
         stderr: {stderr}"
    );
}

// ===========================================================================
// CRITERION 3 — read-only + fact-free (I3/I7): `doctor` needs no running daemon,
// opens no socket, makes no network call. Encoded by RUNNING it with the daemon-
// dial env pointed at unequivocally-dead paths and asserting it STILL produces
// findings (a doctor that dialed the daemon would hang/fail against the dead paths).
// ===========================================================================

/// CRITERION 3 (no daemon needed; opens no socket). Point `REZIDNT_SOCKET` at a
/// path that does not exist and `REZIDNT_DB` at a dead path — there is NO reachable
/// daemon — and assert `doctor` still produces a findings report (parseable `--json`
/// with checks) rather than failing to connect. A preflight that dialed the daemon
/// (to emit a first-run fact, or to check a live socket by CONNECTING) would fail
/// here; the writable-path check must be a filesystem probe of the PATH, never a
/// connect. The git check is given a real git so the ONLY variable under test is
/// "does doctor need the daemon" — and the report existing at all is the proof.
///
/// Written RED: `doctor` absent → no JSON → `parse_checks` panics (the correct red:
/// the preflight is unbuilt, not that it wrongly dialed).
#[test]
fn doctor_needs_no_daemon_and_opens_no_socket() {
    let dir = tempfile::tempdir().expect("tempdir");
    // The writable-check inputs point INTO a real writable dir (satisfiable). The
    // DAEMON-DIAL envs point at unequivocally dead paths: if doctor CONNECTS to a
    // daemon it will fail; it must only STAT/probe the writable path, never dial.
    let sock = dir.path().join("rezidnt.sock");
    let lock = dir.path().join("mcp.lock");

    let mut env: Vec<(&str, &str)> = vec![
        ("REZIDNT_SOCKET", sock.to_str().unwrap()),
        ("REZIDNT_LOCKFILE", lock.to_str().unwrap()),
        ("REZIDNT_DB", "/nonexistent/rezidnt-doctor-no-daemon.db"),
    ];
    // Give it a real git so a non-git failure can't mask the point (daemon-freeness).
    let git_dir = which_dir("git");
    if let Some(ref d) = git_dir {
        env.push(("PATH", d.to_str().unwrap()));
    }

    let (code, stdout, stderr) = if git_dir.is_some() {
        run_doctor_clean_env(&["--json"], &env)
    } else {
        // No git on the runner: keep the inherited PATH (so other checks resolve)
        // but still assert the daemon-freeness via the report existing.
        run_doctor(&["--json"], &env)
    };
    assert_not_clap_usage_error(&stderr);

    let checks = parse_checks(&stdout, &stderr, code);
    assert!(
        !checks.is_empty(),
        "`doctor` must run and produce findings with NO reachable daemon (I3/I7 — it emits \
         no fact and opens no socket; it is a pure local, read-only preflight). It must not \
         require a daemon to connect to. checks: {checks:#?}; stderr: {stderr}"
    );
}

// ===========================================================================
// Test-local helper — resolve the directory holding an executable on the current
// PATH, WITHOUT shelling out (so the test itself makes no assumption about which
// utilities exist). Returns None if not found. Used to build a controlled PATH that
// DOES contain git for the positive/clean tests, and to skip them where absent.
// ===========================================================================

/// Return the directory containing `bin` on the current `PATH`, or None. Tries the
/// bare name and, on Windows, the `.exe`/`.cmd`/`.bat` PATHEXT variants. This is a
/// pure filesystem walk of `PATH` — no subprocess — so it introduces no new
/// external dependency into the test.
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
