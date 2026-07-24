//! REGISTRY-CONVERGENCE ORACLE — the two structural obligations of DR-046
//! §Decision 8 that can be judged WITHOUT running a daemon (criteria C3 and the
//! host-side backstop for C4).
//!
//! HOST-RUNNABLE, and deliberately so. `bins/rezidentd/src/main.rs` declares
//! `mod runs`, `mod mcp`, `mod gates` under `#[cfg(unix)]`, so the ENTIRE
//! daemon implementation — and therefore every daemon unit test — is invisible
//! to host `/vet` on Windows. This file reads the manifest and the sources as
//! text, so it runs everywhere. DR-046 §Consequences (4) calls the repoint the
//! highest-blast-radius change in the arc; leaving all of its guards behind a
//! `#[cfg(unix)]` gate would repeat the criticism that record makes of guard
//! (c).
//!
//! ## RED MODE (stated plainly, per test)
//!
//! Both tests are ASSERT-RED today, for the right reason, and both are
//! LOAD-BEARING-ON-REGRESSION afterwards:
//!
//! - the manifest guard fails because `bins/rezidentd/Cargo.toml`
//!   `[dependencies]` has no `rezidnt-adapter-git` at all (DR-046 §Decision 8:
//!   "the daemon must take its first dependency on `rezidnt-adapter-git`");
//! - the single-emitter guard fails because `bins/rezidentd/src/runs.rs`
//!   publishes its own `worktree.allocated` today.
//!
//! ## What the second guard is, and is not (disclosure)
//!
//! It is a SOURCE-TEXT guard. It matches the literal
//! `Subject::new("worktree.allocated")` and nothing else. A daemon that
//! assembled that subject from a variable, a constant, or a format string would
//! slip past it. It is therefore a BACKSTOP, not the judge: the judge is
//! `bins/rezidentd/tests/registry_convergence_e2e.rs`, which counts the facts
//! on a replayed log. The backstop exists because the judge is `#[cfg(unix)]`
//! and the regression it catches — two `worktree.allocated` facts per
//! allocation — is the one DR-046 §Decision 8 calls the non-negotiable
//! condition of the repoint. The literal is disclosed here rather than buried
//! so a reader knows exactly how wide the window is.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Dependency KEYS declared under `[<table>]`, comments stripped, up to the next
/// table header at column 0. Mirrors the house manifest-guard helper in
/// `bench/harness/tests/testkit_dev_only.rs`.
fn dependency_keys_in_table(manifest: &str, table: &str) -> Vec<String> {
    let header = format!("[{table}]");
    let mut in_table = false;
    let mut keys = Vec::new();
    for line in manifest.lines() {
        let code = line.split('#').next().unwrap_or("");
        let trimmed = code.trim_start();
        if trimmed.starts_with('[') {
            in_table = trimmed.starts_with(&header)
                && trimmed[header.len().min(trimmed.len())..]
                    .chars()
                    .next()
                    .map(char::is_whitespace)
                    .unwrap_or(true);
            continue;
        }
        if in_table && !trimmed.is_empty() {
            let before_eq = trimmed.split('=').next().unwrap_or(trimmed).trim();
            let key = before_eq
                .split('.')
                .next()
                .unwrap_or(before_eq)
                .trim()
                .to_string();
            if !key.is_empty() {
                keys.push(key);
            }
        }
    }
    keys
}

/// CRITERION C3 — `rezidentd` takes its first PRODUCTION dependency on
/// `rezidnt-adapter-git`.
///
/// DR-046 §Decision 8 records that no crate depends on it today and that the
/// daemon's only mention is a doc comment. Until that changes, the shipped
/// `GitAdapter` registry and its double-claim guard are unreachable code and
/// DR-044 §Decision 3's conflict semantics describe a mechanism the golden path
/// does not have.
///
/// `[dependencies]`, never `[dev-dependencies]`: a dev-only edge would let the
/// tests reach the registry while production kept its private git-CLI
/// allocator, which is precisely the split-path state this slice exists to end.
/// The negative leg is asserted explicitly so that mistake reads as a failure
/// rather than as a pass.
#[test]
fn rezidentd_depends_on_the_git_adapter_in_production() {
    let manifest = read(&crate_root().join("Cargo.toml"));
    let runtime = dependency_keys_in_table(&manifest, "dependencies");
    let dev = dependency_keys_in_table(&manifest, "dev-dependencies");

    // Asserted on `[dependencies]` ONLY. A dev-only edge is a distinct and
    // worse failure than no edge at all — it would let the test suite reach the
    // registry while production kept its private git-CLI allocator, the exact
    // split-path state this slice exists to end — so the dev table is printed
    // in the message to make that case diagnosable rather than checked
    // separately (a second assertion would be dead code under the first).
    assert!(
        runtime.iter().any(|k| k == "rezidnt-adapter-git"),
        "DR-046 §Decision 8: the daemon must take its FIRST dependency on `rezidnt-adapter-git`, \
         in PRODUCTION `[dependencies]`. Without it the `RepoSubstrate` registry and its \
         sole-allocator double-claim guard are unreachable from the golden path, and DR-044 \
         §Decision 3's conflict semantics stay a description of a mechanism the code does not \
         have. A dev-only edge does NOT satisfy this. Runtime deps: {runtime:?}; dev deps: {dev:?}"
    );
}

/// CRITERION C4 (host-side backstop) — the daemon does not mint its OWN
/// `worktree.allocated` once allocation is routed through the adapter.
///
/// DR-046 §Decision 8 names this the non-negotiable condition of the repoint:
/// the adapter emits `worktree.allocated` and so does `bins/rezidentd/src/runs.rs`,
/// so repointing without silencing one side DOUBLE-EMITS. Exactly one fact per
/// allocation is what every downstream fold assumes; two would inflate every
/// worktree count derived from the log and would make `WorktreeState` fold an
/// allocation twice.
///
/// See the module header for the disclosure: this matches one literal, and the
/// counting judge lives in the `#[cfg(unix)]` e2e board.
#[test]
fn the_daemon_does_not_publish_its_own_worktree_allocated_fact() {
    const LITERAL: &str = r#"Subject::new("worktree.allocated")"#;
    let runs = crate_root().join("src").join("runs.rs");
    let source = read(&runs);

    let hits = source.matches(LITERAL).count();
    assert_eq!(
        hits,
        0,
        "DR-046 §Decision 8: exactly ONE `worktree.allocated` per allocation is the \
         non-negotiable condition of the repoint. The git adapter emits one; if \
         {} still constructs {LITERAL} itself, every allocation lands twice on the log. \
         Silence the daemon-side emitter and let the allocation fact come from the adapter, \
         carrying the envelope `workspace` and the vet verdict causation the daemon supplies \
         on the request.",
        runs.display()
    );
}
