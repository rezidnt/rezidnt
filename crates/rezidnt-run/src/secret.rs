//! Daemon-side secret resolution seam (DR-029 §Decision 2/3): the [`SecretSource`]
//! I4 trait + its host-file MVP backend. The egress fold
//! ([`crate::egress::fold_egress_policy`]) asks a `SecretSource` to resolve each
//! `[egress.secrets]` `secret_ref` into a [`BrokeredSecret`]; a ref the source
//! cannot resolve is DROPPED (a loud `credential.dropped` fact), NEVER a
//! fake/empty secret.
//!
//! ## The authority boundary (the `REZIDNT_ADMIN_PERMIT` precedent, DR-020)
//!
//! [`HostFileSecretSource`] reads a host TOML (`secret_ref = "value"`) pointed at
//! by `REZIDNT_EGRESS_SECRETS`, living OUTSIDE any workspace spec — a dev
//! physically cannot self-grant a secret by editing a repo file. Absent env ⇒ an
//! empty source (nothing resolvable, honest); a set-but-missing/malformed file ⇒
//! an honest [`RunError`], NEVER a silently-empty source that would drop the
//! boundary.
//!
//! ## Leak-discipline (DR-026 crit 5)
//!
//! The resolved value lives ONLY inside the [`BrokeredSecret`], whose
//! `Debug`/`Display` are redacted; it is reachable solely through the audited
//! `.expose()` on the upstream-write path. This module never `.expose()`s and
//! never logs a value — it constructs the `BrokeredSecret` and hands it back.

use std::collections::BTreeMap;
use std::path::Path;

use crate::RunError;
use crate::egress::BrokeredSecret;

/// The daemon-side secret-resolution seam (I4 — DR-029 §Decision 2). Resolves a
/// `secret_ref` (a `[egress.secrets]` LABEL) to its [`BrokeredSecret`] value.
/// Object-safe so the daemon can hold a `Box<dyn SecretSource>` and swap the
/// backend (the host-file MVP now, an `op`-CLI backend later behind the SAME
/// trait, DR-029 §Decision 4).
///
/// `resolve` returns:
/// - `Ok(Some(secret))` — the ref resolved to a value (brokered, value redacted);
/// - `Ok(None)` — the ref is UNRESOLVABLE (the DROP signal: the fold drops the
///   mapping with a loud `credential.dropped` fact, never a fake secret);
/// - `Err(_)` — the source itself could not be consulted (a genuine failure), NOT
///   the routine "this ref is absent" case.
pub trait SecretSource {
    /// Resolve `secret_ref` to its brokered value, `Ok(None)` if the source does
    /// not hold it (the DROP signal — never an error, never a fabricated secret).
    fn resolve(&self, secret_ref: &str) -> Result<Option<BrokeredSecret>, RunError>;
}

/// The host secrets-file MVP backend (DR-029 §Decision 3): a flat host TOML
/// (`secret_ref = "value"`) living OUTSIDE any workspace spec — the
/// `REZIDNT_ADMIN_PERMIT` authority-boundary precedent applied to secrets. The
/// values are held process-lifetime in memory; each is redacted the moment it
/// leaves as a [`BrokeredSecret`].
pub struct HostFileSecretSource {
    /// The `secret_ref → value` map parsed from the host file. Empty for an
    /// absent source (no file configured). Never serialized/logged — the values
    /// are secrets; only a resolved [`BrokeredSecret`] (redacted) ever leaves.
    secrets: BTreeMap<String, String>,
}

impl HostFileSecretSource {
    /// Build from an OPTIONAL host-file path (the injected-path ctor the unit
    /// tests drive; env-isolation-safe). `None` ⇒ an EMPTY source (nothing
    /// resolvable — the `absent REZIDNT_EGRESS_SECRETS ⇒ empty source`
    /// semantics). A `Some(path)` that is missing or not valid TOML is an honest
    /// [`RunError`], NEVER a silently-empty source that drops the authority
    /// boundary (DR-029 §Decision 3; DR-020 precedent).
    pub fn from_path(path: Option<&Path>) -> Result<Self, RunError> {
        let Some(path) = path else {
            return Ok(Self {
                secrets: BTreeMap::new(),
            });
        };
        // A set-but-missing file is an honest error (the boundary must never
        // silently drop to empty — the DR-020 admin-permit discipline).
        let text = std::fs::read_to_string(path).map_err(|e| {
            RunError::Spec(format!(
                "read REZIDNT_EGRESS_SECRETS host secrets file {}: {e}",
                path.display()
            ))
        })?;
        // A malformed file (not `secret_ref = "value"` TOML) is an honest error,
        // never a silently-empty source.
        let secrets: BTreeMap<String, String> = toml::from_str(&text).map_err(|e| {
            RunError::Spec(format!(
                "parse REZIDNT_EGRESS_SECRETS host secrets file {} (expected `secret_ref = \"value\"` TOML): {e}",
                path.display()
            ))
        })?;
        Ok(Self { secrets })
    }

    /// Build from the real `REZIDNT_EGRESS_SECRETS` env var (the daemon path,
    /// mirroring `admin_permit_layer`'s env read, DR-020). Absent env ⇒ an empty
    /// source; a set env pointing at a missing/malformed file ⇒ an honest error.
    /// This is the ONE reachability point that touches the shared-process env
    /// var; every other case is exercised through [`Self::from_path`].
    pub fn from_env() -> Result<Self, RunError> {
        match std::env::var_os("REZIDNT_EGRESS_SECRETS") {
            Some(p) => Self::from_path(Some(Path::new(&p))),
            None => Self::from_path(None),
        }
    }
}

impl SecretSource for HostFileSecretSource {
    fn resolve(&self, secret_ref: &str) -> Result<Option<BrokeredSecret>, RunError> {
        // A ref the file holds resolves to a redacted BrokeredSecret; an absent
        // ref resolves to Ok(None) — the DROP signal, NEVER a fabricated secret
        // (DR-029 §Decision 2). This constructs the secret; it never `.expose()`s
        // and never logs the value.
        Ok(self
            .secrets
            .get(secret_ref)
            .map(|value| BrokeredSecret::new(secret_ref, value)))
    }
}
