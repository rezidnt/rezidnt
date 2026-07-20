//! Badges (doc §12): per-AgentRun capability tokens, 256-bit random, opaque.
//! The token is the secret; the badge id is the loggable identifier — the
//! token itself never lands on the fabric.
//!
//! ## SP4b (DR-017): agent badges become macaroons
//! An agent badge is a first-party-caveat macaroon over the vendored
//! `blake3::keyed_hash` MAC (DR-017 §Decision 1 — ZERO new dependency, I7). A
//! holder can NARROW its badge offline (append a caveat, re-key the running
//! sig — no root key), the daemon VERIFIES the chain from its process-lifetime
//! root key and evaluates every caveat against the request context. The
//! construction (DR-017 §Decision 2, design §4):
//! ```text
//! sig₀   = blake3::keyed_hash(root_key, identifier)
//! sigᵢ₊₁ = blake3::keyed_hash(sigᵢ.as_bytes(), serialize(caveatᵢ))
//! verify = recompute chain from root_key; CONSTANT-TIME compare sig;
//!          then eval every caveat against the request context
//! ```
//! Monotonicity is the BINDING security invariant (I6): attenuation only
//! narrows — a widening bug is privilege escalation. The operator badge
//! ([`Badge`]) stays the DR-005 opaque daemon-lifetime class, untouched.

use serde::{Deserialize, Serialize};

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
/// agent spawn), the badge token injected under [`BADGE_ENV_VAR`]. Deny by
/// pattern, keep the boring rest.
///
/// SP4b (DR-017): `badge_token` is the value carried under `REZIDNT_BADGE`.
/// The env SEAM is unchanged — a single scrubbed-then-injected `REZIDNT_BADGE`
/// — only the token VALUE flips from a DR-005 opaque hex token to a serialized
/// agent macaroon ([`Macaroon::to_wire`]). It is still inline under the 32 KiB
/// cap (a macaroon is an identifier + a few caveats), never CAS (I2). The
/// scrubbing discipline and exactly-once injection are untouched.
///
/// Denylist (DEFAULT): any var whose name ends in `_TOKEN`, `_KEY`, `_SECRET`,
/// or `_PASSWORD`, plus `AWS_*` credentials and `*CONNECTION_STRING*` names.
pub fn scrubbed_env(
    parent: impl Iterator<Item = (String, String)>,
    badge_token: &str,
) -> Vec<(String, String)> {
    let mut child: Vec<(String, String)> = parent.filter(|(name, _)| !is_denied(name)).collect();
    // Drop any inherited badge var before injecting, so the injection below
    // is "exactly once" even under a nested-spawn parent.
    child.retain(|(name, _)| name != BADGE_ENV_VAR);
    child.push((BADGE_ENV_VAR.to_string(), badge_token.to_string()));
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
// ===========================================================================
// SP4b — macaroon-attenuated agent badges (DR-017, design §4–§5).
// ===========================================================================

/// The daemon's process-lifetime 256-bit macaroon root key (DR-017 §Decision 2,
/// design §4). `rand`-minted at startup, NEVER on the fabric — it is the whole
/// trust anchor: possession of it is the authority to mint + verify agent
/// macaroons. A daemon restart re-mints (badges are run-scoped/short-lived —
/// DR-017 §Decision 6), matching the operator-badge daemon-lifetime model.
///
/// `Debug` deliberately omits the key bytes.
#[derive(Clone)]
pub struct RootKey([u8; 32]);

impl std::fmt::Debug for RootKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootKey").finish_non_exhaustive()
    }
}

impl RootKey {
    /// Mint a fresh root key from the OS CSPRNG (the production path; `rand`
    /// already vendored, I7).
    pub fn mint() -> RootKey {
        use rand::RngCore as _;
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        RootKey(key)
    }

    /// Construct from explicit bytes — the test seam so verify is pinnable and
    /// replayable (I6). Not a production path (the daemon mints via [`RootKey::mint`]).
    pub fn from_bytes(bytes: [u8; 32]) -> RootKey {
        RootKey(bytes)
    }
}

