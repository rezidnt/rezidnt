//! rezidnt MCP surface (doc §9, I5: MCP-first).
//!
//! Implementation shape (S3 implementer decision, recorded in the handoff):
//! a HAND-ROLLED thin JSON-RPC 2.0 layer rather than rmcp. The board pins the
//! observable JSON-RPC messages, the schemars-generated `inputSchema` values,
//! and two bespoke transports (in-process duplex stdio, lockfile-announced
//! loopback HTTP) — a dependency-light dispatch loop carries less risk than
//! adapting an SDK's server model to those pins (I7: every new dependency is
//! attack surface).
//!
//! Shape law (binding for this crate, set by the S3 board): the core is
//! TRANSPORT-AGNOSTIC — [`McpCore::handle`] maps one JSON-RPC 2.0 request
//! value to one response value. Transports (stdio lines, loopback HTTP) are
//! thin byte shims over that seam (I4).
//!
//! Surface pinned by the board:
//! - tools: `open_project`, `spawn_agent`, `gate_explain`, `tail_events`;
//!   every `inputSchema` served by `tools/list` MUST equal
//!   `schemars::schema_for!` of the matching `rezidnt_types::mcp` type
//!   (doc §9 no-drift rule).
//! - resources: `rezidnt://run/<ulid>/dossier` — the run's folded dossier
//!   state (I3: derived from the log, never a side store).
//! - badges (doc §12): mutating tools are refused with a machine-readable
//!   code BEFORE any side effect when the badge is missing or unknown.
//! - tool errors ride the MCP result shape: `isError: true` and
//!   `content[0].text` parsing as JSON `{"code": "...", ...}` ([`codes`]).

pub mod lockfile;

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use rezidnt_fabric::Fabric;
use rezidnt_run::badge::Badge;
use rezidnt_run::spec::ProjectSpec;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tracing::Instrument as _;

/// Machine-readable tool/resource error codes (mirrors the socket-side
/// `rezidnt_proto::codes` discipline: strings, additive evolution).
pub mod codes {
    /// A mutating tool was called with no `badge` argument.
    pub const BADGE_REQUIRED: &str = "badge.required";
    /// The presented badge token is not one the daemon issued (or it was
    /// revoked).
    pub const BADGE_INVALID: &str = "badge.invalid";
    /// A run ULID that the log does not know.
    pub const RUN_UNKNOWN: &str = "run.unknown";
    /// `open_project` carried a spec that failed to parse/validate (§13).
    pub const SPEC_INVALID: &str = "spec.invalid";
    /// `gate_explain` on a run with no gate verdict on the log. Honest
    /// absence — NEVER coerced to a pass (I6).
    pub const GATE_NO_VERDICT: &str = "gate.no_verdict";
    /// Implementer additions (DEFAULT, additive): refusals the board does not
    /// pin but the surface needs to stay machine-readable everywhere.
    /// A required tool argument is missing or of the wrong type.
    pub const ARGS_INVALID: &str = "args.invalid";
    /// A mutating tool was called on a core with no substrate wired (a bare
    /// [`McpCore`] outside the daemon).
    pub const SUBSTRATE_UNAVAILABLE: &str = "substrate.unavailable";
    /// `spawn_agent` named a workspace this daemon has not opened.
    pub const WORKSPACE_UNKNOWN: &str = "workspace.unknown";
    /// `spawn_agent` named an agent the workspace's spec does not define.
    pub const AGENT_UNKNOWN: &str = "agent.unknown";
    /// The spawn itself failed after all checks passed.
    pub const SPAWN_FAILED: &str = "spawn.failed";
}

/// MCP protocol version this server speaks (DEFAULT: the current spec rev).
const PROTOCOL_VERSION: &str = "2025-06-18";

/// `tail_events` server default when the caller sends no `limit` (DEFAULT).
const TAIL_DEFAULT_LIMIT: usize = 1024;

/// Max request-body bytes the loopback HTTP transport will accumulate before
/// rejecting the request 413-class (DEFAULT). Mirrors the 64 KiB HEAD cap and
/// leaves headroom over the I2 32 KiB *payload* rule (the JSON-RPC envelope
/// wraps the payload). Bounds the memory a single loopback connection can
/// force the daemon to allocate. Cheap to revisit.
const BODY_CAP_BYTES: usize = 64 * 1024;

