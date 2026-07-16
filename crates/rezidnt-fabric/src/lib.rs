//! rezidnt event fabric: append-only SQLite log (WAL), blake3 hash chain,
//! `tokio::sync::broadcast` fan-out, replay-from-ULID resync.
//!
//! Canonical design: doc §5 (delivery semantics) and §6 (log schema).

pub mod burst;
pub mod bus;
pub mod log;

pub use bus::{Fabric, RecvError, Subscriber};
pub use log::{CHAIN_GENESIS, EventLog, LogRow, Seq, chain_hash};

/// Fabric-domain errors (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum FabricError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("event: {0}")]
    Event(#[from] rezidnt_types::EventError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("duplicate event id {id} — append is exactly-once by ULID uniqueness")]
    DuplicateId { id: ulid::Ulid },
    #[error("hash chain broken at seq {seq}: {reason}")]
    ChainBroken { seq: log::Seq, reason: String },
    /// Implementer addition (S0): a stored row failed to decode back into an
    /// envelope (bad ULID text, unparseable ts, chain column of the wrong
    /// width). Distinct from `ChainBroken` — this is corruption of the row
    /// encoding, not a hash mismatch.
    #[error("log row {seq} is malformed: {reason}")]
    Malformed { seq: log::Seq, reason: String },
    /// Implementer addition (S0): the event handed to `append` carries a ts
    /// that does not format as RFC3339 — the event was never appended, so no
    /// row or seq is involved (distinct from `Malformed`, which names a
    /// stored row).
    #[error("cannot append event {id}: ts does not format as RFC3339: {reason}")]
    TsUnformattable { id: ulid::Ulid, reason: String },
}
