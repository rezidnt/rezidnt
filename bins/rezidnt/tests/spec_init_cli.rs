//! DR-036 sub-slice `spec-init` ORACLE — the HOST-runnable generator contract
//! for `rezidnt spec init` (criteria 1, 3, 4). Drives the REAL binary
//! (`CARGO_BIN_EXE_rezidnt`) and grounds every assertion on the REAL project-spec
//! parser the daemon uses to `open` (`rezidnt_run::spec::ProjectSpec`), so a
//! generator that emits drifting TOML fails here, not at a hand-rolled shadow
//! shape. Cross-platform on purpose (no socket, no `#![cfg(unix)]`): host `/vet`
//! covers these.
//!
//! ## The surface this board PINS for the implementer (the smallest honest one)
//!   - Verb: `rezidnt spec init [DIR]` — a new subcommand of the EXISTING `rezidnt`
//!     CLI (I7, one binary; NOT a new bin). `DIR` is an OPTIONAL positional target
//!     directory; absent = the current working directory. The file written is
//!     ALWAYS `<DIR>/rezidnt.toml` (the §13 filename the golden path / `open`
//!     expects). We drive it with an explicit `DIR` so the tests never depend on
//!     (or mutate) the runner's real cwd.
//!   - `--defaults` — the non-interactive path: write a minimal valid single-agent
//!     §13 spec with NO prompts (this is the path these deterministic tests drive).
//!   - `--force` — the explicit overwrite flag (criterion 3). Without it, an
//!     existing `rezidnt.toml` is NEVER clobbered.
//!
//! ## RED MODE — no-such-subcommand-red (honesty, mirroring resolve-permit)
//! `rezidnt spec init` does NOT exist yet: `Cmd` has no `Spec` verb, so clap exits
//! with an "unrecognized subcommand" USAGE error (exit 2) BEFORE any generator
//! logic runs. Two honesty consequences, both handled below:
//!   - The write/parse tests (crit 1) fail because NO file is written (clap errored
//!     out) — the `rezidnt.toml` is absent, so the read+parse panics. That is the
//!     correct red (the generator is unbuilt), and the stderr guard confirms it is
//!     the ABSENT subcommand, not a spurious pass.
//!   - The clobber-guard test (crit 3) asserts exit 2 AND that stderr is NOT clap's
//!     `unrecognized subcommand` / `unexpected argument` message — otherwise a bare
//!     exit-2 check would false-green while the subcommand is absent (clap's own
//!     usage error is also exit 2). This is the exact `run_resolve` + stderr-guard
//!     idiom from `operator_resolve_permit_cli.rs`.
//!
//! DR-004 exit classes (mirror `bins/rezidnt/src/main.rs`): a LOCAL input/usage
//! error (a clobber without `--force`) is exit 2. Success is exit 0.

use std::path::Path;
use std::process::Command;

use rezidnt_run::spec::ProjectSpec;

