//! C3a oracle (DR-025 — the Linux OS-sandbox slice) — the REAL `bwrap`
//! confinement behavior: an agent CONFINED to its worktree binds (criterion 1),
//! an out-of-bounds filesystem access DENIED (criterion 2), and the bwrap-PRESENT
//! arm of the loud-degrade contract (criterion 4).
//!
//! ## SUITE PLACEMENT — WSL-ONLY, #[cfg(unix)] + requires `bwrap` on the box.
//! This whole file is `#[cfg(unix)]`, so the HOST (Windows) compiles it to ZERO
//! tests — host /vet neither runs nor is satisfied by it
//! ([[vet-is-host-side-wsl-insufficient]]). It runs GREEN only WSL-side, and only
//! where `bwrap` is installed. The pure confinement-LOGIC, no-widening, and
//! degrade-probe suites are the HOST-runnable oracles for the same criteria:
//!   - `crates/rezidnt-gate/tests/path_confinement_native_c3a.rs`  (crit 1, 2)
//!   - `crates/rezidnt-run/tests/sandbox_no_widening_c3a.rs`        (crit 3)
//!   - `crates/rezidnt-run/tests/sandbox_degrade_and_deps_c3a.rs`   (crit 4, 5)
//!   - `crates/rezidnt-fabric/tests/sandbox_unavailable_fold_c3a.rs` (crit 4 log)
//!
//! ## STATUS — GREEN on WSL (impl landed; `BwrapSubstrate` confines for real).
//! These tests were `#[ignore]`-gated while the `bwrap` substrate was a `todo!()`
//! stub (a real-bwrap test that passed before the impl existed would test nothing
//! — test honesty). The implementer built `BwrapSubstrate::{availability,
//! spawn_confined}` + `bwrap_argv`/`probe_backend`, so the gate is removed and all
//! three run un-ignored on a WSL box with `bwrap` at `/usr/bin/bwrap`.
//!
//! ## THE POLICY BINDS "WORKTREE + TOOLCHAIN" (DR-025 §Decision — not a weakening).
//! Each confinement policy binds the worktree READ-WRITE plus the toolchain
//! READ-ONLY (`/usr` + `/bin` + `/lib` + `/lib64`). On a usr-merged distro
//! (Ubuntu-24.04: `/bin` → `usr/bin`, `/lib` → `usr/lib`), those toolchain binds
//! are what lets `bwrap` resolve `/bin/sh`'s interpreter + shared libs inside the
//! namespace so the confined shell EXECS — without them the child fails to exec
//! and every assertion is vacuous. NO-WIDENING held: every added bind is READ-ONLY
//! toolchain, never `/`, never the worktree's parent, never a writable escape;
//! only the worktree is writable. The criterion-3 host suite still guards that
//! run-supplied args cannot add binds — these are binds the DAEMON folds from the
//! toolchain layer (the sanctioned authority path).

#![cfg(unix)]

use std::path::PathBuf;

use rezidnt_run::sandbox::{Availability, Bind, SandboxPolicy, SandboxSubstrate};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

// The expected implementer artifact: the Linux bwrap backend behind the trait.
// Referenced (not defined) here so this file pins the entry-point NAME the
// implementer must provide. Until it exists, the crate fails to compile THIS
// cfg(unix) target on WSL — the honest RED for the missing impl.
use rezidnt_run::sandbox::BwrapSubstrate;

/// A trivial confined plan: run `/bin/sh -c <script>` under confinement. The
/// harness bin is a shell so the confinement test can drive a real read/write.
fn sh_plan(script: &str) -> SpawnPlan {
    let agent = AgentSpec {
        name: "confined".to_string(),
        harness: "claude-code".to_string(),
        bin_override: Some(PathBuf::from("/bin/sh")),
        ..AgentSpec::default()
    };
    let mut plan = SpawnPlan::for_claude_code(&agent, "badge-wire", std::env::vars());
    // Replace the claude args with a shell command for the confinement probe.
    plan.args = vec!["-c".to_string(), script.to_string()];
    plan
}

