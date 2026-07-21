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
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    /// Resolve `secret_ref`, distinguishing a RESOLVED value from a DROP that
    /// carries a per-floor `reason` (DR-030 §Decision 5 — I6 interrogability: "why
    /// was this credential dropped?" answers per floor: `op` unavailable vs
    /// auth-fail vs resolution-fail). The default delegates to [`Self::resolve`]
    /// and attaches the generic DR-029 unresolvable-drop reason, so a backend that
    /// only overrides `resolve` (the host-file MVP, a test recorder) keeps working;
    /// the `op` backend OVERRIDES this to carry the distinguishing floor reason.
    ///
    /// The `reason` is a LABEL only — it NEVER carries a resolved value or the
    /// service-account token (DR-030 §Decision 3); it is loggable/factable.
    fn resolve_with_reason(&self, secret_ref: &str) -> Result<SecretResolution, RunError> {
        Ok(match self.resolve(secret_ref)? {
            Some(secret) => SecretResolution::Resolved(secret),
            None => SecretResolution::Dropped {
                reason: DROP_REASON_UNRESOLVABLE.to_string(),
            },
        })
    }
}

/// The generic DR-029 unresolvable-drop reason the host-file MVP (and any backend
/// that only overrides [`SecretSource::resolve`]) carries — preserved verbatim from
/// the shipped `dropped_fact` default so the host-file path is untouched.
pub(crate) const DROP_REASON_UNRESOLVABLE: &str = "secret_ref unresolvable by the configured SecretSource — \
     mapping dropped, host mediated-without-injection";

/// The outcome of [`SecretSource::resolve_with_reason`]: either a resolved
/// (redacted) [`BrokeredSecret`], or a DROP carrying a distinguishing `reason`
/// LABEL (never a value/token). This is the carrier the fold threads into a
/// `CredentialDrop` so the loud `credential.dropped` fact rides a per-floor reason
/// (DR-030 §Decision 5, I6).
#[derive(Debug)]
pub enum SecretResolution {
    /// The ref resolved to a brokered value (value redacted at the type level).
    Resolved(BrokeredSecret),
    /// The ref is UNRESOLVABLE — DROP the mapping (never a fake secret) with a
    /// distinguishing, loggable `reason` (I6). Carries NO value/token.
    Dropped { reason: String },
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

/// The `op://` reference scheme the [`OpSecretSource`] owns and the
/// [`CompositeSecretSource`] dispatches on (DR-030 §Decision 2).
pub(crate) const OP_SCHEME: &str = "op://";

/// The three DISTINGUISHING op degrade-floor reasons (DR-030 §Decision 5, I6). Each
/// is a LABEL — it never carries the resolved value or the service-account token.
const OP_REASON_UNAVAILABLE: &str = "op backend UNAVAILABLE — the `op` CLI could not be exec'd (not on PATH / \
     absent binary); op:// ref dropped, host mediated-without-injection";
const OP_REASON_AUTH_FAIL: &str = "op backend AUTH FAILURE — `op read` refused auth (OP_SERVICE_ACCOUNT_TOKEN \
     unset/invalid); op:// ref dropped, host mediated-without-injection";
const OP_REASON_RESOLUTION_FAIL: &str = "op backend RESOLUTION FAILURE — `op read` exited nonzero (item/field not \
     found, no network, not authorized); op:// ref dropped, host \
     mediated-without-injection";

/// The 1Password `op`-CLI secret-resolution backend (DR-030 §Decision 1): for an
/// `op://vault/item/field` `secret_ref`, EXEC `<op-bin> read op://vault/item/field`
/// (`std::process`, exec'd NOT linked — I7; the pasta/bwrap/git precedent, NO
/// 1Password SDK crate), capture stdout, TRIM the trailing newline `op` emits, and
/// return it as a redacted [`BrokeredSecret`]. The service-account token reaches
/// `op` via the `OP_SERVICE_ACCOUNT_TOKEN` env ONLY — never argv, never a log, never
/// a fact (DR-030 §Decision 3).
///
/// Honest degrade (the pasta/bwrap availability-probe pattern — a missing tool is a
/// VERDICT, not a crash, DR-025): a spawn error (op absent) ⇒ UNAVAILABLE; `op read`
/// nonzero exit (auth refused with the token unset, or item/field not found / no
/// network) ⇒ a DROP. Each DROP carries a DISTINGUISHING [`SecretResolution`] reason
/// via [`SecretSource::resolve_with_reason`]; the plain [`SecretSource::resolve`]
/// collapses every floor to the `Ok(None)` DROP signal (the DR-029 contract).
pub struct OpSecretSource {
    /// The `op` binary to exec (`"op"` on PATH for the daemon path; an injected
    /// path pointing at the fake `op` for the host tests). Exec'd, never linked.
    binary: PathBuf,
    /// The env passed to the child `op` — carries `OP_SERVICE_ACCOUNT_TOKEN` (the
    /// daemon's token in production; injected by the host tests so they never touch
    /// the shared-process env). NEVER logged; the token is env-only (DR-030
    /// §Decision 3).
    child_env: Vec<(String, String)>,
}

impl OpSecretSource {
    /// The daemon path: default `op` binary (resolved on PATH), reading the real
    /// `OP_SERVICE_ACCOUNT_TOKEN` from the daemon's env so the child `op` auths.
    /// The token is captured here (env-only) and passed to the child; it is never
    /// logged/factored. Absent/empty token ⇒ the child `op` refuses ⇒ an honest
    /// auth-fail DROP (never a fake secret).
    pub fn new() -> Self {
        let mut child_env = Vec::new();
        if let Some(token) = std::env::var_os("OP_SERVICE_ACCOUNT_TOKEN") {
            child_env.push((
                "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
                token.to_string_lossy().into_owned(),
            ));
        }
        Self {
            binary: PathBuf::from("op"),
            child_env,
        }
    }

