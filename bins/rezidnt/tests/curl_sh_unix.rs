//! DR-037 sub-slice `install-script` ORACLE (UNIX-gated) — drives the clean-room
//! `install.sh` (repo root) against a LOCAL FIXTURE "release" so the installer's
//! contract is machine-checked without a real GitHub release. The golden path's
//! `curl | sh` step (§1/§18) hands an operator this script; these tests pin the three
//! criteria it must meet (DR-037 §Slicing `install-script`):
//!   1. It FETCHES the release artifacts and INSTALLS both binaries on PATH.
//!   2. It VERIFIES each artifact against the published sha256 BEFORE install and is
//!      FAIL-CLOSED on a mismatch — no partial install (I6 never-half-install).
//!   3. It REFUSES an unsupported platform in plain language, installing nothing
//!      (DR-037 Linux/WSL-first scope fence; I6).
//!
//! ## The install.sh contract these tests pin (env seams)
//! The script is driven entirely by overridable env seams so a test (or a mirror /
//! air-gapped install) needs no network and no real release:
//!   - `REZIDNT_BASE_URL`   — base the assets are fetched from. A `file://<dir>` URL
//!     is served by `cp` (no curl/network); real installs use the https release URL.
//!   - `REZIDNT_VERSION`    — release tag; set here so the GitHub "latest" API path
//!     (the only un-unit-tested branch, exercised in real use) is skipped.
//!   - `REZIDNT_INSTALL_DIR`— where the two binaries are placed (default ~/.local/bin).
//!   - `REZIDNT_OS`/`REZIDNT_ARCH` — override `uname -s`/`uname -m`, so the platform
//!     gate is exercisable deterministically off the real host.
//!
//! Asset names (the `release-ci` workflow's published contract):
//! `rezidnt-x86_64-unknown-linux-musl`, `rezidentd-x86_64-unknown-linux-musl`,
//! `SHA256SUMS` (a `sha256sum`-format line per binary).
//!
//! `#![cfg(unix)]`: the script is POSIX `sh` and uses `sha256sum` — it runs on WSL,
//! not host Windows. Per the project's host-vs-WSL rule, this file's lints run on WSL.
//!
//! Authoring intent, past-tense-safe: written RED before `install.sh` existed —
//! `install_sh_path()` panics with the honest "not written yet" if the file is
//! absent, and every other assertion states the CONTRACT it pins (stays true once the
//! script exists). The test asserts the installer's contract; it does NOT write it.
#![cfg(unix)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The published musl target triple the `release-ci` workflow builds for.
const TARGET: &str = "x86_64-unknown-linux-musl";

/// Locate `install.sh` at the repo root by walking UP from this crate's manifest
/// dir (`<repo>/bins/rezidnt`). Panics with the honest RED anchor if absent — the
/// message states the CONTRACT (the script must exist), staying true post-write.
fn install_sh_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR should have a repo-root grandparent (<repo>/bins/rezidnt)");
    let p = root.join("install.sh");
    assert!(
        p.exists(),
        "install.sh not found (install-script not written yet): expected the clean-room \
         installer at {} — the DR-037 `install-script` slice must deliver it",
        p.display()
    );
    p
}

/// Pure-std sha256 is not available; the `release-ci` workflow and `install.sh` both
/// use the system `sha256sum`. Shell it here so the fixture's `SHA256SUMS` is written
/// in exactly the format the installer verifies (no format skew between test + tool).
fn sha256_line(dir: &Path, name: &str) -> String {
    let out = Command::new("sha256sum")
        .arg(name)
        .current_dir(dir)
        .output()
        .expect("run sha256sum for the fixture");
    assert!(out.status.success(), "sha256sum failed for {name}");
    String::from_utf8(out.stdout).expect("sha256sum output is utf8")
}

/// Build a fixture "release" dir holding the two named binaries (each a tiny
/// executable script with distinct content, so an install can be proven to place the
/// RIGHT asset at the RIGHT name) plus a `SHA256SUMS` covering both. Returns the dir.
fn fixture_release(tmp: &Path) -> PathBuf {
    let rel = tmp.join("release");
    std::fs::create_dir_all(&rel).expect("mk release dir");
    let assets = [
        (
            format!("rezidnt-{TARGET}"),
            "#!/bin/sh\necho FIXTURE-rezidnt\n",
        ),
        (
            format!("rezidentd-{TARGET}"),
            "#!/bin/sh\necho FIXTURE-rezidentd\n",
        ),
    ];
    let mut sums = String::new();
    for (name, body) in &assets {
        std::fs::write(rel.join(name), body).expect("write fixture asset");
        sums.push_str(&sha256_line(&rel, name));
    }
    std::fs::write(rel.join("SHA256SUMS"), sums).expect("write SHA256SUMS");
    rel
}

/// Run `sh install.sh` with the given extra env, against a `file://` fixture base and
/// a fresh install dir. Returns (exit_code, stdout, stderr, install_dir).
fn run_install(tmp: &Path, base_dir: &Path, extra_env: &[(&str, &str)]) -> RunResult {
    // A per-call unique install dir so tests never collide.
    let mut h = DefaultHasher::new();
    format!("{extra_env:?}{base_dir:?}").hash(&mut h);
    let install_dir = tmp.join(format!("bin-{:x}", h.finish()));

    let mut cmd = Command::new("sh");
    cmd.arg(install_sh_path());
    cmd.env("REZIDNT_BASE_URL", format!("file://{}", base_dir.display()));
    cmd.env("REZIDNT_VERSION", "v0.0.0-fixture");
    cmd.env("REZIDNT_INSTALL_DIR", &install_dir);
    // Default to a supported platform; individual tests override.
    cmd.env("REZIDNT_OS", "Linux");
    cmd.env("REZIDNT_ARCH", "x86_64");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn sh install.sh");
    RunResult {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        install_dir,
    }
}