/// Default agent-badge lifetime (DR-017 §Decision 6): agent macaroons are
/// run-scoped and short-lived — a restart re-mints the root key, so a badge
/// never outlives the daemon anyway. A generous window (an agent run is
/// single-digit minutes on the golden path) that still bounds a leaked token's
/// usefulness. DEFAULT — cheap to revisit; not a BINDING knob.
pub const DEFAULT_BADGE_TTL: std::time::Duration = std::time::Duration::from_secs(60 * 60 * 12);

/// The daemon's wall-clock UTC as an RFC3339 string — the `now` the door and
/// the mint supply to the (pure, clock-free) verifier. Reading the clock HERE
/// (an enforcement decision at the edge) and passing it in is I6-clean: `verify`
/// never reads an ambient clock (DR-017 §Decision 3). Infallible for the fixed
/// Rfc3339 description over a valid `OffsetDateTime`.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// `now + ttl` as an RFC3339 string — the `not_after` of a freshly-minted
/// agent badge's base [`Caveat::Expiry`]. Same clock-at-the-edge discipline as
/// [`now_rfc3339`].
pub fn expiry_from_now(ttl: std::time::Duration) -> String {
    let not_after = time::OffsetDateTime::now_utc()
        .saturating_add(time::Duration::try_from(ttl).unwrap_or(time::Duration::ZERO));
    not_after
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// The base caveat set for a freshly-minted agent badge (DR-017 / seam 2): the
/// run's SCOPE, permissive enough that the run's own legitimate governed calls
/// verify — a `Workspace` pin plus an `Expiry`, both deterministic from the
/// run's scope. Deliberately NO `Verb`/`Role` base caveat: narrowing verbs or
/// roles is what ATTENUATION adds for a sub-agent, not the base mint (a
/// restrictive base would refuse the run's own spawn/open/merge calls). The
/// `not_after` is a caller-supplied RFC3339 timestamp (see [`expiry_from_now`])
/// — clock at the edge, never inside verify (I6).
pub fn base_caveats(workspace: &str, not_after: impl Into<String>) -> Vec<Caveat> {
    vec![
        Caveat::Workspace {
            workspace: workspace.to_string(),
        },
        Caveat::Expiry {
            not_after: not_after.into(),
        },
    ]
}

/// A first-party caveat: a small structured predicate the daemon evaluates at
/// verify time (design §4). EXACTLY one of four kinds. Serialized canonically
/// (`#[serde(tag = "kind", rename_all = "snake_case")]`) so it wire-matches the
/// ratified `permit.delegated.added_caveats` shape (ontology) and so the
/// keyed-MAC chain is deterministic — a reordered or edited caveat breaks the
/// MAC (design §5). No third-party discharge — the daemon is the sole authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Caveat {
    /// Restrict the badge to a single workspace (a WorkspaceId ULID text).
    Workspace { workspace: String },
    /// Restrict the badge to a set of state-mutating verbs (`spawn`/`open`/`merge`).
    Verb { verbs: Vec<String> },
    /// The badge is invalid AT and after `not_after` (RFC3339 UTC; half-open
    /// `[.., not_after)` validity — the safe capability reading, DR-017 §Decision 3).
    Expiry { not_after: String },
    /// Pin the badge to an RBAC role (an opaque role string the policy interprets).
    Role { role: String },
}