    /// Point this source at an INJECTED op-binary path, chainably (the host-test
    /// seam — `OpSecretSource::new().with_binary(<fake op>)` points the source at the
    /// compiled fake `op`, composing with `new()`'s real env-token read; no live
    /// 1Password). Inherent to `OpSecretSource` — `op` is the only backend with a
    /// binary to exec (DR-030 §Decision 1); the DR-029 `SecretSource` seam carries no
    /// `with_binary`. Carries NO secret.
    pub fn with_binary(mut self, op_binary: impl Into<PathBuf>) -> Self {
        self.binary = op_binary.into();
        self
    }

    /// Inject the child env the source hands to `op` (in production the daemon's own
    /// env; in host tests the token + fake-op knobs, so the test never mutates the
    /// shared-process env). Named to match the host-suite seam.
    pub fn with_child_env(mut self, child_env: Vec<(String, String)>) -> Self {
        self.child_env = child_env;
        self
    }

    /// Exec `<bin> read <secret_ref>` with the token env, returning the resolution
    /// verdict. NEVER logs the value or the token; NEVER `.expose()`s.
    fn resolve_op(&self, secret_ref: &str) -> SecretResolution {
        // Build the op-shaped argv: `read <op://ref>`. The ref is a NAME, not a
        // secret — safe on argv.
        let mut cmd = Command::new(&self.binary);
        cmd.arg("read").arg(secret_ref);
        // The source's `child_env` is the AUTHORITATIVE token source (DR-030
        // §Decision 3): `new()` reads the daemon's real OP_SERVICE_ACCOUNT_TOKEN
        // into it; tests inject it explicitly. The child otherwise inherits the
        // process env (so `op` finds PATH/HOME to run), but we first STRIP any
        // ambient token/knobs so a token that is NOT in `child_env` cannot leak in
        // from the process env — the auth floor must be decided by the source's own
        // inputs, not a stray inherited var (isolation-safe; matches the "token
        // env-only, sourced by the daemon" contract).
        cmd.env_remove("OP_SERVICE_ACCOUNT_TOKEN");
        // Token via ENV ONLY (DR-030 §Decision 3) — never argv/log.
        cmd.envs(
            self.child_env
                .iter()
                .map(|(k, v)| (OsStr::new(k), OsStr::new(v))),
        );

        let output = match cmd.output() {
            Ok(output) => output,
            // Spawn error ⇒ op is UNAVAILABLE (not on PATH / absent binary) — a
            // VERDICT, not a crash (DR-025 precedent). The io error string is a
            // syscall/path message; it carries NO token (env-only) and NO value
            // (nothing resolved). We do NOT fold it into the reason to keep the
            // reason a stable per-floor label.
            Err(_) => {
                return SecretResolution::Dropped {
                    reason: OP_REASON_UNAVAILABLE.to_string(),
                };
            }
        };

        if output.status.success() {
            // Success: stdout is the value. Trim the trailing newline `op read`
            // emits (a stray newline would corrupt an injected Authorization
            // header, DR-030 §Decision 1). The value lives ONLY inside the redacted
            // BrokeredSecret — never logged, never `.expose()`d here.
            let value = String::from_utf8_lossy(&output.stdout);
            let value = value.strip_suffix('\n').unwrap_or(&value);
            let value = value.strip_suffix('\r').unwrap_or(value);
            SecretResolution::Resolved(BrokeredSecret::new(secret_ref, value))
        } else {
            // Nonzero exit. The fake op (and real `op`) exits with a distinct code
            // for the auth floor (no token) vs the resolution floor (item/field not
            // found / no network). Real `op` uses exit 1 for auth; our fake uses 1
            // for auth and 3 for resolution. We DISTINGUISH the auth floor by exit
            // code 1, everything else nonzero ⇒ resolution fail. The token/value
            // never touch the reason (env-only, DR-030 §Decision 3).
            let reason = if output.status.code() == Some(1) {
                OP_REASON_AUTH_FAIL
            } else {
                OP_REASON_RESOLUTION_FAIL
            };
            SecretResolution::Dropped {
                reason: reason.to_string(),
            }
        }
    }
}

impl Default for OpSecretSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretSource for OpSecretSource {
    fn resolve(&self, secret_ref: &str) -> Result<Option<BrokeredSecret>, RunError> {
        // Collapse every floor to the Ok(None) DROP signal (the DR-029 contract);
        // the distinguishing reason rides `resolve_with_reason`.
        Ok(match self.resolve_op(secret_ref) {
            SecretResolution::Resolved(secret) => Some(secret),
            SecretResolution::Dropped { .. } => None,
        })
    }