/// MCP-domain errors (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("lockfile: {0}")]
    Lockfile(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode/decode: {0}")]
    Json(#[from] serde_json::Error),
}

/// The set of badges the surface will honor on mutating calls (doc §12).
/// Maps token → loggable badge id; the token itself is never logged.
#[derive(Debug, Default)]
pub struct BadgeBook {
    entries: BTreeMap<String, String>,
}

impl BadgeBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit a minted badge: its token becomes valid on mutating calls,
    /// attributable in the log as `badge.id()`.
    pub fn admit(&mut self, badge: &Badge) {
        self.entries
            .insert(badge.token_hex(), badge.id().to_string());
    }

    /// Loggable id for a presented token; `None` = refuse (`badge.invalid`).
    pub fn id_for(&self, token: &str) -> Option<&str> {
        self.entries.get(token).map(String::as_str)
    }
}

/// Boxed future alias for the substrate seam (no async-trait dependency).
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A machine-readable refusal from a substrate operation; becomes an
/// `isError: true` tool result carrying `code`.
#[derive(Debug, Clone)]
pub struct ToolRefusal {
    pub code: String,
    pub message: String,
}

impl ToolRefusal {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// The request-scoped `open_project` acknowledgement: the workspace ULID and
/// the correlation id every materialization fact of this open carries.
#[derive(Debug, Clone)]
pub struct OpenAck {
    pub workspace: String,
    pub correlation: String,
}

/// The daemon-side seam behind the mutating tools (I4: the core stays
/// transport- and substrate-agnostic; the daemon implements this over its run
/// substrate). Read-only tools and resources never touch it — they interrogate
/// the fabric directly (I3).
pub trait McpSubstrate: Send + Sync {
    /// Materialize a workspace from an ALREADY-VALIDATED §13 spec (the core
    /// parses first: badge → spec parse → substrate, doc §12 ordering).
    fn open_project(&self, spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>>;

    /// Spawn one spec agent; idempotent by `idempotency_key` (same key, same
    /// run ULID, exactly one spawn). Returns the run ULID text.
    fn spawn_agent(
        &self,
        workspace: String,
        agent: String,
        idempotency_key: String,
    ) -> BoxFuture<Result<String, ToolRefusal>>;
}

/// The transport-agnostic MCP core: one JSON-RPC request in, one response
/// out, side effects on the fabric only (I3: the log is truth).
pub struct McpCore {
    fabric: Arc<Fabric>,
    /// Interior mutability so a transport can admit the operator badge after
    /// construction (poison recovery: a token map holds no cross-key
    /// invariant, continuing with the inner value is sound).
    badges: RwLock<BadgeBook>,
    substrate: Option<Arc<dyn McpSubstrate>>,
}

impl McpCore {
    pub fn new(fabric: Fabric, badges: BadgeBook) -> Self {
        Self::new_shared(Arc::new(fabric), badges)
    }

    /// Construct over an already-shared fabric (the daemon owns one fabric
    /// serving both the socket and the MCP surface).
    pub fn new_shared(fabric: Arc<Fabric>, badges: BadgeBook) -> Self {
        Self {
            fabric,
            badges: RwLock::new(badges),
            substrate: None,
        }
    }

    /// Wire the mutating-tool substrate (builder-style; the daemon calls this,
    /// bare test cores skip it).
    pub fn with_substrate(mut self, substrate: Arc<dyn McpSubstrate>) -> Self {
        self.substrate = Some(substrate);
        self
    }

    /// The fabric this surface publishes to and reads from (tests assert
    /// side effects — and their absence — through it).
    pub fn fabric(&self) -> &Fabric {
        &self.fabric
    }

    /// Admit a badge on the live core (the HTTP transport mints the operator
    /// badge at serve time, doc §12).
    pub fn admit_badge(&self, badge: &Badge) {
        self.badges
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .admit(badge);
    }

