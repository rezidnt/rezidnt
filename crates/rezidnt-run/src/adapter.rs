//! claude-code adapter (DR-001 native harness adapters): maps recorded or
//! live `--output-format stream-json` lines to fabric facts. Tested ONLY
//! against recorded transcripts (`spec/fixtures/transcripts/`) — zero network.
//!
//! Version gate: the adapter refuses an untested CLI major rather than guess
//! (a harness that ships weekly must not silently break the fabric).

use serde_json::Value;

use crate::RunId;

/// CLI majors this adapter's transcript contract is recorded against.
pub const TESTED_CLI_MAJORS: &[u64] = &[2];

/// Errors for adapter mapping (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("stream line is not valid JSON: {0}")]
    BadLine(#[from] serde_json::Error),
    #[error(
        "untested harness major {major} (tested: {TESTED_CLI_MAJORS:?}); refusing — re-record the transcript contract"
    )]
    UntestedMajor { major: u64 },
    #[error("unparseable harness version {version:?}")]
    BadVersion { version: String },
}

/// A fact the adapter derived from one stream line, ready for the fabric.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedFact {
    /// Ontology subject name (e.g. `agent.message`).
    pub subject: String,
    pub payload: Value,
}

/// Accept or refuse a harness version string (semver-ish, e.g. "2.1.191").
pub fn version_gate(version: &str) -> Result<(), AdapterError> {
    let _ = version;
    todo!("S1: parse major, refuse if not in TESTED_CLI_MAJORS")
}

/// Stateful per-run mapper over stream-json lines.
#[derive(Debug)]
pub struct ClaudeCodeAdapter {
    run: RunId,
    session_id: Option<String>,
}

impl ClaudeCodeAdapter {
    pub fn new(run: RunId) -> Self {
        Self {
            run,
            session_id: None,
        }
    }

    /// Map one JSONL line to zero or more facts.
    ///
    /// Contract (pinned by the transcript tests):
    /// - `system/init` → `agent.status.changed` (spawning→running) and the
    ///   session id is captured for run checkpointing (`--resume`).
    /// - `assistant` text content → `agent.message`.
    /// - `assistant` `tool_use` content → `agent.tool.invoked` (one per block).
    /// - `result` → `agent.completed` carrying cost/usage/turns/duration and
    ///   the session id (dossier accounting, DR-001).
    /// - Unknown/unmapped line types (hooks, rate limits, future additions)
    ///   → `Ok(vec![])`: tolerated, never an error (additive evolution).
    /// - Non-JSON input → `AdapterError::BadLine`.
    pub fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError> {
        let _ = line;
        todo!("S1: serde_json parse + type dispatch")
    }

    /// Session id from `system/init`, once seen (run checkpointing seam).
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn run(&self) -> RunId {
        self.run
    }
}
