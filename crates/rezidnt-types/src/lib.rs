//! rezidnt event envelope and id newtypes.
//!
//! Canonical shape: `docs/rezidnt-architecture.md` §5 (BINDING in shape;
//! additive evolution only). This crate owns all serde derives so a binary
//! re-encoding is a later drop-in (doc §5).
//!
//! S0 oracle note: constructors and wire codecs are `todo!()` stubs. The
//! failing tests in `tests/envelope.rs` are the implementer's work order.

pub mod taxonomy;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ulid::Ulid;

/// I2 hard cap: payloads above this become CAS refs, never inline bytes.
/// Measured on the compact JSON encoding of the payload value.
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024;

/// Adapter/component that emitted an event (e.g. `daemon`, `git-adapter`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(String);

impl SourceId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Workspace identity. Newtyped per rust-conventions ("newtype every id").
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct WorkspaceId(Ulid);

impl WorkspaceId {
    pub fn new(id: Ulid) -> Self {
        Self(id)
    }

    pub fn ulid(&self) -> Ulid {
        self.0
    }
}

/// Dot-namespaced subject, `noun.verb[.qualifier]` (spec/ontology.md, BINDING
/// grammar). Subjects are never renamed, only deprecated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Subject(String);

impl Subject {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Errors for envelope construction and wire codec (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("payload is {actual} bytes (compact JSON); hard cap is {MAX_PAYLOAD_BYTES} — I2")]
    PayloadTooLarge { actual: usize },
    #[error("envelope encode/decode: {0}")]
    Json(#[from] serde_json::Error),
}

/// The event envelope (doc §5, BINDING in shape; additive evolution only).
///
/// `payload` is deliberately **not** a public field: the ≤32 KiB invariant is
/// enforced by [`Event::new`] / [`Event::from_parts`] (rust-conventions: no
/// pub-field leakage of invariants). `Deserialize` is implemented manually
/// (via the private [`EventWire`] shape) so the cap is **structural**: every
/// deserialization path — plain serde included — enforces it, not just the
/// [`Event::from_json_line`] entry point.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// Time-ordered, globally unique. ULID uniqueness is the exactly-once key.
    pub id: Ulid,
    /// Daemon clock, UTC, RFC3339 on the wire.
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    /// Payload schema version for this subject (taxonomy v0 mints everything at 1).
    pub v: u16,
    pub source: SourceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceId>,
    pub subject: Subject,
    /// Groups a causal chain (one `open`, one gate run).
    pub correlation: Ulid,
    /// The event that directly triggered this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation: Option<Ulid>,
    payload: serde_json::Value,
}

/// Private deserialization shape: field-for-field the envelope, minus the
/// invariant. Every `Deserialize` for [`Event`] funnels through this +
/// [`check_payload_size`], making the I2 cap structural rather than an
/// entry-point convention. Unknown fields are tolerated (additive evolution,
/// doc §5) because this derive does not deny them.
#[derive(Deserialize)]
struct EventWire {
    id: Ulid,
    #[serde(with = "time::serde::rfc3339")]
    ts: OffsetDateTime,
    v: u16,
    source: SourceId,
    #[serde(default)]
    workspace: Option<WorkspaceId>,
    subject: Subject,
    correlation: Ulid,
    #[serde(default)]
    causation: Option<Ulid>,
    payload: serde_json::Value,
}

impl TryFrom<EventWire> for Event {
    type Error = EventError;

    fn try_from(wire: EventWire) -> Result<Self, EventError> {
        check_payload_size(&wire.payload)?;
        let EventWire {
            id,
            ts,
            v,
            source,
            workspace,
            subject,
            correlation,
            causation,
            payload,
        } = wire;
        Ok(Self {
            id,
            ts,
            v,
            source,
            workspace,
            subject,
            correlation,
            causation,
            payload,
        })
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = EventWire::deserialize(deserializer)?;
        Self::try_from(wire).map_err(serde::de::Error::custom)
    }
}

