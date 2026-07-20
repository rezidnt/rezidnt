//! DR-023 oracle — STRUCTURAL guard for CRITERION 1: the extracted shared
//! socket-driving client `crates/rezidnt-client` introduces NO new external
//! (third-party) dependency. It sits on internal `rezidnt-*` crates + `std`
//! (the UDS path) only — DR-023 §Decision + §Invariant-fit I7: "NO new external
//! dependency. One internal crate (`rezidnt-client`) + a dev-only test-support
//! crate; the client sits on `rezidnt-proto` + std UDS."
//!
//! WHY THIS LIVES IN bench/harness/tests: the harness is the crate whose
//! `DaemonDriver` consumes `rezidnt-client`; a dep-graph regression in the
//! client is a regression in the harness's own closure, so the guard rides here
//! (and is HOST-RUNNABLE — it parses TOML by path, no `#[cfg(unix)]` gate, so
//! host `/vet` executes it, unlike the WSL-only `real_driver.rs`).
//!
//! ── RED MECHANISM (dual nature — stated plainly for test honesty & the auditor)
//! This guard has an HONEST TWO-PHASE life:
//!   1. RED-TODAY (absence-red): `crates/rezidnt-client/Cargo.toml` does NOT
//!      exist yet. `client_manifest()` reads it BY PATH and PANICS with a
//!      tracking message when the file is missing — so this test is genuinely
//!      RED right now, and cannot be green until the implementer mints the crate.
//!      It is NOT a green-by-satisfaction stay-green guard at board time; it
//!      fails because the artifact it judges is absent.
//!   2. LOAD-BEARING-ON-VIOLATION (post-mint): once the crate exists with a
//!      clean manifest the test goes green, and thereafter it flips RED the
//!      instant a new EXTERNAL dependency is declared in the client's runtime
//!      `[dependencies]` (the exact I7 violation DR-023 forbids). The
//!      allow-list below is the approved closure; anything outside it that is
//!      not a `rezidnt-*` internal path dep trips the guard.
//!
//! FALSIFIABILITY: this is genuinely falsifiable — add e.g. `nix` or `bytes` to
//! `rezidnt-client`'s `[dependencies]` and this test goes RED (an unknown crate
//! name that is not an internal `rezidnt-*` dep). It cannot be satisfied by an
//! empty/absent manifest (that path panics), and the positive assertion below
//! (the client MUST depend on `rezidnt-proto`) forbids a vacuous "no external
//! deps because no deps at all" pass.

use std::path::PathBuf;

/// Absolute path to the (as-yet-unminted) client crate's manifest.
fn client_manifest_path() -> PathBuf {
    // bench/harness/ -> workspace root -> crates/rezidnt-client/Cargo.toml
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/rezidnt-client/Cargo.toml")
}

/// Read the client crate's manifest, PANICKING with a tracking note if it does
/// not exist yet. This panic IS the RED-today mechanism (phase 1 above): until
/// the implementer mints `crates/rezidnt-client`, there is nothing to judge and
/// the guard fails honestly rather than passing vacuously.
fn client_manifest() -> String {
    let path = client_manifest_path();
    std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "DR-023 CRITERION 1 (RED until the crate is minted): expected the shared \
             socket-driving client manifest at {} — it does not exist yet. The implementer \
             mints `crates/rezidnt-client` (internal lib on rezidnt-proto + std UDS, NO new \
             external dependency); this guard then turns green and stays load-bearing against \
             any external dep added later.",
            path.display()
        )
    })
}

