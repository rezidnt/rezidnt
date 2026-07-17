//! The loopback-HTTP announcement lockfile (doc §9: DEFAULT port 0,
//! announced via lockfile, not a fixed port).
//!
//! Shape (DEFAULT, flagged in the S3 oracle report): a single JSON object.
//! The `badge` field is the OPERATOR badge token — how a local client
//! (Claude Code) authorizes its mutating MCP calls (doc §12: badges on every
//! mutating call; the lockfile is 0600, so possession = the local user).
//! Unknown fields are tolerated (additive evolution, house rule).

use std::io::Write as _;
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

/// Write the lockfile atomically, mode 0600 on unix: the temp file is
/// CREATED private (never chmodded after the fact — the badge token must
/// not be world-readable for even a moment), then renamed into place.
pub fn write_atomic(path: &Path, lockfile: &Lockfile) -> Result<(), McpError> {
    let parent = path.parent().ok_or_else(|| {
        McpError::Lockfile(format!(
            "lockfile path {} has no parent dir",
            path.display()
        ))
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| {
            McpError::Lockfile(format!("lockfile path {} has no file name", path.display()))
        })?
        .to_string_lossy();
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));

    // O_EXCL (create_new) semantics: the tmp file is always FRESHLY minted, so
    // its mode is always the 0600 we mint here — a pre-existing hostile tmp at
    // the predictable `.<name>.tmp-<pid>` path can never leak its mode or
    // content into the lockfile. A stale tmp is unlinked, then recreated
    // exclusively; if the unlink races another creator, the create_new fails
    // loudly rather than reusing a foreign fd.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    // Remove any stale/hostile tmp so create_new can mint a fresh 0600 file.
    // A missing file is fine; anything else is a real error.
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut file = options.open(&tmp)?;
    file.write_all(serde_json::to_string(lockfile)?.as_bytes())?;
    // Durable before visible: the rename must never expose a torn file.
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read and parse a lockfile (tolerating unknown fields).
pub fn read(path: &Path) -> Result<Lockfile, McpError> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}
