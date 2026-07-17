//! The loopback-HTTP announcement lockfile (doc §9: DEFAULT port 0,
//! announced via lockfile, not a fixed port).
//!
//! Shape (DEFAULT, flagged in the S3 oracle report): a single JSON object.
//! The `badge` field is the OPERATOR badge token — how a local client
//! (Claude Code) authorizes its mutating MCP calls (doc §12: badges on every
//! mutating call; the lockfile is 0600, so possession = the local user).
//! Unknown fields are tolerated (additive evolution, house rule).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::McpError;

/// The announced endpoint. Written atomically (temp + rename) so a reader
/// never observes a torn file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lockfile {
    /// Daemon pid — lets a reader detect a stale file.
    pub pid: u32,
    /// The actual bound port on 127.0.0.1 (never 0).
    pub port: u16,
    /// Full JSON-RPC endpoint URL (`http://127.0.0.1:<port>/...`).
    pub url: String,
    /// Operator badge token (hex) for mutating tools from local clients.
    pub badge: String,
}

/// Write the lockfile atomically, mode 0600 on unix.
pub fn write_atomic(path: &Path, lockfile: &Lockfile) -> Result<(), McpError> {
    let _ = (path, lockfile);
    todo!("S3 implementer: temp file + rename, 0600")
}

/// Read and parse a lockfile (tolerating unknown fields).
pub fn read(path: &Path) -> Result<Lockfile, McpError> {
    let _ = path;
    todo!("S3 implementer: read + serde parse")
}