/// Skip the body when bwrap is not installed on this box — the confinement arm
/// needs the real tool. (The bwrap-ABSENT degrade arm is the host-runnable probe
/// suite, not here.) Returns true when the caller should early-return.
fn bwrap_absent(sub: &BwrapSubstrate) -> bool {
    !matches!(sub.availability(), Availability::Available)
}

/// CRITERION 1 — an agent CONFINED to its worktree binds RUNS, and a read/write
/// INSIDE an allowed bind SUCCEEDS. Bind a writable temp worktree; the confined
/// shell writes a file inside it; the write succeeds and the child exits 0.
///
/// FIXTURE REPAIR (oracle, C3a): the policy binds the worktree READ-WRITE plus
/// the TOOLCHAIN read-only. DR-025 §Decision says the folded policy binds
/// "worktree + toolchain", so binding the toolchain is CORRECT, not a weakening.
/// The earlier `/bin`-only fixture under-bound the toolchain: on a usr-merged
/// distro (Ubuntu-24.04: `/bin` → `usr/bin`, `/lib` → `usr/lib`, `/lib64` →
/// `usr/lib64`), `bwrap` cannot resolve `/bin/sh`'s ELF interpreter + shared libs
/// inside the namespace, so the confined child fails to exec (`execvp /bin/sh: No
/// such file or directory`) and the write never lands. Binding `/usr` + `/lib` +
/// `/lib64` read-only supplies exactly the interpreter/libs the shell needs.
///
/// NO-WIDENING held: every added bind is READ-ONLY toolchain (`/usr`, `/lib`,
/// `/lib64`) — never `/`, never the worktree's PARENT, never a writable escape.
/// Only the worktree itself is writable. The criterion-3 no-widening suite
/// (host) still guards that run-supplied args cannot add binds; this fixture
/// adds binds the DAEMON would fold from the toolchain layer, the sanctioned
/// authority path.
#[test]
fn confined_agent_writes_inside_its_worktree_bind() {
    let sub = BwrapSubstrate::default();
    if bwrap_absent(&sub) {
        return; // no bwrap on this box — the confinement arm is not applicable
    }
    let wt = tempfile::tempdir().expect("worktree tempdir");
    let inside = wt.path().join("hello.txt");

    // Folded authority: the worktree is a WRITABLE bind, plus the TOOLCHAIN
    // read-only (the interpreter + shared libs `/bin/sh` needs on a usr-merged
    // distro). Real daemon folds both from the project spec + toolchain layer;
    // the test stands in for that fold — DR-025 §Decision ("worktree + toolchain").
    let policy = SandboxPolicy::from_folded_authority(
        vec![
            Bind::writable(wt.path()),
            Bind::read_only("/usr"),
            Bind::read_only("/bin"),
            Bind::read_only("/lib"),
            Bind::read_only("/lib64"),
        ],
        true,
    );
    let plan = sh_plan(&format!("echo hi > {}", inside.display()));

    let child = sub
        .spawn_confined(&plan, &policy)
        .expect("a confined spawn under an available bwrap succeeds");
    assert_eq!(
        child.backend, "bwrap",
        "the spawn ran under the bwrap backend"
    );

    // The write INSIDE the writable bind landed (criterion 1: inside succeeds).
    // (The implementer's SandboxedChild carries the reap seam; the test waits on
    // the pid via the reaper or the child handle the impl exposes.)
    // NOTE: the exact wait API is the implementer's; this asserts the observable
    // effect — the file exists on the host side of the writable bind.
    // A short bounded wait for the child to finish its single write.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        inside.exists(),
        "a write INSIDE the writable worktree bind succeeded (CRITERION 1)"
    );
}

