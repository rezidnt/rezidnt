//! c3-egress-fold oracle (DR-029) — CRITERION 2 (HOST-provable): the `SecretSource`
//! I4 trait + its host-file MVP backend resolve a `secret_ref -> BrokeredSecret`.
//! Absent `REZIDNT_EGRESS_SECRETS` ⇒ an EMPTY source (nothing resolvable); a
//! set-but-missing/malformed file ⇒ an honest error (never a silently-empty source
//! that drops the boundary); an unresolvable `secret_ref` ⇒ `Ok(None)` (the fold
//! DROPS that mapping with a loud fact — never a fake/empty secret, DR-029 §Decision
//! 2/3). The resolved `BrokeredSecret` keeps its value redacted under Debug/Display.
//!
//! Mirrors the `REZIDNT_ADMIN_PERMIT` host-authority-file precedent
//! (`bins/rezidentd/src/main.rs:121-160`, DR-020): env-pointed, OUTSIDE any workspace
//! spec, a dev physically cannot self-grant.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure TOML parse + trait resolve; no netns).
//!
//! ## RED MODE — COMPILE-RED. There is no `SecretSource` trait and no
//! `HostFileSecretSource` on `rezidnt_run` yet. This file cannot compile until the
//! implementer adds the `secret` seam — that IS the failing state.
//!
//! IMPLEMENTER ADDS (the seam this pins):
//!   - `pub trait SecretSource { fn resolve(&self, secret_ref: &str)
//!         -> Result<Option<BrokeredSecret>, RunError>; }` (I4, daemon-side).
//!   - `pub struct HostFileSecretSource` with:
//!       - `HostFileSecretSource::from_path(Option<&Path>) -> Result<Self, RunError>`
//!         — the INJECTED-PATH ctor the unit tests drive (env-var-isolation-safe:
//!         `None` ⇒ empty source; a missing/malformed file ⇒ `RunError`); AND
//!       - `HostFileSecretSource::from_env() -> Result<Self, RunError>` — reads
//!         `REZIDNT_EGRESS_SECRETS` (the ONE test that touches the real env),
//!         mirroring `admin_permit_layer`'s env read.
//!   - the host TOML shape is `secret_ref = "value"` (a flat top-level table), so a
//!     `secret_ref` present in the file resolves to `BrokeredSecret::new(ref, value)`
//!     and one absent resolves to `Ok(None)` (the DROP-with-loud-fact signal).

use std::io::Write;

use rezidnt_run::RunError;
// COMPILE-RED until the `secret` seam exists on `rezidnt_run`.
use rezidnt_run::secret::{HostFileSecretSource, SecretSource};

/// A distinctive secret value that must stay redacted everywhere but `.expose()`.
const TOKEN_VALUE: &str = "ghp_hostfile_secret_value_MUST_STAY_REDACTED_0xC3EGRESS";

/// Write a host secrets TOML (`secret_ref = "value"`) to a temp file, returning the
/// tempfile handle (kept alive by the caller so the path stays valid).
fn write_secrets_file(body: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("secrets tempfile");
    f.write_all(body.as_bytes()).expect("write secrets file");
    f.flush().expect("flush secrets file");
    f
}

/// CRITERION 2 (positive) — the host-file backend resolves a `secret_ref` present in
/// the file into a `BrokeredSecret` carrying the value (reachable only via
/// `.expose()`), and its `secret_ref()` is the requested label. Uses the
/// INJECTED-PATH ctor so no shared-process env var is touched (test-isolation-safe).
///
/// COMPILE-RED until `HostFileSecretSource::from_path` + `SecretSource::resolve`
/// exist.
#[test]
fn host_file_backend_resolves_secret_ref_to_brokered_secret() {
    let file = write_secrets_file(&format!(
        "gh_token = \"{TOKEN_VALUE}\"\nother_ref = \"unrelated\"\n"
    ));
    let source = HostFileSecretSource::from_path(Some(file.path()))
        .expect("a well-formed secrets file loads");

    let resolved = source
        .resolve("gh_token")
        .expect("resolve does not error on a present ref")
        .expect("gh_token resolves to a brokered secret");

    assert_eq!(
        resolved.expose(),
        TOKEN_VALUE,
        "the host-file backend resolves the secret_ref to its VALUE (the ONE .expose() path)"
    );
    assert_eq!(
        resolved.secret_ref(),
        "gh_token",
        "the resolved BrokeredSecret carries the requested secret_ref LABEL"
    );
}

