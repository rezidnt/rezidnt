//! S1 oracle: §13 project-spec parsing.

use rezidnt_run::RunError;
use rezidnt_run::spec::ProjectSpec;

/// The doc §13 example, verbatim structure. It must parse with zero edits —
/// the golden path runs on the generated file untouched.
const DOC_13_EXAMPLE: &str = r#"
[project]
name = "acme-checkout"
repo = "."

[[workspace.tab]]
name = "build"
panes = [{ cmd = "just watch" }, { cmd = "just test --watch" }]

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["vet", "pre_merge"]

[gates.pre_merge]
verifiers = [
  { native = "tests-pass" },
  { exec = "verifiers/scope-check", params = { allow = ["src/checkout/**"] } },
]
"#;

#[test]
fn spec_parses_the_doc_section_13_example() {
    let spec = ProjectSpec::from_toml_str(DOC_13_EXAMPLE).expect("§13 example must parse");
    assert_eq!(spec.name, "acme-checkout");
    assert_eq!(spec.repo, std::path::Path::new("."));
    assert_eq!(spec.agents.len(), 1);
    let agent = &spec.agents[0];
    assert_eq!(agent.name, "impl");
    assert_eq!(agent.harness, "claude-code");
    assert_eq!(agent.worktree, "auto");
    assert_eq!(agent.gates, ["vet", "pre_merge"]);
    assert!(agent.bin_override.is_none());
}

/// `[[workspace.tab]]` and `[gates.*]` are Phase-3/Phase-2 surface: parsed
/// and tolerated in S1, never an error (already exercised above), and their
/// absence is equally fine.
#[test]
fn spec_minimal_project_plus_agent_parses() {
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
    .expect("minimal spec must parse");
    assert_eq!(spec.agents[0].gates, Vec::<String>::new());
}

/// A spec without a project name is an honest RunError::Spec, not a panic or
/// a silently defaulted value.
#[test]
fn spec_missing_project_name_is_an_error() {
    let result = ProjectSpec::from_toml_str(
        r#"
[project]
repo = "."
"#,
    );
    match result {
        Err(RunError::Spec(msg)) => assert!(
            msg.contains("name"),
            "error should name the missing field, got: {msg}"
        ),
        other => panic!("expected RunError::Spec, got {other:?}"),
    }
}
