//! rezidnt socket protocol (doc §9).
//!
//! Wire contract pinned for S0:
//! - transport: UDS at `$XDG_RUNTIME_DIR/rezidnt.sock`, fallback
//!   `~/.local/state/rezidnt/rezidnt.sock` (Windows named pipe exists in the
//!   design; S0 does not test it);
//! - frames: JSON Lines. The first line the daemon sends on every connection
//!   is the versioned hello `{"proto":1,"schema":"<ontology hash>","daemon":"<semver>"}`;
//!   subsequent lines are event envelopes verbatim (`rezidnt_types::Event`
//!   JSONL) for tail subscribers;
//! - a proto **major** mismatch disconnects with a machine-readable upgrade
//!   hint ([`ProtoError::ProtoMismatch`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current protocol major. Mismatched majors disconnect.
pub const PROTO_VERSION: u32 = 1;

/// The versioned hello — always the first frame on a connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    /// Ontology hash (schema identity of the subject taxonomy).
    pub schema: String,
    /// Daemon semver.
    pub daemon: String,
}

/// Protocol-domain errors (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("proto major mismatch: peer speaks {got}, this end speaks {want} — {hint}")]
    ProtoMismatch { got: u32, want: u32, hint: String },
    #[error("frame encode/decode: {0}")]
    Json(#[from] serde_json::Error),
}

/// Encode a hello as one JSONL frame (single line, no trailing newline).
pub fn encode_hello(hello: &Hello) -> Result<String, ProtoError> {
    Ok(serde_json::to_string(hello)?)
}

/// Decode a hello frame. Tolerates unknown fields (additive evolution).
pub fn decode_hello(line: &str) -> Result<Hello, ProtoError> {
    Ok(serde_json::from_str(line)?)
}

/// Enforce the proto-major rule: `Err(ProtoMismatch)` with a non-empty,
/// machine-readable upgrade hint when `hello.proto != PROTO_VERSION`.
///
/// The hint is a compact JSON object (`{"action":"upgrade", ...}`) so a
/// disconnected peer can parse the remedy instead of scraping prose.
pub fn check_hello(hello: &Hello) -> Result<(), ProtoError> {
    if hello.proto != PROTO_VERSION {
        return Err(ProtoError::ProtoMismatch {
            got: hello.proto,
            want: PROTO_VERSION,
            hint: format!(
                "{{\"action\":\"upgrade\",\"required_proto\":{PROTO_VERSION},\"peer_proto\":{}}}",
                hello.proto
            ),
        });
    }
    Ok(())
}

/// Client request — the first line a client sends after reading the hello
/// (S1 protocol addition). Tagged by `op`; unknown ops are a decode error.
///
/// Back-compat rule (S0 clients sent no request line): a connection that
/// writes nothing is served as `Tail { subject: None }` — the S0 behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Replay from seq 0 then stream live, optionally filtered by subject.
    Tail {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
    },
    /// Materialize a project: parse the §13 spec, allocate, spawn (S1).
    Open { spec_toml: String },
    /// Replay a run's capture ring, then proxy live bytes (dtach model).
    Attach { run: ulid::Ulid },
    /// Record replay-divergence integrity alarms on the log (DR-006). The CLI
    /// computes the divergence(s) with its direct read, then routes the APPEND
    /// through the daemon's single writer (I3): the daemon dedups by
    /// (run, gate, verifier) against alarms already on the log and appends an
    /// `integrity.alarm` fact through its Fabric for each new divergence.
    RecordAlarms { alarms: Vec<AlarmRecord> },
    /// The harness PEP asks the daemon PDP "may this action proceed?" over the
    /// socket (SP1; design §3/§5, DR-008/DR-009). The small descriptor rides
    /// inline; bulk action context is a `context_ref` CAS-ref string, never
    /// inline bytes (I2). `badge` (caller identity) and `context_ref` are both
    /// optional on the wire — absent = OMITTED, never null. The daemon answers
    /// with [`Reply::PermitDecision`].
    RequestPermission {
        run: String,
        request_id: String,
        action: String,
        tool: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        badge: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_ref: Option<String>,
    },
}

/// One replay-divergence alarm the CLI asks the daemon to make durable
/// (DR-006). Mirrors the `integrity.alarm` v1 payload
/// (`{run, gate, verifier, recorded, replayed}`); verdicts are the concrete
/// verdict strings (`pass | fail | inconclusive`), never coerced (I6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlarmRecord {
    pub run: String,
    pub gate: String,
    pub verifier: String,
    pub recorded: String,
    pub replayed: String,
}

/// Encode a request as one JSONL frame (single line, no trailing newline).
pub fn encode_request(request: &Request) -> Result<String, ProtoError> {
    Ok(serde_json::to_string(request)?)
}

/// Decode a request frame. Unknown fields tolerated; unknown `op` is an
/// honest decode error.
pub fn decode_request(line: &str) -> Result<Request, ProtoError> {
    Ok(serde_json::from_str(line)?)
}

