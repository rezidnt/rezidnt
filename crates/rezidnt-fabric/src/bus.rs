//! In-process fan-out: `tokio::sync::broadcast`, at-least-once to live
//! subscribers (doc §5). Append to the log is the commit point; broadcast
//! happens after the append succeeds.
//!
//! BINDING client rule (doc §5): a subscriber that overflows its buffer
//! receives `Lagged(n)` and MUST resync from the log by last-seen ULID —
//! never pretend continuity.
//!
//! Ordering model: the ring carries `(seq, event)` and every staleness,
//! dedup, and gap decision is made on **seq** — seq IS append order by
//! construction. Event ids are deliberately NOT assumed monotonic in append
//! order: minting an id and appending it are separate critical sections, so
//! concurrent publishers can append a smaller id after a larger one (and the
//! log legitimately accepts replayed/backfilled ids out of id order).

use std::sync::Mutex;

use rezidnt_types::Event;
use tokio::sync::broadcast;
use ulid::Ulid;

use crate::FabricError;
use crate::log::{EventLog, LogRow, Seq};

/// Subscriber-side receive error. `Lagged` is a *protocol event*, not a bug:
/// it obligates the resync path.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecvError {
    #[error("subscriber lagged; {0} events dropped — resync from the log (BINDING, doc §5)")]
    Lagged(u64),
    #[error("fabric closed")]
    Closed,
}

/// What travels the ring: the envelope plus its assigned append position.
#[derive(Debug, Clone)]
struct Published {
    seq: Seq,
    event: Event,
}

/// The bus: owns the log and the broadcast sender.
///
/// The log sits behind a `std::sync::Mutex` because `publish` is a blocking
/// (SQLite) operation: callers in async contexts must reach it via
/// `spawn_blocking` (rust-conventions: no blocking in async). The broadcast
/// send happens *while the log lock is held*, so ring order equals append
/// (seq) order even under concurrent publishers — that equality is what the
/// subscriber's seq-based adjudication relies on.
pub struct Fabric {
    log: Mutex<EventLog>,
    tx: broadcast::Sender<Published>,
}

/// Poison recovery: the log connection holds no in-memory invariant that a
/// panicking publisher could have half-applied (each append is one SQLite
/// transaction), so continuing with the inner value is sound.
fn lock_log(log: &Mutex<EventLog>) -> std::sync::MutexGuard<'_, EventLog> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Fabric {
    /// `capacity` is the broadcast ring size; small capacities are used by
    /// tests to force `Lagged`.
    pub fn new(log: EventLog, capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self {
            log: Mutex::new(log),
            tx,
        }
    }

    /// Append (commit point), then broadcast. Returns the assigned seq.
    pub fn publish(&self, event: Event) -> Result<Seq, FabricError> {
        let mut log = lock_log(&self.log);
        let seq = log.append(&event)?;
        // A send error only means "no live subscribers" — the event is
        // already durable in the log (the commit point), so it is not a
        // failure to surface.
        let _ = self.tx.send(Published { seq, event });
        Ok(seq)
    }

    /// New live subscriber positioned at the current stream head.
    pub fn subscribe(&self) -> Subscriber {
        Subscriber {
            rx: self.tx.subscribe(),
            last_seq: None,
            last_id: None,
            pending: None,
        }
    }

    /// Log replay for the resync path: every event after `last_seen`
    /// (`None` = from the beginning), in append order.
    pub fn replay_since(&self, last_seen: Option<Ulid>) -> Result<Vec<Event>, FabricError> {
        lock_log(&self.log).read_since(last_seen)
    }

    /// Row-level replay (internal): [`Subscriber::resync`] needs the seq of
    /// the last replayed row to advance its append-position cursor.
    fn replay_rows_since(&self, last_seen: Option<Ulid>) -> Result<Vec<LogRow>, FabricError> {
        lock_log(&self.log).read_rows_since(last_seen)
    }
}

