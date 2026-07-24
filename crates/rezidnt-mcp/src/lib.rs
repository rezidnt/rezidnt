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
//! - tools: `open_project`, `spawn_agent`, `gate_explain`, `tail_events`,
//!   `board_view` (DR-039 read-only fleet projection);
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
use std::sync::{Arc, OnceLock, RwLock};

use rezidnt_cas::Cas;
use rezidnt_fabric::Fabric;
use rezidnt_gate::permit::PermitVerifierSpec;
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
    /// DR-035 §Decision 3 — `resolve_permit` was called with a broad scope
    /// (`scope="run_tool"`) but no `ttl_ms`. Broad OR permanent, never both: a
    /// broad grant MUST be time-boxed so the dangerous quadrant (broad AND
    /// permanent) is structurally unreachable on the log. Additive code — older
    /// peers tolerate an unknown refusal code (I5).
    pub const SCOPE_REQUIRES_TTL: &str = "scope.requires_ttl";
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

/// DR-032 §Decision 1: the substrate's acknowledgement of a driven kill — the
/// reaper's returned stop description (`reaper::stop_with_escalation`). The
/// emitted `agent.signaled` fact does NOT depend on these bytes for its
/// operator attribution (that comes from the verified badge id + the caller's
/// reason); these ride the fact's `signal`/`escalation` fields for the
/// interrogable "how it was stopped" record.
#[derive(Debug, Clone)]
pub struct KillAck {
    /// The signal the reaper delivered (`"SIGTERM"` / `"SIGKILL"`).
    pub signal: String,
    /// The escalation stage, when the reaper escalated (`"term"` → answered on
    /// SIGTERM; `"kill"`/`Some("kill")` → escalated to SIGKILL). `None` when the
    /// substrate reports no escalation detail.
    pub escalation: Option<String>,
}

/// The resolved `[gates.permit]` verifier set for a run — the ordered verifier
/// entries the PDP dispatches (SP-wire, DR-011; SP3 adds exec entries, DR-015).
/// The daemon folds this from the applied spec (`workspace.spec.applied`, keyed
/// by workspace, I3); the core injects the run's folded state as pinned params
/// and aggregates via [`rezidnt_gate::permit::aggregate_async`] (natives run
/// sync in-process; an exec entry runs as an `await`ed §8 subprocess).
///
/// An EMPTY set is honest-undecidable: the aggregator escalates it, never a
/// synthesized allow (I6). NO config resolved at all (bare core, no substrate)
/// degrades the same way — escalate/deny, never allow (DR-011 §3).
#[derive(Debug, Clone, Default)]
pub struct PermitConfig {
    verifiers: Vec<PermitVerifierSpec>,
}

impl PermitConfig {
    /// Build a config from native `(name, params)` pairs in dispatch order.
    pub fn natives(entries: &[(&str, Value)]) -> Self {
        Self {
            verifiers: entries
                .iter()
                .map(|(name, params)| PermitVerifierSpec::native(*name, params.clone()))
                .collect(),
        }
    }

    /// Build a config from already-resolved verifier specs (the daemon path,
    /// where the specs come from the applied `[gates.permit]` block).
    pub fn from_specs(verifiers: Vec<PermitVerifierSpec>) -> Self {
        Self { verifiers }
    }

    /// The resolved verifier set, in dispatch order.
    pub fn verifiers(&self) -> &[PermitVerifierSpec] {
        &self.verifiers
    }
}

/// A transport-neutral permit decision request (SP2, DR-013 decision 1). Both
/// callers build this: the MCP JSON-RPC adapter ([`McpCore::call_request_permission`])
/// from its `args`, and the socket handler from a
/// `rezidnt_proto::Request::RequestPermission`. Extracting it means the PDP flow
/// lives in exactly one place ([`McpCore::decide_permit`]) — MCP and socket
/// facts are byte-identical, no fork (I3).
#[derive(Debug, Clone)]
pub struct PermitRequest {
    /// The run whose per-run state (intent allowlist + spend accumulator) the
    /// PDP folds (I3).
    pub run: String,
    /// The caller-supplied correlation token (the PEP's, over the socket). When
    /// `Some`, the decision echoes it — the PEP's ask and the on-log fact share
    /// one id. When `None` (MCP without a token), the PDP mints one.
    pub request_id: Option<String>,
    /// The action being authorized (e.g. `tool.invoke`).
    pub action: String,
    /// The tool the request axis reads (e.g. `Bash`).
    pub tool: String,
    /// The caller's badge token. The MCP adapter checks this at its own door
    /// (badge-first, §12) BEFORE calling `decide_permit`; the socket transport
    /// omits it (0600 UDS is the identity, DR-013 decision 3). When present it
    /// only decorates the `permit.requested` fact's `badge_id` (best-effort
    /// resolution) — `decide_permit` NEVER refuses on a missing/unknown badge.
    pub badge: Option<String>,
    /// Bulk action context as a CAS ref string — never inline bytes (I2).
    pub context_ref: Option<String>,
    /// The path axis the native verifiers read (`path-scope`, etc.).
    pub paths: Option<Value>,
}

/// The three-valued authorization decision (SP2, DR-013 decision 1) — the wire
/// vocabulary the PDP reaches, NEVER coerced (I6). Mirrors
/// [`rezidnt_gate::permit::PermitDecision`] at the MCP-surface boundary so the
/// socket handler maps it to `rezidnt_proto::Reply::PermitDecision` without
/// reaching into the gate crate's internal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// `allow` — the action may proceed.
    Allow,
    /// `deny` — the action is blocked.
    Deny,
    /// `ask` — escalate to a human; inconclusive is never coerced to allow (I6).
    Ask,
}

impl Decision {
    /// The wire word carried to the PEP (`allow | deny | ask`).
    pub fn as_word(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        }
    }
}

impl From<rezidnt_gate::permit::PermitDecision> for Decision {
    fn from(d: rezidnt_gate::permit::PermitDecision) -> Self {
        match d {
            rezidnt_gate::permit::PermitDecision::Grant => Decision::Allow,
            rezidnt_gate::permit::PermitDecision::Deny => Decision::Deny,
            rezidnt_gate::permit::PermitDecision::Escalate => Decision::Ask,
        }
    }
}

/// The transport-neutral outcome of a permit decision (SP2, DR-013 decision 1).
/// The socket handler maps this to `Reply::PermitDecision`; the MCP adapter maps
/// it back to a `tool_ok`. The on-log facts (`permit.requested` + one decision
/// fact) are ALREADY emitted by [`McpCore::decide_permit`] before it returns —
/// no caller re-emits (I3).
#[derive(Debug, Clone)]
pub struct PermitOutcome {
    /// The correlation token this decision resolved under — the caller's when
    /// supplied, else the minted one. The socket echoes it back to the PEP.
    pub request_id: String,
    /// The three-valued decision, never coerced (I6).
    pub decision: Decision,
    /// Why, on a deny/ask (the deciding verifier's message); absent on a
    /// trivially-granted allow.
    pub reason: Option<String>,
    /// DR-035 §Decision 1 — the incoming `permit.requested`'s envelope-ULID
    /// timestamp (ms), the anchor the DR-034 live-unblock threads BACK into
    /// `recheck_resolution` so a resolution's TTL is measured against the HELD
    /// request's ORIGINAL ask time (the request happened once, on the first pass).
    /// Captured from the `permit.requested` event this decision emitted; `0` on a
    /// re-decide path that did not mint a fresh request (the value is only read by
    /// the escalate→hold path, which always carries the first-pass timestamp).
    pub requested_ms: u64,
}

/// PDP-domain errors from [`McpCore::decide_permit`] (thiserror per lib
/// convention). Distinct from a JSON-RPC error tuple: the socket handler maps
/// these to `Reply::Error`, the MCP adapter maps them to a `(-32603, msg)`.
#[derive(Debug, thiserror::Error)]
pub enum PdpError {
    /// A daemon-side fault while reaching the decision (CAS/log/aggregate).
    #[error("permit decision: {0}")]
    Internal(String),
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

    /// Resolve the applied `[gates.permit]` verifier set for a run (SP-wire,
    /// DR-011 §1). The daemon folds this from its opened-workspace registry
    /// (`workspace.spec.applied`, keyed by workspace, I3); config selection is a
    /// substrate capability, like `open_project`/`spawn_agent` (I4). `None` when
    /// the run maps to no configured permit gate — the PDP then degrades to
    /// escalate/deny, never a synthesized allow (I6).
    fn permit_config_for(&self, run: String) -> BoxFuture<Option<PermitConfig>>;