    /// Handle one JSON-RPC 2.0 message. Returns `Some(response)` for
    /// requests, `None` for notifications. Never panics on garbage input —
    /// malformed JSON-RPC gets a spec error object (-32600/-32601/-32602).
    pub async fn handle(&self, request: Value) -> Option<Value> {
        let id = request.get("id").cloned().filter(|v| !v.is_null());
        let method = request.get("method").and_then(Value::as_str);
        let Some(method) = method else {
            // No method: a request (id present) gets -32600; a broken
            // notification gets silence (JSON-RPC 2.0 §4.1).
            return id.map(|id| rpc_error(id, -32600, "invalid request: no method"));
        };
        let id = id?; // notification: no response, no side effect (S3 surface)
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        let outcome = match method {
            "initialize" => Ok(initialize_result()),
            "tools/list" => tools_list(),
            "tools/call" => self.tools_call(params).await,
            "resources/read" => self.resources_read(params).await,
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match outcome {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => rpc_error(id, code, &message),
        })
    }

    /// `tools/call` dispatch. Tool-level failures are MCP tool results with
    /// `isError: true` (machine-readable [`codes`]); only protocol misuse
    /// (unknown tool, non-object params) is a JSON-RPC error.
    async fn tools_call(&self, params: Value) -> RpcOutcome {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "tools/call params require a name".to_string()))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match name {
            "open_project" => self.call_open_project(args).await,
            "spawn_agent" => self.call_spawn_agent(args).await,
            "gate_explain" => self.call_gate_explain(args).await,
            "tail_events" => self.call_tail_events(args).await,
            other => Err((-32602, format!("unknown tool: {other}"))),
        }
    }

    /// §12 door for mutating tools: the badge is checked BEFORE any parsing
    /// or side effect. Returns the loggable badge id on success.
    fn check_badge(&self, args: &Value) -> Result<String, Value> {
        let Some(token) = args.get("badge").and_then(Value::as_str) else {
            return Err(tool_refused(
                codes::BADGE_REQUIRED,
                "mutating tools require a badge argument (doc §12)",
            ));
        };
        let book = self
            .badges
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match book.id_for(token) {
            Some(id) => Ok(id.to_string()),
            None => Err(tool_refused(
                codes::BADGE_INVALID,
                "badge token is not one this daemon issued",
            )),
        }
    }

    async fn call_open_project(&self, args: Value) -> RpcOutcome {
        // Ordering pinned by the board: badge → spec parse → substrate.
        let _badge_id = match self.check_badge(&args) {
            Ok(id) => id,
            Err(refusal) => return Ok(refusal),
        };
        let Some(spec_toml) = args.get("spec_toml").and_then(Value::as_str) else {
            return Ok(tool_refused(
                codes::SPEC_INVALID,
                "open_project requires spec_toml",
            ));
        };
        if let Err(e) = ProjectSpec::from_toml_str(spec_toml) {
            return Ok(tool_refused(codes::SPEC_INVALID, format!("{e}")));
        }
        let Some(substrate) = &self.substrate else {
            return Ok(tool_refused(
                codes::SUBSTRATE_UNAVAILABLE,
                "no run substrate is wired to this MCP core",
            ));
        };
        match substrate.open_project(spec_toml.to_string()).await {
            Ok(ack) => Ok(tool_ok(json!({
                "workspace": ack.workspace,
                "correlation": ack.correlation,
            }))),
            Err(refusal) => Ok(tool_refused(&refusal.code, &refusal.message)),
        }
    }

