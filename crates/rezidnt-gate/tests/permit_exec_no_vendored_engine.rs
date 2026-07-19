//! SP3 oracle — I7 structural guard: no vendored policy-engine binary enters the
//! build; the reference judge is a LOCAL ARGV (DR-015 §Decision 4; design §5 /
//! §8 crit 6). rezidnt ships the DISPATCH, not OPA/Cedar. This is a real
//! assertion (not a note): it scans the workspace manifests for a policy-engine
//! dependency and asserts the reference policy is a committed argv fixture.
//!
//! Not `#[cfg(unix)]`: the manifest scan + fixture-presence check are
//! platform-neutral (the guard should hold on every host that builds rezidnt).

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// CRITERION 6 (I7) — no policy-engine crate is a build dependency. A vendored
/// OPA/Cedar/Rego engine would show up as a Cargo dependency; assert none of the
/// known policy-engine crate names appears in ANY workspace manifest. If SP3 (or
/// anyone) tried to satisfy the acceptance by bundling an engine, this fails.
#[test]
fn no_policy_engine_crate_is_a_build_dependency() {
    // Known policy-engine crate names (the ones a "bring-your-own-DSL" temptation
    // would reach for). This list is the guard; extend it if a new engine crate
    // appears in the ecosystem.
    const FORBIDDEN_ENGINE_CRATES: &[&str] = &[
        "regorus",      // Rego/OPA policy engine (Rust)
        "cedar-policy", // AWS Cedar (Rust)
        "cedar_policy", // underscore spelling, defensive
        "open-policy-agent",
        "opa",
    ];

    let root = workspace_root();
    let mut manifests: Vec<PathBuf> = Vec::new();
    collect_cargo_tomls(&root, &mut manifests);
    assert!(
        !manifests.is_empty(),
        "expected to find workspace Cargo.toml manifests under {}",
        root.display()
    );

    for manifest in &manifests {
        let text = std::fs::read_to_string(manifest)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
        // Only inspect dependency-table lines: a crate name appearing as a
        // dependency key. A comment or doc mention is not a build dependency.
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            for engine in FORBIDDEN_ENGINE_CRATES {
                // `name = ...` or `name.workspace = ...` or `"name" = ...`
                let is_dep_key = trimmed.starts_with(&format!("{engine} "))
                    || trimmed.starts_with(&format!("{engine}="))
                    || trimmed.starts_with(&format!("{engine}."))
                    || trimmed.starts_with(&format!("\"{engine}\""));
                assert!(
                    !is_dep_key,
                    "I7 violation (CRITERION 6): policy-engine crate `{engine}` is a build \
                     dependency in {} — SP3 ships the exec DISPATCH, not a bundled engine \
                     (DR-015 §Decision 4). Line: {line}",
                    manifest.display()
                );
            }
        }
    }
}

/// CRITERION 6 (I7) — the reference judge is a committed LOCAL ARGV, not a
/// vendored engine: the reference policy program exists under
/// `spec/fixtures/policies/` and is a plain script (bytes on disk, dispatched as
/// argv), proving SP3's acceptance is met by a local program the operator could
/// swap, never a build-time engine.
#[test]
fn reference_policy_is_a_committed_local_argv() {
    let policy = workspace_root().join("spec/fixtures/policies/permit_tool_policy.sh");
    assert!(
        policy.is_file(),
        "the reference permit policy must be a committed local argv fixture at {} \
         (I7: a local program, not a bundled engine — CRITERION 6)",
        policy.display()
    );
    let body = std::fs::read_to_string(&policy).expect("read reference policy");
    assert!(
        body.contains("verdict"),
        "the reference policy speaks the §8 VerifierOutput contract (emits a `verdict`) — \
         it is the deterministic judge, a local argv (CRITERION 6): {}",
        policy.display()
    );
}

/// Recursively collect `Cargo.toml` files, skipping `target/` build dirs.
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
