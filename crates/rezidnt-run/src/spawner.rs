//! Spawner (DR-001): `tokio::process` for headless children. `portable-pty`
//! arrives only with the first TTY-demanding harness — not S1.

use std::path::PathBuf;

use crate::spec::AgentSpec;

/// A fully resolved spawn: argv + scrubbed env, ready for `tokio::process`.
/// Pure and inspectable so tests pin it without spawning anything.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The claude-code `PreToolUse` hook settings the daemon writes into the
    /// worktree (`.claude/settings.json`, design §3(2)) when the agent opts
    /// into the permit PEP. `None` for a non-permit spawn — a run without a
    /// `[gates.permit]` gate spawns exactly as today, no mid-run interception
    /// (DR-014 §Decision 2; the honest absence `pep?` records downstream).
    /// Rendered to a JSON string so callers inspect / write it without coupling
    /// to a settings struct shape.
    pub permit_hook_config: Option<String>,
}

impl SpawnPlan {
    /// Build the claude-code headless invocation for one agent (DR-001):
    /// `claude -p --output-format stream-json --verbose`, honoring
    /// `bin_override`, env scrubbed with the badge token injected. No PEP wiring
    /// — a non-permit spawn (see [`SpawnPlan::for_claude_code_permit`] for the
    /// permit-gated variant).
    ///
    /// SP4b (DR-017): `badge_token` is the value injected under `REZIDNT_BADGE`.
    /// The env seam is unchanged; only the token VALUE flips from a DR-005
    /// opaque hex token to a serialized agent macaroon
    /// ([`crate::badge::Macaroon::to_wire`]) — inline under the 32 KiB cap (I2).
    pub fn for_claude_code(
        agent: &AgentSpec,
        badge_token: &str,
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
            env: crate::badge::scrubbed_env(parent_env, badge_token),
            permit_hook_config: None,
        }
    }

    /// Build the claude-code invocation for a permit-gated agent (DR-014
    /// §Decision 2; design §3): the base [`SpawnPlan::for_claude_code`] plan,
    /// PLUS the PEP wiring when the agent declares a `[gates.permit]` gate —
    /// `REZIDNT_RUN` + `REZIDNT_SOCKET` injected into the scrubbed env (so the
    /// hook discovers its run deterministically and dials the right daemon,
    /// never cwd-guessed) and a `PreToolUse` hook config naming
    /// `rezidnt permit-hook`.
    ///
    /// An agent WITHOUT a permit gate gets exactly the [`SpawnPlan::for_claude_code`]
    /// plan back — no env injection, no hook config — so a non-permit run
    /// spawns as today (the honest absence `pep?` records downstream). The
    /// injection is keyed on the spec's `gates` list containing `"permit"`.
    pub fn for_claude_code_permit(
        agent: &AgentSpec,
        badge_token: &str,
        parent_env: impl Iterator<Item = (String, String)>,
        run_id: &str,
        socket: &str,
    ) -> Self {
        let mut plan = Self::for_claude_code(agent, badge_token, parent_env);
        if !agent.gates.iter().any(|g| g == "permit") {
            return plan; // no permit gate → spawns exactly as today (design §3)
        }
        // Deterministic run discovery + the daemon UDS the hook dials (design
        // §3(1)). Injected into the already-scrubbed env; additive, so the
        // badge injection is untouched.
        plan.env
            .push(("REZIDNT_RUN".to_string(), run_id.to_string()));
        plan.env
            .push(("REZIDNT_SOCKET".to_string(), socket.to_string()));
        // The `PreToolUse` hook config pointing claude-code at the PEP — the
        // `rezidnt permit-hook` CLI subcommand (DR-014 §Decision 1). Rendered
        // as claude-code settings JSON (`.claude/settings.json` shape).
        plan.permit_hook_config = Some(permit_hook_settings());
        plan
    }

    /// The injected `PreToolUse` hook config (design §3(2)), if this is a
    /// permit-gated plan; `None` for a non-permit spawn.
    pub fn permit_hook_config(&self) -> Option<&str> {
        self.permit_hook_config.as_deref()
    }

    /// An empty placeholder plan — the pure wrapper renderers (`bwrap_argv*`,
    /// `connector_argv`) deliberately do NOT read the plan for wrapper directives
    /// (the C6/DR-024 no-widening guard), so a caller that renders ONLY the folded
    /// wrapper (e.g. the composed dataplane's bwrap prefix, where the confined
    /// program is appended separately, not from a plan) needs a plan value the
    /// renderer will ignore. No `Default` impl: this is deliberately named to
    /// document that the plan is a no-op here, never a source of confinement.
    pub fn default_placeholder() -> Self {
        Self {
            bin: PathBuf::new(),
            args: Vec::new(),
            env: Vec::new(),
            permit_hook_config: None,
        }
    }
}

/// The claude-code `.claude/settings.json` fragment wiring the `PreToolUse`
/// hook to the `rezidnt permit-hook` subcommand (design §3(2)). A single
/// matcher-all `PreToolUse` hook so every tool call is asked of the daemon PDP.
fn permit_hook_settings() -> String {
    serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        { "type": "command", "command": "rezidnt permit-hook" }
                    ]
                }
            ]
        }
    })
    .to_string()
}
