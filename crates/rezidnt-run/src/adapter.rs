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

/// Inline cap for `agent.message` text (DEFAULT, ontology v1 baseline):
/// larger bodies go to the CAS and the payload carries `ref` instead of
/// `text`. The swap happens at the publishing edge (the daemon owns the CAS;
/// this mapper stays pure), sized against this constant.
pub const MESSAGE_INLINE_CAP: usize = 8 * 1024;

/// Truncation cap for `agent.tool.invoked` `input_summary` (DEFAULT): a
/// human-readable glimpse, never the bulk input (I2).
const INPUT_SUMMARY_CAP: usize = 256;

/// Accept or refuse a harness version string (semver-ish, e.g. "2.1.191").
pub fn version_gate(version: &str) -> Result<(), AdapterError> {
    let major: u64 = version
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AdapterError::BadVersion {
            version: version.to_string(),
        })?;
    if !TESTED_CLI_MAJORS.contains(&major) {
        return Err(AdapterError::UntestedMajor { major });
    }
    Ok(())
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
        let value: Value = serde_json::from_str(line)?;
        let facts = match value["type"].as_str() {
            Some("system") => self.map_system(&value),
            Some("assistant") => self.map_assistant(&value),
            Some("result") => vec![self.map_result(&value)],
            // Unknown/unmapped line types (hooks, rate limits, user echoes,
            // future additions): tolerated noise — additive evolution.
            _ => vec![],
        };
        Ok(facts)
    }

    /// `system/init` → running + session capture; other system subtypes
    /// (hook_started, hook_response, …) are tolerated noise.
    fn map_system(&mut self, value: &Value) -> Vec<MappedFact> {
        if value["subtype"].as_str() != Some("init") {
            return vec![];
        }
        if let Some(session) = value["session_id"].as_str() {
            self.session_id = Some(session.to_string());
        }
        vec![MappedFact {
            subject: "agent.status.changed".to_string(),
            payload: serde_json::json!({
                "run": self.run,
                "from": "spawning",
                "to": "running",
            }),
        }]
    }

    /// Assistant content blocks, in block order: `text` → `agent.message`,
    /// `tool_use` → `agent.tool.invoked` (one per block). Other block kinds
    /// (`thinking`, …) are tolerated noise.
    fn map_assistant(&self, value: &Value) -> Vec<MappedFact> {
        let Some(blocks) = value["message"]["content"].as_array() else {
            return vec![];
        };
        let mut facts = Vec::new();
        for block in blocks {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        facts.push(MappedFact {
                            subject: "agent.message".to_string(),
                            payload: serde_json::json!({
                                "run": self.run,
                                "role": "assistant",
                                "text": text,
                            }),
                        });
                    }
                }
                Some("tool_use") => {
                    if let Some(tool) = block["name"].as_str() {
                        let mut payload = serde_json::json!({
                            "run": self.run,
                            "tool": tool,
                        });
                        if let Some(summary) = input_summary(&block["input"])
                            && let Some(obj) = payload.as_object_mut()
                        {
                            obj.insert("input_summary".to_string(), Value::String(summary));
                        }
                        facts.push(MappedFact {
                            subject: "agent.tool.invoked".to_string(),
                            payload,
                        });
                    }
                }
                _ => {}
            }
        }
        facts
    }

    /// `result` → `agent.completed` (dossier accounting, DR-001). Accounting
    /// fields the harness omits default to zero rather than failing the
    /// completion fact (unpinned call, flagged for the auditor).
    fn map_result(&self, value: &Value) -> MappedFact {
        let status = if value["subtype"].as_str() == Some("success")
            && value["is_error"] != Value::Bool(true)
        {
            "success"
        } else {
            "error"
        };
        let number_or_zero = |v: &Value| -> Value {
            if v.is_number() {
                v.clone()
            } else {
                Value::from(0)
            }
        };
        let mut payload = serde_json::json!({
            "run": self.run,
            "status": status,
            "cost": {
                "total_usd": number_or_zero(&value["total_cost_usd"]),
                "input_tokens": number_or_zero(&value["usage"]["input_tokens"]),
                "output_tokens": number_or_zero(&value["usage"]["output_tokens"]),
            },
            "num_turns": number_or_zero(&value["num_turns"]),
            "duration_ms": number_or_zero(&value["duration_ms"]),
        });
        let session = value["session_id"].as_str().or(self.session_id.as_deref());
        if let Some(session) = session
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("session_id".to_string(), Value::String(session.to_string()));
        }
        MappedFact {
            subject: "agent.completed".to_string(),
            payload,
        }
    }

    /// Session id from `system/init`, once seen (run checkpointing seam).
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn run(&self) -> RunId {
        self.run
    }
}

/// Compact, truncated rendering of a tool input for `input_summary` — a
/// glimpse for humans, never the bulk input (that would be I2 smuggling).
/// `None` when the input is absent.
fn input_summary(input: &Value) -> Option<String> {
    if input.is_null() {
        return None;
    }
    // Compact JSON is deterministic and readable for small inputs.
    let rendered = serde_json::to_string(input).ok()?;
    if rendered.len() <= INPUT_SUMMARY_CAP {
        return Some(rendered);
    }
    let mut cut = INPUT_SUMMARY_CAP;
    while !rendered.is_char_boundary(cut) {
        cut -= 1;
    }
    Some(format!("{}…", &rendered[..cut]))
}
