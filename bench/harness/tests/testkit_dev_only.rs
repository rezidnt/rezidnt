//! DR-023 oracle — STRUCTURAL guard for CRITERION 4: the extracted fixture-
//! builder crate (`make_gated_project` / `gated_stub_harness` / `exec_pass_verifier`
//! / `seed_db_from_fixture`, relocated from `bins/rezidentd/tests/common/mod.rs`)
//! is TEST-SUPPORT — consumed ONLY as a `[dev-dependency]`, never in a
//! PRODUCTION `[dependencies]` table. DR-023 §Decision (C) + §Consequences:
//! "Production `DaemonDriver` does NOT depend on these … it cannot bloat the
//! binary." The fixture staging (git init, stub scripts, chmod) must never enter
//! the shipped harness dep graph.
//!
//! DR-023 §67 leaves the crate's exact NAME an implementation call
//! (`rezidnt-testkit` vs a shared module). This guard therefore asserts by the
//! PROPERTY, not a single hardcoded name: the harness's PRODUCTION `[dependencies]`
//! must stay EXACTLY the approved production set (the internal crates it folds the
//! log with, plus wire-serde). Any internal `rezidnt-*` crate the harness gains
//! beyond that set — the testkit under whatever name — leaking into runtime
//! `[dependencies]` (rather than `[dev-dependencies]`) trips the guard. As a
//! belt-and-braces name check, if the crate IS named `rezidnt-testkit`, its
//! presence in runtime deps is called out explicitly.
//!
//! HOST-RUNNABLE: parses TOML by path only — no `#[cfg(unix)]` gate — so host
//! `/vet` executes it (the WSL-only `real_driver.rs` cannot carry this guard).
//!
//! ── RED MECHANISM (dual nature — stated plainly, test honesty)
//!   1. STAY-GREEN-BY-SATISFACTION AT BOARD TIME (load-bearing-on-violation, NOT
//!      assert-red-today): the harness's CURRENT production `[dependencies]` are
//!      already the clean approved set (`rezidnt-types`, `serde`, `serde_json`) —
//!      the testkit does not exist yet, so it cannot have leaked yet. This test
//!      is GREEN right now against the correct pre-extraction manifest. Per test
//!      honesty (same class as `manifest_hygiene.rs` and
//!      `rezidnt-tui/tests/read_only.rs::crate_has_no_writer_dependency`), it
//!      CANNOT be made red pre-implementation without the oracle itself
//!      committing the very violation it forbids (declaring testkit as a prod
//!      dep). It is therefore a REGRESSION GUARD, not an assert-red oracle.
//!   2. LOAD-BEARING-ON-VIOLATION: it flips RED the moment an implementer wiring
//!      `DaemonDriver`'s fixture staging reaches for the testkit in runtime
//!      `[dependencies]` instead of `[dev-dependencies]`. That is exactly the
//!      criterion-4 violation, and this is the guard that catches it in host
//!      `/vet`.
//!
//! (The RED-until-minted, assert-red side of DR-023's structural criteria lives
//! in `client_deps_hygiene.rs`, whose judged artifact — the client manifest —
//! is absent today and so panics red. This file's judged artifact — the
//! harness's own manifest — already exists and is already clean, so honesty
//! demands it be labeled a stay-green guard rather than faked into a false red.)

use std::path::PathBuf;

/// Read this crate's (bench/harness) own manifest.
fn harness_manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    std::fs::read_to_string(&path).expect("bench/harness/Cargo.toml exists")
}