/// The runtime `[dependencies]` table text of the client manifest, comments
/// stripped (everything from a `#` to end-of-line dropped) so the scan reads
/// DEPENDENCY DECLARATIONS only, never doc prose that may name a forbidden crate
/// to explain why it is forbidden. Stops at the next `[table]` header at column
/// 0 (so `[dev-dependencies]`/`[package]` are excluded — dev-deps do not ship in
/// the client's production closure).
fn client_runtime_dependency_lines() -> Vec<String> {
    let raw = client_manifest();
    let mut in_deps = false;
    let mut out = Vec::new();
    for line in raw.lines() {
        // Strip comment prose first so a `#`-commented crate name is never read
        // as a declaration (the manifest_hygiene.rs precedent).
        let code = line.split('#').next().unwrap_or("");
        let trimmed = code.trim_start();
        if trimmed.starts_with('[') {
            in_deps = trimmed.starts_with("[dependencies]");
            continue;
        }
        if in_deps && !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// The dependency KEY (crate/table name) declared on a manifest line, i.e. the
/// text left of the first `=` or `.` — `serde.workspace = true` -> `serde`,
/// `rezidnt-proto = { ... }` -> `rezidnt-proto`, `tokio = "1"` -> `tokio`.
fn dependency_key(line: &str) -> String {
    let before_eq = line.split('=').next().unwrap_or(line).trim();
    // `foo.workspace = true` style: the key is the segment before the first dot.
    before_eq
        .split('.')
        .next()
        .unwrap_or(before_eq)
        .trim()
        .to_string()
}

/// CRITERION 1 (I7): `rezidnt-client`'s runtime dependencies are internal
/// `rezidnt-*` path crates + std ONLY — NO new external/third-party crate.
///
/// The allow-list is the ALREADY-APPROVED closure DR-023 sanctions for a socket
/// client that speaks the existing wire: `rezidnt-proto` (the wire types the
/// client rides — REQUIRED, asserted positively below) and its sibling internal
/// crates that the driving path may legitimately read (types/state for decoding
/// facts off the tail). `serde`/`serde_json` are the wire-serde already carried
/// transitively via `rezidnt-types`/`rezidnt-proto` and are in the workspace
/// approved set — permitted here as the ONLY non-internal names, since the
/// client necessarily (de)serializes frames. ANY name outside this set that is
/// not an internal `rezidnt-*` crate is a NEW external dependency and trips the
/// guard.
#[test]
fn client_declares_no_new_external_dependency() {
    let lines = client_runtime_dependency_lines();

    // Non-internal crate names the client is permitted to carry: the wire-serde
    // already in the workspace approved set and transitively present via the
    // proto/types crates. Everything else non-`rezidnt-*` is a NEW external dep.
    const APPROVED_NON_INTERNAL: &[&str] = &["serde", "serde_json"];

    let mut offenders = Vec::new();
    for line in &lines {
        let key = dependency_key(line);
        if key.is_empty() {
            continue;
        }
        let internal = key.starts_with("rezidnt-");
        let approved = APPROVED_NON_INTERNAL.contains(&key.as_str());
        if !internal && !approved {
            offenders.push(key);
        }
    }

    assert!(
        offenders.is_empty(),
        "DR-023 CRITERION 1 (I7): `rezidnt-client` must add NO new external dependency — its \
         runtime [dependencies] may only be internal `rezidnt-*` crates + the already-approved \
         wire-serde (serde/serde_json) + std UDS. New external dep(s) declared: {offenders:?}. \
         A third-party crate here needs its own DR (DR-023 §Invariant-fit I7)."
    );

    // Positive pin: the client MUST ride the existing wire types, so the
    // "no external deps" pass is not vacuously satisfied by an empty deps table
    // (a client that depends on nothing is not the shared socket client DR-023
    // mints — it would be de-facto proof the extraction did not happen).
    let has_proto = lines
        .iter()
        .any(|l| dependency_key(l).starts_with("rezidnt-proto"));
    assert!(
        has_proto,
        "DR-023 §Decision: `rezidnt-client` speaks the EXISTING wire — it must depend on \
         `rezidnt-proto` (socket_path / decode_hello / check_hello / encode_request). A client \
         with no rezidnt-proto dep did not extract the shared driving seam."
    );
}