/// Loose parts for [`Event::from_parts`] — replay/adapter paths that already
/// carry an id and timestamp. Validation happens in `from_parts`, not here.
#[derive(Debug, Clone)]
pub struct EventParts {
    pub id: Ulid,
    pub ts: OffsetDateTime,
    pub v: u16,
    pub source: SourceId,
    pub workspace: Option<WorkspaceId>,
    pub subject: Subject,
    pub correlation: Ulid,
    pub causation: Option<Ulid>,
    pub payload: serde_json::Value,
}

/// Process-wide monotonic ULID source: sequential mints within the same
/// millisecond increment the random component, so `Event::new` ids are
/// strictly time-ordered (doc §5: `id` is time-ordered and globally unique).
static ULID_GENERATOR: std::sync::Mutex<Option<ulid::Generator>> = std::sync::Mutex::new(None);

fn mint_ulid() -> Ulid {
    // Poison recovery: the generator holds no invariant beyond "last id
    // handed out"; continuing with the inner value after a poisoning panic
    // is safe (worst case the next id is re-randomized by the crate).
    let mut guard = ULID_GENERATOR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let generator = guard.get_or_insert_with(ulid::Generator::new);
    loop {
        match generator.generate() {
            Ok(id) => return id,
            // Overflow of the 80-bit random component within one
            // millisecond: retry until the clock tick advances (bounded by
            // 1 ms of spinning; practically unreachable).
            Err(ulid::MonotonicError::Overflow) => std::hint::spin_loop(),
        }
    }
}

/// I2 gate shared by every construction path: measure the compact JSON
/// encoding of the payload and reject anything over [`MAX_PAYLOAD_BYTES`].
fn check_payload_size(payload: &serde_json::Value) -> Result<(), EventError> {
    let actual = serde_json::to_string(payload)?.len();
    if actual > MAX_PAYLOAD_BYTES {
        return Err(EventError::PayloadTooLarge { actual });
    }
    Ok(())
}

impl Event {
    /// Mint a new event: fresh time-ordered ULID id, `ts` = now (UTC, daemon
    /// clock). Rejects payloads whose compact JSON encoding exceeds
    /// [`MAX_PAYLOAD_BYTES`] with [`EventError::PayloadTooLarge`].
    pub fn new(
        source: SourceId,
        workspace: Option<WorkspaceId>,
        subject: Subject,
        correlation: Ulid,
        causation: Option<Ulid>,
        v: u16,
        payload: serde_json::Value,
    ) -> Result<Self, EventError> {
        check_payload_size(&payload)?;
        Ok(Self {
            id: mint_ulid(),
            ts: OffsetDateTime::now_utc(),
            v,
            source,
            workspace,
            subject,
            correlation,
            causation,
            payload,
        })
    }

    /// Assemble an event from explicit parts (replay, adapters, tests).
    /// Same payload-size enforcement as [`Event::new`].
    pub fn from_parts(parts: EventParts) -> Result<Self, EventError> {
        check_payload_size(&parts.payload)?;
        let EventParts {
            id,
            ts,
            v,
            source,
            workspace,
            subject,
            correlation,
            causation,
            payload,
        } = parts;
        Ok(Self {
            id,
            ts,
            v,
            source,
            workspace,
            subject,
            correlation,
            causation,
            payload,
        })
    }

    /// Payload accessor (read-only; the size invariant is constructor-enforced).
    pub fn payload(&self) -> &serde_json::Value {
        &self.payload
    }

    /// Encode as one JSON Lines frame (doc §5: JSONL on the wire, JSON in the
    /// log column). Optional fields are *absent* when `None`, never `null`.
    pub fn to_json_line(&self) -> Result<String, EventError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Decode one JSON Lines frame. MUST tolerate unknown fields at both the
    /// envelope level and inside the payload (additive evolution, doc §5) and
    /// MUST enforce the payload cap. Decodes via the wire shape directly so
    /// an oversized payload surfaces as the typed
    /// [`EventError::PayloadTooLarge`] (plain-serde paths get the same check
    /// inside `Deserialize`, but as an opaque serde error).
    pub fn from_json_line(line: &str) -> Result<Self, EventError> {
        let wire: EventWire = serde_json::from_str(line)?;
        Self::try_from(wire)
    }
}
