//! DR-048 slice A oracle: `AgentSpec` gains an optional `model` — the trial
//! matrix's model axis. Declared-vs-absent semantics mirror `harness_version`
//! (DR-012): absent means "harness default", never synthesized.
//!
//! Three legs, all pinned here:
//!
//! Parse leg — `model = "…"` on an `[[agent]]` table parses to `Some`; an
//! agent without the key is `None` (serde default, additive).
//!
//! Argv leg — when set, the model appears in the `SpawnPlan` argv; when
//! absent, the argv is byte-identical to today's (the stay-green criterion).
//! Flags are the REAL CLI flags, verified against the installed binaries on
//! 2026-07-25, not guessed: claude-code takes `--model <value>`; codex takes
//! `-m, --model <MODEL>` per `codex exec --help` (codex-cli 0.145.0) — the
//! long form `--model` is pinned for both.
//!
//! Preimage leg — ORACLE CALL, stated: `model` BELONGS in the vet preimage
//! (`agent_spec_toml`). Under DR-048 the model is a governed axis of the
//! spawn; a vet verdict is CAS-pinned to the `refs["spec"]` preimage (§8,
//! I6), and if two trial variants differing only by model hashed to identical
//! preimages, "why did this candidate pass vet" could not be interrogated
//! per-variant. Handling mirrors `harness_version` exactly: emitted when
//! declared, NOTHING when absent — so every existing spec's preimage bytes
//! (and therefore its CAS hash) are unmoved. If the implementer believes the
//! criterion itself is wrong, that dispute routes to /dr, not to a weakened
//! test.
//!
//! RED MODE: compile-red today. `AgentSpec` has no `model` field and
//! `SpawnPlan::for_codex` does not exist — this file does not compile until
//! slice A phase 1 lands. That IS the failing state. (Note for the
//! implementer: `model: None` is written explicitly in the absent-leg
//! literals below, deliberately — an `..AgentSpec::default()` spread would
//! have compiled, and passed, today, testing nothing.)

use rezidnt_run::badge::{BADGE_ENV_VAR, Badge};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::{AgentSpec, ProjectSpec, agent_spec_toml};

fn agent(harness: &str, model: Option<&str>) -> AgentSpec {
    AgentSpec {
        name: "impl".into(),
        harness: harness.into(),
        worktree: "auto".into(),
        model: model.map(Into::into),
        ..AgentSpec::default()
    }
}

/// `model = "…"` parses from an `[[agent]]` table; an agent without the key
/// is honestly absent (`None`), never defaulted to a model name.
#[test]
fn model_parses_from_agent_toml_and_is_absent_when_undeclared() {
    let toml = r#"
[project]
name = "trial"
repo = "."

[[agent]]
name = "variant-a"
harness = "claude-code"
worktree = "auto"
model = "claude-fable-5"

[[agent]]
name = "variant-b"
harness = "claude-code"
worktree = "auto"
"#;
    let spec = ProjectSpec::from_toml_str(toml).expect("spec must parse");
    assert_eq!(spec.agents[0].model.as_deref(), Some("claude-fable-5"));
    assert_eq!(
        spec.agents[1].model, None,
        "undeclared model is absent, never synthesized (DR-012 declared-vs-absent)"
    );
}

/// Set model → `--model <value>` lands on the claude-code argv, appended
/// after the pinned base invocation (the base prefix is untouched).
#[test]
fn model_appends_model_flag_to_claude_code_argv() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_claude_code(
        &agent("claude-code", Some("claude-fable-5")),
        &badge.token_hex(),
        std::iter::empty(),
    );
    assert_eq!(
        plan.args,
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--model",
            "claude-fable-5",
        ]
    );
}

/// Absent model → today's argv, byte-identical (stay-green: a spec that
/// declares no model spawns exactly as before this slice).
#[test]
fn absent_model_leaves_claude_code_argv_unchanged() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_claude_code(
        &agent("claude-code", None),
        &badge.token_hex(),
        std::iter::empty(),
    );
    assert_eq!(
        plan.args,
        ["-p", "--output-format", "stream-json", "--verbose"]
    );
}

