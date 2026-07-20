//! DR-022 benchmark-harness oracle — manifest hygiene (CRITERION 5, I7) and
//! headlessness (I1).
//!
//! ORACLE HONESTY (flagged for the auditor): these are REGRESSION GUARDS, not
//! assert-red oracles. The oracle-authored skeleton manifest is ALREADY clean
//! (no `criterion`/`iai`, no `ratatui`/TUI dep), so these tests are GREEN AT
//! BOARD TIME — green-by-satisfaction against a correct skeleton, not
//! green-by-implementation. They CANNOT be made red pre-implementation without
//! the oracle itself adding the forbidden dependency (which would be the very
//! violation the guard forbids). They become load-bearing the moment an
//! implementer reaches for a bench crate or a render crate: that edit turns
//! these RED. This is the honest mechanism for a "no forbidden dep" criterion —
//! stated plainly rather than faked into a false failure. (Same pattern as
//! `golden_path.rs::dr006_agreement_emits_no_integrity_alarm`, flagged
//! green-by-absence.)

use std::path::PathBuf;

/// The manifest with comment prose stripped, so the guards scan DEPENDENCY
/// DECLARATIONS only — not the documentation that (deliberately) names the
/// forbidden crates to explain why they are forbidden. Everything from a `#`
/// to end-of-line is dropped; what remains is the actual TOML tables/keys.
fn manifest_declarations() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let raw = std::fs::read_to_string(&path).expect("bench/harness/Cargo.toml exists");
    raw.lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// CRITERION 5 (I7): NO `criterion`/`iai`/other bench dependency. The collator
/// is hand-rolled log-replay — it folds recorded facts and counts, it does not
/// micro-benchmark hot loops. A bench dep in the manifest is the exact
/// violation; this guard fails if one is ever added.
#[test]
fn no_bench_dependency_the_collator_is_hand_rolled() {
    let manifest = manifest_declarations().to_lowercase();
    for forbidden in ["criterion", "iai", "divan"] {
        assert!(
            !manifest.contains(forbidden),
            "CRITERION 5 (I7): `{forbidden}` must NOT be a dependency — the collator is \
             hand-rolled log-replay; a bench crate needs its own DR to justify the dep"
        );
    }
}

/// I1 (zero pixels): the harness is HEADLESS — no `ratatui`/TUI/terminal render
/// dependency. It is a socket/log consumer that emits a machine-readable report
/// (a struct/JSON), never a screen. A render dep here would be the I1 violation.
#[test]
fn no_tui_dependency_the_harness_is_headless() {
    let manifest = manifest_declarations().to_lowercase();
    for forbidden in ["ratatui", "crossterm", "termion", "tui"] {
        assert!(
            !manifest.contains(forbidden),
            "I1 (zero pixels): `{forbidden}` must NOT be a dependency — the benchmark harness \
             renders nothing; it emits a machine-readable report over socket/log + CLI only"
        );
    }
}
