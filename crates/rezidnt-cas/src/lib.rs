//! Content-addressed store (doc §10).
//!
//! Blobs live at `<root>/<blake3-hex>`, written once, referenced by
//! [`CasRef`] in events. GC is reachability-from-log and PROVISIONAL — not
//! built here. blake3 is the DEFAULT hash (fast, incremental-friendly).
//!
//! S1 oracle note: methods are `todo!()` stubs; the failing tests in
//! `tests/store.rs` are the implementer's work order.

use std::path::{Path, PathBuf};

use rezidnt_types::refs::CasRef;

/// Errors for store operations (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum CasError {
    #[error("cas io: {0}")]
    Io(#[from] std::io::Error),
    #[error("blob {hash} not found")]
    NotFound { hash: String },
    #[error("blob corrupt: addressed {addressed}, content hashes to {actual}")]
    Corrupt { addressed: String, actual: String },
}

/// A content-addressed store rooted at one directory.
#[derive(Debug)]
pub struct Cas {
    root: PathBuf,
}

impl Cas {
    /// Open (creating the root directory if needed).
    pub fn open(root: &Path) -> Result<Self, CasError> {
        let _ = root;
        todo!("S1: create root if absent, return store")
    }

    /// Store a blob. Write-once: storing identical content returns the same
    /// ref without rewriting. The returned hash is lowercase blake3 hex.
    pub fn put(&self, bytes: &[u8], mime: &str) -> Result<CasRef, CasError> {
        let _ = (bytes, mime);
        todo!("S1: hash, write-once, return CasRef")
    }

    /// Fetch a blob and verify its content against the addressed hash —
    /// corruption is an error, never silently returned data.
    pub fn get(&self, r: &CasRef) -> Result<Vec<u8>, CasError> {
        let _ = r;
        todo!("S1: read, re-hash, verify, return")
    }

    /// Filesystem path a hash resolves to (`<root>/<hex>`).
    pub fn path_for(&self, hash: &str) -> PathBuf {
        let _ = hash;
        todo!("S1: root-joined hash path")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
