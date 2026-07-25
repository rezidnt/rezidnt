//! Harness adapters (DR-001 native harness adapters): map recorded or live
//! agent-CLI event streams to fabric facts. Tested ONLY against recorded
//! transcripts (`spec/fixtures/transcripts/`) — zero network.
//!
//! [`AgentSubstrate`] is the I4 seam (DR-048 slice A): the daemon drives a
//! harness through the trait, never a concrete type. Two implementations ship
//! today — [`ClaudeCodeAdapter`] (`--output-format stream-json`) and
//! [`CodexAdapter`] (`codex exec --json`).
//!
//! Version gate: an adapter refuses an untested CLI major rather than guess
//! (a harness that ships weekly must not silently break the fabric). Each
//! substrate owns its OWN tested-majors list — [`TESTED_CLI_MAJORS`] for
//! claude-code, [`TESTED_CODEX_MAJORS`] for codex.

use serde_json::Value;

use crate::RunId;

/// claude-code CLI majors this adapter's transcript contract is recorded
/// against.
pub const TESTED_CLI_MAJORS: &[u64] = &[2];

/// codex-cli majors [`CodexAdapter`]'s transcript contract is recorded against
/// (the recording is codex-cli 0.145.0 — the 0.x line).
pub const TESTED_CODEX_MAJORS: &[u64] = &[0];

/// Errors for adapter mapping (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("stream line is not valid JSON: {0}")]
    BadLine(#[from] serde_json::Error),
    /// The refusing substrate's tested majors are deliberately NOT named in
    /// this message: the variant is shared across every [`AgentSubstrate`]
    /// impl and each owns a different list (`TESTED_CLI_MAJORS` vs
    /// `TESTED_CODEX_MAJORS`), so naming one would misreport the other's
    /// refusal.
    #[error("untested harness major {major}; refusing — re-record the transcript contract")]
    UntestedMajor { major: u64 },
    #[error("unparseable harness version {version:?}")]
    BadVersion { version: String },
}

/// The I4 run-substrate seam (plan §7, made real by DR-048 slice A): one agent
/// harness's stream contract, behind a dyn-safe trait so the daemon dispatches
/// on a spec's `harness` selector without knowing the concrete adapter.
///
/// Deliberately three methods — the whole surface the capture loop needs:
/// gate the harness version before spawning, fold each stream line into facts,
/// and read back the resume identity for checkpointing.
pub trait AgentSubstrate {
    /// Accept or refuse a harness version string against THIS substrate's
    /// recorded contract (I6-adjacent: refusal is machine-readable, never a
    /// silent guess).
    fn version_gate(&self, version: &str) -> Result<(), AdapterError>;

    /// Map one JSONL stream line to zero or more facts. An unrecognized line
    /// type is `Ok(vec![])` (additive evolution); malformed JSON is
    /// [`AdapterError::BadLine`], never a panic.
    fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError>;

    /// The harness's own session/thread identity, once the stream has
    /// announced it — the resume/checkpoint handle.
    fn session_id(&self) -> Option<&str>;
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

/// Accept or refuse a harness version string (semver-ish, e.g. "2.1.191")
/// against an explicit tested-majors list. The one gate body both substrates
/// share, so a second harness cannot drift into a laxer rule.
fn gate_major(version: &str, tested: &[u64]) -> Result<(), AdapterError> {
    let major: u64 = version
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AdapterError::BadVersion {
            version: version.to_string(),
        })?;
    if !tested.contains(&major) {
        return Err(AdapterError::UntestedMajor { major });
    }
    Ok(())
}

/// Accept or refuse a claude-code version string (semver-ish, e.g. "2.1.191").
pub fn version_gate(version: &str) -> Result<(), AdapterError> {
    gate_major(version, TESTED_CLI_MAJORS)
}

/// Accept or refuse a codex-cli version string (semver-ish, e.g. "0.145.0").
pub fn codex_version_gate(version: &str) -> Result<(), AdapterError> {
    gate_major(version, TESTED_CODEX_MAJORS)
}

