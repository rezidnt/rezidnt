//! C3a oracle (DR-025 — the Linux OS-sandbox slice) — CRITERION 4 (bwrap-absent
//! arm: loud degrade) and CRITERION 5 (no new LINKED dependency).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (no bwrap needed, no #[cfg(unix)]).
//! Criterion 4 has TWO arms:
//!   - bwrap-ABSENT (this file): point the availability probe at a MISSING binary
//!     and assert it reports `Unavailable` with a loggable reason — testable on
//!     ANY host, no bwrap required. The daemon-side "a `sandbox.unavailable` fact
//!     lands, then the run degrades" wiring is pinned in the fact-fold test
//!     (`crates/rezidnt-fabric/tests/sandbox_unavailable_fold_c3a.rs`) and the
//!     daemon integration path.
//!   - bwrap-PRESENT (WSL only): real confinement in
//!     `crates/rezidnt-run/tests/sandbox_bwrap_confinement_c3a.rs` (#[cfg(unix)]).
//!
//! Criterion 5 is a manifest scan — platform-neutral, runs everywhere.
//!
//! RED MODE: **assert-red** for the degrade-probe tests (`probe_backend` is
//! `todo!()` → panic). The dependency-scan test is a STRUCTURAL guard (like the
//! exec no-vendored-engine guard): it holds GREEN today and must STAY green — if
//! the implementer adds a `bwrap`-binding crate to satisfy the slice, it fails.

use std::path::PathBuf;

use rezidnt_run::sandbox::{Availability, probe_backend};

/// CRITERION 4 (bwrap-absent) — the availability probe pointed at a MISSING
/// binary reports `Unavailable` with a loggable reason, NEVER a panic and NEVER
/// a false `Available`. This is the honest-enforcement default (DR-025 §Decision,
/// memo scenario #9): a missing backend announces itself so the daemon can log
/// `sandbox.unavailable` and degrade LOUDLY.
///
/// RED: `probe_backend` is `todo!()` → panic. Green once the which/PATH lookup
/// lands and returns `Unavailable` for an absent binary (never a crash).
#[test]
fn missing_backend_is_unavailable_with_a_reason_not_a_panic() {
    // A path guaranteed absent — the probe must handle "no such binary" as a
    // VERDICT, never an unwrap/panic (the could-not-run discipline).
    let availability = probe_backend("/nonexistent/definitely-not-bwrap-xyz");
    match availability {
        Availability::Unavailable { reason } => {
            assert!(
                !reason.trim().is_empty(),
                "an unavailable backend carries a LOGGABLE reason (the `sandbox.unavailable` \
                 fact's `reason` field) so the degrade is interrogable (I6, CRITERION 4)"
            );
        }
        Availability::Available => panic!(
            "a MISSING backend binary reported Available — that is the silent-allow trap \
             the loud-degrade contract forbids (I6, CRITERION 4)"
        ),
    }
}

/// CRITERION 4 — `Availability::is_available()` is honest: an `Unavailable`
/// verdict is not available. The daemon branches on this to choose confined vs
/// loud-degrade spawn; a wrong answer here is a silent unsandboxed spawn.
#[test]
fn unavailable_is_not_available() {
    let unavailable = Availability::Unavailable {
        reason: "bwrap not found on PATH".to_string(),
    };
    assert!(
        !unavailable.is_available(),
        "an Unavailable backend must not report is_available() — that decides the \
         degrade branch (CRITERION 4)"
    );
    assert!(Availability::Available.is_available());
}

/// CRITERION 5 (I7) — no `bwrap`-BINDING crate is a build dependency: the sandbox
/// mechanism EXECs `bwrap` like the git-CLI, adding zero new LINKED crate
/// (DR-025 §Decision, invariant I7). Mirrors the exec no-vendored-engine guard:
/// scan every workspace manifest and assert no known bubblewrap/namespace FFI
/// crate appears as a dependency.
///
/// This holds GREEN today. It FAILS the moment someone satisfies C3a by linking
/// a bwrap/namespace crate instead of exec'ing the tool — the structural
/// enforcement of "exec, not link".
#[test]
fn no_bwrap_binding_crate_is_a_build_dependency() {
    // Known crates that LINK bubblewrap / do namespace-setup in-process (an FFI
    // or a Rust-native sandbox lib). Exec'ing the `bwrap` TOOL needs none of
    // these; adding one to pass the slice is the I7 regression this guards.
    const FORBIDDEN_SANDBOX_CRATES: &[&str] = &[
        "bubblewrap",    // any bubblewrap-binding crate
        "libbubblewrap", // defensive FFI spelling
        "birdcage",      // Rust-native cross-platform sandbox
        "extrasafe",     // seccomp/landlock in-process sandbox
        "landlock",      // Rust landlock bindings (in-process LSM)
        "syscallz",      // seccomp filter builder
        "seccompiler",   // seccomp BPF compiler
    ];

    let root = workspace_root();
    let mut manifests: Vec<PathBuf> = Vec::new();
    collect_cargo_tomls(&root, &mut manifests);
    assert!(
        !manifests.is_empty(),
        "expected workspace Cargo.toml manifests under {}",
        root.display()
    );

    for manifest in &manifests {
        let text = std::fs::read_to_string(manifest)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue; // a comment / doc mention is not a build dependency
            }
            for crate_name in FORBIDDEN_SANDBOX_CRATES {
                let is_dep_key = trimmed.starts_with(&format!("{crate_name} "))
                    || trimmed.starts_with(&format!("{crate_name}="))
                    || trimmed.starts_with(&format!("{crate_name}."))
                    || trimmed.starts_with(&format!("\"{crate_name}\""));
                assert!(
                    !is_dep_key,
                    "I7 violation (CRITERION 5): sandbox-binding crate `{crate_name}` is a build \
                     dependency in {} — C3a EXECs `bwrap` like the git-CLI, it does not LINK a \
                     sandbox crate (DR-025 §Decision). Line: {line}",
                    manifest.display()
                );
            }
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Recursively collect `Cargo.toml` files, skipping `target/` and `.git`.
fn collect_cargo_tomls(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == ".git" {
                continue;
            }
            collect_cargo_tomls(&path, out);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            out.push(path);
        }
    }
}