/// The codex spawn plan (second substrate, DR-048 slice A): base invocation
/// `codex exec --json` (JSONL events on stdout, prompt via stdin — mirrors
/// the claude-code stdin-prompt shape), env scrubbed with the badge injected,
/// `bin_override` honored. Absent model → no model flag.
#[test]
fn codex_argv_is_pinned_and_env_is_scrubbed_with_badge() {
    let badge = Badge::mint().expect("mint");
    let parent = vec![
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("GITHUB_TOKEN".to_string(), "ghp_secret".to_string()),
    ];
    let plan = SpawnPlan::for_codex(
        &agent("codex", None),
        &badge.token_hex(),
        parent.into_iter(),
    )
    .expect("a codex spec declaring no permit gate plans cleanly");
    assert_eq!(plan.bin, std::path::Path::new("codex"));
    assert_eq!(plan.args, ["exec", "--json"]);
    assert!(plan.env.iter().any(|(k, _)| k == "PATH"));
    assert!(!plan.env.iter().any(|(k, _)| k == "GITHUB_TOKEN"));
    assert!(
        plan.env
            .iter()
            .any(|(k, v)| k == BADGE_ENV_VAR && *v == badge.token_hex())
    );
}

/// Set model → `--model <value>` lands on the codex argv (the real
/// `codex exec` flag per `--help` on codex-cli 0.145.0, long form).
#[test]
fn model_appends_model_flag_to_codex_argv() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_codex(
        &agent("codex", Some("gpt-5-codex")),
        &badge.token_hex(),
        std::iter::empty(),
    )
    .expect("a codex spec declaring no permit gate plans cleanly");
    assert_eq!(plan.args, ["exec", "--json", "--model", "gpt-5-codex"]);
}

/// `bin_override` still redirects the executable only (the pinned-version and
/// contract-test seam works for the second substrate too).
#[test]
fn codex_bin_override_redirects_executable_only() {
    let badge = Badge::mint().expect("mint");
    let mut spec = agent("codex", None);
    spec.bin_override = Some("/opt/harness/codex-0.145.0".into());
    let plan = SpawnPlan::for_codex(&spec, &badge.token_hex(), std::iter::empty())
        .expect("a codex spec declaring no permit gate plans cleanly");
    assert_eq!(plan.bin, std::path::Path::new("/opt/harness/codex-0.145.0"));
    assert_eq!(plan.args, ["exec", "--json"]);
}

/// Declared model lands in the vet preimage, mirroring `harness_version`:
/// emitted between `harness_version` and `allowed_tools`, TOML basic-string
/// quoted. Full byte pin — the preimage is a CAS input (§8), so its exact
/// bytes are the contract.
#[test]
fn declared_model_lands_in_the_vet_preimage() {
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        worktree: "auto".to_string(),
        gates: vec!["vet".to_string()],
        bin_override: None,
        bare: true,
        harness_version: Some("2.1.191".to_string()),
        allowed_tools: vec!["Read".to_string(), "Edit".to_string()],
        role: None,
        model: Some("claude-fable-5".to_string()),
    };

    let expected = "[agent]\n\
        name = \"impl\"\n\
        harness = \"claude-code\"\n\
        bare = true\n\
        harness_version = \"2.1.191\"\n\
        model = \"claude-fable-5\"\n\
        allowed_tools = [\"Read\", \"Edit\"]\n";

    assert_eq!(
        agent_spec_toml(&agent),
        expected,
        "a declared model is part of the governed posture the vet verdict is pinned to"
    );
}

/// Absent model emits NOTHING — the preimage bytes of every existing spec are
/// unmoved, so no already-recorded vet verdict's CAS hash shifts under this
/// slice. Byte-for-byte the same pin `agent_spec_toml_seam.rs` locks today.
#[test]
fn absent_model_leaves_preimage_bytes_unchanged() {
    let agent = AgentSpec {
        name: "impl".to_string(),
        harness: "claude-code".to_string(),
        worktree: "auto".to_string(),
        gates: vec!["vet".to_string(), "pre_merge".to_string()],
        bin_override: None,
        bare: true,
        harness_version: Some("2.1.191".to_string()),
        allowed_tools: vec!["Read".to_string(), "Edit".to_string()],
        role: None,
        model: None,
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
        "absent model must not move the CAS hash of any existing spec"
    );
}