impl Caveat {
    /// Deterministic bytes fed into the keyed-MAC chain. Canonical JSON of the
    /// tagged shape — `serde_json` emits struct fields in declaration order, so
    /// the same caveat always serializes to the same bytes (design §4/§5). This
    /// is also EXACTLY the `added_caveats` wire shape the reducer folds verbatim.
    fn mac_bytes(&self) -> Vec<u8> {
        // Infallible: a Caveat is a finite tagged enum of owned Strings/Vecs.
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// A minted or attenuated agent badge — an identifier, a caveat chain, and the
/// running keyed-MAC signature that binds them (DR-017 §Decision 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Macaroon {
    identifier: String,
    caveats: Vec<Caveat>,
    /// The last sig in the keyed-MAC chain (32 bytes). Serialized as bytes; the
    /// verifier recomputes and constant-time-compares this as a `blake3::Hash`.
    sig: [u8; 32],
}

/// The initial sig: keyed-hash the identifier under the root key (design §4).
fn sig0(root: &RootKey, identifier: &str) -> blake3::Hash {
    blake3::keyed_hash(&root.0, identifier.as_bytes())
}

/// Re-key the running sig with the next caveat: `sigᵢ₊₁ = keyed_hash(sigᵢ, caveatᵢ)`
/// (design §4). The previous sig's 32 bytes ARE the key for the next hop — this
/// is what lets a holder attenuate offline without the root key, and what makes
/// removing/reordering/editing a caveat break the chain.
fn sig_next(prev: &blake3::Hash, caveat: &Caveat) -> blake3::Hash {
    blake3::keyed_hash(prev.as_bytes(), &caveat.mac_bytes())
}

/// Recompute the whole chain from the root key over `(identifier, caveats)`.
fn compute_sig(root: &RootKey, identifier: &str, caveats: &[Caveat]) -> blake3::Hash {
    let mut sig = sig0(root, identifier);
    for caveat in caveats {
        sig = sig_next(&sig, caveat);
    }
    sig
}

impl Macaroon {
    /// Mint at spawn (daemon): identifier binds the run, `base` = the run's
    /// scope. The sig chain is computed from the root key (design §4).
    pub fn mint(root: &RootKey, identifier: impl Into<String>, base: Vec<Caveat>) -> Macaroon {
        let identifier = identifier.into();
        let sig = *compute_sig(root, &identifier, &base).as_bytes();
        Macaroon {
            identifier,
            caveats: base,
            sig,
        }
    }

    /// Attenuate (a holder, offline): append a narrowing caveat and re-key the
    /// running sig — NO root key needed (the delegation property, design §4).
    /// The result is a strictly-narrower macaroon whose chain still verifies.
    pub fn attenuate(&self, c: Caveat) -> Macaroon {
        // The current sig is the key for the next hop; no root key required.
        let prev = blake3::Hash::from_bytes(self.sig);
        let sig = *sig_next(&prev, &c).as_bytes();
        let mut caveats = self.caveats.clone();
        caveats.push(c);
        Macaroon {
            identifier: self.identifier.clone(),
            caveats,
            sig,
        }
    }

    /// The caveat chain (in order; order is MAC-bound — design §5).
    pub fn caveats(&self) -> &[Caveat] {
        &self.caveats
    }

    /// The root identifier that binds the run.
    pub fn identifier(&self) -> String {
        self.identifier.clone()
    }

    /// Loggable badge id: `hex(blake3(sig)[..8])` — the first 8 bytes of
    /// blake3 over the running MAC sig, hex-encoded (DR-018 §Decision (a)).
    ///
    /// SIG-DERIVED, not identifier-derived. Because each appended caveat
    /// re-keys the running sig ([`Macaroon::attenuate`]), the `badge_id`
    /// changes PER attenuation hop while the identifier stays fixed — so a
    /// true same-identifier `attenuate` yields distinct parent/child ids and
    /// the offline-delegation property (no root key re-mint) is preserved
    /// (DR-018 §Context). Shape is UNCHANGED: an 8-byte prefix as 16
    /// lowercase-hex chars, loggable, NEVER the token (I2/§12).
    ///
    /// We hash `blake3(sig)` — the sig hashed, not the raw sig bytes — so no
    /// bytes of the MAC itself land on the fabric (DR-018 §Decision (a) bullet 1).
    pub fn badge_id(&self) -> String {
        hex_lower(&blake3::hash(&self.sig).as_bytes()[..8])
    }

    /// The raw sig bytes (the tamper-test forge seam consumes these; the sig
    /// never lands on the fabric, I2).
    pub fn sig_bytes(&self) -> [u8; 32] {
        self.sig
    }

    /// The sig as a `blake3::Hash` — the value the verifier CONSTANT-TIME
    /// compares (blake3's `Hash: PartialEq` is documented constant-time; the
    /// vendored ct primitive, no new dep — DR-017 §Decision 3 / I7). The
    /// load-bearing seam that keeps the compare off a variable-time `&[u8]` memcmp.
    pub fn sig_hash(&self) -> blake3::Hash {
        blake3::Hash::from_bytes(self.sig)
    }

