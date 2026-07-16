//! S1 oracle: the spawn plan is pure and pinned — argv, bin override, env.

use rezidnt_run::badge::{BADGE_ENV_VAR, Badge};
use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

fn agent(bin_override: Option<&str>) -> AgentSpec {
    AgentSpec {
        name: "impl".into(),
        harness: "claude-code".into(),
        worktree: "auto".into(),
        gates: vec![],
        bin_override: bin_override.map(Into::into),
    }
}

/// DR-001 invocation, exactly: `claude -p --output-format stream-json --verbose`.
#[test]
fn claude_code_argv_is_pinned() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_claude_code(&agent(None), &badge, std::iter::empty());
    assert_eq!(plan.bin, std::path::Path::new("claude"));
    assert_eq!(
        plan.args,
        ["-p", "--output-format", "stream-json", "--verbose"]
    );
}

/// `bin_override` redirects the executable (pinned-version and contract-test
/// seam) without changing the argv contract.
#[test]
fn bin_override_redirects_executable_only() {
    let badge = Badge::mint().expect("mint");
    let plan = SpawnPlan::for_claude_code(
        &agent(Some("/opt/harness/claude-2.1.191")),
        &badge,
        std::iter::empty(),
    );
    assert_eq!(
        plan.bin,
        std::path::Path::new("/opt/harness/claude-2.1.191")
    );
    assert_eq!(
        plan.args,
        ["-p", "--output-format", "stream-json", "--verbose"]
    );
}

/// The plan's env is the scrubbed env: secrets out, badge in.
#[test]
fn plan_env_is_scrubbed_with_badge() {
    let badge = Badge::mint().expect("mint");
    let parent = vec![
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("GITHUB_TOKEN".to_string(), "ghp_secret".to_string()),
    ];
    let plan = SpawnPlan::for_claude_code(&agent(None), &badge, parent.into_iter());
    assert!(plan.env.iter().any(|(k, _)| k == "PATH"));
    assert!(!plan.env.iter().any(|(k, _)| k == "GITHUB_TOKEN"));
    assert!(
        plan.env
            .iter()
            .any(|(k, v)| k == BADGE_ENV_VAR && *v == badge.token_hex())
    );
}