/// CRITERION 2 (redaction holds — leak-discipline, DR-026 crit 5) — the
/// `BrokeredSecret` the source resolves keeps its value REDACTED under Debug and
/// Display; a stray `{:?}`/`{}` cannot leak the resolved bytes.
///
/// COMPILE-RED until the source resolves a `BrokeredSecret`.
#[test]
fn resolved_secret_stays_redacted_under_debug_and_display() {
    let file = write_secrets_file(&format!("gh_token = \"{TOKEN_VALUE}\"\n"));
    let source = HostFileSecretSource::from_path(Some(file.path())).expect("secrets file loads");
    let resolved = source
        .resolve("gh_token")
        .expect("resolve ok")
        .expect("gh_token present");

    let debug = format!("{resolved:?}");
    let display = format!("{resolved}");
    assert!(
        !debug.contains(TOKEN_VALUE) && !display.contains(TOKEN_VALUE),
        "CRITERION 2 VIOLATION: the resolved secret VALUE appeared under Debug ({debug:?}) or \
         Display ({display:?}) — a source-resolved BrokeredSecret must keep the redaction the \
         type guarantees (DR-026 crit 5). Only .expose() may return the value"
    );
    assert!(
        display.contains("<redacted>"),
        "Display of a resolved secret prints the redaction sentinel; got {display:?}"
    );
}

/// CRITERION 2 (empty source, no env) — the INJECTED-PATH ctor with `None` is an
/// EMPTY source: nothing resolves (`Ok(None)` for any ref), never an error. This is
/// the `absent REZIDNT_EGRESS_SECRETS ⇒ empty source` semantics, exercised without
/// touching the shared-process env var (test-isolation-safe).
///
/// COMPILE-RED until `from_path(None)` exists.
#[test]
fn absent_path_is_an_empty_source_nothing_resolves() {
    let source = HostFileSecretSource::from_path(None).expect("an absent path is an empty source");
    assert!(
        source
            .resolve("gh_token")
            .expect("empty source does not error")
            .is_none(),
        "CRITERION 2: an empty source (no configured file) resolves NOTHING — Ok(None), never a \
         fabricated secret (DR-029 §Decision 3: absent env ⇒ empty source)"
    );
}

/// CRITERION 2 (set-but-missing file is an ERROR) — a configured path pointing at a
/// nonexistent file is an honest `RunError`, NEVER a silently-empty source that would
/// drop the boundary (the DR-020 admin-permit precedent: set-but-missing is a startup
/// error, never silently empty).
///
/// COMPILE-RED until `from_path(Some(..))` exists.
#[test]
fn set_but_missing_file_is_an_honest_error_never_silently_empty() {
    let missing = std::env::temp_dir().join("rezidnt-egress-secrets-does-not-exist-0xC3.toml");
    // Ensure it truly does not exist.
    let _ = std::fs::remove_file(&missing);
    match HostFileSecretSource::from_path(Some(&missing)) {
        Err(_) => {}
        Ok(_) => panic!(
            "CRITERION 2 VIOLATION: a configured-but-missing secrets file loaded SILENTLY as an \
             empty source — a set-but-missing file MUST be an honest error, never a silently-empty \
             source that drops the authority boundary (DR-029 §Decision 3; DR-020 precedent)"
        ),
    }
}

/// CRITERION 2 (malformed file is an ERROR) — a configured path pointing at a file
/// that is not valid TOML (`secret_ref = "value"`) is an honest `RunError`, never a
/// silently-empty source.
///
/// COMPILE-RED until `from_path` exists.
#[test]
fn malformed_file_is_an_honest_error() {
    let file = write_secrets_file("this is not = = valid toml [[[\n");
    match HostFileSecretSource::from_path(Some(file.path())) {
        Err(_) => {}
        Ok(_) => panic!(
            "CRITERION 2 VIOLATION: a malformed secrets file loaded SILENTLY — a malformed file \
             MUST be an honest error, never a silently-empty source (DR-029 §Decision 3)"
        ),
    }
}