struct RunResult {
    code: Option<i32>,
    #[allow(dead_code)]
    stdout: String,
    stderr: String,
    install_dir: PathBuf,
}

impl RunResult {
    fn installed(&self, bin: &str) -> Option<PathBuf> {
        let p = self.install_dir.join(bin);
        p.exists().then_some(p)
    }
}

// ===========================================================================
// Criterion 1 — fetch + install BOTH binaries on PATH.
// ===========================================================================

/// Happy path: with a valid fixture release, `install.sh` places `rezidnt` and
/// `rezidentd` in the install dir, each executable and carrying the fixture asset's
/// content (proving the target-suffixed asset was installed under the bare name).
#[test]
fn installs_both_binaries_from_a_fixture_release() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rel = fixture_release(tmp.path());

    let r = run_install(tmp.path(), &rel, &[]);
    assert_eq!(
        r.code,
        Some(0),
        "install.sh must exit 0 on a valid fixture release; stderr: {}",
        r.stderr
    );

    for (bin, marker) in [
        ("rezidnt", "FIXTURE-rezidnt"),
        ("rezidentd", "FIXTURE-rezidentd"),
    ] {
        let path = r
            .installed(bin)
            .unwrap_or_else(|| panic!("install.sh must place `{bin}` in the install dir"));
        let mode = std::fs::metadata(&path)
            .expect("stat installed bin")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "installed `{bin}` must be executable (mode {mode:o})"
        );
        let content = std::fs::read_to_string(&path).expect("read installed bin");
        assert!(
            content.contains(marker),
            "installed `{bin}` must be the fixture asset (missing marker `{marker}`): {content:?}"
        );
    }
}

// ===========================================================================
// Criterion 2 — verify sha256 BEFORE install; FAIL-CLOSED, no partial install.
// ===========================================================================

/// A tampered artifact (its bytes changed AFTER `SHA256SUMS` was written) must make
/// the checksum verification fail, so `install.sh` exits non-zero and installs
/// NEITHER binary — the fail-closed, all-or-nothing guarantee (no half-install).
#[test]
fn checksum_mismatch_is_fail_closed_no_partial_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rel = fixture_release(tmp.path());

    // Corrupt one binary's bytes without updating SHA256SUMS → its recorded hash no
    // longer matches. Verification must catch this before any install happens.
    std::fs::write(
        rel.join(format!("rezidnt-{TARGET}")),
        "#!/bin/sh\necho TAMPERED\n",
    )
    .expect("tamper the fixture asset");

    let r = run_install(tmp.path(), &rel, &[]);
    assert_ne!(
        r.code,
        Some(0),
        "install.sh must FAIL (non-zero) when a fetched artifact fails its sha256 check; \
         stderr: {}",
        r.stderr
    );
    assert!(
        r.installed("rezidnt").is_none() && r.installed("rezidentd").is_none(),
        "on a checksum failure install.sh must install NEITHER binary (fail-closed, no \
         partial install) — found rezidnt={:?} rezidentd={:?}",
        r.installed("rezidnt"),
        r.installed("rezidentd"),
    );
}

// ===========================================================================
// Criterion 3 — refuse an unsupported platform in plain language, install nothing.
// ===========================================================================

/// A non-Linux OS is outside DR-037's Linux/WSL-first scope: `install.sh` must refuse
/// (non-zero), name the supported platform plainly, and install nothing (I6 — never a
/// half-install on a platform whose substrate does not run).
#[test]
fn unsupported_os_is_refused_with_no_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rel = fixture_release(tmp.path());

    let r = run_install(tmp.path(), &rel, &[("REZIDNT_OS", "Darwin")]);
    assert_ne!(
        r.code,
        Some(0),
        "install.sh must refuse a non-Linux OS (Darwin); stderr: {}",
        r.stderr
    );
    assert!(
        r.stderr.to_lowercase().contains("linux"),
        "the refusal must plainly name the supported platform (Linux/WSL); stderr: {}",
        r.stderr
    );
    assert!(
        r.installed("rezidnt").is_none() && r.installed("rezidentd").is_none(),
        "a refused platform must install nothing"
    );
}

/// An unsupported ARCH (aarch64 — explicitly deferred in DR-037) is likewise refused
/// with nothing installed. Pairs with the OS gate so BOTH legs of the scope fence are
/// pinned, not just one.
#[test]
fn unsupported_arch_is_refused_with_no_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rel = fixture_release(tmp.path());

    let r = run_install(tmp.path(), &rel, &[("REZIDNT_ARCH", "aarch64")]);
    assert_ne!(
        r.code,
        Some(0),
        "install.sh must refuse an unsupported arch (aarch64, deferred in DR-037); stderr: {}",
        r.stderr
    );
    assert!(
        r.installed("rezidnt").is_none() && r.installed("rezidentd").is_none(),
        "a refused arch must install nothing"
    );
}
