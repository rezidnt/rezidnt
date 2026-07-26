//! Content-addressed store (doc §10).
//!
//! Blobs live at `<root>/<blake3-hex>`, written once, referenced by
//! [`CasRef`] in events. GC is reachability-from-log and PROVISIONAL — not
//! built here. blake3 is the DEFAULT hash (fast, incremental-friendly).

use std::path::{Path, PathBuf};

use rezidnt_types::refs::CasRef;

/// Length of a CAS address in hex characters: blake3 is 32 bytes, so 64.
/// [`Cas::put`] returns exactly this shape and nothing else addresses a blob.
const ADDRESS_HEX_LEN: usize = 64;

/// Is this string a CAS ADDRESS — exactly [`ADDRESS_HEX_LEN`] LOWERCASE hex
/// characters?
///
/// LOWERCASE only, deliberately narrow: [`Cas::put`] emits lowercase, so a
/// lowercase-only rule admits every address this store can actually hold.
/// Accepting uppercase would admit a string addressing nothing on a
/// case-sensitive filesystem and a DIFFERENT-cased duplicate on Windows.
fn is_address(hash: &str) -> bool {
    hash.len() == ADDRESS_HEX_LEN && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Errors for store operations (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum CasError {
    #[error("cas io: {0}")]
    Io(#[from] std::io::Error),
    #[error("blob {hash} not found")]
    NotFound { hash: String },
    #[error("blob corrupt: addressed {addressed}, content hashes to {actual}")]
    Corrupt { addressed: String, actual: String },
    /// DR-058 §Decision 4 — the argument is not a CAS ADDRESS, so no path was
    /// ever joined and nothing on disk was touched. Deliberately DISTINCT from
    /// [`CasError::NotFound`]: that one says the store LOOKED and did not find,
    /// which would be false here and would misstate why (I6).
    ///
    /// The message carries NO echo of the offending string and no fact about
    /// the store — the verdict is a pure function of the argument the caller
    /// already holds, so every rejected shape is refused identically.
    #[error("not a CAS address: expected exactly {expected} lowercase hex characters")]
    InvalidAddress { expected: usize },
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

        // blake3 hex IS an address by construction, so the guard admits it;
        // `?` rather than an unreachable! keeps the invariant stated in one
        // place (the guard) instead of asserted twice.
        let hash = blake3::hash(bytes).to_hex().to_string();
        let dest = self.path_for(&hash)?;
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
    ///
    /// Inherits [`Cas::path_for`]'s address guard: a `hash` that is not an
    /// address is [`CasError::InvalidAddress`] with no filesystem access at
    /// all — never `NotFound` (which would claim the store looked) and never
    /// `Corrupt` (which would prove it read something).
    pub fn get(&self, r: &CasRef) -> Result<Vec<u8>, CasError> {
        let path = self.path_for(&r.hash)?;
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

    /// Filesystem path a hash resolves to (`<root>/<hex>`), or
    /// [`CasError::InvalidAddress`] if the argument is not an address.
    ///
    /// THE CRATE-LEVEL GUARD (DR-058 §Decision 4). `PathBuf::join` REPLACES
    /// the root on an absolute component and normalizes no `..`, so an
    /// unvalidated string joined here is an arbitrary host path. The shape is
    /// therefore checked BEFORE the join — the refusal is a pure function of
    /// the argument, decidable without touching the filesystem, so no caller
    /// can turn this into an existence oracle.
    ///
    /// Callers reaching this with data they did not mint (a caller's argument,
    /// a subprocess's stdout) must map [`CasError::InvalidAddress`] the way
    /// they already map [`CasError::NotFound`]: can't-run, never a decision.
    pub fn path_for(&self, hash: &str) -> Result<PathBuf, CasError> {
        if !is_address(hash) {
            return Err(CasError::InvalidAddress {
                expected: ADDRESS_HEX_LEN,
            });
        }
        Ok(self.root.join(hash))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
