//! Oracle (S4-debrief LOW, I4-adjacent): the vet CAS preimage —
//! `agent_spec_toml` — is DUPLICATED. The CLI's vet path
//! (`bins/rezidnt/src/main.rs::agent_spec_toml`) and the daemon's gate path
//! (`bins/rezidentd/src/gates.rs::agent_spec_toml`) each carry a byte-for-byte
//! copy. They MUST produce identical bytes: both feed the SAME
//! content-hash-pinned `refs["spec"]` preimage the vet natives verify (§8
//! determinism). Two copies is a divergence waiting to happen — a one-line
//! edit to one produces a different CAS hash and silently splits the CLI's vet
//! verdict from the daemon's.
//!
//! MY CALL (stated in the oracle report): EXTRACTION, not a runtime
//! cross-binary equality test. The two `agent_spec_toml` fns are PRIVATE to two
//! separate binary crates — a runtime test could only compare them by making
//! both `pub` and importing across bin targets, which bins do not cleanly
//! export. The honest fix that makes them identical BY CONSTRUCTION is a single
//! shared fn in `rezidnt-run` (which both bins already depend on and which owns
//! `AgentSpec`); both call sites then delegate. This test pins that shared
//! seam: it exercises `rezidnt_run::spec::agent_spec_toml` directly and locks
//! its exact byte output, so a future edit that would have drifted one copy now
//! breaks one test in one place.
//!
//! RED MODE: compile-red today. `rezidnt_run::spec::agent_spec_toml` does not
//! exist yet — the fn still lives (twice) in the bins. This file does not
//! compile until the shared fn is extracted; that IS the failing state (the
//! implementer extracts it and rewires both bins, then this goes green).

use rezidnt_run::spec::{AgentSpec, agent_spec_toml};

/// The exact bytes the current daemon/CLI copies produce for a fully-governed
/// agent — computed from reading both copies at oracle time (name, harness,
/// bare, harness_version, then a bracketed allowed_tools list). The extracted
/// fn must reproduce these bytes verbatim so the CAS preimage is unchanged by
/// the refactor (the vet hash must not move).
#[test]
fn agent_spec_toml_pins_the_exact_governed_preimage() {
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        worktree: "auto".to_string(),
        gates: vec!["vet".to_string(), "pre_merge".to_string()],
        bin_override: None,
        bare: true,
        harness_version: Some("2.1.191".to_string()),
        allowed_tools: vec!["Read".to_string(), "Edit".to_string()],
        // SP4a additive field (DR-016): compile-only here — `agent_spec_toml`
        // does not emit `role`, so the pinned preimage bytes are unchanged.
        role: None,
    };

    let expected = "[agent]\n\
        name = \"impl\"\n\
        harness = \"claude-code\"\n\
        bare = true\n\
        harness_version = \"2.1.191\"\n\
        allowed_tools = [\"Read\", \"Edit\"]\n";

    assert_eq!(
        agent_spec_toml(&agent),
        expected,
        "the extracted vet preimage must byte-match the copies both bins ship — \
         a drift here moves the CAS hash and splits CLI vet from daemon vet (§8)"
    );
}

/// The minimal ungoverned agent: only the always-present fields, no optional
/// lines. Pins that absent `harness_version` / empty `allowed_tools` emit
/// NOTHING (the copies gate those lines behind `if let Some` / `if !empty`).
#[test]
fn agent_spec_toml_omits_absent_optional_fields() {
    let agent = AgentSpec {
        name: "bare".to_string(),
        harness: "claude-code".to_string(),
        worktree: "auto".to_string(),
        gates: vec![],
        bin_override: None,
        bare: false,
        harness_version: None,
        allowed_tools: vec![],
        role: None,
    };

    let expected = "[agent]\n\
        name = \"bare\"\n\
        harness = \"claude-code\"\n\
        bare = false\n";

    assert_eq!(
        agent_spec_toml(&agent),
        expected,
        "absent harness_version and empty allowed_tools emit no lines — the \
         preimage surface is intentionally minimal"
    );
}

/// Basic-string quoting of values with a backslash and a quote — both copies
/// do `\\`→`\\\\` then `"`→`\\"`. Pins that the shared fn keeps that escaping,
/// so a name with special chars hashes the same on both paths.
#[test]
fn agent_spec_toml_escapes_quotes_and_backslashes() {
    let agent = AgentSpec {
        name: r#"weird"\name"#.to_string(),
        harness: "claude-code".to_string(),
        worktree: "auto".to_string(),
        gates: vec![],
        bin_override: None,
        bare: false,
        harness_version: None,
        allowed_tools: vec![],
        role: None,
    };

    let expected = "[agent]\n\
        name = \"weird\\\"\\\\name\"\n\
        harness = \"claude-code\"\n\
        bare = false\n";

    assert_eq!(
        agent_spec_toml(&agent),
        expected,
        "TOML basic-string escaping must survive the extraction unchanged"
    );
}
