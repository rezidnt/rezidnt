//! rezidnt MCP surface (doc §9, I5: MCP-first).
//!
//! S3 oracle note: the type layer below is real; every behavior seam is a
//! `todo!()` stub. The failing tests under `tests/` are the implementer's
//! work order.
//!
//! Shape law (binding for this crate, set by the S3 board): the core is
//! TRANSPORT-AGNOSTIC — [`McpCore::handle`] maps one JSON-RPC 2.0 request
//! value to one response value. Transports (stdio lines, loopback HTTP) are
//! thin byte shims over that seam. Whether the implementer adopts `rmcp` or
//! hand-rolls the layer, the observable JSON-RPC messages are what the tests
//! pin — never SDK internals.
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
use std::path::Path;
use std::sync::Arc;

use rezidnt_fabric::Fabric;
use rezidnt_run::badge::Badge;
use tokio::io::{AsyncRead, AsyncWrite};

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
}

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

/// The transport-agnostic MCP core: one JSON-RPC request in, one response
/// out, side effects on the fabric only (I3: the log is truth).
pub struct McpCore {
    fabric: Fabric,
    badges: BadgeBook,
}

impl McpCore {
    pub fn new(fabric: Fabric, badges: BadgeBook) -> Self {
        Self { fabric, badges }
    }

    /// The fabric this surface publishes to and reads from (tests assert
    /// side effects — and their absence — through it).
    pub fn fabric(&self) -> &Fabric {
        &self.fabric
    }

    pub fn badges(&self) -> &BadgeBook {
        &self.badges
    }

    /// Handle one JSON-RPC 2.0 message. Returns `Some(response)` for
    /// requests, `None` for notifications. Never panics on garbage input —
    /// malformed JSON-RPC gets a spec error object (-32600/-32601/-32602).
    pub async fn handle(&self, request: serde_json::Value) -> Option<serde_json::Value> {
        let _ = request;
        todo!(
            "S3 implementer: JSON-RPC dispatch (initialize, tools/list, tools/call, resources/read)"
        )
    }
}

/// Serve MCP over a byte stream, newline-delimited JSON-RPC — the stdio
/// transport shape (doc §9), testable in-process over a duplex pipe.
pub async fn serve_stdio<R, W>(core: Arc<McpCore>, reader: R, writer: W) -> Result<(), McpError>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    let _ = (core, reader, writer);
    todo!("S3 implementer: line-delimited JSON-RPC over the stream")
}

/// A running loopback-HTTP transport. Dropping it stops the listener.
pub struct HttpHandle {
    /// The ACTUAL bound port (never 0, never fixed — doc §9).
    pub port: u16,
    /// Full endpoint URL clients POST JSON-RPC to, as announced in the
    /// lockfile (e.g. `http://127.0.0.1:<port>/mcp`).
    pub url: String,
}

/// Serve MCP over loopback HTTP on `127.0.0.1:0` and announce the bound
/// endpoint by writing the lockfile at `lockfile_path` (doc §9: port 0,
/// announced via lockfile — not a fixed port).
pub async fn serve_http(core: Arc<McpCore>, lockfile_path: &Path) -> Result<HttpHandle, McpError> {
    let _ = (core, lockfile_path);
    todo!("S3 implementer: bind 127.0.0.1:0, write lockfile, serve JSON-RPC over HTTP POST")
}