    /// Serialize for `REZIDNT_BADGE` injection — a macaroon is an identifier +
    /// a few small caveats, inline under the 32 KiB cap, never CAS (I2).
    pub fn to_wire(&self) -> String {
        // Infallible for this finite owned shape.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse a serialized macaroon (from `REZIDNT_BADGE`). A malformed wire form
    /// is a [`VerifyError::Malformed`], never a panic.
    pub fn from_wire(wire: &str) -> Result<Macaroon, VerifyError> {
        serde_json::from_str(wire).map_err(|e| VerifyError::Malformed(e.to_string()))
    }

    /// TEST-ONLY forge seam: build a macaroon from explicit parts WITHOUT
    /// recomputing the sig chain — the only way to construct a tampered
    /// macaroon (a removed/edited/reordered caveat under a stolen sig, or a
    /// fabricated sig). Verify MUST reject any such forgery (the MAC chain
    /// breaks). Not a production path.
    #[doc(hidden)]
    pub fn from_parts(
        identifier: impl Into<String>,
        caveats: Vec<Caveat>,
        sig: [u8; 32],
    ) -> Macaroon {
        Macaroon {
            identifier: identifier.into(),
            caveats,
            sig,
        }
    }
}

/// The request context a macaroon is verified against (design §4). The `now`
/// timestamp is PASSED IN by the caller — verify NEVER reads an ambient clock
/// (DR-017 §Decision 3, I6: replayable). Built fluently.
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    workspace: Option<String>,
    verb: Option<String>,
    now: Option<String>,
    role: Option<String>,
}

impl RequestContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// The workspace this request acts in (evaluated against `Caveat::Workspace`).
    pub fn workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    /// The verb this request performs (evaluated against `Caveat::Verb`).
    pub fn verb(mut self, verb: impl Into<String>) -> Self {
        self.verb = Some(verb.into());
        self
    }

    /// The caller-supplied RFC3339 UTC timestamp (evaluated against
    /// `Caveat::Expiry`; NO ambient clock — I6).
    pub fn now(mut self, now: impl Into<String>) -> Self {
        self.now = Some(now.into());
        self
    }

    /// The role this request carries (evaluated against `Caveat::Role`).
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }
}

/// The resolved authority a verified macaroon grants (design §5). Each axis is
/// the NARROWING of every caveat on that axis; `None` = unrestricted on that
/// axis. [`Capability::is_subset_of`] makes monotonicity a first-class
/// assertion — attenuation can only shrink a capability, never grow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// The single workspace the badge is confined to (`None` = any workspace).
    workspace: Option<String>,
    /// The allowed verb set (`None` = any verb). A narrowing `Verb` caveat
    /// intersects this set.
    verbs: Option<std::collections::BTreeSet<String>>,
    /// The effective expiry: the EARLIEST `not_after` across all expiry caveats
    /// (`None` = never expires). Narrowing takes the min.
    not_after: Option<String>,
    /// The required role (`None` = any role).
    role: Option<String>,
}

impl Capability {
    /// Monotonicity, first-class (design §5): `self ⊆ other` iff `self` grants
    /// no authority `other` does not. A child produced by attenuation is always
    /// a subset of its parent; a widening would violate this and is a privilege
    /// escalation defect (I6).
    pub fn is_subset_of(&self, other: &Capability) -> bool {
        // Workspace: a narrower cap either matches other's pin or other is
        // unrestricted (None). If self is unrestricted but other is pinned,
        // self is BROADER -> not a subset.
        let workspace_ok = match (&self.workspace, &other.workspace) {
            (_, None) => true,            // other allows any workspace
            (Some(s), Some(o)) => s == o, // both pinned: must match
            (None, Some(_)) => false,     // self any, other pinned -> broader
        };

        // Verbs: self's allowed set must be a subset of other's.
        let verbs_ok = match (&self.verbs, &other.verbs) {
            (_, None) => true, // other allows any verb
            (Some(s), Some(o)) => s.is_subset(o),
            (None, Some(_)) => false, // self any, other restricted -> broader
        };

        // Expiry: self must expire no later than other (self's window subset of
        // other's). RFC3339 Zulu compares lexicographically == chronologically.
        let expiry_ok = match (&self.not_after, &other.not_after) {
            (_, None) => true,            // other never expires
            (Some(s), Some(o)) => s <= o, // self expires at-or-before other
            (None, Some(_)) => false,     // self never expires, other does -> broader
        };

        // Role: a narrower cap either matches other's role or other is any.
        let role_ok = match (&self.role, &other.role) {
            (_, None) => true,
            (Some(s), Some(o)) => s == o,
            (None, Some(_)) => false,
        };

        workspace_ok && verbs_ok && expiry_ok && role_ok
    }
}