/// CRITERION 2 — a write OUTSIDE the binds is DENIED, not a silent success. The
/// confined shell tries to write `/etc/rezidnt-c3a-probe` (outside every bind);
/// under `--unshare-all` + the bind set, `/etc` is not writable in the sandbox,
/// so the file does NOT appear on the host and the child's write fails. The
/// denial is observable (no host-side file), never a silent success.
///
/// NON-VACUOUS (the oracle's honesty guard): the shell MUST actually EXEC for
/// this to test a real denial — so the policy binds the TOOLCHAIN read-only
/// (`/usr` + `/lib` + `/lib64`, same as criterion 1) so `/bin/sh` resolves its
/// interpreter/libs and RUNS. It does NOT bind `/etc`, so the write the running
/// shell attempts is DENIED by confinement, not by a failed exec. Without the
/// toolchain binds, the shell would fail to exec (`execvp /bin/sh: No such file
/// or directory`) and "no file at /etc" would pass VACUOUSLY (a failed exec, not
/// a blocked write) — theater the oracle refuses. The `wt`-inside sentinel below
/// PROVES the shell ran: it writes a marker inside the writable bind FIRST, then
/// attempts the escape; the marker must exist (shell ran) AND the escape must not.
#[test]
fn confined_agent_write_outside_binds_is_denied() {
    let sub = BwrapSubstrate::default();
    if bwrap_absent(&sub) {
        return;
    }
    let wt = tempfile::tempdir().expect("worktree tempdir");
    let ran_marker = wt.path().join("shell-ran.txt");
    let outside = PathBuf::from("/etc/rezidnt-c3a-probe");
    // Ensure a stale probe from a prior run does not mask a real denial.
    let _ = std::fs::remove_file(&outside);

    // Toolchain bound READ-ONLY so the shell EXECS (non-vacuous); `/etc` is NOT
    // bound, so the running shell's write there is denied by confinement. No
    // widening: every added bind is read-only toolchain, never `/`, never the
    // worktree's parent; only the worktree is writable.
    let policy = SandboxPolicy::from_folded_authority(
        vec![
            Bind::writable(wt.path()),
            Bind::read_only("/usr"),
            Bind::read_only("/bin"),
            Bind::read_only("/lib"),
            Bind::read_only("/lib64"),
        ],
        true,
    );
    // The shell writes an INSIDE marker (proving it ran), THEN attempts the
    // out-of-bounds escape. The escape must fail even though the shell executed.
    let plan = sh_plan(&format!(
        "echo ran > {}; echo escaped > {}",
        ran_marker.display(),
        outside.display()
    ));

    let _child = sub
        .spawn_confined(&plan, &policy)
        .expect("the confined spawn itself launches; the WRITE inside is what is denied");
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Non-vacuous guard: the shell ACTUALLY RAN (the inside marker landed), so
    // the /etc write was ATTEMPTED and blocked — not skipped by a failed exec.
    assert!(
        ran_marker.exists(),
        "the confined shell must actually EXEC and run (inside marker present) so the \
         out-of-bounds write is genuinely ATTEMPTED — otherwise the denial below is \
         vacuous (a failed exec, not a blocked write). The toolchain binds make it run."
    );
    assert!(
        !outside.exists(),
        "CRITERION 2: a write OUTSIDE the binds must be DENIED — no host-side file at \
         {} may appear. A file here is a confinement HOLE (a silent success), the exact \
         'sandbox with a hole is worse than none' failure (design §8.3)",
        outside.display()
    );
}

/// CRITERION 4 (bwrap-PRESENT arm) — when bwrap IS available, the substrate
/// reports `Available` and confines for real (no degrade). The complement of the
/// host-runnable bwrap-ABSENT probe: here availability drives the CONFINED path,
/// not the loud degrade.
#[test]
fn bwrap_present_reports_available_and_confines() {
    let sub = BwrapSubstrate::default();
    // On a box WITH bwrap this is Available; on a box without, this test is not
    // the applicable arm (the host-runnable probe covers absent). Assert only the
    // positive when present.
    if let Availability::Available = sub.availability() {
        assert_eq!(sub.backend(), "bwrap");
    } else {
        // bwrap not installed here — the confinement arm is covered elsewhere.
        eprintln!("bwrap absent on this box; the bwrap-present arm is not applicable here");
    }
}
