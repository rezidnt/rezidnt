//! Append-only event log. Schema is doc §6 verbatim (SQLite, WAL mode):
//!
//! ```sql
//! CREATE TABLE events (
//!   seq        INTEGER PRIMARY KEY,
//!   id         TEXT NOT NULL UNIQUE,
//!   ts         TEXT NOT NULL,
//!   v          INTEGER NOT NULL,
//!   source     TEXT NOT NULL,
//!   workspace  TEXT,
//!   subject    TEXT NOT NULL,
//!   correlation TEXT NOT NULL,
//!   causation  TEXT,
//!   payload    TEXT NOT NULL,
//!   chain      BLOB NOT NULL
//! );
//! -- idx_events_subject (subject, seq) · idx_events_ws (workspace, seq) · idx_events_corr (correlation)
//! ```
//!
//! Chain rule (doc §6/§12): `chain = blake3(prev.chain || id || payload)` where
//! `prev.chain` is the raw 32 chain bytes of the previous row
//! ([`CHAIN_GENESIS`] = 32 zero bytes for the first row), `id` is the 26-char
//! ULID string as ASCII bytes (exactly the TEXT stored in `id`), and `payload`
//! is the compact JSON text exactly as stored in the `payload` column.
//! The golden fixtures `spec/fixtures/s0_chain_valid.jsonl` /
//! `s0_chain_tamper.jsonl` pin this formula with precomputed values.

use std::path::Path;

use rezidnt_types::{Event, EventParts, SourceId, Subject, WorkspaceId};
use ulid::Ulid;

use crate::FabricError;

/// Monotonic append order (`events.seq`).
pub type Seq = i64;

/// `prev.chain` for the first row: 32 zero bytes (pinned by the golden
/// fixtures; the architecture doc does not name a genesis value — flagged in
/// the S0 oracle work order).
pub const CHAIN_GENESIS: [u8; 32] = [0u8; 32];

/// One row of the log: seq + chain bytes + the decoded envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRow {
    pub seq: Seq,
    pub chain: [u8; 32],
    pub event: Event,
}

/// Pure chain-link function, exposed so tests can pin it with known-answer
/// vectors. `payload_json` is the exact TEXT stored in the payload column.
pub fn chain_hash(prev: &[u8; 32], id: &Ulid, payload_json: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(prev);
    hasher.update(id.to_string().as_bytes());
    hasher.update(payload_json.as_bytes());
    *hasher.finalize().as_bytes()
}

/// Doc §6 DDL with `IF NOT EXISTS` semantics (accepts existing databases,
/// including doc-verbatim ones created by other writers).
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
  seq        INTEGER PRIMARY KEY,
  id         TEXT NOT NULL UNIQUE,
  ts         TEXT NOT NULL,
  v          INTEGER NOT NULL,
  source     TEXT NOT NULL,
  workspace  TEXT,
  subject    TEXT NOT NULL,
  correlation TEXT NOT NULL,
  causation  TEXT,
  payload    TEXT NOT NULL,
  chain      BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_subject ON events(subject, seq);
CREATE INDEX IF NOT EXISTS idx_events_ws       ON events(workspace, seq);
CREATE INDEX IF NOT EXISTS idx_events_corr     ON events(correlation);
";

const SELECT_COLUMNS: &str =
    "seq, id, ts, v, source, workspace, subject, correlation, causation, payload, chain";

/// The append-only event log. Append is the commit point (doc §5).
pub struct EventLog {
    conn: rusqlite::Connection,
}

