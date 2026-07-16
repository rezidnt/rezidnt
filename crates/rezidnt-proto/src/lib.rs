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
