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

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            // write! to a String is infallible.
            let _ = write!(out, "{byte:02x}");
            out
        })
}

impl Badge {
    /// Mint a badge: 256 random bits; id = first 8 bytes of blake3(token), hex.
    pub fn mint() -> Result<Self, RunError> {
        use rand::RngCore as _;
        let mut token = [0u8; 32];
        // ThreadRng is a CSPRNG reseeded from the OS (doc §12: the token is
        // the capability secret).
        rand::rng().fill_bytes(&mut token);
        let id = hex_lower(&blake3::hash(&token).as_bytes()[..8]);
        Ok(Self { token, id })
    }

    /// Loggable identifier (safe for event payloads).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The secret, hex-encoded, for [`BADGE_ENV_VAR`] injection at spawn.
    pub fn token_hex(&self) -> String {
        hex_lower(&self.token)
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
    let mut child: Vec<(String, String)> = parent.filter(|(name, _)| !is_denied(name)).collect();
    // Drop any inherited badge var before injecting, so the injection below
    // is "exactly once" even under a nested-spawn parent.
    child.retain(|(name, _)| name != BADGE_ENV_VAR);
    child.push((BADGE_ENV_VAR.to_string(), badge.token_hex()));
    child
}

/// The denylist match. Names are compared ASCII-uppercased: environment
/// secret names are conventionally uppercase, and a lowercase `db_password`
/// is the same secret, not a different variable (DEFAULT — unpinned call).
fn is_denied(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.ends_with("_TOKEN")
        || upper.ends_with("_KEY")
        || upper.ends_with("_SECRET")
        || upper.ends_with("_PASSWORD")
        || upper.starts_with("AWS_")
        || upper.contains("CONNECTION_STRING")
}