/// Machine-readable error codes on [`Reply::Error`] frames (S3 board).
/// Strings, not an enum, so peers built against an older proto still parse
/// codes they do not know (additive evolution).
pub mod codes {
    /// `attach` named a run the daemon does not know.
    pub const RUN_UNKNOWN: &str = "run.unknown";
    /// `open` carried a spec that failed to parse/validate (§13).
    pub const SPEC_INVALID: &str = "spec.invalid";
    /// A daemon-side failure while servicing the request (e.g. a log append
    /// failed during `record_alarms`). The client maps this to its
    /// substrate-fault exit class.
    pub const INTERNAL: &str = "internal";
    /// The daemon received a well-formed request op it does not yet serve on
    /// this transport. Honest (never a coerced decision): SP1 pins the
    /// `request_permission` WIRE shape (this proto), but the socket-side PDP
    /// handler is not wired here yet — the MCP surface is the SP1 decision path.
    pub const OP_NOT_SERVED: &str = "op.not_served";
}

/// Request-scoped reply frame (S3 proto addition, parked from S2).
///
/// Requests are one-per-connection, so "request-scoped" means: the FIRST
/// frame the daemon writes after reading a `Request` (and after the hello)
/// answers THAT request on THAT connection — an ack or a machine-readable
/// error, never a silent stream start and never a bare disconnect.
///
/// Wire pins (daemon-side emission is verified red by
/// `bins/rezidentd/tests/attach_and_ack.rs`):
/// - `open` success → `{"reply":"open_ok","workspace":"<ulid>","correlation":"<ulid>"}`
///   where `correlation` is the causal-chain id every materialization fact of
///   this open carries (ties the ack to the log);
/// - any failure → `{"reply":"error","op":"<request op>","code":"<codes::*>",
///   "message":"…"}` with `run` echoed for attach errors.
///
/// The type layer is real (serde is not the thing under test); this is a
/// pinned wire shape, additive-evolution rules as everywhere else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    /// The open succeeded: the workspace exists and its materialization
    /// facts (all carrying `correlation`) are on the fabric.
    OpenOk {
        workspace: ulid::Ulid,
        correlation: ulid::Ulid,
    },
    /// A `record_alarms` request completed (DR-006): the daemon has appended
    /// every NEW `integrity.alarm` fact through its single writer and the log
    /// is durable. `appended` counts the facts actually written (dedup skips
    /// alarms already on the log, so a re-run acks `appended: 0`). Acking only
    /// after the append lands is what makes the CLI's post-return log read
    /// race-free.
    AlarmsRecorded { appended: usize },
    /// The permit decision the PDP reached for a [`Request::RequestPermission`]
    /// (SP1; DR-008 §4). `decision` is exactly one of `allow | deny | ask` —
    /// `ask` is the escalate/inconclusive branch, carried VERBATIM to the PEP
    /// and NEVER coerced to `allow` (I6). `reason` says why on a deny/ask so a
    /// blocked agent can read it; absent on a trivially-granted allow.
    PermitDecision {
        request_id: String,
        decision: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The request failed. `code` is machine-readable ([`codes`]).
    Error {
        /// The request op this error answers (`"open"`, `"attach"`, …).
        op: String,
        code: String,
        /// Human-facing detail; never required for programmatic handling.
        message: String,
        /// Echoed run id for attach errors.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run: Option<ulid::Ulid>,
    },
}

/// Encode a reply as one JSONL frame (single line, no trailing newline).
pub fn encode_reply(reply: &Reply) -> Result<String, ProtoError> {
    Ok(serde_json::to_string(reply)?)
}

/// Decode a reply frame. Unknown fields tolerated (additive evolution).
pub fn decode_reply(line: &str) -> Result<Reply, ProtoError> {
    Ok(serde_json::from_str(line)?)
}

/// Pure socket-path resolution (testable without touching the process env):
/// `Some(xdg_runtime_dir)` → `<xdg>/rezidnt.sock`; otherwise
/// `<home>/.local/state/rezidnt/rezidnt.sock` (doc §9 fallback).
pub fn socket_path_from(xdg_runtime_dir: Option<&Path>, home: &Path) -> PathBuf {
    match xdg_runtime_dir {
        Some(xdg) => xdg.join("rezidnt.sock"),
        None => home
            .join(".local")
            .join("state")
            .join("rezidnt")
            .join("rezidnt.sock"),
    }
}

/// Env-reading wrapper over [`socket_path_from`]. `REZIDNT_SOCKET` overrides
/// everything (the integration tests and multi-daemon setups depend on it).
#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("REZIDNT_SOCKET") {
        return PathBuf::from(explicit);
    }
    let xdg = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    // HOME is effectively always set on unix; "." keeps this total rather
    // than panicking in a degenerate environment.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    socket_path_from(xdg.as_deref(), &home)
}