/// CRITERION 2 (unresolvable ref ⇒ DROP, not a fake secret) — a `secret_ref` the
/// file does NOT hold resolves to `Ok(None)`: the fold DROPS that mapping (with a
/// loud `credential.dropped` fact — asserted in the facts suite), NEVER a fake/empty
/// secret, never an injection of the empty string (DR-029 §Decision 2).
///
/// COMPILE-RED until `resolve` exists.
#[test]
fn unresolvable_secret_ref_resolves_none_never_a_fake_secret() {
    let file = write_secrets_file(&format!("gh_token = \"{TOKEN_VALUE}\"\n"));
    let source = HostFileSecretSource::from_path(Some(file.path())).expect("secrets file loads");

    let missing = source
        .resolve("not_in_the_file")
        .expect("resolving an absent ref is not an error — it is a DROP signal");
    assert!(
        missing.is_none(),
        "CRITERION 2 VIOLATION: an unresolvable secret_ref returned a secret — it MUST resolve to \
         Ok(None) so the fold DROPS the mapping (never a fake/empty secret, DR-029 §Decision 2)"
    );
    // And the drop must never be a BrokeredSecret carrying the empty string.
    if let Some(fake) = missing {
        panic!(
            "an unresolvable secret_ref must be None, not a BrokeredSecret (got secret_ref={:?}) \
             — the empty-string injection DR-029 §Decision 2 forbids",
            fake.secret_ref()
        );
    }
}

/// CRITERION 2 (the ONE real-env read — kept isolated) — `from_env()` reads the real
/// `REZIDNT_EGRESS_SECRETS` var, mirroring `admin_permit_layer`. This is the single
/// test that touches the shared-process env var; it is deliberately isolated and
/// asserts only the env-read seam (a set env pointing at a good file resolves), so a
/// parallel test cannot flake it (it sets, resolves, and never assumes the var was
/// unset by the harness — [[env-var test isolation]]). All OTHER cases are covered
/// by `from_path` above.
///
/// COMPILE-RED until `from_env()` exists. `#[ignore]`-free: it self-sets the var.
#[test]
fn from_env_reads_the_real_env_var_seam() {
    let file = write_secrets_file(&format!("gh_token = \"{TOKEN_VALUE}\"\n"));
    // SAFETY: single-var set, this test owns the read below; the other cases use
    // from_path so they never contend on this var. Restored at the end.
    let prev = std::env::var_os("REZIDNT_EGRESS_SECRETS");
    unsafe {
        std::env::set_var("REZIDNT_EGRESS_SECRETS", file.path());
    }
    let result = HostFileSecretSource::from_env().and_then(|s| s.resolve("gh_token"));
    // Restore BEFORE asserting so a panic does not leak the var to other tests.
    unsafe {
        match prev {
            Some(v) => std::env::set_var("REZIDNT_EGRESS_SECRETS", v),
            None => std::env::remove_var("REZIDNT_EGRESS_SECRETS"),
        }
    }
    let resolved = result
        .expect("from_env reads the pointed file")
        .expect("gh_token resolves through the env-configured source");
    assert_eq!(
        resolved.expose(),
        TOKEN_VALUE,
        "from_env() reads REZIDNT_EGRESS_SECRETS and resolves the secret_ref (the DR-020 \
         admin-permit env-read precedent applied to secrets)"
    );
}

/// Compile-time proof the `SecretSource` trait is object-safe (daemon-side dispatch,
/// a swappable I4 backend — the `op`-CLI backend is the next impl behind the SAME
/// trait, DR-029 §Decision 4). A `Box<dyn SecretSource>` must be constructible.
///
/// COMPILE-RED until the trait + backend exist.
#[test]
fn secret_source_is_object_safe_for_daemon_dispatch() {
    let file = write_secrets_file(&format!("gh_token = \"{TOKEN_VALUE}\"\n"));
    let boxed: Box<dyn SecretSource> =
        Box::new(HostFileSecretSource::from_path(Some(file.path())).expect("loads"));
    let _: Result<Option<_>, RunError> = boxed.resolve("gh_token");
}
