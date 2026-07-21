//! C3b+c oracle (DR-026 — the L7 egress-MITM + credential-brokering slice) —
//! CRITERION 7 (degrade CLOSED, the connector/CA-absent host arm) and CRITERION 8
//! (the added LINKED deps are `rustls` + `rcgen` ONLY).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (no connector/CA needed, no #[cfg(unix)]).
//! Criterion 7 has TWO arms:
//!   - backend-ABSENT (this file): point the availability probe at a MISSING
//!     connector and assert it reports `Unavailable` with a loggable reason AND
//!     that the degrade direction is CLOSED (is_available() → false drives the
//!     no-network/no-injection branch). Testable on ANY host, no `pasta`/CA
//!     required. The daemon-side "an `egress.unavailable` fact lands, then the run
//!     keeps the sealed netns and injects nothing" wiring is pinned in the
//!     fact-fold test (`crates/rezidnt-fabric/tests/egress_unavailable_fold_c3bc.rs`).
//!   - backend-PRESENT (WSL only): real mediated egress + termination + injection
//!     in `crates/rezidnt-run/tests/egress_mediation_c3bc.rs` (#[cfg(unix)]).
//!
//! Criterion 8 is a manifest scan — platform-neutral, runs everywhere.
//!
//! GREEN (c3bc-decide, DR-027): `PastaProxy::availability` is implemented, so the
//! degrade-probe tests pin the real CLOSED-degrade decision. The dependency-scan
//! test is a STRUCTURAL guard (like C3a's no-bwrap-crate scan): it constrains the
//! delta to `rustls`+`rcgen` and FAILS if any OTHER new linked crate is added to
//! satisfy the slice.

use std::path::PathBuf;

use rezidnt_run::egress::{EgressAvailability, EgressProxy, PastaProxy};

/// CRITERION 7 (backend-absent, degrade CLOSED) — the availability probe pointed
/// at a MISSING connector reports `Unavailable` with a loggable reason, NEVER a
/// panic and NEVER a false `Available`. This is the INVERSE of C3a's loud-OPEN
/// degrade (DR-026 §Decision): a missing backend announces itself so the daemon
/// logs `egress.unavailable` and degrades CLOSED — keeps the sealed netns (no
/// network), injects nothing. A false `Available` here would be a silent OPEN —
/// the exact "never unmediated egress, never a leaked secret" failure.
///
/// RED: `PastaProxy::availability` is `todo!()` → panic. Green once the probe
/// (connector + CA) lands and returns `Unavailable` for an absent connector.
#[test]
fn missing_connector_is_unavailable_with_a_reason_not_a_panic() {
    let proxy = PastaProxy {
        connector_bin: Some("/nonexistent/definitely-not-pasta-xyz".to_string()),
    };
    match proxy.availability() {
        EgressAvailability::Unavailable { reason } => {
            assert!(
                !reason.trim().is_empty(),
                "an unavailable egress backend carries a LOGGABLE reason (the \
                 `egress.unavailable` fact's `reason`) so the CLOSED degrade is interrogable \
                 (I6, CRITERION 7)"
            );
        }
        EgressAvailability::Available => panic!(
            "a MISSING connector reported Available — that is the silent-OPEN trap the \
             degrade-CLOSED contract forbids (unmediated egress / a leaked secret; I6, \
             CRITERION 7)"
        ),
    }
}

/// CRITERION 7 — `EgressAvailability::is_available()` is honest: an `Unavailable`
/// verdict is not available. The daemon branches on this to choose mediated-egress
/// vs CLOSED-degrade (sealed netns, no injection); a wrong answer here is a silent
/// open egress and possibly a leaked secret — the catastrophic direction.
#[test]
fn unavailable_is_not_available_and_degrade_is_closed() {
    let unavailable = EgressAvailability::Unavailable {
        reason: "pasta not found on PATH".to_string(),
    };
    assert!(
        !unavailable.is_available(),
        "an Unavailable egress backend must not report is_available() — that decides the \
         CLOSED-degrade branch (no network, no injection; CRITERION 7)"
    );
    assert!(EgressAvailability::Available.is_available());
}

/// CRITERION 8 (I7) — the added LINKED deps are `rustls` + `rcgen` ONLY: the
/// connector (`pasta`) is EXEC'd like `bwrap`/the git-CLI (zero new linked crate),
/// TLS termination links `rustls`, and the CA/leaf certs link `rcgen` — and
/// NOTHING ELSE new (DR-026 §Consequences, invariant I7 ⚠️). Mirrors C3a's
/// no-bwrap-crate scan, INVERTED: instead of a pure forbid-list, this scans for
/// OTHER TLS/PKI/proxy/connector-binding crates that would signal an unowned dep
/// delta — the slice's honest cost is exactly two crates, no smuggled third.
///
/// This holds GREEN today (no TLS/PKI crate is linked yet — the seam is
/// `todo!()`). It stays a STRUCTURAL guard: when the implementer adds `rustls` +
/// `rcgen`, those two are EXPECTED; any OTHER new TLS/PKI/connector crate here is
/// the I7 regression this guards ("the DR states the added linked deps and why —
/// no other").
#[test]
fn added_linked_deps_are_rustls_and_rcgen_only() {
    // The ONLY two linked deps DR-026 ratifies for this slice. Anything else in
    // the family below is an unowned delta.
    const ALLOWED_NEW_TLS_PKI: &[&str] = &["rustls", "rcgen"];

    // Known TLS/PKI/connector crates that would be a NEW linked dep if added to
    // satisfy the slice OTHER than the two allowed. `pasta`/`slirp4netns` are
    // EXEC'd tools, not crates, so they never appear here. A hand-rolled MITM is
    // forbidden by the DR (rustls/rcgen are the correct cost), so a raw-crypto or
    // alternate-TLS crate here is the regression.
    const WATCHED_TLS_PKI_CRATES: &[&str] = &[
        "rustls",      // ALLOWED (TLS termination)
        "rcgen",       // ALLOWED (CA + leaf certs)
        "native-tls",  // an alternate TLS stack — not the ratified choice
        "openssl",     // hand-rolled-adjacent PKI/TLS — forbidden delta
        "openssl-sys", // its -sys sibling
        "boring",      // BoringSSL binding — not ratified
        "boring-sys",  //
        "webpki",      // a cert-verification crate not named in the DR
        "x509-parser", // ad-hoc cert parsing — not the ratified rcgen path
        "hyper-proxy", // a proxy crate — the proxy is ours (rustls), not linked
        "hudsucker",   // a MITM-proxy crate — the DR builds the proxy, not links one
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
            for crate_name in WATCHED_TLS_PKI_CRATES {
                let is_dep_key = trimmed.starts_with(&format!("{crate_name} "))
                    || trimmed.starts_with(&format!("{crate_name}="))
                    || trimmed.starts_with(&format!("{crate_name}."))
                    || trimmed.starts_with(&format!("\"{crate_name}\""));
                if is_dep_key && !ALLOWED_NEW_TLS_PKI.contains(crate_name) {
                    panic!(
                        "I7 violation (CRITERION 8): TLS/PKI crate `{crate_name}` is a build \
                         dependency in {} — DR-026 ratifies EXACTLY `rustls` + `rcgen` as the \
                         slice's linked-dep delta, no other. The connector is EXEC'd (zero \
                         linked dep) and the proxy is BUILT on rustls/rcgen, never a smuggled \
                         third crate. Line: {line}",
                        manifest.display()
                    );
                }
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
