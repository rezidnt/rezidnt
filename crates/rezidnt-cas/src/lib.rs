//! Content-addressed store (doc §10).
//!
//! Blobs live at `<root>/<blake3-hex>`, written once, referenced by
//! [`CasRef`] in events. GC is reachability-from-log and PROVISIONAL — not
//! built here. blake3 is the DEFAULT hash (fast, incremental-friendly).

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
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Store a blob. Write-once: storing identical content returns the same
    /// ref without rewriting. The returned hash is lowercase blake3 hex.
    ///
    /// Writes go through a uniquely named temp file in the root followed by a
    /// rename, so a concurrent reader never observes a half-written blob. If
    /// the destination already exists (idempotent re-put, or a concurrent
    /// writer won the race) the content is identical by construction — same
    /// hash, same bytes — so the existing blob is left untouched.
    pub fn put(&self, bytes: &[u8], mime: &str) -> Result<CasRef, CasError> {
        // Temp-name uniqueness across threads of this process; pid covers
        // cross-process writers sharing a root.
        static PUT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let hash = blake3::hash(bytes).to_hex().to_string();
        let dest = self.path_for(&hash);
        if !dest.exists() {
            let n = PUT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let tmp = self
                .root
                .join(format!(".tmp-{hash}-{}-{n}", std::process::id()));
            std::fs::write(&tmp, bytes)?;
            if let Err(rename_err) = std::fs::rename(&tmp, &dest) {
                // A concurrent writer may have landed the identical blob
                // first (Windows rename refuses to replace). Losing the race
                // is success; anything else is a real error.
                let _ = std::fs::remove_file(&tmp);
                if !dest.exists() {
                    return Err(rename_err.into());
                }
            }
        }
        Ok(CasRef {
            hash,
            bytes: bytes.len() as u64,
            mime: mime.to_string(),
        })
    }

    /// Fetch a blob and verify its content against the addressed hash —
    /// corruption is an error, never silently returned data.
    pub fn get(&self, r: &CasRef) -> Result<Vec<u8>, CasError> {
        let path = self.path_for(&r.hash);
        let content = match std::fs::read(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CasError::NotFound {
                    hash: r.hash.clone(),
                });
            }
            Err(e) => return Err(e.into()),
        };
        let actual = blake3::hash(&content).to_hex().to_string();
        if actual != r.hash {
            return Err(CasError::Corrupt {
                addressed: r.hash.clone(),
                actual,
            });
        }
        Ok(content)
    }

    /// Filesystem path a hash resolves to (`<root>/<hex>`).
    pub fn path_for(&self, hash: &str) -> PathBuf {
        self.root.join(hash)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
