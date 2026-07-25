//! Harness adapters (DR-001 native harness adapters): map recorded or live
//! agent-CLI event streams to fabric facts. Tested ONLY against recorded
//! transcripts (`spec/fixtures/transcripts/`) — zero network.
//!
//! [`AgentSubstrate`] is the I4 seam (plan §7, made real by DR-048 slice A):
//! one dyn-safe trait, two implementations — [`ClaudeCodeAdapter`]
//! (`--output-format stream-json`) and [`CodexAdapter`] (`codex exec --json`).
//!
//! The DAEMON DOES NOT YET DISPATCH THROUGH IT. `bins/rezidentd/src/runs.rs`
//! still constructs [`ClaudeCodeAdapter`] concretely; that wiring is deferred
//! by DR-048 §Decision 6 until the worktree-release-lifecycle slice merges.
//! The trait is the seam it will dispatch on — stated as future work, not as a
//! present-tense fact, because a doc asserting a mechanism the code lacks is
//! the exact silent-wrong class this arc has already produced repeatedly.
//!
//! Version gate: an adapter refuses an UNTESTED version rather than guess (a
//! harness that ships weekly must not silently break the fabric). Each
//! substrate owns its own tested list AND its own pin depth — see
//! [`version_gate`] / [`codex_version_gate`].

use serde_json::Value;

use crate::RunId;

/// claude-code CLI majors this adapter's transcript contract is recorded
/// against. Major-depth is the protective pin for a ≥1.0 line: under semver a
/// breaking change to a 2.x CLI must bump to 3.x, so accepting all of 2.x is a
/// gate that still refuses something real.
pub const TESTED_CLI_MAJORS: &[u64] = &[2];

/// codex-cli majors [`CodexAdapter`] accepts. Kept as a separate axis from
/// [`TESTED_CODEX_VERSIONS`] so an out-of-line major refuses as
/// [`AdapterError::UntestedMajor`] with the major named.
pub const TESTED_CODEX_MAJORS: &[u64] = &[0];

/// codex-cli `(major, minor)` pairs the transcript contract is ACTUALLY
/// recorded against — the recording is codex-cli 0.145.0.
///
/// Codex is gated at MAJOR.MINOR, not major. This is the deliberate policy
/// choice (DR-048 slice A): semver's 0.y.z rule makes MINOR the breaking axis
/// below 1.0, so `majors == [0]` alone would accept every future 0.x codex
/// release forever against a contract recorded at exactly one of them — a gate
/// that refuses nothing is not a gate, and codex ships far faster than
/// claude-code. Widening this list is a deliberate act that means "the
/// transcript contract was re-recorded at that version".
pub const TESTED_CODEX_VERSIONS: &[(u64, u64)] = &[(0, 145)];

/// Errors for adapter mapping (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("stream line is not valid JSON: {0}")]
    BadLine(#[from] serde_json::Error),
    /// The refusing substrate's own tested list rides the variant: the variant
    /// is shared across every [`AgentSubstrate`] impl and each owns a different
    /// list, so the list must travel WITH the refusal — naming one substrate's
    /// const in the message would misreport the other's, and dropping it
    /// entirely would trade away the I6 "why blocked returns the evidence"
    /// property to buy that accuracy.
    #[error(
        "untested harness major {major} (tested majors: {tested:?}); refusing — re-record the transcript contract"
    )]
    UntestedMajor { major: u64, tested: &'static [u64] },
    /// A version inside a tested major but outside the recorded minors — the
    /// refusal a pre-1.0 harness needs (see [`TESTED_CODEX_VERSIONS`]).
    #[error(
        "untested harness version {major}.{minor} (tested: {tested:?}); refusing — re-record the transcript contract"
    )]
    UntestedMinor {
        major: u64,
        minor: u64,
        tested: &'static [(u64, u64)],
    },
    #[error("unparseable harness version {version:?}")]
    BadVersion { version: String },
    /// The live stream falsified a premise the RECORDED contract rests on.
    /// Refusing loudly is the only honest move: the alternative is emitting a
    /// fact the stream does not support, and facts are the log, and the log is
    /// truth (I3).
    #[error(
        "{harness}: recorded stream contract violated — {detail}; refusing rather than logging a fact the stream does not support"
    )]
    ContractViolated {
        harness: &'static str,
        detail: String,
    },
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
/// against an explicit tested list. The one gate body every substrate shares,
/// so the RULE cannot diverge between harnesses — but the DEPTH is per
/// substrate: `tested_minors` is `Some` only for a harness whose recorded
/// contract is pinned finer than its major (see [`TESTED_CODEX_VERSIONS`] for
/// why a pre-1.0 CLI needs that).
fn gate_version(
    version: &str,
    tested_majors: &'static [u64],
    tested_minors: Option<&'static [(u64, u64)]>,
) -> Result<(), AdapterError> {
    let bad = || AdapterError::BadVersion {
        version: version.to_string(),
    };
    let mut parts = version.split('.');
    let major: u64 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    if !tested_majors.contains(&major) {
        return Err(AdapterError::UntestedMajor {
            major,
            tested: tested_majors,
        });
    }
    let Some(tested) = tested_minors else {
        return Ok(());
    };
    let minor: u64 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    if !tested.contains(&(major, minor)) {
        return Err(AdapterError::UntestedMinor {
            major,
            minor,
            tested,
        });
    }
    Ok(())
}