/// Run `rezidnt spec init [extra…]` with the given env; return (exit, stdout,
/// stderr). No `REZIDNT_SOCKET` / `REZIDNT_DB` is set unless a test adds it —
/// criterion 4 relies on that (a pure local generator dials no daemon).
fn run_spec_init(extra: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("spec").arg("init").args(extra);
    // Isolate from any real dev daemon/state: point the daemon-dial env at dead
    // paths so a (buggy) generator that tried to dial would fail loudly rather
    // than touch a live socket. Criterion 4 asserts the generator NEVER dials.
    cmd.env("REZIDNT_SOCKET", "/nonexistent/rezidnt-spec-init-dead.sock");
    cmd.env("REZIDNT_DB", "/nonexistent/rezidnt-spec-init-dead.db");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn rezidnt spec init");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The generated spec path under a target dir (the §13 filename `open` expects).
fn generated_spec(dir: &Path) -> std::path::PathBuf {
    dir.join("rezidnt.toml")
}

// ===========================================================================
// CRITERION 1 — `spec init --defaults` writes a §13-shape rezidnt.toml that
// PARSES against the REAL daemon parser (rezidnt_run::spec::ProjectSpec), with
// no prompt. Grounding the assertion on the real parser is what makes a drifting
// generator fail (not a hand-rolled shadow shape).
// ===========================================================================

/// CRITERION 1 — `rezidnt spec init --defaults <dir>` writes `<dir>/rezidnt.toml`
/// WITHOUT prompting, and the bytes DESERIALIZE into the real
/// `rezidnt_run::spec::ProjectSpec` (the same type `read_spec` in
/// `bins/rezidnt/src/main.rs` parses before `open`). A minimal valid spec has a
/// `[project]` (name + repo) and at least one `[[agent]]` (harness + worktree).
///
/// RED today: the `spec` subcommand is absent, so clap errors out (exit 2) and
/// writes NO file — `read_to_string` on the missing `rezidnt.toml` panics with
/// the "generator never wrote it" message. Correct red: the generator is unbuilt.
#[test]
fn defaults_writes_parseable_section13_spec() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (code, _out, stderr) = run_spec_init(&["--defaults", dir.path().to_str().unwrap()], &[]);

    assert_eq!(
        code,
        Some(0),
        "spec init --defaults must SUCCEED (exit 0) writing a minimal valid spec; stderr: {stderr}"
    );

    let path = generated_spec(dir.path());
    let toml = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "spec init --defaults must WRITE {} — the generator wrote nothing (subcommand \
             absent / wrong target path?): {e}; stderr: {stderr}",
            path.display()
        )
    });

    // The load-bearing pin: the generated bytes parse into the REAL daemon spec
    // type. A generator that drifts from what `open` accepts fails HERE.
    let spec = ProjectSpec::from_toml_str(&toml).unwrap_or_else(|e| {
        panic!(
            "the generated rezidnt.toml must deserialize into the REAL \
             rezidnt_run::spec::ProjectSpec the daemon parses (a drifting shape is the \
             risk DR-036 §Consequences names) — parse error: {e}; generated:\n{toml}"
        )
    });

    assert!(
        !spec.name.trim().is_empty(),
        "the §13 [project].name must be non-empty (the `open` success line needs it): {spec:#?}"
    );
    assert!(
        !spec.agents.is_empty(),
        "a minimal §13 spec declares at least one [[agent]] (DR-036 Design: project + \
         at least one agent): {spec:#?}"
    );
    let agent = &spec.agents[0];
    assert!(
        !agent.harness.trim().is_empty(),
        "the default [[agent]] names a harness (the AgentSubstrate selector): {agent:#?}"
    );
    assert!(
        !agent.worktree.trim().is_empty(),
        "the default [[agent]] sets worktree (DR-036 Design: worktree=auto, the \
         sole-allocator model): {agent:#?}"
    );
}

/// CRITERION 1 (no-prompt proof) — `--defaults` must NOT block on stdin. We give
/// the child a CLOSED stdin (EOF immediately); a prompt-driven read on EOF would
/// either hang (killed by the harness) or error — either way it would not cleanly
/// write the spec + exit 0. A clean exit 0 with the file present proves the
/// `--defaults` path took NO interactive input.
///
/// RED today: subcommand absent → exit 2, no file. (When built, this also fences
/// `--defaults` from accidentally sharing the interactive read path.)
#[test]
fn defaults_does_not_block_on_stdin() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rezidnt"));
    cmd.arg("spec")
        .arg("init")
        .arg("--defaults")
        .arg(dir.path().to_str().unwrap())
        .env("REZIDNT_SOCKET", "/nonexistent/rezidnt-spec-init-dead.sock")
        // Closed stdin: a prompt read returns EOF instantly. The `--defaults`
        // path must never wait for a line here.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().expect("spawn rezidnt spec init (null stdin)");

    assert_eq!(
        out.status.code(),
        Some(0),
        "spec init --defaults must complete with a CLOSED stdin (it prompts for NOTHING) — \
         exit {:?}, stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        generated_spec(dir.path()).exists(),
        "the spec was written even though stdin was closed — proving --defaults is \
         non-interactive"
    );
}