/// The dependency KEYS declared under a given table header (`[dependencies]` or
/// `[dev-dependencies]`), comments stripped, up to the next `[table]` header at
/// column 0. A key is the text left of the first `=` or `.`.
fn dependency_keys_in_table(manifest: &str, table: &str) -> Vec<String> {
    let header = format!("[{table}]");
    let mut in_table = false;
    let mut keys = Vec::new();
    for line in manifest.lines() {
        let code = line.split('#').next().unwrap_or("");
        let trimmed = code.trim_start();
        if trimmed.starts_with('[') {
            in_table = trimmed.starts_with(&header)
                // exact-match the table name so `[dependencies]` does not also
                // capture `[dev-dependencies]` (which starts with `[dev-`, so it
                // already won't — but be explicit): require the header token to
                // end right after the name.
                && trimmed[header.len()..]
                    .chars()
                    .next()
                    .map(|c| c.is_whitespace() || c == '\0')
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

/// CRITERION 4: the fixture-builder (testkit) crate never leaks into the
/// harness's PRODUCTION `[dependencies]`.
///
/// Asserted BY PROPERTY (name-agnostic per DR-023 §67): the harness's runtime
/// `[dependencies]` must stay within the approved PRODUCTION closure — the
/// internal crates it needs to FOLD THE LOG (`rezidnt-types`) plus wire-serde.
/// Any OTHER internal `rezidnt-*` crate appearing in runtime deps is the testkit
/// (or another fixture-staging crate) leaking into shipped code — the exact
/// violation. `rezidnt-client` is the ONE new internal PRODUCTION dep DR-023
/// sanctions here (DaemonDriver drives via it), so it is on the allow-list; the
/// testkit is NOT.
#[test]
fn harness_production_deps_exclude_the_fixture_builder_crate() {
    let manifest = harness_manifest();
    let runtime = dependency_keys_in_table(&manifest, "dependencies");

    // The approved PRODUCTION internal-crate closure for the harness:
    //  - rezidnt-types: the log-fold envelope types (already present).
    //  - rezidnt-client: the ONE new production dep DR-023 mints — the shared
    //    socket-driving client `DaemonDriver` runs on (allowed once the driver
    //    lands; harmless before, since it simply won't appear yet).
    // Wire-serde (serde/serde_json) is the approved non-internal set.
    const APPROVED_PROD_INTERNAL: &[&str] = &["rezidnt-types", "rezidnt-client"];
    const APPROVED_NON_INTERNAL: &[&str] = &["serde", "serde_json"];

    let mut leaked = Vec::new();
    for key in &runtime {
        let internal = key.starts_with("rezidnt-");
        if internal {
            if !APPROVED_PROD_INTERNAL.contains(&key.as_str()) {
                // An internal crate outside the approved production closure in
                // runtime deps — the fixture-builder (testkit) crate leaking
                // into shipped code, whatever its name.
                leaked.push(key.clone());
            }
        } else if !APPROVED_NON_INTERNAL.contains(&key.as_str()) {
            // Also surface any surprise non-internal prod dep (keeps the guard
            // from silently ignoring a testkit published under a non-rezidnt
            // name); the client_deps_hygiene guard owns the external-dep axis,
            // this one is scoped to the leak, so we only flag names that are
            // neither approved-serde nor internal.
            leaked.push(key.clone());
        }
    }

    assert!(
        leaked.is_empty(),
        "DR-023 CRITERION 4: the fixture-builder / testkit crate must be a \
         `[dev-dependency]` only — production `[dependencies]` of bench/harness pulled \
         non-approved crate(s) {leaked:?}. The staging surface (git init, stub scripts, chmod) \
         must NOT enter the shipped harness dep graph (DR-023 §Decision (C) / §Consequences); \
         move it under `[dev-dependencies]`."
    );
}

/// Belt-and-braces, name-specific leg: IF the fixture-builder crate is minted
/// under the sketched name `rezidnt-testkit`, it must appear ONLY under
/// `[dev-dependencies]`, never `[dependencies]`. This is redundant with the
/// property check above for that name, but pins the DR-023-sketched name
/// explicitly so a reviewer scanning for `rezidnt-testkit` sees the intent
/// directly. (If the impl picks another name, this leg is a no-op — the
/// property check above still catches the leak.)
#[test]
fn rezidnt_testkit_if_present_is_dev_only() {
    let manifest = harness_manifest();
    let runtime = dependency_keys_in_table(&manifest, "dependencies");

    assert!(
        !runtime.iter().any(|k| k == "rezidnt-testkit"),
        "DR-023 CRITERION 4: `rezidnt-testkit` (the sketched fixture-builder crate name) must \
         NOT be a production dependency of bench/harness — it is DEV-ONLY test-support. Declare \
         it under `[dev-dependencies]` (where `real_driver.rs` consumes it), never \
         `[dependencies]`."
    );
    // No positive dev-dep assertion is pinned here: the crate may not exist yet
    // (its dev-dep landing is proven by `real_driver.rs` linking the builders on
    // the WSL run), and DR-023 §67 leaves the name open. The load-bearing pin is
    // the NEGATIVE: whatever its name, it never enters production deps.
}