/// Accept or refuse a claude-code version string (semver-ish, e.g. "2.1.191").
/// Pinned at MAJOR depth — see [`TESTED_CLI_MAJORS`].
pub fn version_gate(version: &str) -> Result<(), AdapterError> {
    gate_version(version, TESTED_CLI_MAJORS, None)
}

/// Accept or refuse a codex-cli version string (semver-ish, e.g. "0.145.0").
/// Pinned at MAJOR.MINOR depth — see [`TESTED_CODEX_VERSIONS`].
pub fn codex_version_gate(version: &str) -> Result<(), AdapterError> {
    gate_version(version, TESTED_CODEX_MAJORS, Some(TESTED_CODEX_VERSIONS))
}

/// Compact numeric coercion for accounting fields: a number rides verbatim,
/// anything else (absent, null, wrong type) becomes an honest `0` rather than
/// failing the fact or fabricating a value.
fn number_or_zero(v: &Value) -> Value {
    if v.is_number() {
        v.clone()
    } else {
        Value::from(0)
    }
}

/// The accounting fields of an `agent.completed` fact, before rendering.
///
/// Every substrate renders its completion THROUGH this type, so the fact's key
/// set is emitted from exactly ONE `json!` literal ([`Completion::into_fact`])
/// and cannot drift between harnesses — the shape is single-sourced by
/// construction, and `completion_fact_shape_is_single_sourced` in
/// `tests/codex_adapter_guards.rs` pins it by test as well.
///
/// A field the harness's format does not carry is an honest `0` supplied by
/// the caller (codex has no dollar cost and no duration), never a missing key
/// and never a fabricated number.
struct Completion {
    run: RunId,
    status: &'static str,
    total_usd: Value,
    input_tokens: Value,
    output_tokens: Value,
    num_turns: Value,
    duration_ms: Value,
    session_id: Option<String>,
}

impl Completion {
    /// The ONE `agent.completed` payload literal in this crate.
    fn into_fact(self) -> MappedFact {
        let mut payload = serde_json::json!({
            "run": self.run,
            "status": self.status,
            "cost": {
                "total_usd": self.total_usd,
                "input_tokens": self.input_tokens,
                "output_tokens": self.output_tokens,
            },
            "num_turns": self.num_turns,
            "duration_ms": self.duration_ms,
        });
        // `session_id` stays CONDITIONAL (not a null key): absent means the
        // stream never announced a resume identity — DR-012 declared-vs-absent.
        if let Some(session) = self.session_id
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("session_id".to_string(), Value::String(session));
        }
        MappedFact {
            subject: "agent.completed".to_string(),
            payload,
        }
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
        Completion {
            run: self.run,
            status,
            total_usd: number_or_zero(&value["total_cost_usd"]),
            input_tokens: number_or_zero(&value["usage"]["input_tokens"]),
            output_tokens: number_or_zero(&value["usage"]["output_tokens"]),
            num_turns: number_or_zero(&value["num_turns"]),
            duration_ms: number_or_zero(&value["duration_ms"]),
            session_id: value["session_id"]
                .as_str()
                .map(String::from)
                .or_else(|| self.session_id.clone()),
        }
        .into_fact()
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
///   [`ClaudeCodeAdapter::map_result`] emits (both render through
///   [`Completion`]).
/// - everything else (`turn.started`, the machine-local `error` item, future
///   additions) → `Ok(vec![])`: tolerated noise, never an error.
///
/// # Why `turn.completed` is a RUN-terminal fact (I3)
///
/// `agent.completed` means THE RUN FINISHED: the reducer folds it by setting
/// the run's status to `completed` and OVERWRITING its cost totals
/// (`rezidnt-state`), and DR-048 slice C collates exactly those totals into a
/// leaderboard. So it may only be emitted at run end, carrying RUN totals —
/// emitting it per-turn would announce completion at the first turn boundary
/// and report one turn's tokens as the whole run's cost.
///
/// For `codex exec --json` that is derivable from the recording without
/// guessing: exec is single-shot. The recorded stream opens exactly one
/// `turn.started`, closes exactly one `turn.completed`, and the process exits
/// — stream end IS run end, and the single turn's usage IS the run total.
///
/// That premise is GUARDED, not assumed. A second `turn.completed` on one
/// stream falsifies it, and the adapter then refuses with
/// [`AdapterError::ContractViolated`] rather than emitting a second "the run
/// finished" fact or silently dropping the later turn's tokens. `num_turns` is
/// therefore the honest count of completed turns observed (always 1 while the
/// premise holds), not decoration.
///
/// # Deliberate gaps, stated rather than guessed
///
/// The codex event format carries no dollar cost and no duration, so
/// `cost.total_usd` and `duration_ms` are honest zeros (the house zero-default
/// convention) — never a fabricated number; cross-vendor cost comparison is
/// explicitly deferred (DR-048 §Decision 3). No turn-level FAILURE line has
/// been recorded, so none is mapped: a codex run that fails without emitting
/// `turn.completed` produces NO completion fact at all, and its failure
/// surfaces through the child's exit status rather than through a verdict this
/// adapter invented.
#[derive(Debug)]
pub struct CodexAdapter {
    run: RunId,
    thread_id: Option<String>,
    completed_turns: u64,
}

/// Harness name carried on [`AdapterError::ContractViolated`] refusals.
const CODEX_HARNESS: &str = "codex";

/// The run outcome as the codex format states it.
///
/// The EVENT TYPE is the only outcome signal the stream carries — there is no
/// status field on `turn.completed`. `turn.completed` is the harness
/// POSITIVELY asserting the turn finished, so the status is READ OFF that
/// assertion; it is not stamped onto an absence. Every other line type returns
/// `None` and maps to no fact at all, which is the I6-honest handling of an
/// outcome this adapter has never been shown: an unknown is never coerced into
/// a pass. When a failing codex transcript is recorded, its terminal line type
/// gets an arm here — and not before.
fn turn_outcome(line_type: &str) -> Option<&'static str> {
    match line_type {
        "turn.completed" => Some("success"),
        _ => None,
    }
}

