//! DR-059 HOST-SIDE BACKSTOP — source-text guards for the gate-side patch
//! wiring, on the `registry_convergence_structure.rs` precedent and for the
//! same reason: `bins/rezidentd/src/main.rs` declares `mod runs`/`mod gates`
//! under `#[cfg(unix)]`, so the behavioral judge
//! (`dr059_patch_e2e.rs`) is invisible to host `/vet`. This file reads the
//! sources as text and runs everywhere.
//!
//! ## What these guards are, and are not (disclosure, house rule)
//!
//! SOURCE-TEXT guards, honestly labeled: the behavior IS reachable — on unix,
//! where `dr059_patch_e2e.rs` judges it end to end — and these are the
//! host-visible BACKSTOP, not the judge. Each matches a quoted literal and
//! nothing else; an implementer assembling the key or mime from fragments
//! slips past them and is caught by the unix judge. The literals are
//! disclosed here so a reader knows exactly how wide the window is.
//!
//! ## Mutation proof (prove-your-guard-by-mutation, house rule)
//!
//! The tree at cut time IS the mutant: the prose (this file, DR-059, the
//! ontology bullets) exists and the code does not — grep-verified this
//! session, the quoted literal `"patch"` appears in neither `gates.rs` nor
//! `runs.rs`, and `"text/x-diff-summary"` appears nowhere in `gates.rs` —
//! and every guard below FAILS against it. That is the
//! delete-the-code-keep-the-prose configuration, observed rather than
//! simulated. After landing, re-running the deletion re-reds them.
//!
//! RED MODE: ASSERT-RED, all three, per the mutation-proof paragraph above.

use std::path::PathBuf;

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Criterion 3 backstop — `gates.rs` pins the summary under the corrected
/// mime literal. (The full behavioral claim — bytes unchanged, mime moved —
/// is the unix judge's; this pins that the corrected literal exists
/// gate-side at all, which today it does not.)
#[test]
fn gates_pins_the_summary_under_the_corrected_mime() {
    assert!(
        src("gates.rs").contains("\"text/x-diff-summary\""),
        "bins/rezidentd/src/gates.rs must pin the diff summary under the \
         corrected `text/x-diff-summary` mime (DR-059 §Decision 4). The old \
         `text/x-diff` label is FALSE on summary bytes and load-bearing on \
         cas_read's mime echo. Behavioral judge: dr059_patch_e2e.rs (unix)."
    );
}

/// Criterion 1 backstop (gate-time emit) — `runs.rs` emits the `patch` key
/// on the gate-time `diff.ready` payload.
#[test]
fn the_gate_time_diff_ready_carries_a_patch_key() {
    assert!(
        src("runs.rs").contains("\"patch\""),
        "bins/rezidentd/src/runs.rs (run_pre_merge's gate-time diff.ready \
         payload) must carry the quoted `\"patch\"` key — the second CAS ref \
         of real `git diff` bytes (DR-059 §Decision 1; ontology \
         `diff.ready.patch?`). Behavioral judge: dr059_patch_e2e.rs (unix)."
    );
}

/// Criterion 2 backstop — `gates.rs::merge_worktree` republishes `patch` on
/// `diff.merged`, exactly as `diff` already rides through.
#[test]
fn merge_worktree_republishes_the_patch_key() {
    assert!(
        src("gates.rs").contains("\"patch\""),
        "bins/rezidentd/src/gates.rs (merge_worktree's diff.merged payload) \
         must republish the quoted `\"patch\"` key — the SAME gate-time ref \
         threaded through as `diff` is (DR-059 §Decision 1; ontology \
         `diff.merged.patch?`). Behavioral judge: dr059_patch_e2e.rs (unix)."
    );
}