impl EventLog {
    /// Open (or create) the log at `path`: WAL mode, doc §6 DDL with
    /// `IF NOT EXISTS` semantics so an existing log — including one written by
    /// another process or left behind by a crash — is accepted and recovered.
    pub fn open(path: &Path) -> Result<Self, FabricError> {
        let conn = rusqlite::Connection::open(path)?;
        // WAL: committed transactions survive a SIGKILLed process.
        // synchronous=NORMAL is the documented WAL pairing: durable against
        // process crash (the S0 exit demo), fsync deferred to checkpoints.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Append one event. Computes the next chain link, assigns `seq`, commits.
    /// A duplicate ULID yields [`FabricError::DuplicateId`] (exactly-once).
    ///
    /// One durable transaction per append: the commit point is per-event, so
    /// a crash at any instant leaves a whole-row prefix, never a torn row.
    pub fn append(&mut self, event: &Event) -> Result<Seq, FabricError> {
        let id_text = event.id.to_string();
        let payload_text =
            serde_json::to_string(event.payload()).map_err(rezidnt_types::EventError::from)?;
        let ts_text = event
            .ts
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| FabricError::TsUnformattable {
                id: event.id,
                reason: e.to_string(),
            })?;

        let tx = self.conn.transaction()?;
        // Same-process this pre-check is race-free (the caller holds `&mut
        // self` and the check shares the insert's transaction). Cross-process
        // the friendly DuplicateId is NOT guaranteed — the schema's UNIQUE
        // constraint backstops as `FabricError::Sqlite`.
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1)",
            rusqlite::params![id_text],
            |r| r.get(0),
        )?;
        if duplicate {
            return Err(FabricError::DuplicateId { id: event.id });
        }
        let head: Option<(Seq, Vec<u8>)> = tx
            .query_row(
                "SELECT seq, chain FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        let (prev_seq, prev_chain) = match head {
            Some((seq, bytes)) => (seq, chain_bytes(seq, &bytes)?),
            None => (0, CHAIN_GENESIS),
        };
        let seq = prev_seq + 1;
        let chain = chain_hash(&prev_chain, &event.id, &payload_text);
        tx.execute(
            "INSERT INTO events (seq, id, ts, v, source, workspace, subject, correlation, causation, payload, chain)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                seq,
                id_text,
                ts_text,
                event.v,
                event.source.as_str(),
                event.workspace.map(|w| w.ulid().to_string()),
                event.subject.as_str(),
                event.correlation.to_string(),
                event.causation.map(|c| c.to_string()),
                payload_text,
                chain.as_slice(),
            ],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    /// Read rows with `seq >= from`, ascending.
    pub fn read_from(&self, from: Seq) -> Result<Vec<LogRow>, FabricError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM events WHERE seq >= ?1 ORDER BY seq ASC"
        ))?;
        let rows = stmt.query_map(rusqlite::params![from], row_to_raw)?;
        rows.map(|raw| raw_to_log_row(raw?)).collect()
    }

    /// Resync read (BINDING client rule, doc §5): every event *after*
    /// `last_seen` in append order; `None` replays from the beginning.
    pub fn read_since(&self, last_seen: Option<Ulid>) -> Result<Vec<Event>, FabricError> {
        Ok(self
            .read_rows_since(last_seen)?
            .into_iter()
            .map(|r| r.event)
            .collect())
    }

    /// Row-level [`EventLog::read_since`] (subscribers advance their append
    /// position from the returned seqs).
    ///
    /// Resolution: `last_seen` is located by exact id and the read starts at
    /// the row after its seq — append order, never id order (ids are not
    /// monotonic in append order). Append precedes delivery, so anything a
    /// subscriber has seen is in the log; if the id is nonetheless absent
    /// (foreign cursor, truncated database), the read fails safe toward
    /// at-least-once: full replay from the beginning — duplicates are the
    /// acceptable failure mode, silent gaps are not.
    pub fn read_rows_since(&self, last_seen: Option<Ulid>) -> Result<Vec<LogRow>, FabricError> {
        let Some(id) = last_seen else {
            return self.read_from(1);
        };
        let seen_seq: Option<Seq> = self
            .conn
            .query_row(
                "SELECT seq FROM events WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        match seen_seq {
            Some(seq) => self.read_from(seq + 1),
            None => self.read_from(1),
        }
    }

    /// Walk the full chain; `Ok(n)` = n rows verified. A mismatched link
    /// (tampered payload, reordered/edited row) yields
    /// [`FabricError::ChainBroken`] naming the first bad seq.
    pub fn verify_chain(&self) -> Result<u64, FabricError> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, id, payload, chain FROM events ORDER BY seq ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Seq>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut prev = CHAIN_GENESIS;
        let mut verified = 0u64;
        for row in rows {
            let (seq, id_text, payload_text, stored) = row?;
            let id = parse_ulid(seq, "id", &id_text)?;
            let stored = chain_bytes(seq, &stored)?;
            let expected = chain_hash(&prev, &id, &payload_text);
            if stored != expected {
                return Err(FabricError::ChainBroken {
                    seq,
                    reason: "stored chain link does not match blake3(prev.chain || id || payload)"
                        .into(),
                });
            }
            prev = stored;
            verified += 1;
        }
        Ok(verified)
    }

    /// Last row, if any (chain head for continued appends after restart).
    pub fn last_row(&self) -> Result<Option<LogRow>, FabricError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM events ORDER BY seq DESC LIMIT 1"
        ))?;
        let mut rows = stmt.query_map([], row_to_raw)?;
        match rows.next() {
            None => Ok(None),
            Some(raw) => Ok(Some(raw_to_log_row(raw?)?)),
        }
    }
}

/// Raw column tuple, pre-decode (kept `rusqlite`-error-typed inside query_map).
type RawRow = (
    Seq,
    String,
    String,
    u16,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    String,
    Vec<u8>,
);

fn row_to_raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
    ))
}

fn parse_ulid(seq: Seq, field: &str, text: &str) -> Result<Ulid, FabricError> {
    Ulid::from_string(text).map_err(|e| FabricError::Malformed {
        seq,
        reason: format!("{field} is not a ULID ({text:?}): {e}"),
    })
}

fn chain_bytes(seq: Seq, bytes: &[u8]) -> Result<[u8; 32], FabricError> {
    bytes.try_into().map_err(|_| FabricError::Malformed {
        seq,
        reason: format!("chain column is {} bytes, expected 32", bytes.len()),
    })
}

fn raw_to_log_row(raw: RawRow) -> Result<LogRow, FabricError> {
    let (seq, id, ts, v, source, workspace, subject, correlation, causation, payload, chain) = raw;
    let malformed = |field: &str, reason: String| FabricError::Malformed {
        seq,
        reason: format!("{field}: {reason}"),
    };
    let ts = time::OffsetDateTime::parse(&ts, &time::format_description::well_known::Rfc3339)
        .map_err(|e| malformed("ts", e.to_string()))?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| malformed("payload", e.to_string()))?;
    let event = Event::from_parts(EventParts {
        id: parse_ulid(seq, "id", &id)?,
        ts,
        v,
        source: SourceId::new(source),
        workspace: workspace
            .map(|w| parse_ulid(seq, "workspace", &w).map(WorkspaceId::new))
            .transpose()?,
        subject: Subject::new(subject),
        correlation: parse_ulid(seq, "correlation", &correlation)?,
        causation: causation
            .map(|c| parse_ulid(seq, "causation", &c))
            .transpose()?,
        payload,
    })?;
    Ok(LogRow {
        seq,
        chain: chain_bytes(seq, &chain)?,
        event,
    })
}
