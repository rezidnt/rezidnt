//! Spawner (DR-001): `tokio::process` for headless children. `portable-pty`
//! arrives only with the first TTY-demanding harness — not S1.

use std::path::PathBuf;

use crate::badge::Badge;
use crate::spec::AgentSpec;

/// A fully resolved spawn: argv + scrubbed env, ready for `tokio::process`.
/// Pure and inspectable so tests pin it without spawning anything.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl SpawnPlan {
    /// Build the claude-code headless invocation for one agent (DR-001):
    /// `claude -p --output-format stream-json --verbose`, honoring
    /// `bin_override`, env scrubbed with the badge injected.
    pub fn for_claude_code(
        agent: &AgentSpec,
        badge: &Badge,
        parent_env: impl Iterator<Item = (String, String)>,
    ) -> Self {
        Self {
            bin: agent
                .bin_override
                .clone()
                .unwrap_or_else(|| PathBuf::from("claude")),
            args: ["-p", "--output-format", "stream-json", "--verbose"]
                .into_iter()
                .map(String::from)
                .collect(),
            env: crate::badge::scrubbed_env(parent_env, badge),
        }
    }
}