/// Macaroon verification failure (thiserror per lib convention). The refusing
/// caveat's `kind` is surfaced so `gate_explain` can say WHICH caveat refused
/// (I6 interrogability, DR-017 §Decision 3).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    /// The keyed-MAC chain did not recompute to the presented sig: a forged,
    /// tampered, reordered, or foreign-root-key macaroon.
    #[error("badge signature does not verify (tampered, forged, or foreign root key)")]
    BadSignature,
    /// A caveat's predicate was not satisfied by the request context. `kind`
    /// names the refusing caveat (`workspace`/`verb`/`expiry`/`role`).
    #[error("badge caveat unsatisfied: {kind}")]
    CaveatUnsatisfied { kind: String },
    /// The serialized macaroon could not be parsed (`from_wire`).
    #[error("badge malformed: {0}")]
    Malformed(String),
}

/// Verify a macaroon against the daemon root key + a request context (design
/// §4). Recompute the sig chain from `root`, CONSTANT-TIME compare `m.sig` (as
/// a `blake3::Hash`), THEN evaluate every caveat against `ctx`. Any unsatisfied
/// caveat -> `Err(CaveatUnsatisfied)`; a broken MAC -> `Err(BadSignature)`. Pure
/// and replayable — expiry is evaluated against `ctx.now`, never an ambient
/// clock (I6).
pub fn verify(
    m: &Macaroon,
    root: &RootKey,
    ctx: &RequestContext,
) -> Result<Capability, VerifyError> {
    // 1. Recompute the chain and constant-time compare as blake3::Hash values.
    //    `blake3::Hash: PartialEq` is documented constant-time (the vendored ct
    //    primitive — no `subtle`/`constant_time_eq`, I7).
    let recomputed = compute_sig(root, &m.identifier, &m.caveats);
    if recomputed != m.sig_hash() {
        return Err(VerifyError::BadSignature);
    }

    // 2. Evaluate every caveat against the context, accumulating the resolved
    //    capability. A caveat the context violates refuses (I6: kind surfaced).
    let mut cap = Capability {
        workspace: None,
        verbs: None,
        not_after: None,
        role: None,
    };
    for caveat in &m.caveats {
        match caveat {
            Caveat::Workspace { workspace } => {
                // The request must act in this workspace (if it declares one).
                if let Some(req_ws) = &ctx.workspace
                    && req_ws != workspace
                {
                    return Err(VerifyError::CaveatUnsatisfied {
                        kind: "workspace".to_string(),
                    });
                }
                cap.workspace = Some(workspace.clone());
            }
            Caveat::Verb { verbs } => {
                let allowed: std::collections::BTreeSet<String> = verbs.iter().cloned().collect();
                if let Some(req_verb) = &ctx.verb
                    && !allowed.contains(req_verb)
                {
                    return Err(VerifyError::CaveatUnsatisfied {
                        kind: "verb".to_string(),
                    });
                }
                // Intersect into the resolved allowed set (monotone narrowing).
                cap.verbs = Some(match cap.verbs.take() {
                    Some(prev) => prev.intersection(&allowed).cloned().collect(),
                    None => allowed,
                });
            }
            Caveat::Expiry { not_after } => {
                // Half-open [.., not_after): the timestamp AT not_after is
                // EXPIRED (DR-017 §Decision 3 — the safe capability reading).
                if let Some(now) = &ctx.now
                    && now.as_str() >= not_after.as_str()
                {
                    return Err(VerifyError::CaveatUnsatisfied {
                        kind: "expiry".to_string(),
                    });
                }
                // Effective expiry = the earliest not_after (min narrows).
                cap.not_after = Some(match cap.not_after.take() {
                    Some(prev) if prev <= *not_after => prev,
                    _ => not_after.clone(),
                });
            }
            Caveat::Role { role } => {
                if let Some(req_role) = &ctx.role
                    && req_role != role
                {
                    return Err(VerifyError::CaveatUnsatisfied {
                        kind: "role".to_string(),
                    });
                }
                cap.role = Some(role.clone());
            }
        }
    }
    Ok(cap)
}
