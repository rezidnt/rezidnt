//! SP4a oracle — CRITERION 1 (parse): `AgentSpec` parses the additive
//! `role` field (DR-016 §Decision 2; design permit-roles-delegation-sp4 §4;
//! ontology `agent.spawned.role?`).
//!
//! A `[[agent]]` block with `role = "reviewer"` parses to
//! `AgentSpec.role == Some("reviewer")`; an agent with no `role` parses to
//! `None` — ABSENT, never a synthesized default like `"contributor"` (DR-012
//! declared-vs-absent discipline; ontology line 195: "never synthesized to a
//! default"). Mirrors the existing `bare` / `harness_version` spec-parse
//! precedent (the S1 `tests/spec.rs` board + `spec.rs` field docs).
//!
//! API SHAPE THE IMPLEMENTER MUST MATCH: a new field on `AgentSpec`,
//! `#[serde(default)] pub role: Option<String>` — additive, mirroring
//! `harness_version: Option<String>` (crates/rezidnt-run/src/spec.rs:48).
//!
//! RED MODE — COMPILE-RED: `AgentSpec` has no `role` field today
//! (crates/rezidnt-run/src/spec.rs:27-53), so `agent.role` does not resolve.
//! This file does not compile until the field lands; that IS the failing
//! state. Once the field exists (with `#[serde(default)]`), the parse
//! assertions decide green.

use rezidnt_run::spec::ProjectSpec;

/// CRITERION 1 (positive leg) — a `[[agent]]` block declaring `role = "reviewer"`
/// parses to `Some("reviewer")`. The role is taken VERBATIM (rezidnt mints no
/// role vocabulary — DR-016 does not design the role taxonomy; ontology line
/// 195 "an opaque string the policy interprets").
///
/// COMPILE-RED until `AgentSpec.role` exists.
#[test]
fn agent_spec_parses_declared_role() {
    let spec = ProjectSpec::from_toml_str(
        r#"
[project]
name = "roles"
repo = "."

[[agent]]
name = "rev"
harness = "claude-code"
worktree = "auto"
role = "reviewer"
"#,
    )
    .expect("spec with a role must parse");
    assert_eq!(
        spec.agents[0].role.as_deref(),
        Some("reviewer"),
        "a declared `role = \"reviewer\"` parses to Some(\"reviewer\") verbatim \
         (DR-016 §Decision 2; opaque string, no rezidnt-minted vocabulary)"
    );
}

/// CRITERION 1 (the honesty leg — load-bearing) — an agent with NO `role` key
/// parses to `None`. Absence is the honest "no role declared", NEVER synthesized
/// to a default (DR-012; ontology `agent.spawned.role?`: "never synthesized to a
/// default like `\"contributor\"`"). This is the assertion that makes a
/// synthesized-default regression a test failure.
///
/// COMPILE-RED until the field exists; then this pins absent-is-None.
#[test]
fn agent_spec_absent_role_folds_none_never_default() {
    let spec = ProjectSpec::from_toml_str(
        r#"
[project]
name = "roles"
repo = "."

[[agent]]
name = "plain"
harness = "claude-code"
worktree = "auto"
"#,
    )
    .expect("spec without a role must parse");
    assert_eq!(
        spec.agents[0].role, None,
        "a role-less `[[agent]]` parses to None — absence is honest, never a \
         synthesized default like \"contributor\" (DR-012; ontology role? line)"
    );
}

/// CRITERION 1 (declared-empty-string is DISTINCT from absent) — a `role = ""`
/// declaration parses to `Some("")`, NOT `None`. This mirrors DR-012's
/// declared-vs-absent discipline: an explicitly-declared (even empty) value is a
/// present intent, kept distinct from the never-declared `None`. The policy — not
/// rezidnt — interprets an empty role string; rezidnt does not collapse it to
/// absent.
///
/// COMPILE-RED until the field exists. (If the implementer's serde shape collapses
/// `""` to `None`, THIS is the test that catches the honesty loss; if that
/// collapse is later argued correct, it routes to /dr — the oracle does not
/// pre-weaken it.)
#[test]
fn agent_spec_declared_empty_role_is_some_not_none() {
    let spec = ProjectSpec::from_toml_str(
        r#"
[project]
name = "roles"
repo = "."

[[agent]]
name = "empty"
harness = "claude-code"
worktree = "auto"
role = ""
"#,
    )
    .expect("spec with an empty-string role must parse");
    assert_eq!(
        spec.agents[0].role.as_deref(),
        Some(""),
        "a DECLARED empty role is Some(\"\"), distinct from an ABSENT role (None) \
         — DR-012 declared-vs-absent; the policy interprets the empty string, \
         rezidnt does not collapse it to absent"
    );
}
