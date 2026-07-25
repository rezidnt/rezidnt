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

/// Read a harness's `usage` object into [`TokenUsage`], distinguishing
/// NOT REPORTED from reported-as-zero.
///
/// The whole object present ⇒ the harness reported accounting, and a missing
/// sub-field inside it is the ordinary zero default. The object absent ⇒ the
/// harness measured nothing (a failed `codex exec` turn carries no `usage` at
/// all) ⇒ `None`, and the token keys are omitted from the fact entirely.
fn reported_usage(usage: &Value) -> Option<TokenUsage> {
    usage.is_object().then(|| TokenUsage {
        input: number_or_zero(&usage["input_tokens"]),
        output: number_or_zero(&usage["output_tokens"]),
    })
}

/// The accounting fields of an `agent.completed` fact, before rendering.
///
/// Every substrate renders its completion THROUGH this type, so the fact's key
/// set is emitted from exactly ONE `json!` literal ([`Completion::into_fact`])
/// and cannot drift between harnesses — the shape is single-sourced by
/// construction, and `completion_fact_shape_is_single_sourced` in
/// `tests/codex_adapter_guards.rs` pins it by test as well.
///
/// # Zero versus absence
///
/// A field the harness's format NEVER carries for ANY outcome is an honest `0`
/// supplied by the caller (codex has no dollar cost and no duration). A field
/// the harness carries for some outcomes and genuinely did not MEASURE for this
/// one is ABSENT — see [`TokenUsage`]. The two are different claims: `0` says
/// "measured, and it was nothing"; absence says "never measured". Collapsing
/// the second into the first would let a failed run read as a free one on the
/// DR-048 slice C leaderboard.
struct Completion {
    run: RunId,
    status: &'static str,
    total_usd: Value,
    /// `None` when the harness reported NO token accounting for this run — the
    /// token keys are then omitted entirely rather than emitted as zeros.
    usage: Option<TokenUsage>,
    num_turns: Value,
    duration_ms: Value,
    session_id: Option<String>,
    /// The harness's own failure reason, VERBATIM, when it reported one. Kept
    /// opaque: a harness may put a JSON-encoded upstream response here (codex
    /// 0.145.0 does) or plain prose, so parsing it would pin structure no
    /// recording promises.
    error_message: Option<String>,
}

/// Token counts a harness reported for a run.
///
/// Modeled as one unit, present-or-absent together, because that is how the
/// wire carries it: codex emits a whole `usage` object or none at all (a failed
/// turn has none), and claude-code always emits one. There is no recorded case
/// of a half-reported count.
struct TokenUsage {
    input: Value,
    output: Value,
}