impl CodexAdapter {
    pub fn new(run: RunId) -> Self {
        Self {
            run,
            thread_id: None,
            completed_turns: 0,
        }
    }

    /// Map one `codex exec --json` line to zero or more facts. Unknown line
    /// types are tolerated (`Ok(vec![])`); non-JSON input is
    /// [`AdapterError::BadLine`]; a stream that breaks the recorded contract is
    /// [`AdapterError::ContractViolated`].
    pub fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError> {
        let value: Value = serde_json::from_str(line)?;
        let facts = match value["type"].as_str() {
            Some("thread.started") => self.map_thread_started(&value),
            Some("item.completed") => self.map_item_completed(&value),
            // A recorded terminal line type ends the RUN (see the type docs);
            // `turn.started`, the machine-local `error` item, and anything the
            // CLI adds later are tolerated noise — additive evolution.
            Some(line_type) => match turn_outcome(line_type) {
                Some(status) => vec![self.map_run_completed(&value, status)?],
                None => vec![],
            },
            None => vec![],
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

    /// A recorded turn-terminal line → `agent.completed` carrying RUN totals.
    ///
    /// Guards the single-turn premise the run-terminal mapping rests on: see
    /// the type docs for why exec-mode stream end is run end, and why a second
    /// completed turn must refuse rather than emit.
    fn map_run_completed(
        &mut self,
        value: &Value,
        status: &'static str,
    ) -> Result<MappedFact, AdapterError> {
        self.completed_turns += 1;
        if self.completed_turns > 1 {
            return Err(AdapterError::ContractViolated {
                harness: CODEX_HARNESS,
                detail: format!(
                    "a second turn-terminal line arrived on one `codex exec --json` stream \
                     (completed turn {}), falsifying the single-shot premise the recorded \
                     contract rests on. `agent.completed` was already emitted as this run's \
                     terminal fact carrying turn 1's usage as the RUN total, so the run's true \
                     totals are no longer derivable from the log — re-record the transcript \
                     contract for multi-turn exec before these numbers are trusted",
                    self.completed_turns
                ),
            });
        }
        Ok(Completion {
            run: self.run,
            status,
            // The codex format carries neither of these — honest zeros, never
            // fabricated numbers (DR-048 §Decision 3 defers cross-vendor cost).
            total_usd: Value::from(0),
            duration_ms: Value::from(0),
            input_tokens: number_or_zero(&value["usage"]["input_tokens"]),
            output_tokens: number_or_zero(&value["usage"]["output_tokens"]),
            // No aggregate turn count on the wire: the observed count, which
            // the guard above holds at 1 for as long as the premise holds.
            num_turns: Value::from(self.completed_turns),
            session_id: self.thread_id.clone(),
        }
        .into_fact())
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