    async fn call_spawn_agent(&self, args: Value) -> RpcOutcome {
        let _badge_id = match self.check_badge(&args) {
            Ok(id) => id,
            Err(refusal) => return Ok(refusal),
        };
        let field = |name: &str| -> Result<String, Value> {
            args.get(name)
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| {
                    tool_refused(codes::ARGS_INVALID, format!("spawn_agent requires {name}"))
                })
        };
        let (workspace, agent, key) =
            match (field("workspace"), field("agent"), field("idempotency_key")) {
                (Ok(w), Ok(a), Ok(k)) => (w, a, k),
                (Err(r), ..) | (_, Err(r), _) | (.., Err(r)) => return Ok(r),
            };
        let Some(substrate) = &self.substrate else {
            return Ok(tool_refused(
                codes::SUBSTRATE_UNAVAILABLE,
                "no run substrate is wired to this MCP core",
            ));
        };
        match substrate.spawn_agent(workspace, agent, key).await {
            Ok(run) => Ok(tool_ok(json!({"run": run}))),
            Err(refusal) => Ok(tool_refused(&refusal.code, &refusal.message)),
        }
    }

    /// I6 interrogability (doc §8): the recorded verdict, the failing
    /// verifier, its evidence CAS refs, and the EXACT inputs — all VERBATIM
    /// from the verdict fact on the log (I3: derived, never re-judged). The
    /// interrogation itself lands as one `gate.explained` fact.
    async fn call_gate_explain(&self, args: Value) -> RpcOutcome {
        let Some(run) = args.get("run").and_then(Value::as_str) else {
            return Ok(tool_refused(
                codes::ARGS_INVALID,
                "gate_explain requires run",
            ));
        };
        let events = self.replay(None).await?;
        // The LATEST verdict fact for this run wins (append order).
        let verdict_fact = events.iter().rev().find(|e| {
            matches!(
                e.subject.as_str(),
                "gate.passed" | "gate.failed" | "gate.inconclusive"
            ) && e.payload()["run"] == json!(run)
        });
        let Some(fact) = verdict_fact else {
            return Ok(tool_refused(
                codes::GATE_NO_VERDICT,
                format!(
                    "no gate verdict on the log for run {run} — honest absence, not a pass (I6)"
                ),
            ));
        };
        let verdict = match fact.subject.as_str() {
            "gate.passed" => "pass",
            "gate.failed" => "fail",
            _ => "inconclusive",
        };
        let payload = fact.payload();
        let mut explain = json!({
            "run": run,
            "gate": payload["gate"],
            "verdict": verdict,
            "verifier": payload["verifier"],
            "evidence": payload["evidence"],
            "inputs": payload["inputs"],
        });
        if let (Some(reason), Some(obj)) = (payload.get("reason"), explain.as_object_mut()) {
            obj.insert("reason".to_string(), reason.clone());
        }

        // gate.explained v1 (ratified): `run` is the pinned minimum; `gate` /
        // `verdict` are optional triage context. The explanation content is
        // derived from the log, never duplicated into the payload (I3).
        let explained = rezidnt_types::Event::new(
            rezidnt_types::SourceId::new("rezidnt-mcp"),
            fact.workspace,
            rezidnt_types::Subject::new("gate.explained"),
            fact.correlation,
            Some(fact.id),
            1,
            json!({"run": run, "gate": payload["gate"], "verdict": verdict}),
        )
        .map_err(|e| (-32603, format!("construct gate.explained: {e}")))?;
        let fabric = Arc::clone(&self.fabric);
        tokio::task::spawn_blocking(move || fabric.publish(explained))
            .await
            .map_err(|e| (-32603, format!("publish task panicked: {e}")))?
            .map_err(|e| (-32603, format!("append gate.explained: {e}")))?;

        Ok(tool_ok(explain))
    }

    /// Verbatim envelopes from the log, append order; `since` is an exclusive
    /// ULID lower bound.
    async fn call_tail_events(&self, args: Value) -> RpcOutcome {
        let since = match args.get("since").and_then(Value::as_str) {
            None => None,
            Some(text) => Some(ulid::Ulid::from_string(text).map_err(|e| {
                // Not a refusal path the board pins; -32602 keeps it honest.
                (-32602, format!("since is not a ULID: {e}"))
            })?),
        };
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|l| usize::try_from(l).ok())
            .unwrap_or(TAIL_DEFAULT_LIMIT);
        let mut events = self.replay(since).await?;
        events.truncate(limit);
        let envelopes = events
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| (-32603, format!("encode envelope: {e}")))?;
        Ok(tool_ok(json!({"events": envelopes})))
    }

    /// `resources/read` — `rezidnt://run/<ulid>/dossier`, the rezidnt-state
    /// fold of the log (I3: derived state, never a side store). Misses answer
    /// with machine-readable contents, never an error and never a hang.
    async fn resources_read(&self, params: Value) -> RpcOutcome {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or((-32602, "resources/read params require a uri".to_string()))?
            .to_string();
        let Some(run) = uri
            .strip_prefix("rezidnt://run/")
            .and_then(|rest| rest.strip_suffix("/dossier"))
        else {
            return Err((-32602, format!("unknown resource uri: {uri}")));
        };
        let events = self.replay(None).await?;
        let graph = rezidnt_state::fold(events.iter());
        let body = match graph.agent_runs.get(run) {
            Some(state) => {
                serde_json::to_value(state).map_err(|e| (-32603, format!("encode dossier: {e}")))?
            }
            None => json!({
                "code": codes::RUN_UNKNOWN,
                "run": run,
                "message": "no such run on the log",
            }),
        };
        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": body.to_string(),
            }]
        }))
    }

    /// Log replay off the async threads (SQLite is blocking; rust-conventions:
    /// no blocking in async).
    async fn replay(
        &self,
        since: Option<ulid::Ulid>,
    ) -> Result<Vec<rezidnt_types::Event>, (i64, String)> {
        let fabric = Arc::clone(&self.fabric);
        tokio::task::spawn_blocking(move || fabric.replay_since(since))
            .await
            .map_err(|e| (-32603, format!("replay task panicked: {e}")))?
            .map_err(|e| (-32603, format!("replay log: {e}")))
    }
}