    fn resolve_with_reason(&self, secret_ref: &str) -> Result<SecretResolution, RunError> {
        Ok(self.resolve_op(secret_ref))
    }
}

/// The scheme-dispatch composite (DR-030 §Decision 2): routes a `secret_ref` to the
/// backend that owns its scheme — an `op://…` ref → the `op` arm; a plain label →
/// the host-file arm. NO fallthrough: a DROP on one arm is an HONEST verdict, not a
/// "try the next" (a fallthrough could serve the WRONG secret from the other
/// backend). Both backends coexist so one project's mixed `[egress.secrets]`
/// resolves in one fold.
pub struct CompositeSecretSource {
    op_arm: Box<dyn SecretSource>,
    host_file_arm: Box<dyn SecretSource>,
}

impl CompositeSecretSource {
    /// Compose an `op` arm (owns the `op://` scheme) and a host-file arm (owns plain
    /// labels). Each arm is any [`SecretSource`] so the dispatch is observable/
    /// testable (the host suites inject recording backends).
    pub fn new(op_arm: Box<dyn SecretSource>, host_file_arm: Box<dyn SecretSource>) -> Self {
        Self {
            op_arm,
            host_file_arm,
        }
    }

    /// The arm that owns `secret_ref`'s scheme — the SOLE dispatch point (used by
    /// both `resolve` and `resolve_with_reason` so routing is identical).
    fn arm_for(&self, secret_ref: &str) -> &dyn SecretSource {
        if secret_ref.starts_with(OP_SCHEME) {
            self.op_arm.as_ref()
        } else {
            self.host_file_arm.as_ref()
        }
    }
}

impl SecretSource for CompositeSecretSource {
    fn resolve(&self, secret_ref: &str) -> Result<Option<BrokeredSecret>, RunError> {
        // Dispatch by scheme; NO fallthrough — a DROP stays a DROP on the owning arm.
        self.arm_for(secret_ref).resolve(secret_ref)
    }

    fn resolve_with_reason(&self, secret_ref: &str) -> Result<SecretResolution, RunError> {
        // Same scheme-dispatch, threading the owning arm's per-floor reason.
        self.arm_for(secret_ref).resolve_with_reason(secret_ref)
    }
}
