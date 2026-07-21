//! c3-egress-fold oracle (DR-029) — CRITERION 1 (HOST-provable): the project-spec
//! `[egress]` block parses. `allowlist = ["github.com", …]` + the `[egress.secrets]`
//! `host -> secret_ref` table parse into the spec types; an ABSENT block folds to an
//! EMPTY allowlist (deny-all — absent NEVER means open, DR-029 §Decision 1); a
//! MALFORMED block is an honest `RunError::Spec`, never a silently-empty allowlist
//! that would drop the deny-all boundary.
//!
//! Mirrors the existing spec-parse seam (`tests/spec.rs`, `tests/spec_role.rs`) and
//! the `[egress.secrets]` map holds LABELS only (a `secret_ref`, never a value —
//! repo-safe, DR-029 §Decision 1).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure TOML parse of the spec types, no
//! `#[cfg(unix)]`, no netns). Runs on every host, Windows /vet included.
//!
//! ## RED MODE — COMPILE-RED. `ProjectSpec` has no `egress` field and there is no
//! `EgressSpec`/`EgressSecrets` type on `rezidnt_run::spec` yet. This file cannot
//! compile until the implementer adds the `[egress]` block to `ProjectSpec`
//! (alongside `gates`, `spec.rs:14-23`) — that IS the failing state.
//!
//! IMPLEMENTER ADDS (the seam this pins):
//!   - `#[serde(default)] pub egress: EgressSpec` on `ProjectSpec` (absent ⇒ default
//!     ⇒ empty allowlist ⇒ deny-all).
//!   - `pub struct EgressSpec { #[serde(default)] pub allowlist: Vec<String>,
//!     #[serde(default)] pub secrets: BTreeMap<String, String> }` where `secrets`
//!     is the `[egress.secrets]` `host -> secret_ref` LABEL map.
//!   - a malformed `[egress]` (wrong-typed `allowlist`, etc.) surfaces as
//!     `RunError::Spec`, never a silently-empty allowlist.

use rezidnt_run::RunError;
use rezidnt_run::spec::ProjectSpec;

/// A spec carrying a non-empty `[egress]` allowlist + an `[egress.secrets]`
/// host->secret_ref LABEL map. The values are secret_refs (labels), NEVER secret
/// values — repo-safe (DR-029 §Decision 1).
const SPEC_WITH_EGRESS: &str = r#"
[project]
name = "acme-checkout"
repo = "."

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"

[egress]
allowlist = ["github.com", "api.github.com"]

[egress.secrets]
"github.com" = "gh_token"
"api.github.com" = "gh_api_token"
"#;

/// CRITERION 1 (positive) — the `[egress]` block parses into the spec: the allowlist
/// hosts AND the `[egress.secrets]` `host -> secret_ref` LABEL map are both readable.
/// The secrets map carries the REF labels (`gh_token`), never a value — repo-safe.
///
/// COMPILE-RED until `ProjectSpec.egress` (an `EgressSpec` with `allowlist` +
/// `secrets`) exists.
#[test]
fn egress_block_parses_allowlist_and_secret_ref_map() {
    let spec = ProjectSpec::from_toml_str(SPEC_WITH_EGRESS).expect("spec with [egress] must parse");

    assert_eq!(
        spec.egress.allowlist,
        vec!["github.com".to_string(), "api.github.com".to_string()],
        "the [egress] allowlist hosts parse in declared order"
    );
    // The [egress.secrets] map is host -> secret_ref (a LABEL, never a value).
    assert_eq!(
        spec.egress.secrets.get("github.com").map(String::as_str),
        Some("gh_token"),
        "[egress.secrets] maps a host to a secret_ref LABEL (repo-safe — never a value, DR-029)"
    );
    assert_eq!(
        spec.egress
            .secrets
            .get("api.github.com")
            .map(String::as_str),
        Some("gh_api_token")
    );
    // Non-vacuous: a host with no secret mapping is absent, not synthesized.
    assert!(
        !spec.egress.secrets.contains_key("evil.example.com"),
        "an unmapped host has no secret_ref — the map is exactly what was declared"
    );
}

/// CRITERION 1 (the honesty leg — deny-all default, load-bearing) — a spec with NO
/// `[egress]` block folds to an EMPTY allowlist + an EMPTY secrets map: ABSENT means
/// DENY-ALL, never open (DR-029 §Decision 1; the DR-028 default preserved). If a
/// future change made an absent block mean "allow all", this fails first.
///
/// COMPILE-RED until the field exists; then this makes an absent-means-open
/// regression a test failure.
#[test]
fn absent_egress_block_folds_deny_all_empty_allowlist() {
    let spec = ProjectSpec::from_toml_str(
        r#"
[project]
name = "tiny"
repo = "."

[[agent]]
name = "a"
harness = "claude-code"
worktree = "auto"
"#,
    )
    .expect("a spec with no [egress] block still parses");

    assert!(
        spec.egress.allowlist.is_empty(),
        "CRITERION 1 VIOLATION: an ABSENT [egress] block did not fold to an EMPTY allowlist — \
         absent MUST mean deny-all, never open (DR-029 §Decision 1, DR-028 default preserved)"
    );
    assert!(
        spec.egress.secrets.is_empty(),
        "an absent [egress] block brokers no secrets (empty [egress.secrets] map)"
    );
}

/// CRITERION 1 (the honesty leg — malformed is an ERROR, never silently empty) — a
/// MALFORMED `[egress]` block (here `allowlist` given as a string, not an array) is
/// an honest `RunError::Spec`, NEVER a silently-empty allowlist. A silent empty
/// would drop the deny-all boundary the same way the DR-020 admin-permit precedent
/// refuses a silently-empty admin layer.
///
/// COMPILE-RED until the field exists; then this makes a swallow-the-error
/// regression a test failure.
#[test]
fn malformed_egress_block_is_an_honest_error_never_silently_empty() {
    let result = ProjectSpec::from_toml_str(
        r#"
[project]
name = "bad"
repo = "."

[egress]
allowlist = "github.com"
"#,
    );
    match result {
        Err(RunError::Spec(msg)) => assert!(
            !msg.is_empty(),
            "the malformed-[egress] error carries a diagnostic message"
        ),
        Ok(spec) => panic!(
            "CRITERION 1 VIOLATION: a malformed [egress] block (allowlist as a string, not an \
             array) parsed SILENTLY to allowlist={:?} — a malformed block MUST be an honest \
             RunError::Spec, never a silently-empty (deny-all-dropping) allowlist (DR-029 \
             §Decision 1)",
            spec.egress.allowlist
        ),
        other => panic!("expected RunError::Spec on a malformed [egress] block, got {other:?}"),
    }
}