/// Compact numeric coercion for accounting fields: a number rides verbatim,
/// anything else (absent, null, wrong type) becomes an honest `0` rather than
/// failing the fact or fabricating a value. Shared by both substrates so the
/// `agent.completed` payload shape cannot drift between harnesses.
fn number_or_zero(v: &Value) -> Value {
    if v.is_number() {
        v.clone()
    } else {
        Value::from(0)
    }
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

/// The claude-code substrate behind the I4 seam. The inherent methods stay —
/// the existing constructor API and concrete call sites are untouched by the
/// extraction (DR-048 stay-green criterion) — and the trait delegates to them,
/// so there is exactly one mapping body per substrate.
impl AgentSubstrate for ClaudeCodeAdapter {
    fn version_gate(&self, version: &str) -> Result<(), AdapterError> {
        version_gate(version)
    }

    fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError> {
        ClaudeCodeAdapter::map_line(self, line)
    }

    fn session_id(&self) -> Option<&str> {
        ClaudeCodeAdapter::session_id(self)
    }
}

/// Stateful per-run mapper over `codex exec --json` lines (DR-048 slice A —
/// the second substrate, which is what proves the seam is a seam).
///
/// Contract pinned by the recorded transcript
/// (`spec/fixtures/transcripts/codex_exec_v0.145.0.jsonl`, codex-cli 0.145.0):
/// - `thread.started` → `agent.status.changed` (spawning→running); the carried
///   `thread_id` (the `codex exec resume` identity) is captured as the session
///   id, mirroring claude-code's `system/init` capture.
/// - `item.completed` whose item `type` is `agent_message` → `agent.message`.
/// - `turn.completed` → `agent.completed` in the SAME payload shape
///   [`ClaudeCodeAdapter::map_result`] emits.
/// - everything else (`turn.started`, the machine-local `error` item, future
///   additions) → `Ok(vec![])`: tolerated noise, never an error.
///
/// Deliberate gaps, stated rather than guessed: the codex event format carries
/// no dollar cost and no turn duration, so `cost.total_usd` and `duration_ms`
/// are honest zeros (the house zero-default convention) — never a fabricated
/// number, and cross-vendor cost comparison is explicitly deferred (DR-048
/// §Decision 3). It carries no aggregate turn count either, so `num_turns` is
/// the deterministic count of `turn.completed` lines observed on THIS stream.
/// A turn-level FAILURE line is not mapped: no failing codex transcript has
/// been recorded, and this crate's rule is that adapter contracts come from
/// recordings, never from memory of a CLI's docs.
#[derive(Debug)]
pub struct CodexAdapter {
    run: RunId,
    thread_id: Option<String>,
    turns: u64,
}

impl CodexAdapter {
    pub fn new(run: RunId) -> Self {
        Self {
            run,
            thread_id: None,
            turns: 0,
        }
    }

    /// Map one `codex exec --json` line to zero or more facts. Unknown line
    /// types are tolerated (`Ok(vec![])`); non-JSON input is
    /// [`AdapterError::BadLine`].
    pub fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError> {
        let value: Value = serde_json::from_str(line)?;
        let facts = match value["type"].as_str() {
            Some("thread.started") => self.map_thread_started(&value),
            Some("item.completed") => self.map_item_completed(&value),
            Some("turn.completed") => vec![self.map_turn_completed(&value)],
            // `turn.started`, the machine-local `error` item, and anything the
            // CLI adds later: tolerated noise — additive evolution.
            _ => vec![],
        };
        Ok(facts)
    }

    /// `thread.started` → running + thread capture (the resume identity).
    fn map_thread_started(&mut self, value: &Value) -> Vec<MappedFact> {
        if let Some(thread) = value["thread_id"].as_str() {
            self.thread_id = Some(thread.to_string());
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

    /// `item.completed` carrying an `agent_message` item → `agent.message`.
    /// Other item types (`error`, and whatever codex adds next) map to nothing.
    fn map_item_completed(&self, value: &Value) -> Vec<MappedFact> {
        let item = &value["item"];
        if item["type"].as_str() != Some("agent_message") {
            return vec![];
        }
        let Some(text) = item["text"].as_str() else {
            return vec![];
        };
        vec![MappedFact {
            subject: "agent.message".to_string(),
            payload: serde_json::json!({
                "run": self.run,
                "role": "assistant",
                "text": text,
            }),
        }]
    }

    /// `turn.completed` → `agent.completed` (dossier accounting). See the type
    /// docs for the zero-default fields the codex format does not carry.
    fn map_turn_completed(&mut self, value: &Value) -> MappedFact {
        self.turns += 1;
        let mut payload = serde_json::json!({
            "run": self.run,
            "status": "success",
            "cost": {
                "total_usd": 0,
                "input_tokens": number_or_zero(&value["usage"]["input_tokens"]),
                "output_tokens": number_or_zero(&value["usage"]["output_tokens"]),
            },
            "num_turns": self.turns,
            "duration_ms": 0,
        });
        if let Some(thread) = self.thread_id.as_deref()
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("session_id".to_string(), Value::String(thread.to_string()));
        }
        MappedFact {
            subject: "agent.completed".to_string(),
            payload,
        }
    }

    /// The codex thread id from `thread.started`, once seen (`codex exec
    /// resume` seam — codex's equivalent of claude-code's session id).
    pub fn session_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    pub fn run(&self) -> RunId {
        self.run
    }
}

impl AgentSubstrate for CodexAdapter {
    fn version_gate(&self, version: &str) -> Result<(), AdapterError> {
        codex_version_gate(version)
    }

    fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError> {
        CodexAdapter::map_line(self, line)
    }

    fn session_id(&self) -> Option<&str> {
        CodexAdapter::session_id(self)
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