/// `Ok(result value)` or `Err((json-rpc code, message))`.
type RpcOutcome = Result<Value, (i64, String)>;

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {"tools": {}, "resources": {}},
        "serverInfo": {
            "name": "rezidnt",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

/// The S3 tool surface. Every `inputSchema` is GENERATED from the
/// `rezidnt_types::mcp` shapes via schemars (doc §9 BINDING no-drift rule).
fn tools_list() -> RpcOutcome {
    let schema = |s: schemars::Schema| -> Result<Value, (i64, String)> {
        serde_json::to_value(s).map_err(|e| (-32603, format!("encode schema: {e}")))
    };
    Ok(json!({
        "tools": [
            {
                "name": "open_project",
                "description": "Materialize a workspace from a §13 project spec (mutating: badge required).",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::OpenProjectArgs))?,
            },
            {
                "name": "spawn_agent",
                "description": "Spawn one spec agent in an open workspace (mutating: badge and idempotency key required).",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::SpawnAgentArgs))?,
            },
            {
                "name": "gate_explain",
                "description": "Why is this run blocked: the failing verifier, evidence refs, and exact inputs (I6).",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::GateExplainArgs))?,
            },
            {
                "name": "tail_events",
                "description": "Read event envelopes from the log, in log order; `since` is exclusive.",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::TailEventsArgs))?,
            },
        ]
    }))
}

/// A successful tool result: machine-readable JSON in `content[0].text`.
fn tool_ok(payload: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": payload.to_string()}],
        "isError": false,
    })
}

/// A refused tool call: `isError: true`, `content[0].text` carries the code.
fn tool_refused(code: impl AsRef<str>, message: impl AsRef<str>) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": json!({
                "code": code.as_ref(),
                "message": message.as_ref(),
            }).to_string(),
        }],
        "isError": true,
    })
}

/// Serve MCP over a byte stream, newline-delimited JSON-RPC — the stdio
/// transport shape (doc §9), testable in-process over a duplex pipe.
pub async fn serve_stdio<R, W>(core: Arc<McpCore>, reader: R, mut writer: W) -> Result<(), McpError>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => core.handle(request).await,
            // Unparseable frame: JSON-RPC parse error, id unknowable → null.
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(response) = response {
            let mut frame = response.to_string();
            frame.push('\n');
            writer.write_all(frame.as_bytes()).await?;
            writer.flush().await?;
        }
    }
    Ok(())
}