/// CRITERION 1 (interactive path is REACHABLE — deterministic drive) — the
/// interactive generator (no `--defaults`) reads plain stdin lines (I1: line
/// prompts, NOT a TUI). We drive it deterministically by piping canned answers
/// and assert the RESULT parses into the real `ProjectSpec`, so the interactive
/// path is pinned as reachable and honest (its output is a real §13 spec), not
/// left entirely to a manual check.
///
/// The exact prompt ORDER/COUNT is the implementer's to choose; this test feeds a
/// generous line-buffer of plausible answers (name, repo, agent name, harness)
/// plus trailing blank lines to accept defaults, and asserts ONLY that the run
/// exits 0 and the written file parses. If the implementer's prompt sequence
/// differs, they adjust the canned input here — the CONTRACT pinned is "an
/// interactive drive produces a parseable §13 spec", not a specific script.
///
/// RED today: subcommand absent → exit 2, no file → parse panics on the missing
/// file. Correct red.
#[test]
fn interactive_piped_answers_write_parseable_spec() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("mkdir repo");

    let mut child = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .arg("spec")
        .arg("init")
        .arg(dir.path().to_str().unwrap())
        .env("REZIDNT_SOCKET", "/nonexistent/rezidnt-spec-init-dead.sock")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn interactive rezidnt spec init");

    // Canned answers, one per line, generous trailing blanks to accept defaults.
    // Order the implementer is expected to prompt in (project name, repo, agent
    // name, harness); blanks accept any remaining defaults (worktree=auto, gates).
    let canned = format!(
        "my-project\n{repo}\nimpl\nclaude-code\n\n\n\n\n",
        repo = repo.display()
    );
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(canned.as_bytes())
        .expect("write canned answers");

    let out = child.wait_with_output().expect("interactive run output");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the interactive spec init, fed canned line answers, must exit 0 — it is a plain \
         stdin/stdout prompt flow (I1), reachable and non-hanging; stderr: {stderr}"
    );

    let path = generated_spec(dir.path());
    let toml = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "the interactive path must WRITE {} from the piped answers: {e}; stderr: {stderr}",
            path.display()
        )
    });
    ProjectSpec::from_toml_str(&toml).unwrap_or_else(|e| {
        panic!(
            "the interactively-generated rezidnt.toml must parse into the real ProjectSpec \
             (the interactive path emits the SAME §13 shape as --defaults): {e}; generated:\n{toml}"
        )
    });
}

// ===========================================================================
// CRITERION 3 — clobber guard. `spec init` refuses to overwrite an EXISTING
// rezidnt.toml without `--force`, exits 2 (DR-004 LOCAL input/usage class), and
// leaves the existing file BYTE-UNCHANGED. With `--force`, it overwrites.
// ===========================================================================

/// A sentinel an operator would recognize if it were clobbered — distinct from
/// anything the generator would emit (the marker key proves byte-preservation).
const EXISTING_SENTINEL: &str = "# DO-NOT-CLOBBER sentinel spec (operator-authored)\n\
[project]\nname = \"operator-authored\"\nrepo = \".\"\n\n[[agent]]\nname = \"hand\"\n\
harness = \"claude-code\"\nworktree = \"auto\"\n";

