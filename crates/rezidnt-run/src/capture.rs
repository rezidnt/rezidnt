//! Capture (DR-001): per-run ring buffer for live tail; full stream chunked
//! into the CAS with manifest facts carrying refs only (I2 — bytes never
//! touch the fabric).

use rezidnt_cas::{Cas, CasError};
use rezidnt_types::refs::CasRef;

use crate::RunId;

/// Ring capacity DEFAULT (DR-001: 256 KiB).
pub const DEFAULT_RING_BYTES: usize = 256 * 1024;

/// CAS chunk size DEFAULT: one chunk per 64 KiB of captured stream.
pub const DEFAULT_CHUNK_BYTES: usize = 64 * 1024;

/// Fixed-capacity byte ring: keeps the newest `capacity` bytes, in order.
#[derive(Debug)]
pub struct RingBuffer {
    capacity: usize,
    buf: std::collections::VecDeque<u8>,
}

impl RingBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            buf: std::collections::VecDeque::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Append bytes, evicting the oldest on overflow. A single push larger
    /// than the whole capacity retains only its newest `capacity` bytes.
    pub fn push(&mut self, bytes: &[u8]) {
        let keep = if bytes.len() > self.capacity {
            // Everything currently retained would be evicted anyway.
            self.buf.clear();
            &bytes[bytes.len() - self.capacity..]
        } else {
            let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.capacity);
            self.buf.drain(..overflow);
            bytes
        };
        self.buf.extend(keep);
    }

    /// The retained tail, oldest byte first.
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }
}

/// One captured chunk's manifest entry: position + ref, never bytes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ManifestEntry {
    pub run: RunId,
    /// 0-based chunk ordinal within the run's capture stream.
    pub chunk: u64,
    pub r#ref: CasRef,
}

/// Chunk a captured stream into the CAS; returns the ordered manifest.
/// Every entry's JSON payload is ≤ the I2 cap by construction (it carries a
/// ref, not bytes) — that property is pinned by test, not convention.
pub fn chunk_into_cas(
    cas: &Cas,
    run: RunId,
    stream: &[u8],
    chunk_bytes: usize,
) -> Result<Vec<ManifestEntry>, CasError> {
    // `chunks(0)` panics; a zero request degrades to 1-byte chunks rather
    // than panicking in library code (rust-conventions).
    let chunk_bytes = chunk_bytes.max(1);
    stream
        .chunks(chunk_bytes)
        .enumerate()
        .map(|(ordinal, chunk)| {
            let r#ref = cas.put(chunk, "application/octet-stream")?;
            Ok(ManifestEntry {
                run,
                chunk: ordinal as u64,
                r#ref,
            })
        })
        .collect()
}
