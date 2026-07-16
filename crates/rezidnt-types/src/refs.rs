//! Content-addressed references (doc §10).
//!
//! Events carry [`CasRef`]s, never bulk bytes (I2). The store itself lives in
//! `rezidnt-cas`; the ref type lives here because payload schemas embed it.

use serde::{Deserialize, Serialize};

/// Reference to a blob in the content-addressed store.
///
/// Wire shape BINDING-in-shape like the envelope: `{hash, bytes, mime}`.
/// `hash` is the lowercase blake3 hex (64 chars) of the blob content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CasRef {
    pub hash: String,
    pub bytes: u64,
    pub mime: String,
}
