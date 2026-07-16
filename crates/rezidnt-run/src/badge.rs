//! Badges (doc §12): per-AgentRun capability tokens, 256-bit random, opaque.
//! The token is the secret; the badge id is the loggable identifier — the
//! token itself never lands on the fabric.

use crate::RunError;

/// Environment variable the spawner injects the badge token under.
pub const BADGE_ENV_VAR: &str = "REZIDNT_BADGE";

/// A minted badge. `Debug` deliberately omits the token.
pub struct Badge {
    token: [u8; 32],
    id: String,
}

impl std::fmt::Debug for Badge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Badge")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Badge {
    /// Mint a badge: 256 random bits; id = first 8 bytes of blake3(token), hex.
    pub fn mint() -> Result<Self, RunError> {
        todo!("S1: 32 random bytes + derived public id")
    }

    /// Loggable identifier (safe for event payloads).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The secret, hex-encoded, for [`BADGE_ENV_VAR`] injection at spawn.
    pub fn token_hex(&self) -> String {
        let _ = &self.token;
        todo!("S1: lowercase hex of the 32 token bytes")
    }
}

/// Build a child environment from the parent's: secrets scrubbed (doc §12
/// "exec verifiers run with a scrubbed environment" — same discipline at
/// agent spawn), badge injected. Deny by pattern, keep the boring rest.
///
/// Denylist (DEFAULT): any var whose name ends in `_TOKEN`, `_KEY`, `_SECRET`,
/// or `_PASSWORD`, plus `AWS_*` credentials and `*CONNECTION_STRING*` names.
pub fn scrubbed_env(
    parent: impl Iterator<Item = (String, String)>,
    badge: &Badge,
) -> Vec<(String, String)> {
    let _ = (parent, badge);
    todo!("S1: filter denylisted names, append (BADGE_ENV_VAR, token_hex)")
}