/// CRITERION 3 (refuse without --force, byte-unchanged) — with an existing
/// `rezidnt.toml`, `spec init --defaults` (no `--force`) must EXIT 2 (DR-004 local
/// input/usage) and leave the file's bytes EXACTLY as they were.
///
/// HONEST-RED GUARD (the resolve-permit idiom): clap's unrecognized-subcommand
/// error is ALSO exit 2, so a bare code check would false-green while `spec` is
/// absent. We therefore ALSO assert stderr is NOT clap's `unrecognized
/// subcommand` / `unexpected argument` message — i.e. the guard must be the
/// SUBCOMMAND refusing to clobber, not clap rejecting an unknown verb. And we
/// assert the sentinel bytes survive regardless. RED until the guard is built.
#[test]
fn refuses_clobber_without_force_and_leaves_bytes_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = generated_spec(dir.path());
    std::fs::write(&path, EXISTING_SENTINEL).expect("write existing spec");
    let before = std::fs::read(&path).expect("read before");

    let (code, _out, stderr) = run_spec_init(&["--defaults", dir.path().to_str().unwrap()], &[]);

    let lc = stderr.to_lowercase();
    assert!(
        !lc.contains("unrecognized subcommand") && !lc.contains("unexpected argument"),
        "RED-HONESTY: the clobber guard must be `spec init` itself refusing to overwrite, \
         NOT clap rejecting an absent subcommand (both are exit 2) — stderr: {stderr}"
    );
    assert_eq!(
        code,
        Some(2),
        "an existing rezidnt.toml without --force is a LOCAL input/usage error (exit 2, \
         DR-004) — the generator must refuse to clobber; stderr: {stderr}"
    );

    let after = std::fs::read(&path).expect("read after");
    assert_eq!(
        before, after,
        "a refused clobber must leave the existing spec BYTE-UNCHANGED — the generator must \
         never truncate/partially-write before deciding to refuse"
    );
}

/// CRITERION 3 (--force overwrites) — with `--force`, `spec init --defaults`
/// OVERWRITES the existing file: it exits 0 and the bytes CHANGE to a freshly
/// generated §13 spec that parses into the real `ProjectSpec`.
///
/// RED today: subcommand absent → exit 2 (not 0), file unchanged → the "bytes
/// changed" / "parses" assertions fail. Correct red.
#[test]
fn force_overwrites_existing_spec() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = generated_spec(dir.path());
    std::fs::write(&path, EXISTING_SENTINEL).expect("write existing spec");
    let before = std::fs::read_to_string(&path).expect("read before");

    let (code, _out, stderr) = run_spec_init(
        &["--defaults", "--force", dir.path().to_str().unwrap()],
        &[],
    );

    assert_eq!(
        code,
        Some(0),
        "spec init --defaults --force must OVERWRITE and exit 0; stderr: {stderr}"
    );
    let after = std::fs::read_to_string(&path).expect("read after");
    assert_ne!(
        before, after,
        "with --force the sentinel must be REPLACED by the generated spec (bytes change)"
    );
    ProjectSpec::from_toml_str(&after).unwrap_or_else(|e| {
        panic!("the force-overwritten spec must still be a parseable §13 ProjectSpec: {e}\n{after}")
    });
}

// ===========================================================================
// CRITERION 4 — fact-free (I3): the generator needs NO running daemon and dials
// nothing. `--defaults` succeeds with NO reachable socket. (The run helper above
// already points REZIDNT_SOCKET at a dead path; this test states the intent
// explicitly and asserts success — a generator that dialed the daemon would fail
// against the dead socket.)
// ===========================================================================

/// CRITERION 4 — `spec init --defaults` is a PURE local file generator: it emits
/// no fabric fact and dials no daemon. With `REZIDNT_SOCKET` pointed at a path
/// that does not exist (no daemon reachable) it must still SUCCEED (exit 0) and
/// write the spec. A generator that connected to the daemon (to emit a first-run
/// fact, say) would fail here — that failure is exactly what this fences out.
///
/// RED today: subcommand absent → exit 2, no file. Correct red.
#[test]
fn generator_needs_no_daemon() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Explicit: an unequivocally-unreachable socket path. If the generator dials
    // it, the command fails; criterion 4 requires it to NOT dial.
    let (code, _out, stderr) = run_spec_init(
        &["--defaults", dir.path().to_str().unwrap()],
        &[("REZIDNT_SOCKET", "/nonexistent/definitely-no-daemon.sock")],
    );
    assert_eq!(
        code,
        Some(0),
        "spec init --defaults must succeed with NO daemon reachable — it writes a local \
         file and emits nothing to the fabric (I3, DR-036 Design 'fact-free'); stderr: {stderr}"
    );
    assert!(
        generated_spec(dir.path()).exists(),
        "the spec is written with no daemon present — proving the generator is purely local"
    );
}