/// A live subscription. Tracks its append position (`last_seq`, the dedup and
/// gap key) and the id of that same event (`last_id`, the BINDING resync
/// cursor) so the Lagged→resync path is gap- and duplicate-free.
pub struct Subscriber {
    rx: broadcast::Receiver<Published>,
    /// Seq of the latest observed event — append position; all staleness and
    /// contiguity decisions key on this, never on id order.
    last_seq: Option<Seq>,
    /// Id of that same (append-order-latest) event. NOT the max observed id:
    /// the resync cursor must name the append position.
    last_id: Option<Ulid>,
    /// A fresh event pulled while adjudicating a ring overflow (see `recv`):
    /// delivered or discarded on the next call instead of being lost.
    pending: Option<Published>,
}

impl Subscriber {
    /// Next live event. Contract: never yields an event at or before the
    /// subscriber's append position — after a resync, the stale broadcast
    /// backlog overlap is silently dropped by seq (this is what makes resync
    /// duplicate-free without assuming anything about id order).
    ///
    /// Ring-overflow adjudication: the ring reports `Lagged` even when every
    /// overwritten slot held an event the subscriber already covered via
    /// resync. The ring is in seq order, so peeking the oldest survivor
    /// decides it exactly: survivor seq ≤ last_seq → dropped slots are all ≤
    /// last_seq too — provable overlap, no gap, `Lagged` suppressed; survivor
    /// seq == last_seq + 1 → contiguous, nothing missed, deliver it; survivor
    /// seq > last_seq + 1 (or no position yet) → real gap: `Lagged` is
    /// surfaced (obligating the BINDING resync) and the survivor is parked in
    /// `pending` — the resync also covers it (append precedes delivery), so
    /// its stale copy is discarded by the seq filter afterwards.
    pub async fn recv(&mut self) -> Result<Event, RecvError> {
        loop {
            if let Some(p) = self.pending.take()
                && !self.is_stale(p.seq)
            {
                self.observe(p.seq, p.event.id);
                return Ok(p.event);
            }
            match self.rx.recv().await {
                Ok(p) => {
                    if self.is_stale(p.seq) {
                        continue; // stale overlap from before a resync
                    }
                    self.observe(p.seq, p.event.id);
                    return Ok(p.event);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Peek the oldest survivor to adjudicate. This cannot
                    // block: after a lag the receiver is repositioned onto a
                    // non-empty ring.
                    match self.rx.recv().await {
                        Ok(p) => {
                            if self.is_stale(p.seq) {
                                continue; // provable overlap — no gap
                            }
                            if self.last_seq.is_some_and(|s| p.seq == s + 1) {
                                // Contiguous with our position: every dropped
                                // slot was ≤ last_seq — nothing was missed.
                                self.observe(p.seq, p.event.id);
                                return Ok(p.event);
                            }
                            self.pending = Some(p);
                            return Err(RecvError::Lagged(n));
                        }
                        // Raced with more sends: still lagging — surface it.
                        Err(broadcast::error::RecvError::Lagged(m)) => {
                            return Err(RecvError::Lagged(n + m));
                        }
                        // Sender gone; the lag still obligates a final resync.
                        Err(broadcast::error::RecvError::Closed) => {
                            return Err(RecvError::Lagged(n));
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
            }
        }
    }

    fn is_stale(&self, seq: Seq) -> bool {
        self.last_seq.is_some_and(|s| seq <= s)
    }

    fn observe(&mut self, seq: Seq, id: Ulid) {
        self.last_seq = Some(seq);
        self.last_id = Some(id);
    }

    /// Id of the latest event this subscriber has observed in **append
    /// order** (via recv or resync) — the BINDING resync cursor (doc §5).
    pub fn last_seen(&self) -> Option<Ulid> {
        self.last_id
    }

    /// The BINDING resync: fetch everything missed from the log by last-seen
    /// ULID and advance the cursor past it. Returns the missed events in
    /// append order. Call after `recv` returns [`RecvError::Lagged`].
    pub fn resync(&mut self, fabric: &Fabric) -> Result<Vec<Event>, FabricError> {
        let rows = fabric.replay_rows_since(self.last_id)?;
        if let Some(last) = rows.last() {
            self.observe(last.seq, last.event.id);
        }
        Ok(rows.into_iter().map(|r| r.event).collect())
    }
}