    /// DR-032 §Decision 1: drive the EXISTING `reaper::stop_with_escalation`
    /// (reaper.rs) to terminate the run's process, reporting the stop as a
    /// [`KillAck`]. The core drives this ONLY after the operator-badge door
    /// admits the caller (a refused kill never reaches here — no side effect,
    /// I3), then emits ONE attributed `agent.signaled` fact through the single
    /// writer. A `ToolRefusal` here (e.g. the run is not live) becomes a
    /// machine-readable tool error and emits NO fact.
    fn kill_run(&self, run: String) -> BoxFuture<Result<KillAck, ToolRefusal>>;
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
    /// The CAS the permit gate runs against (native verifiers write evidence
    /// blobs and carry refs, I2). The daemon wires its own; a bare test core
    /// gets a lazily-opened ephemeral CAS (see [`McpCore::permit_cas`]).
    cas: Option<Arc<Cas>>,
    /// Ephemeral fallback CAS for a core with no wired CAS — opened once under
    /// the OS temp dir on first permit decision, then reused.
    ephemeral_cas: OnceLock<Arc<Cas>>,
    /// A statically-injected permit config (SP-wire, DR-011): the resolved
    /// `[gates.permit]` verifier set to dispatch for EVERY run on this core.
    /// The daemon-wired path resolves per-run via
    /// [`McpSubstrate::permit_config_for`]; a static config is the test-double /
    /// single-workspace seam. `None` here AND no substrate ⇒ no config ⇒
    /// escalate/deny, never a synthesized allow (I6, DR-011 §3).
    permit_config: Option<PermitConfig>,
    /// DR-017 (SP4b): the daemon's process-lifetime macaroon root key — the
    /// trust anchor `check_badge` verifies agent macaroons against. `None` on a
    /// bare/keyless core, in which case NO agent macaroon can verify (an agent
    /// badge presented to a keyless core is `badge.invalid`). The daemon wires
    /// its minted key via [`McpCore::with_root_key`]; the opaque operator badge
    /// path (DR-005) does not use it.
    root_key: Option<rezidnt_run::badge::RootKey>,
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
            cas: None,
            ephemeral_cas: OnceLock::new(),
            permit_config: None,
            root_key: None,
        }
    }

    /// Wire the daemon's macaroon root key (builder-style; the daemon mints one
    /// at startup and wires it here, DR-017 §Decision 6). A core with no root key
    /// verifies no agent macaroon — an agent badge presented to a keyless core is
    /// `badge.invalid`. Mirrors [`McpCore::with_substrate`].
    pub fn with_root_key(mut self, root: rezidnt_run::badge::RootKey) -> Self {
        self.root_key = Some(root);
        self
    }

    /// Wire the mutating-tool substrate (builder-style; the daemon calls this,
    /// bare test cores skip it).
    pub fn with_substrate(mut self, substrate: Arc<dyn McpSubstrate>) -> Self {
        self.substrate = Some(substrate);
        self
    }

    /// Wire the CAS the permit gate runs against (builder-style; the daemon
    /// wires its own CAS, bare test cores fall back to an ephemeral one).
    pub fn with_cas(mut self, cas: Arc<Cas>) -> Self {
        self.cas = Some(cas);
        self
    }

    /// Wire a STATIC permit config (SP-wire, DR-011): the resolved
    /// `[gates.permit]` verifier set the PDP dispatches for every run on this
    /// core. The daemon-wired path resolves per-run via the substrate instead;
    /// this builder is the single-workspace / test-double seam. Absent config
    /// (this unset AND no substrate) degrades to escalate/deny (I6).
    pub fn with_permit_config(mut self, config: PermitConfig) -> Self {
        self.permit_config = Some(config);
        self
    }

    /// Wire THREE already-resolved permit layers (SP4c-wire, DR-020 §Decision 3):
    /// the resolved `[gates.permit]` verifier set for each authority level
    /// (admin/dev/session). Mirrors [`Self::with_permit_config`] but stores the
    /// merged set as `PermitConfig::from_specs(compose_layers(admin, dev,
    /// session))` — so a later (dev/session) layer can never un-Fail an earlier
    /// (admin) layer's deny (the aggregate has no allow-override primitive, DR-019
    /// Decision 1). The daemon-wired path resolves the three layers per-run via
    /// the substrate (`permit_config_for`); this builder is the single-workspace /
    /// test-double seam for injecting three pre-resolved layers.
    pub fn with_layered_permit_config(
        mut self,
        admin: Vec<PermitVerifierSpec>,
        dev: Vec<PermitVerifierSpec>,
        session: Vec<PermitVerifierSpec>,
    ) -> Self {
        self.permit_config = Some(PermitConfig::from_specs(
            rezidnt_gate::permit::compose_layers(admin, dev, session),
        ));
        self
    }

    /// The CAS the permit natives run against: the wired one, or a
    /// lazily-opened ephemeral CAS under the OS temp dir. Opening the ephemeral
    /// store can fail (fs error); that surfaces as a decision refusal, never a
    /// panic and never a coerced verdict.
    fn permit_cas(&self) -> Result<Arc<Cas>, (i64, String)> {
        if let Some(cas) = &self.cas {
            return Ok(Arc::clone(cas));
        }
        if let Some(cas) = self.ephemeral_cas.get() {
            return Ok(Arc::clone(cas));
        }
        let root = std::env::temp_dir().join(format!("rezidnt-mcp-cas-{}", std::process::id()));
        let cas = Arc::new(
            Cas::open(&root).map_err(|e| (-32603, format!("open ephemeral permit cas: {e}")))?,
        );
        // Race: another caller may have set it first — reuse the winner.
        let _ = self.ephemeral_cas.set(Arc::clone(&cas));
        Ok(Arc::clone(self.ephemeral_cas.get().unwrap_or(&cas)))
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
            "kill_run" => self.call_kill_run(args).await,
            "resolve_permit" => self.call_resolve_permit(args).await,
            "request_permission" => self.call_request_permission(args).await,
            "gate_explain" => self.call_gate_explain(args).await,
            "tail_events" => self.call_tail_events(args).await,
            "board_view" => self.call_board_view(args).await,
            other => Err((-32602, format!("unknown tool: {other}"))),
        }
    }

    /// §12 door for mutating tools: the badge is checked BEFORE any parsing
    /// or side effect. Returns the loggable badge id on success.
    ///
    /// DR-017 §Decision 4 — DUAL-PATH. The opaque operator badge (DR-005
    /// `BadgeBook`, token-equality) is tried FIRST and left completely
    /// unchanged. If it is not an admitted operator token, the presented value
    /// is parsed as an agent MACAROON and verified against the daemon root key
    /// under a request context (this `verb`, and the `workspace`/`now` args).
    /// A violated caveat, a broken MAC chain, a foreign root key, or a keyless
    /// core all yield `badge.invalid` with no side effect. Success yields the
    /// loggable `badge_id` (`hex(blake3(sig)[..8])`, sig-derived — DR-018 §(a)).
    ///
    /// `verb` is DERIVED from the tool by the caller (`spawn_agent` → "spawn",
    /// `open_project` → "open", `merge` → "merge") — the request's action for
    /// the macaroon's `Verb` caveat.
    fn check_badge(&self, args: &Value, verb: &str) -> Result<String, Value> {
        let Some(presented) = args.get("badge").and_then(Value::as_str) else {
            return Err(tool_refused(
                codes::BADGE_REQUIRED,
                "mutating tools require a badge argument (doc §12)",
            ));
        };

        // Path 1 — the opaque operator badge (DR-005), UNCHANGED. Token-equality
        // against the BadgeBook. An admitted operator token passes the door here.
        {
            let book = self
                .badges
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(id) = book.id_for(presented) {
                return Ok(id.to_string());
            }
        }

        // Path 2 — the agent macaroon (DR-017). Verify against the daemon root
        // key + caveat-eval. A keyless core cannot verify any macaroon.
        use rezidnt_run::badge::{Macaroon, RequestContext, verify};
        let Some(root) = &self.root_key else {
            return Err(tool_refused(
                codes::BADGE_INVALID,
                "badge is not an issued operator token and this core holds no macaroon root key",
            ));
        };
        let macaroon = match Macaroon::from_wire(presented) {
            Ok(m) => m,
            Err(_) => {
                return Err(tool_refused(
                    codes::BADGE_INVALID,
                    "badge is neither an issued operator token nor a parseable agent macaroon",
                ));
            }
        };
        // The request context: workspace + now are caller args; verb is derived.
        // Absent workspace/now leave that axis unconstrained on the request side
        // — a caveat present on the macaroon still refuses if the request cannot
        // satisfy it (a Workspace caveat with no request workspace passes, matching
        // verify's "if the request declares one" semantics; the door callers that
        // require a workspace enforce presence separately).
        // HONESTY NOTE (DR-018 §Consequences 1): this context carries NO
        // `.role(...)`. A `Role` caveat on a child (attenuated) badge is therefore
        // INERT at this §12 door — `verify` never refuses on role here. Role
        // narrowing is enforced by the SP4a permit PDP, NOT the badge door. Do
        // NOT read a `permit.delegated` fact as "the door refuses a wrong-role
        // child"; the door does not. (Wiring role into the door is out of SP4b
        // scope — it would change the enforcement surface.)
        let mut ctx = RequestContext::new().verb(verb);
        if let Some(ws) = args.get("workspace").and_then(Value::as_str) {
            ctx = ctx.workspace(ws);
        }
        // The door supplies the timestamp (I6): `verify` never reads an ambient
        // clock; the ENFORCEMENT decision — what "now" is — is made HERE. A
        // caller-supplied `now` (the pinned tool arg) wins so a decision stays
        // replayable from the exact inputs; ABSENT, the daemon reads real
        // wall-clock time at the door so an expiry caveat is ALWAYS evaluated
        // (a missing `now` must not silently skip expiry — that would let an
        // expired badge through). Reading the clock at the edge and passing it
        // in is I6-clean (an enforcement input, not a replayed verifier).
        //
        // HONESTY NOTE (DR-018 §Consequences 2): when `args["now"]` is ABSENT the
        // daemon reads wall-clock `now_rfc3339()` here. That injected clock read
        // is not recorded on any fact, so a `debrief` replay of THIS call cannot
        // reproduce its exact expiry evaluation from log inputs alone. Acceptable
        // as an enforcement-edge read (a missing `now` must not silently skip
        // expiry), but flagged so replayability is not overclaimed for the
        // absent-`now` path.
        let now = args
            .get("now")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(rezidnt_run::badge::now_rfc3339);
        ctx = ctx.now(now);
        match verify(&macaroon, root, &ctx) {
            Ok(_cap) => Ok(macaroon.badge_id()),
            Err(_) => Err(tool_refused(
                codes::BADGE_INVALID,
                "agent macaroon failed verify (tampered/forged/foreign-root) or a caveat was unsatisfied",
            )),
        }
    }

    async fn call_open_project(&self, args: Value) -> RpcOutcome {
        // Ordering pinned by the board: badge → spec parse → substrate.
        let _badge_id = match self.check_badge(&args, "open") {
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
        let _badge_id = match self.check_badge(&args, "spawn") {
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

    /// DR-032 §Decision 1 — the OPERATOR-ONLY §12 door. Unlike [`check_badge`]'s
    /// DUAL path, this admits ONLY the opaque operator badge (DR-005 `BadgeBook`,
    /// token-equality). A well-formed agent MACAROON — even one that would verify
    /// for a spawn on this same (root-keyed) core — is REFUSED `BADGE_INVALID` on
    /// POLICY: terminating a run is an operator action, not an agent
    /// self-action. No badge → `BADGE_REQUIRED`. Checked BEFORE any side effect
    /// (a refused kill emits no fact, I3). Returns the loggable operator badge id.
    fn check_operator_badge(&self, args: &Value) -> Result<String, Value> {
        let Some(presented) = args.get("badge").and_then(Value::as_str) else {
            return Err(tool_refused(
                codes::BADGE_REQUIRED,
                "kill_run requires an operator badge argument (DR-032 §1, doc §12)",
            ));
        };
        // ONLY the opaque operator badge (DR-005) admits. The macaroon path is
        // deliberately NOT tried — an agent cannot self-kill (DR-032 §1). A
        // presented value that is not an admitted operator token is refused,
        // whether it is a well-formed macaroon or garbage.
        let book = self
            .badges
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match book.id_for(presented) {
            Some(id) => Ok(id.to_string()),
            None => Err(tool_refused(
                codes::BADGE_INVALID,
                "kill_run is operator-only: an agent macaroon cannot terminate a run (DR-032 §1)",
            )),
        }
    }

    /// `kill_run` — DR-032 §Decision 1. The OPERATOR-ONLY mutating tool that
    /// terminates a run. Door discipline (§12): the operator badge is checked
    /// BEFORE any side effect ([`check_operator_badge`] rejects the macaroon
    /// path on policy). On admit, the substrate drives the EXISTING reaper
    /// (`stop_with_escalation`); then the core emits EXACTLY ONE `agent.signaled`
    /// fact through the single writer (I3 — the client never writes the log),
    /// carrying `operator_badge_id` = the VERIFIED operator id (never the token,
    /// §12/I2) and the caller-supplied `reason`. A refused kill emits NO fact.
    async fn call_kill_run(&self, args: Value) -> RpcOutcome {
        // §12 door FIRST — operator-only. A refusal returns before any effect.
        let operator_badge_id = match self.check_operator_badge(&args) {
            Ok(id) => id,
            Err(refusal) => return Ok(refusal),
        };
        // Deserialize THROUGH the advertised shape so the served inputSchema and
        // the accepted args cannot diverge (doc §9 no-drift). The reason rides
        // the fact when present (I6) and is omitted when the caller gave none —
        // never synthesized.
        let parsed: rezidnt_types::mcp::KillRunArgs = match serde_json::from_value(args.clone()) {
            Ok(parsed) => parsed,
            Err(_) => return Ok(tool_refused(codes::ARGS_INVALID, "kill_run requires run")),
        };
        let run = parsed.run;
        let reason = parsed.reason;

        let Some(substrate) = &self.substrate else {
            return Ok(tool_refused(
                codes::SUBSTRATE_UNAVAILABLE,
                "no run substrate is wired to this MCP core",
            ));
        };
        // Drive the reaper (behind the substrate seam). A substrate refusal
        // (e.g. the run is not live) is a machine-readable tool error and emits
        // NO fact — refuse before effect (I3).
        let ack = match substrate.kill_run(run.clone()).await {
            Ok(ack) => ack,
            Err(refusal) => return Ok(tool_refused(&refusal.code, &refusal.message)),
        };

        // Emit EXACTLY ONE attributed `agent.signaled` fact through the single
        // writer (I3). `operator_badge_id` is the loggable verified id, NEVER the
        // token (§12/I2). `run`/`signal`/`escalation` follow the reaper's stop.
        let mut payload = json!({
            "run": run,
            "signal": ack.signal,
            "operator_badge_id": operator_badge_id,
        });
        if let Some(obj) = payload.as_object_mut() {
            if let Some(escalation) = &ack.escalation {
                obj.insert("escalation".to_string(), json!(escalation));
            }
            if let Some(reason) = &reason {
                obj.insert("reason".to_string(), json!(reason));
            }
        }
        self.publish_fact("agent.signaled", payload).await?;

        let mut result = json!({
            "run": run,
            "signal": ack.signal,
        });
        if let (Some(escalation), Some(obj)) = (&ack.escalation, result.as_object_mut()) {
            obj.insert("escalation".to_string(), json!(escalation));
        }
        Ok(tool_ok(result))
    }

    /// `resolve_permit` — DR-033 §Decision 1 (slice 2). The OPERATOR-ONLY
    /// mutating tool by which a human resolves a previously-escalated permit.
    /// Door discipline (§12): the operator badge is checked BEFORE any side effect
    /// ([`check_operator_badge`] rejects the macaroon path on policy — resolving
    /// is an operator action, not agent self-action, mirroring `kill_run`). A
    /// refused resolve emits NO fact (I3). Unlike `kill_run`, a resolution is a
    /// PURE fact emit — NO substrate seam: on admit the core emits EXACTLY ONE
    /// `permit.resolved` fact through the single writer, carrying
    /// `operator_badge_id` = the VERIFIED operator id (never the token, §12/I2),
    /// the human `decision` verbatim (never coerced — the PDP coerces on the next
    /// ask, I6), the escalated `request_id`, and the DAEMON-DERIVED
    /// `action`/`target` descriptor (the next-ask match key). The operator
    /// supplies NEITHER: the daemon folds the run and looks the escalation up by
    /// `request_id`, stamping the REAL `(action, target)` the operator never types
    /// (DR-033 §Design, /debrief FAIL close — a hardcoded operator `target` broke
    /// the PDP action-identity match). An UNKNOWN `request_id` (no folded
    /// `permit.requested` to derive from) is REFUSED, NO fact emitted (I3/I6).
    /// `reason` rides when supplied.
    async fn call_resolve_permit(&self, args: Value) -> RpcOutcome {
        // §12 door FIRST — operator-only. A refusal returns before any effect.
        let operator_badge_id = match self.check_operator_badge(&args) {
            Ok(id) => id,
            Err(refusal) => return Ok(refusal),
        };
        // Deserialize THROUGH the advertised (TRIMMED) shape so the served
        // inputSchema and the accepted args cannot diverge (doc §9 no-drift). The
        // operator supplies NO action/target — the daemon derives them. The reason
        // rides the fact when present (I6), omitted when absent — never synthesized.
        let parsed: rezidnt_types::mcp::ResolvePermitArgs =
            match serde_json::from_value(args.clone()) {
                Ok(parsed) => parsed,
                Err(_) => {
                    return Ok(tool_refused(
                        codes::ARGS_INVALID,
                        "resolve_permit requires badge, run, request_id, decision",
                    ));
                }
            };

        // DR-035 §Decision 3 — THE COUPLING GUARD (the security-critical structural
        // guarantee). A broad (`scope="run_tool"`) resolution MUST carry a bounded
        // `ttl_ms`: broad OR permanent, never both. Validated AFTER the badge door and
        // BEFORE any derive/emit, so a broad-and-permanent `permit.resolved` can never
        // reach the log (no partial state, I3) — the dangerous quadrant is structurally
        // unreachable, not merely discouraged. Absent scope (DR-033 request-scoped) is
        // permanent-by-default, unchanged: the coupling binds ONLY the broad case.
        if parsed.scope.as_deref() == Some("run_tool") && parsed.ttl_ms.is_none() {
            return Ok(tool_refused(
                codes::SCOPE_REQUIRES_TTL,
                "resolve_permit: a broad scope (scope=\"run_tool\") requires a ttl_ms — \
                 broad OR permanent, never both (DR-035 §Decision 3); the broad-and-permanent \
                 quadrant is structurally forbidden",
            ));
        }

        // DERIVE `(action, target)` from the log by `request_id` (DR-033 §Design).
        // Fold the run and look the escalation up in the permit ledger. If ABSENT
        // (no `permit.requested` folded for this request_id) the daemon cannot
        // derive a match key and REFUSES with the honest unknown-escalation code
        // (NOT `ARGS_INVALID` — the args parsed fine), emitting NO fact (I3/I6).
        let folded = self.fold_run_state(&parsed.run).await?;
        let (action, target) = match folded.permit_ledger.get(&parsed.request_id) {
            Some(entry) if !entry.action.is_empty() => (entry.action.clone(), entry.target.clone()),
            _ => {
                return Ok(tool_refused(
                    codes::RUN_UNKNOWN,
                    "resolve_permit: no escalation with that request_id on the run's log — \
                     the daemon cannot derive an action/target and will not fabricate one",
                ));
            }
        };

        // Emit EXACTLY ONE `permit.resolved` fact through the single writer (I3).
        // `operator_badge_id` is the loggable verified id, NEVER the token
        // (§12/I2). `action`/`target` are DAEMON-DERIVED (not operator inputs).
        // `decision` is the human input verb, folded verbatim (I6).
        let mut payload = json!({
            "run": parsed.run,
            "request_id": parsed.request_id,
            "action": action,
            "target": target.unwrap_or_else(|| json!({})),
            "decision": parsed.decision,
            "operator_badge_id": operator_badge_id,
        });
        if let (Some(reason), Some(obj)) = (&parsed.reason, payload.as_object_mut()) {
            obj.insert("reason".to_string(), json!(reason));
        }
        // DR-035 §Decision 1: the optional TTL rides the fact VERBATIM when the
        // operator time-boxes the resolution; absent = permanent (DR-033 §Decision
        // 2). The reducer folds it onto `PermitResolution.ttl_ms` and anchors the
        // deadline at THIS fact's envelope ULID (`resolved_at_ms`), so expiry is a
        // pure fold with no created_at on the fact (I3, DR-035 §Decision 1).
        if let (Some(ttl_ms), Some(obj)) = (parsed.ttl_ms, payload.as_object_mut()) {
            obj.insert("ttl_ms".to_string(), json!(ttl_ms));
        }
        // DR-035 §Decision 2: the optional grant-all scope rides the fact VERBATIM so
        // the reducer folds the broadening onto `PermitResolution.scope` and the PDP
        // matches any action on this `(run, tool)`. Absent = OMITTED (never null) =
        // DR-033 exact request-scoped match. Mirrors the `ttl_ms` insert above; the
        // coupling guard already guaranteed a present scope carries a ttl_ms.
        if let (Some(scope), Some(obj)) = (&parsed.scope, payload.as_object_mut()) {
            obj.insert("scope".to_string(), json!(scope));
        }
        self.publish_fact("permit.resolved", payload).await?;

        Ok(tool_ok(json!({
            "run": parsed.run,
            "request_id": parsed.request_id,
            "decision": parsed.decision,
        })))
    }

    /// `request_permission` — the daemon IS the PDP (design §5, DR-008/DR-009).
    /// Ordering (§12 door discipline): badge FIRST (the caller of an
    /// authorization decision must be identified), then the request fact, then
    /// the decision. The decision is three-valued (`allow | deny | ask`) and
    /// NEVER coerced — `inconclusive` surfaces as `ask` (route to a human, I6).
    /// Both the `permit.requested` fact and one decision fact land on the log
    /// (I3: the permission stream is first-class in `tail`).
    async fn call_request_permission(&self, args: Value) -> RpcOutcome {
        // §12: the MCP door checks the badge BEFORE any decision or side effect.
        // The socket transport skips this door (0600 UDS is the identity, DR-013
        // decision 3) by calling `decide_permit` directly; the shared PDP flow
        // never re-checks the badge, so the door lives here at the MCP edge.
        let _badge_id = match self.check_badge(&args, "permit") {
            Ok(id) => id,
            Err(refusal) => return Ok(refusal),
        };
        let field = |name: &str| -> Result<String, Value> {
            args.get(name)
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| {
                    tool_refused(
                        codes::ARGS_INVALID,
                        format!("request_permission requires {name}"),
                    )
                })
        };
        let (run, action, tool) = match (field("run"), field("action"), field("tool")) {
            (Ok(r), Ok(a), Ok(t)) => (r, a, t),
            (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => return Ok(e),
        };
        // Build the transport-neutral request and run the ONE PDP path. The MCP
        // caller echoes a supplied `request_id` when present (parity with the
        // socket's PEP token), else `decide_permit` mints one.
        let req = PermitRequest {
            run,
            request_id: args
                .get("request_id")
                .and_then(Value::as_str)
                .map(String::from),
            action,
            tool,
            badge: args.get("badge").and_then(Value::as_str).map(String::from),
            context_ref: args
                .get("context_ref")
                .and_then(Value::as_str)
                .map(String::from),
            paths: args.get("paths").cloned(),
        };
        let outcome = self
            .decide_permit(req)
            .await
            .map_err(|e| (-32603, e.to_string()))?;

        let mut result = json!({ "decision": outcome.decision.as_word() });
        if let (Some(r), Some(obj)) = (outcome.reason, result.as_object_mut()) {
            obj.insert("reason".to_string(), Value::String(r));
        }
        Ok(tool_ok(result))
    }

    /// The transport-neutral PDP entrypoint (SP2, DR-013 decision 1). Both the
    /// MCP JSON-RPC adapter and the socket handler call this; it performs the
    /// ENTIRE decision flow and emits the two on-log facts (`permit.requested` +
    /// one decision fact) itself, so no caller re-emits (I3 — MCP and socket
    /// facts are byte-identical, no fork).
    ///
    /// Badge: this method NEVER refuses on a missing/unknown badge — the §12
    /// door is the MCP adapter's (DR-013 decision 3: the socket's 0600 UDS is
    /// its identity). A present `badge` only decorates the `permit.requested`
    /// fact's `badge_id` (best-effort resolution).
    pub async fn decide_permit(&self, req: PermitRequest) -> Result<PermitOutcome, PdpError> {
        let PermitRequest {
            run,
            request_id,
            action,
            tool,
            badge,
            context_ref,
            paths,
        } = req;

        // The caller's token when supplied (the PEP's, over the socket); else
        // mint — so the PEP's ask and the on-log decision fact share one id.
        let request_id = request_id.unwrap_or_else(|| ulid::Ulid::new().to_string());

        // Best-effort badge id for the requested fact (never a refusal here).
        let badge_id = badge.as_deref().and_then(|token| {
            self.badges
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .id_for(token)
                .map(str::to_string)
        });

        // The permit.requested fact (I3). Bulk context rides as a ref string,
        // never inline bytes (I2); the descriptor is small scalars.
        let mut requested = json!({
            "run": run,
            "request_id": request_id,
            "action": action,
            "target": { "tool": tool },
        });
        if let Some(id) = &badge_id {
            requested["badge_id"] = json!(id);
        }
        if let Some(ref cref) = context_ref {
            requested["context_ref"] = json!(cref);
        }
        // DR-035: capture THIS request's envelope-ULID timestamp — the anchor the
        // TTL expiry filter compares against a resolution's deadline. It is the
        // same ULID that lands on the log, so the comparison is a pure fold of
        // on-log timestamps (I3), and it flows onto the returned `PermitOutcome`
        // so DR-034's live-unblock re-checks against the ORIGINAL held ask time.
        let requested_ms = self
            .publish_fact("permit.requested", requested)
            .await
            .map_err(|(_, m)| PdpError::Internal(m))?
            .timestamp_ms();

        // SP-wire (DR-011): dispatch the CONFIGURED `[gates.permit]` verifier
        // set — not a hardcoded single verifier — and aggregate via
        // `permit::aggregate`. Config resolution is a substrate capability
        // (DR-011 §1); the core folds the run's state itself (DR-011 §2).
        let cas = self.permit_cas().map_err(|(_, m)| PdpError::Internal(m))?;

        // 1. Resolve the applied verifier set: a static config (test / single
        //    workspace) wins; else the substrate resolves it per-run; else NO
        //    config — the aggregator escalates an empty set (I6, DR-011 §3).
        let config = match &self.permit_config {
            Some(config) => config.clone(),
            None => match &self.substrate {
                Some(substrate) => substrate
                    .permit_config_for(run.clone())
                    .await
                    .unwrap_or_default(),
                None => PermitConfig::default(),
            },
        };

        // 2. Fold this run's per-run state from the fabric the core holds
        //    (DR-011 §2; the same discipline `resources_read` uses). The intent
        //    allowlist and the spend accumulator are injected as CONTENT-PINNED
        //    params — NEVER live state, NEVER re-derived (determinism BINDING).
        let folded = self
            .fold_run_state(&run)
            .await
            .map_err(|(_, m)| PdpError::Internal(m))?;

        // 2b. DR-033 §Decision 1 (slice 2) — the PDP LEDGER-CHECK (the crux):
        //     BEFORE verifier dispatch, consult the folded log. If a human
        //     `permit.resolved` answers this exact action `(run, tool,
        //     action/target)`, APPLY the human decision — emit `permit.granted`
        //     (allow) / `permit.denied` (deny) carrying `resolved_from` = the
        //     resolution's `request_id` — instead of re-escalating. The match keys
        //     on ACTION IDENTITY, not `request_id` (re-minted per ask, §Context).
        //     Request-scoped (§Decision 3): a resolution for one action never
        //     grants another — `resolution_for` returns `None` and the normal
        //     verifier path runs. The applied outcome is a REAL logged decision
        //     fact (I3 — interrogable, `resolved_from` chains to WHO/WHY, I6), NOT
        //     a silent coercion of the escalation (a RECORDED human override).
        //     The emit is factored into `apply_folded_resolution` so DR-034's
        //     live-unblock re-decide (`recheck_resolution`) applies the SAME
        //     ledger-check without re-running the `permit.requested`/verifier path.
        if let Some(outcome) = self
            .apply_folded_resolution(&folded, &run, &action, &tool, &request_id, requested_ms)
            .await?
        {
            return Ok(outcome);
        }

        // 3. The request axis + folded state as pinned params. Each verifier's
        //    own config (`allow`, caps, knobs) rides its `PermitVerifierSpec`
        //    and the aggregator merges it over this base.
        let mut base_params = json!({ "tool": tool });
        if let Some(obj) = base_params.as_object_mut() {
            if let Some(paths) = &paths {
                obj.insert("paths".to_string(), paths.clone());
            }
            // DR-012 option B: inject the `allowed_tools` key whenever intent is
            // DECLARED (`Some`, even `Some([])`), so a declared-empty lockdown
            // reaches `IntentLock` as a present-but-empty key (→ off-task path);
            // OMIT it only when intent is genuinely ABSENT (`None` → cannot-run).
            if let Some(intent) = &folded.intent {
                obj.insert("allowed_tools".to_string(), json!(intent.allowed_tools));
            }
            // DR-016 §Decision 2 (SP4a): inject the folded RBAC role as a permit
            // input axis — PRESENT iff a role was declared (`Some`, even
            // `Some("")`), OMITTED when genuinely ABSENT (`None`). Mirrors the
            // DR-012 declared-vs-absent discipline the `allowed_tools` injection
            // follows: a role-less run carries no `role` key, so a role-keyed
            // policy sees no role axis and escalates (inconclusive → ask, I6),
            // never a synthesized grant. The role is a content-pinned param, never
            // live state (determinism BINDING).
            if let Some(role) = &folded.role {
                obj.insert("role".to_string(), json!(role));
            }
            obj.insert(
                "cumulative_spend_usd".to_string(),
                json!(folded.permit_accumulators.cumulative_spend_usd),
            );
            // DR-021 (C1): inject the per-run window action count so SpendCap's
            // rate-limit leg can fire. `granted` is the deterministic, replayable
            // count of granted actions this run — the folded window count the PDP
            // owns (like `cumulative_spend_usd`). The caps + `rate_limit` ride the
            // verifier's OWN spec params (merged over this base), so only the two
            // folded-state axes are injected here. Content-pinned, never live state
            // (determinism BINDING).
            obj.insert(
                "window_action_count".to_string(),
                json!(folded.permit_accumulators.granted),
            );
            // DR-024 (C6): inject the folded running risk score so RiskCap can
            // project (`cumulative_risk_score + this-action risk`) against its
            // caps. `risk_score` is the deterministic, replayable accumulator the
            // PDP owns (folded from prior GRANTED permit facts, I3) — mirror the
            // `cumulative_spend_usd` injection above. RiskCap's caps + scorer table
            // ride its OWN spec params (merged over this base), NOT injected here;
            // this-action risk is COMPUTED by the verifier from the axis (DR-024
            // Q4). Content-pinned, never live state (determinism BINDING).
            obj.insert(
                "cumulative_risk_score".to_string(),
                json!(folded.permit_accumulators.risk_score),
            );
        }

        let input = rezidnt_gate::VerifierInput {
            gate: rezidnt_gate::permit::LIFECYCLE_POINT.to_string(),
            workspace: None,
            refs: BTreeMap::new(),
            params: base_params,
            timeout_ms: rezidnt_gate::DEFAULT_TIMEOUT_MS,
        };

        // 4. Aggregate the configured set IN ORDER (first Fail short-circuits →
        //    Deny; else any Inconclusive → Escalate; else Grant). The aggregate
        //    verdict maps via `decision_for` (I6: inconclusive → ask, never
        //    coerced). SP3 lifts aggregation to the ASYNC layer (DR-015
        //    §Decision 2, option A): natives run sync/in-process inside the
        //    aggregator (CPU + CAS by design), but an exec permit entry runs as
        //    an `await`ed subprocess through `ExecVerifier` — visible to the
        //    scheduler and the hot-path timeout, never `block_on`'d inside
        //    `spawn_blocking`.
        // Time the AGGREGATE span ONLY (§10.2 decision latency): the monotonic
        // clock wraps just the `aggregate_async` await — the deciding policy's
        // latency — NOT the surrounding CAS pin or `publish_fact`, which would
        // conflate policy cost with I/O. The timer wraps the call
        // unconditionally, so even the empty-set escalate carries a `cost_ms`.
        let agg_start = std::time::Instant::now();
        let outcome = rezidnt_gate::permit::aggregate_async(config.verifiers(), &input, &cas)
            .await
            .map_err(|e| PdpError::Internal(format!("permit aggregate: {e}")))?;
        let cost_ms = agg_start.elapsed().as_millis() as u64;

        let decision = Decision::from(outcome.decision);
        let reason = outcome.evidence.first().map(|e| e.msg.clone());

        // 5. Emit ONE aggregate decision fact carrying the DECIDING verifier's
        //    policy_ref (its merged params, pinned to CAS — I2 ref not inline)
        //    and evidence_ref (its evidence blob). `gate_explain` then surfaces
        //    the REAL deciding verifier, not a hardcoded `tool-allowlist`.
        let policy_bytes = json!({
            "gate": "permit",
            "verifier": outcome.deciding_verifier,
            // The DECIDING LAYER (SP4c-wire, DR-020 §Decision 4): pins the
            // authority that decided (`admin`/`dev`/`session`) alongside the
            // (possibly ambiguous) verifier NAME, so `gate_explain` answers "why
            // blocked" with the deciding layer — DR-019 criterion 2 made LIVE on
            // the wire. `None` only for the empty-set escalate (no verifier
            // decided, so no layer to name).
            "layer": outcome.deciding_layer.map(|l| l.as_str()),
            "params": outcome.deciding_params,
        })
        .to_string();
        let policy_ref = {
            let cas = Arc::clone(&cas);
            tokio::task::spawn_blocking(move || {
                cas.put(policy_bytes.as_bytes(), "application/json")
            })
            .await
            .map_err(|e| PdpError::Internal(format!("policy pin task panicked: {e}")))?
            .map_err(|e| PdpError::Internal(format!("pin policy: {e}")))?
        };
        // The deciding verifier's evidence blob (if any) carries as the
        // decision's evidence_ref (I2: ref, never inline bytes). The aggregator
        // already recovered the blob's HONEST metadata (true `bytes`, from a
        // store `stat`) into `deciding_evidence_ref` — carry it verbatim rather
        // than reconstruct a `CasRef` with a fabricated `bytes: 0` from the bare
        // `cas:blake3:` string. A durable decision fact must not misstate its own
        // evidence blob's size (I3 fact fidelity).
        let evidence_ref = outcome.deciding_evidence_ref.clone();

        // DR-024 C6 (Q5 producer seam): stamp `risk_delta` onto a GRANTED fact
        // ONLY (the granted-only fold source, Q3). The delta is the SHARED
        // `risk_score` fn on the SAME content-pinned axis + table RiskCap used for
        // its verdict, so the stamped scalar and the verdict CANNOT diverge (Q5
        // option iii — no `VerifierOutput`/`PermitOutcome` risk field). A denied or
        // escalated action never ran, so it carries no delta (nothing to fold).
        // If no `risk-cap` verifier is configured (no table), no delta is stamped.
        let risk_delta = if outcome.decision == rezidnt_gate::permit::PermitDecision::Grant {
            config
                .verifiers()
                .iter()
                .find(|spec| spec.name == "risk-cap")
                .map(|spec| {
                    // Reconstruct RiskCap's exact view: the request axis with the
                    // verifier's own spec params (the `risk_table`) merged over it.
                    let merged = rezidnt_gate::permit::merge_params(&input.params, &spec.params);
                    let table = merged.get("risk_table").cloned().unwrap_or(Value::Null);
                    rezidnt_gate::risk_score(&merged, &table)
                })
        } else {
            None
        };

        let (subject, payload) = rezidnt_gate::permit::decided_fact(
            outcome.verdict,
            &run,
            &request_id,
            &policy_ref,
            evidence_ref.as_ref(),
            reason.as_deref(),
            rezidnt_gate::permit::DecisionDeltas {
                cost_ms: Some(cost_ms),
                risk_delta,
                ..Default::default()
            },
        );
        self.publish_fact(subject, payload)
            .await
            .map_err(|(_, m)| PdpError::Internal(m))?;

        Ok(PermitOutcome {
            request_id,
            decision,
            reason,
            // DR-035: the first-pass `permit.requested` anchor. On an `ask`
            // outcome the socket's live-unblock threads this back into
            // `recheck_resolution` so a landing resolution's TTL is measured
            // against the ORIGINAL held ask time, not a fresh re-decide clock.
            requested_ms,
        })
    }

    /// DR-034 live-unblock — re-run the DR-033 ledger-check ALONE for a
    /// currently-held escalated request, WITHOUT re-emitting the
    /// `permit.requested`/`permit.escalated` pair the first `decide_permit`
    /// already logged (I3: the wake produces ONLY the applied grant/deny, never a
    /// duplicate requested/escalated pair — a replay reconstructs identically
    /// whether or not the request was held).
    ///
    /// The daemon socket handler calls this each time a `permit.resolved` for the
    /// held run lands within the unblock deadline. It folds the run fresh, and if
    /// a human resolution now answers this exact action `(run, tool)`, emits the
    /// applied `permit.granted`/`permit.denied` (carrying the ORIGINAL held
    /// `request_id` and `resolved_from`) and returns `Some(outcome)` — the wake.
    /// It returns `None` when no resolution yet matches (still escalated → the
    /// caller keeps waiting until the deadline, then fails closed to `ask`).
    /// A resolution for a DIFFERENT action never matches here, so a foreign
    /// resolve never wakes this request (DR-034 §Decision 3 — the ledger-check's
    /// action-identity match is the only gate; no side channel).
    pub async fn recheck_resolution(
        &self,
        run: &str,
        action: &str,
        tool: &str,
        request_id: &str,
        // DR-035: the HELD request's ORIGINAL `permit.requested` envelope-ULID
        // timestamp (the request happened once, on the first pass). The TTL filter
        // measures a landing resolution's deadline against THIS anchor, not a fresh
        // wake clock. Natural interplay: a resolution that lands DURING a live hold
        // is necessarily NEWER than the held request, so its deadline
        // (`resolved_at_ms + ttl >= resolved_at_ms > requested_ms`) is always past
        // the anchor and it always applies — TTL only bites the honored-on-a-
        // LATER-next-ask case, which flows through `decide_permit` with its own
        // fresh anchor. This falls out of the shared filter; no special-casing.
        requested_ms: u64,
    ) -> Result<Option<PermitOutcome>, PdpError> {
        let folded = self
            .fold_run_state(run)
            .await
            .map_err(|(_, m)| PdpError::Internal(m))?;
        self.apply_folded_resolution(&folded, run, action, tool, request_id, requested_ms)
            .await
    }

    /// The DR-033 ledger-check emit, shared by `decide_permit` (first pass) and
    /// `recheck_resolution` (DR-034 wake). If the folded run carries a human
    /// `permit.resolved` matching `(action, tool)`, APPLY it: emit the applied
    /// `permit.granted`/`permit.denied` fact carrying `request_id` (the caller's,
    /// echoed) and `resolved_from` (the resolution's id, so "granted via human
    /// resolution X" is a structured log-derivable read, I6), and return the
    /// applied [`PermitOutcome`]. Return `None` when no resolution answers this
    /// action — the caller then runs (or keeps escalating) the verifier path.
    ///
    /// An unrecognized human verb (neither `allow` nor `deny`) is NEVER coerced to
    /// a grant (I6): it does not apply, so `None` falls through. No `policy_ref`: a
    /// human resolution is neither a policy nor a CAS blob (the ontology's
    /// `permit.granted.resolved_from` — deliberately NOT overloaded onto
    /// `policy_ref`). The applied `(subject, decision, resolved_from)` is resolved
    /// first so the borrow of `folded` ends before the async emit.
    async fn apply_folded_resolution(
        &self,
        folded: &rezidnt_state::AgentRunState,
        run: &str,
        action: &str,
        tool: &str,
        request_id: &str,
        // DR-035: the incoming request's envelope-ULID timestamp — the anchor the
        // TTL expiry filter compares against each resolution's deadline
        // (`resolved_at_ms + ttl_ms`). An EXPIRED resolution is passed over here so
        // `resolution_for` returns `None` and the request re-escalates (I6: the
        // re-escalation is a recorded fact, never a silent grant).
        incoming_ms: u64,
    ) -> Result<Option<PermitOutcome>, PdpError> {
        let applied = folded
            .resolution_for(action, tool, incoming_ms)
            .and_then(|resolution| {
                let (subject, decision) = match resolution.decision.as_str() {
                    "allow" => ("permit.granted", Decision::Allow),
                    "deny" => ("permit.denied", Decision::Deny),
                    _ => return None,
                };
                // DR-035 §Decision 2 / §Invariants I6: carry the matched broad
                // predicate onto the applied fact so a broad grant is a recorded,
                // attributable, EXPLAINABLE outcome (never a silent widening). `None`
                // for a request-scoped grant — the negative control keeps a narrow
                // grant clean of a phantom predicate.
                Some((
                    subject,
                    decision,
                    resolution.request_id.clone(),
                    resolution.scope.clone(),
                ))
            });
        let Some((subject, decision, resolved_from, scope)) = applied else {
            return Ok(None);
        };
        let mut payload = json!({
            "run": run,
            "request_id": request_id,
            "resolved_from": resolved_from,
        });
        // Thread the matched broad predicate onto the applied fact only when present
        // (I6 interrogability); absent for a request-scoped grant so it stays distinct.
        if let (Some(scope), Some(obj)) = (&scope, payload.as_object_mut()) {
            obj.insert("scope".to_string(), json!(scope));
        }
        self.publish_fact(subject, payload)
            .await
            .map_err(|(_, m)| PdpError::Internal(m))?;
        Ok(Some(PermitOutcome {
            request_id: request_id.to_string(),
            decision,
            reason: None,
            // The APPLIED path (grant/deny) is terminal — it never enters the
            // live-unblock hold, so this anchor is never re-read. Echo the caller's
            // incoming timestamp for fidelity rather than a fresh `0`.
            requested_ms: incoming_ms,
        }))
    }

    /// DR-035 §Invariants I6 — build the `expired_resolution` note for a
    /// re-escalation, or `None` when no genuinely-expired resolution explains it.
    /// Derived PURELY from the already-replayed `events` (no second replay, no
    /// synthesis): the escalation's `request_id` finds its `permit.requested`
    /// (the action/tool match key + the anchor timestamp — the ask's OWN envelope
    /// ULID, DR-035 §Decision 1); the folded run's
    /// [`rezidnt_state::AgentRunState::expired_resolution_for`] reports the newest
    /// matching resolution that had EXPIRED by that anchor. The note names WHICH
    /// resolution (`resolved_from`), its operator + reason (chaining WHO/WHY, as an
    /// applied resolution does), and the deadline it lapsed at — so a reader sees
    /// "not applied: resolution X expired at ULID T → re-escalated", never a silent
    /// vanish (I6). Absent when the escalation is an ordinary first-time ask (no
    /// matching resolution) or the matching resolution is permanent/live.
    async fn expired_resolution_note(
        &self,
        events: &[rezidnt_types::Event],
        run: &str,
        escalated: &rezidnt_types::Event,
    ) -> Option<Value> {
        let request_id = escalated.payload()["request_id"].as_str()?;
        // The escalation's own `permit.requested` carries the action/target AND
        // the anchor (its envelope-ULID timestamp — the ask that expired).
        let requested = events.iter().find(|e| {
            e.subject.as_str() == "permit.requested"
                && e.payload()["request_id"].as_str() == Some(request_id)
                && e.payload()["run"] == json!(run)
        })?;
        let action = requested.payload()["action"].as_str()?;
        let tool = requested.payload()["target"]["tool"].as_str()?;
        let incoming_ms = requested.id.timestamp_ms();

        // Fold the run and ask the pure state whether a matching resolution had
        // lapsed by the ask time (the same anchor the filter uses).
        let folded = self.fold_run_state(run).await.ok()?;
        let expired = folded.expired_resolution_for(action, tool, incoming_ms)?;
        let deadline_ms = expired
            .resolved_at_ms
            .saturating_add(expired.ttl_ms.unwrap_or(0));
        let mut note = json!({
            "status": "not applied: resolution expired → re-escalated",
            "resolved_from": expired.request_id,
            "resolved_at_ms": expired.resolved_at_ms,
            "ttl_ms": expired.ttl_ms,
            "deadline_ms": deadline_ms,
            "request_ms": incoming_ms,
        });
        if let Some(obj) = note.as_object_mut() {
            if let Some(badge) = &expired.operator_badge_id {
                obj.insert("operator_badge_id".to_string(), json!(badge));
            }
            if let Some(reason) = &expired.reason {
                obj.insert("reason".to_string(), json!(reason));
            }
        }
        Some(note)
    }

    /// Fold this run's per-run state (intent allowlist + permit accumulators)
    /// from the fabric the core holds (DR-011 §2, I3) — off the async threads
    /// (SQLite replay is blocking). Returns the default state for a run the log
    /// does not know (never a synthesized permit; the aggregator decides).
    async fn fold_run_state(
        &self,
        run: &str,
    ) -> Result<rezidnt_state::AgentRunState, (i64, String)> {
        let events = self.replay(None).await?;
        let run = run.to_string();
        // Fold on a blocking thread: the fold is pure but scans the whole log.
        tokio::task::spawn_blocking(move || {
            let graph = rezidnt_state::fold(events.iter());
            graph.agent_runs.get(&run).cloned().unwrap_or_default()
        })
        .await
        .map_err(|e| (-32603, format!("permit state fold task panicked: {e}")))
    }

    /// Append one fact through the fabric off the async threads (SQLite is
    /// blocking; rust-conventions: no blocking in async). Returns the minted
    /// event's envelope ULID so a caller that needs the fact's on-log timestamp
    /// (DR-035: the `permit.requested` anchor for the TTL expiry filter) reads it
    /// from `id.timestamp_ms()` without a second `now()` — the same ULID that
    /// lands on the log (I3, no divergent clock).
    async fn publish_fact(
        &self,
        subject: &str,
        payload: Value,
    ) -> Result<ulid::Ulid, (i64, String)> {
        let event = rezidnt_types::Event::new(
            rezidnt_types::SourceId::new("rezidnt-mcp"),
            None,
            rezidnt_types::Subject::new(subject),
            ulid::Ulid::new(),
            None,
            1,
            payload,
        )
        .map_err(|e| (-32603, format!("construct {subject}: {e}")))?;
        let id = event.id;
        let fabric = Arc::clone(&self.fabric);
        tokio::task::spawn_blocking(move || fabric.publish(event))
            .await
            .map_err(|e| (-32603, format!("publish {subject} task panicked: {e}")))?
            .map_err(|e| (-32603, format!("append {subject}: {e}")))?;
        Ok(id)
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
        // The LATEST verdict-bearing fact for this run wins (append order). The
        // interrogation resolves BOTH gate verdicts (`gate.passed|failed|
        // inconclusive`) AND permit decisions (`permit.granted|denied|
        // escalated`) — a blocked agent reads WHY on either axis (design §5, I6).
        let verdict_fact = events.iter().rev().find(|e| {
            matches!(
                e.subject.as_str(),
                "gate.passed"
                    | "gate.failed"
                    | "gate.inconclusive"
                    | "permit.granted"
                    | "permit.denied"
                    | "permit.escalated"
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
        // DR-014 §Decision 5: derive the run's enforcement mode from its
        // `agent.spawned.pep?` on the log (I3 — from the log, never a side
        // store), so a reader distinguishes a mid-run-PEP-enforced run from an
        // edge-gated-only one (I4 degradation honesty). Present `pep:
        // "enforced"` ⇒ mid-run-enforced; ABSENT ⇒ edge-gated-only — never
        // synthesized to enforced (the honest absence the ontology mandates).
        let enforcement = if events.iter().any(|e| {
            e.subject.as_str() == "agent.spawned"
                && e.payload()["run"] == json!(run)
                && e.payload()["pep"] == json!("enforced")
        }) {
            "mid-run-enforced"
        } else {
            "edge-gated-only"
        };
        let verdict = match fact.subject.as_str() {
            "gate.passed" => "pass",
            "gate.failed" => "fail",
            "gate.inconclusive" => "inconclusive",
            // Permit decisions map to their wire vocabulary; escalate → ask,
            // NEVER coerced to allow (I6, DR-008 §4).
            "permit.granted" => "allow",
            "permit.denied" => "deny",
            _ => "ask",
        };
        let payload = fact.payload();
        let is_permit = fact.subject.as_str().starts_with("permit.");
        let explain = if is_permit {
            // A permit decision surfaces its deciding policy + evidence + reason
            // so the blocked agent reads WHY (I6; ontology permit.denied). The
            // refs are CAS refs, resolved not inline (I2).
            let mut e = json!({
                "run": run,
                "gate": "permit",
                "verdict": verdict,
                "request_id": payload["request_id"],
                "policy_ref": payload["policy_ref"],
            });
            if let Some(obj) = e.as_object_mut() {
                if let Some(er) = payload.get("evidence_ref") {
                    obj.insert("evidence_ref".to_string(), er.clone());
                }
                if let Some(reason) = payload.get("reason") {
                    obj.insert("reason".to_string(), reason.clone());
                }
                // DR-033 §Decision 1 (slice 2): surface `resolved_from` when the
                // applied decision fact carries it (a human-resolved grant/denial),
                // so a reader tells a human override from a policy grant and can
                // chain to the resolution's operator_badge_id/reason (I6). Present
                // ONLY on a resolution-applied fact; ABSENT on an ordinary
                // verifier-decided grant — never synthesized (a phantom
                // resolved_from would misreport the deciding authority).
                if let Some(resolved_from) = payload.get("resolved_from") {
                    obj.insert("resolved_from".to_string(), resolved_from.clone());
                }
                // DR-035 §Decision 2 / §Invariants I6 — surface the matched broad
                // predicate when the applied fact carries it, so `gate why`/`debrief`
                // render "granted by broad resolution X matching any action on
                // (run, tool)". Present ONLY on a broad grant; ABSENT on a
                // request-scoped grant (the negative control) — never synthesized, so a
                // narrow grant is never misreported as broad (the inverse of a silent
                // widening).
                if let Some(scope) = payload.get("scope") {
                    obj.insert("scope".to_string(), scope.clone());
                }
                // DR-035 §Invariants I6 — expiry is EXPLAINABLE, never a silent
                // vanish. On a re-escalation (`permit.escalated`), if the log holds
                // a resolution that WOULD have matched this action but had EXPIRED
                // by the escalation's own envelope time, surface it: "not applied:
                // resolution X expired at ULID T → re-escalated". Derived purely
                // from the log (the escalation's `permit.requested` gives the
                // action/tool + anchor; the folded run gives the expired
                // resolution) — no synthesis, present ONLY when a genuinely-expired
                // match exists, so an ordinary first-time escalate carries nothing.
                if fact.subject.as_str() == "permit.escalated"
                    && let Some(note) = self.expired_resolution_note(&events, run, fact).await
                {
                    obj.insert("expired_resolution".to_string(), note);
                }
            }
            e
        } else {
            let mut e = json!({
                "run": run,
                "gate": payload["gate"],
                "verdict": verdict,
                "verifier": payload["verifier"],
                "evidence": payload["evidence"],
                "inputs": payload["inputs"],
            });
            if let (Some(reason), Some(obj)) = (payload.get("reason"), e.as_object_mut()) {
                obj.insert("reason".to_string(), reason.clone());
            }
            e
        };
        // Surface the enforcement mode on the explain payload (DR-014 §Decision
        // 5) — the machine-readable distinction a `debrief`/`gate_explain`
        // reader needs so it never presents an edge-gated run as if it had live
        // interception.
        let mut explain = explain;
        if let Some(obj) = explain.as_object_mut() {
            obj.insert("enforcement".to_string(), json!(enforcement));
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
            json!({"run": run, "gate": explain["gate"], "verdict": verdict}),
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

    /// `board_view` — DR-039: the READ-ONLY fleet projection. In the
    /// `tail_events` read class (unbadged, doc §12 as amended by DR-005). The
    /// data path is pure and re-interprets nothing (I3): `replay(None)` (the
    /// whole log) → `rezidnt_state::fold` → `rezidnt_state::project` — the ONE
    /// projection the read-only board also uses (hoisted into `rezidnt-state`,
    /// DR-039 Decision 3; this crate does not re-implement it). One whole-log
    /// fold per call (snapshot cost, same as the `rezidnt board` read).
    async fn call_board_view(&self, _args: Value) -> RpcOutcome {
        let events = self.replay(None).await?;
        let graph = rezidnt_state::fold(events.iter());
        let board_view = rezidnt_state::project(&graph);
        let payload = serde_json::to_value(&board_view)
            .map_err(|e| (-32603, format!("encode board view: {e}")))?;
        Ok(tool_ok(payload))
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
                "name": "kill_run",
                "description": "Terminate a run: OPERATOR-ONLY (DR-032 §1). Requires an operator badge; an agent macaroon is refused. Emits one attributed agent.signaled fact.",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::KillRunArgs))?,
            },
            {
                "name": "resolve_permit",
                "description": "Resolve a previously-escalated permit: OPERATOR-ONLY (DR-033 §1). Requires an operator badge; an agent macaroon is refused. Emits one permit.resolved fact the PDP applies on the next ask.",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::ResolvePermitArgs))?,
            },
            {
                "name": "request_permission",
                "description": "Ask the daemon PDP whether an agent action may proceed: a three-valued decision (allow|deny|ask), never coerced (I6, design §5). Badge required.",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::RequestPermissionArgs))?,
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
            {
                "name": "board_view",
                "description": "Read the derived fleet BoardView (whole-log fold, projected): events folded, workspace open/closed counts, subject histogram, run rows, worktree rows. Read-class, no badge (DR-039).",
                "inputSchema": schema(schemars::schema_for!(rezidnt_types::mcp::BoardViewArgs))?,
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
        let k = match next_read_len(raw.len() - body_start, BODY_CAP_BYTES, buf.len()) {
            None => return respond_body_too_large(&mut stream).await,
            Some(k) => k,
        };
        let n = stream.read(&mut buf[..k]).await?;
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

/// Body-cap read seam (pure): given how many body bytes are already
/// accumulated, the cap, and the size of the next read buffer, decide how many
/// bytes the next read is allowed to append.
///
/// - `None`  => reject now (413-class); the accumulated body has reached the cap
///   and the request is still not complete, so no further bytes are read.
/// - `Some(k)` => read at most `k` bytes; `k` is clamped so the accumulated body
///   NEVER exceeds `cap`. This is the tight bound: `raw`'s body portion is held
///   at `<= cap` at all times, not `cap + one_buffer`.
///
/// Extracting this makes the bound unit-testable off the wire, where the
/// 4096-byte read granularity is otherwise unobservable from a client.
fn next_read_len(accumulated_body: usize, cap: usize, buf_len: usize) -> Option<usize> {
    if accumulated_body >= cap {
        return None;
    }
    Some(buf_len.min(cap - accumulated_body))
}

#[cfg(test)]
mod body_cap_tests {
    use super::{BODY_CAP_BYTES, next_read_len};

    // THE PIN (red-before-fix): the read clamp must never permit the accumulated
    // body to exceed `cap`. Faithful current behavior returns `Some(buf_len)`
    // whenever `accumulated <= cap`, which lets the body reach `cap + buf_len`
    // (one buffer past the cap). The tight contract clamps the final read so the
    // total never crosses `cap`.
    #[test]
    fn read_is_clamped_so_body_never_exceeds_cap() {
        let cap = BODY_CAP_BYTES;
        let buf_len = 4096;

        // Just under the cap: only `remaining` bytes may be read, not a full
        // buffer, or the body would overshoot to `cap + (buf_len - remaining)`.
        let remaining = 100usize;
        let accumulated = cap - remaining;
        match next_read_len(accumulated, cap, buf_len) {
            None => {
                panic!("must still read the final {remaining} bytes at accumulated={accumulated}")
            }
            Some(k) => assert!(
                accumulated + k <= cap,
                "read clamp overshoots the cap: accumulated({accumulated}) + read({k}) = {} > cap({cap}); \
                 the body must never accumulate more than the cap before rejecting",
                accumulated + k
            ),
        }
    }

    // Exactly at the cap with the request still incomplete: reject, do not read.
    #[test]
    fn at_cap_and_still_short_rejects() {
        let cap = BODY_CAP_BYTES;
        assert_eq!(
            next_read_len(cap, cap, 4096),
            None,
            "at the cap and still short, the transport must reject (413), not read more"
        );
    }

    // Well under the cap with lots of headroom: a full buffer read is fine.
    #[test]
    fn under_cap_reads_full_buffer() {
        let cap = BODY_CAP_BYTES;
        let buf_len = 4096;
        assert_eq!(
            next_read_len(0, cap, buf_len),
            Some(buf_len),
            "with a full buffer of headroom the read need not be clamped"
        );
    }
}
