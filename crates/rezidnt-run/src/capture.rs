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
}

impl RingBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self { capacity }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Append bytes, evicting the oldest on overflow.
    pub fn push(&mut self, bytes: &[u8]) {
        let _ = bytes;
        todo!("S1: append with oldest-first eviction")
    }

    /// The retained tail, oldest byte first.
    pub fn snapshot(&self) -> Vec<u8> {
        todo!("S1: contiguous copy of retained bytes")
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
    let _ = (cas, run, stream, chunk_bytes);
    todo!("S1: split, put each chunk, manifest in order")
}