/// A running loopback-HTTP transport. Dropping it stops the listener.
pub struct HttpHandle {
    /// The ACTUAL bound port (never 0, never fixed — doc §9).
    pub port: u16,
    /// Full endpoint URL clients POST JSON-RPC to, as announced in the
    /// lockfile (e.g. `http://127.0.0.1:<port>/mcp`).
    pub url: String,
    /// The transport's dedicated runtime; `Option` so `Drop` can take it and
    /// shut it down without blocking (dropping a `Runtime` inside an async
    /// context would panic; `shutdown_background` never blocks).
    runtime: Option<tokio::runtime::Runtime>,
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Serve MCP over loopback HTTP on `127.0.0.1:0` and announce the bound
/// endpoint by writing the lockfile at `lockfile_path` (doc §9: port 0,
/// announced via lockfile — not a fixed port). Mints the daemon-lifetime
/// OPERATOR badge, admits it on the core, and carries its token in the
/// 0600 lockfile (doc §12: possession = the local user).
///
/// The accept loop runs on its OWN single-worker runtime, not the caller's:
/// the transport must stay responsive even when the caller's runtime is a
/// current-thread executor whose thread is busy (exactly what a blocking
/// stdio-first client embedding looks like).
pub async fn serve_http(core: Arc<McpCore>, lockfile_path: &Path) -> Result<HttpHandle, McpError> {
    // Bind via std (one nonblocking syscall in practice, but kept off the
    // async threads per convention) and hand the socket to the transport
    // runtime below.
    let listener = tokio::task::spawn_blocking(|| std::net::TcpListener::bind(("127.0.0.1", 0)))
        .await
        .map_err(|e| McpError::Transport(format!("bind task panicked: {e}")))??;
    listener.set_nonblocking(true)?;
    let port = listener
        .local_addr()
        .map_err(|e| McpError::Transport(format!("local_addr: {e}")))?
        .port();
    let url = format!("http://127.0.0.1:{port}/mcp");

    let operator =
        Badge::mint().map_err(|e| McpError::Transport(format!("mint operator badge: {e}")))?;
    core.admit_badge(&operator);
    let lock = lockfile::Lockfile {
        pid: std::process::id(),
        port,
        url: url.clone(),
        badge: operator.token_hex(),
    };
    let path = lockfile_path.to_path_buf();
    tokio::task::spawn_blocking(move || lockfile::write_atomic(&path, &lock))
        .await
        .map_err(|e| McpError::Transport(format!("lockfile task panicked: {e}")))??;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("rezidnt-mcp-http")
        .enable_all()
        .build()?;
    let span = tracing::info_span!("adapter", kind = "mcp-http", port);
    runtime.spawn(
        async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(e) => {
                    tracing::error!(error = %e, "mcp http listener registration failed");
                    return;
                }
            };
            loop {
                let (stream, _peer) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        tracing::warn!(error = %e, "mcp http accept failed");
                        continue;
                    }
                };
                let core = Arc::clone(&core);
                let conn_span = tracing::info_span!("adapter", kind = "mcp-http-conn");
                tokio::spawn(
                    async move {
                        if let Err(e) = serve_http_conn(core, stream).await {
                            tracing::debug!(error = %e, "mcp http connection ended");
                        }
                    }
                    .instrument(conn_span),
                );
            }
        }
        .instrument(span),
    );

    Ok(HttpHandle {
        port,
        url,
        runtime: Some(runtime),
    })
}

/// One HTTP/1.1 exchange: read head + body, dispatch, answer, close.
/// Deliberately minimal — the transport is loopback-only and lockfile-gated.
async fn serve_http_conn(
    core: Arc<McpCore>,
    mut stream: tokio::net::TcpStream,
) -> Result<(), McpError> {
    use tokio::io::AsyncReadExt as _;

    let mut raw = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];
    let (head_end, body_start) = loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(()); // peer went away before a full head
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(pos) = find_head_end(&raw) {
            break (pos, pos + 4);
        }
        if raw.len() > 64 * 1024 {
            return Err(McpError::Transport("request head too large".to_string()));
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let content_length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    // I2-adjacent bound: a body over the cap is refused 413-class and never
    // accumulated unbounded. Reject on the declared Content-Length up front,
    // and again in the read loop so a lying/short Content-Length cannot slip
    // an over-cap body past the check.
    if content_length > BODY_CAP_BYTES {
        return respond_body_too_large(&mut stream).await;
    }
    while raw.len() < body_start + content_length {
        if raw.len().saturating_sub(body_start) > BODY_CAP_BYTES {
            return respond_body_too_large(&mut stream).await;
        }
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(()); // truncated body: nothing to answer
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let body = &raw[body_start..body_start + content_length];

    let response = match serde_json::from_slice::<Value>(body) {
        Ok(request) => core.handle(request).await,
        Err(e) => Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let frame = match response {
        Some(response) => {
            let body = response.to_string();
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        // A notification: acknowledged, no JSON-RPC response body.
        None => {
            "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
        }
    };
    stream.write_all(frame.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// Answer an over-cap request with a 413-class status and close. The body is
/// never read unbounded — the caller returns immediately after this.
async fn respond_body_too_large(stream: &mut tokio::net::TcpStream) -> Result<(), McpError> {
    let frame = "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    stream.write_all(frame.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// Position of the `\r\n\r\n` head/body split, if complete.
fn find_head_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}