impl Completion {
    /// The ONE `agent.completed` payload builder in this crate: every substrate
    /// renders through it, so the fact's key set cannot drift between
    /// harnesses. Optional keys are omitted, never emitted as null — a null key
    /// would be a present claim of an absent value (DR-012 declared-vs-absent),
    /// and `tests/codex_adapter_turn_failed.rs` rejects a null token key
    /// explicitly.
    fn into_fact(self) -> MappedFact {
        let mut cost = serde_json::Map::new();
        cost.insert("total_usd".to_string(), self.total_usd);
        if let Some(usage) = self.usage {
            cost.insert("input_tokens".to_string(), usage.input);
            cost.insert("output_tokens".to_string(), usage.output);
        }
        let mut payload = serde_json::json!({
            "run": self.run,
            "status": self.status,
            "cost": Value::Object(cost),
            "num_turns": self.num_turns,
            "duration_ms": self.duration_ms,
        });
        let Some(obj) = payload.as_object_mut() else {
            unreachable!("the payload literal above is a JSON object")
        };
        // Both stay CONDITIONAL: absent session means the stream never
        // announced a resume identity; absent error means it reported no
        // failure reason.
        if let Some(session) = self.session_id {
            obj.insert("session_id".to_string(), Value::String(session));
        }
        if let Some(message) = self.error_message {
            obj.insert(
                "error".to_string(),
                serde_json::json!({ "message": message }),
            );
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
            // claude-code's `result` line always carries a usage object, so the
            // counts are always reported — zeros here mean "reported as zero",
            // which is the honest reading for this harness.
            usage: reported_usage(&value["usage"]),
            num_turns: number_or_zero(&value["num_turns"]),
            duration_ms: number_or_zero(&value["duration_ms"]),
            session_id: value["session_id"]
                .as_str()
                .map(String::from)
                .or_else(|| self.session_id.clone()),
            // The claude-code `result` line's failure detail is not mapped: no
            // failing claude transcript is recorded, and the shape of its error
            // reporting is therefore unpinned. Adding it needs a recording.
            error_message: None,
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
/// - `turn.completed` → `agent.completed` with `status: "success"`, and
///   `turn.failed` → `agent.completed` with `status: "error"` carrying the
///   harness's verbatim failure reason — both in the SAME payload shape
///   [`ClaudeCodeAdapter::map_result`] emits (all three render through
///   [`Completion`]).
/// - everything else → `Ok(vec![])`: tolerated noise, never an error. That
///   includes `turn.started`, the machine-local `item.completed` items of type
///   `error` (the failing recording carries two — a model-metadata warning and
///   the same skills-context notice the SUCCESSFUL probe carries — which is
///   what proves they are notices, not outcome signals), and the top-level
///   `error` line, whose message is subsumed by the `turn.failed` it precedes
///   rather than minted as a fact of its own.
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
/// That premise is GUARDED, not assumed. A second TERMINAL line on one stream
/// — of either outcome — falsifies it, and the adapter then refuses with
/// [`AdapterError::ContractViolated`] rather than emitting a second "the run
/// finished" fact or silently dropping the later turn's accounting. The guard
/// counts terminal lines, not successes: a stream carrying `turn.completed`
/// then `turn.failed` would leave the run's OUTCOME as ambiguous as its
/// totals, which is the same defect.
///
/// # `num_turns` counts terminal turns, of either outcome
///
/// A failed turn is still a turn the run took: the stream positively shows one
/// `turn.started` and one terminal line, so the count is OBSERVED, not
/// inferred. That is what separates it from `usage` below — reporting `0` here
/// would deny a turn the recording shows happening, whereas reporting `0`
/// tokens would invent a measurement the harness never made.
///
/// # Deliberate gaps, stated rather than guessed
///
/// The codex event format carries no dollar cost and no duration for ANY
/// outcome, so `cost.total_usd` and `duration_ms` are honest zeros (the house
/// zero-default convention) — never fabricated numbers; cross-vendor cost
/// comparison is explicitly deferred (DR-048 §Decision 3).
///
/// `turn.failed` carries NO `usage` object, so the token keys are OMITTED from
/// its completion fact rather than emitted as zeros. DR-048 slice C collates
/// these into a leaderboard, where a zero-token failure would read as a free
/// run; absence reads as what it is — never measured.
#[derive(Debug)]
pub struct CodexAdapter {
    run: RunId,
    thread_id: Option<String>,
    /// Turns that reached a TERMINAL line, whatever their outcome — both
    /// `turn.completed` and `turn.failed` count (see the type docs).
    terminal_turns: u64,
}

/// Harness name carried on [`AdapterError::ContractViolated`] refusals.
const CODEX_HARNESS: &str = "codex";

/// The run outcome as the codex format states it.
///
/// The EVENT TYPE is the only outcome signal the stream carries — there is no
/// status field on either terminal line. Both arms are RECORDED, which is what
/// makes this a derivation rather than an inference: the successful probe ends
/// in `turn.completed`, and the failing recording
/// (`codex_exec_v0.145.0_turn_failed.jsonl`, a bogus `-m` model) ends in
/// `turn.failed` and contains NO `turn.completed`. A turn that ends badly
/// therefore cannot reach the success arm.
///
/// Every other line type returns `None` and maps to no fact at all — the
/// I6-honest handling of an outcome this adapter has never been shown. A third
/// terminal type gets an arm here when a transcript records one, and not
/// before.
fn turn_outcome(line_type: &str) -> Option<&'static str> {
    match line_type {
        "turn.completed" => Some("success"),
        "turn.failed" => Some("error"),
        _ => None,
    }
}

impl CodexAdapter {
    pub fn new(run: RunId) -> Self {
        Self {
            run,
            thread_id: None,
            terminal_turns: 0,
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

    /// A recorded turn-terminal line (`turn.completed` or `turn.failed`) →
    /// `agent.completed` carrying RUN totals.
    ///
    /// Guards the single-turn premise the run-terminal mapping rests on: see
    /// the type docs for why exec-mode stream end is run end, and why a second
    /// terminal line must refuse rather than emit.
    fn map_run_completed(
        &mut self,
        value: &Value,
        status: &'static str,
    ) -> Result<MappedFact, AdapterError> {
        self.terminal_turns += 1;
        if self.terminal_turns > 1 {
            return Err(AdapterError::ContractViolated {
                harness: CODEX_HARNESS,
                detail: format!(
                    "a second turn-terminal line arrived on one `codex exec --json` stream \
                     (terminal turn {}), falsifying the single-shot premise the recorded \
                     contract rests on. `agent.completed` was already emitted as this run's \
                     terminal fact carrying turn 1's accounting as the RUN total, so the run's \
                     true totals and its OUTCOME are no longer derivable from the log — \
                     re-record the transcript contract for multi-turn exec before these \
                     numbers are trusted",
                    self.terminal_turns
                ),
            });
        }
        Ok(Completion {
            run: self.run,
            status,
            // The codex format carries neither of these for ANY outcome —
            // honest zeros, never fabricated numbers (DR-048 §Decision 3 defers
            // cross-vendor cost).
            total_usd: Value::from(0),
            duration_ms: Value::from(0),
            // Present on `turn.completed`, ABSENT on `turn.failed` — a failed
            // turn measured no tokens, and absence is the honest way to say so.
            usage: reported_usage(&value["usage"]),
            // Terminal turns OBSERVED, whatever their outcome (see the type
            // docs); the guard above holds this at 1 while the premise holds.
            num_turns: Value::from(self.terminal_turns),
            session_id: self.thread_id.clone(),
            // Read from THIS line's own `error.message`, not from the preceding
            // top-level `error` line: the two carry identical strings in the
            // recording, and sourcing it locally keeps the mapping free of
            // cross-line state and leaves that line as unmapped noise.
            error_message: value["error"]["message"].as_str().map(String::from),
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
