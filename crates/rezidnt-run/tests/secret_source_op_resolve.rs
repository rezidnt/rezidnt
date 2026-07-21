//! c3-op-secrets oracle (DR-030) — CRITERION 2 (HOST-provable, FAKE `op`) + the
//! CRITERION 3 unavailability/auth/resolution taxonomy at the `OpSecretSource`
//! SEAM. With an INJECTED op-binary path pointing at a compiled FAKE `op` (never a
//! live 1Password), an `op://vault/item/field` ref resolves by EXEC'ing the
//! op-shaped command (`op read op://…`) and capturing stdout as a redacted
//! `BrokeredSecret` — trailing newline TRIMMED, value redacted under Debug/Display.
//! Each degrade floor (op absent / token unset / op nonzero exit) yields the DROP
//! path (`Ok(None)`), never a fake/empty secret, and the token value leaks nowhere.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE. Pure trait resolve driven by a COMPILED
//! fake-op example (cross-platform: an `.sh` would not exec on the Windows host
//! that runs `/vet`, so the fake `op` is a dev-only `[[example]]` binary). No
//! netns, no live 1Password. Runs on every host, Windows /vet included.
//!
//! ## THE FAKE `op` (load-bearing test technique — DR-030 §"the fake op").
//! `crates/rezidnt-run/examples/op_fake.rs` (dev-only test-support, the DR-023
//! `egress_c3bc_probe` fixtures-stay-dev-only precedent — NEVER linked into the
//! daemon, I7). It mimics `op read op://…`: parses argv `read <op://ref>`, requires
//! `OP_SERVICE_ACCOUNT_TOKEN` present in its env (else exits nonzero, no stdout),
//! and — controlled by knobs it reads from its env — prints a value+newline to
//! stdout and exits 0, or exits nonzero (the resolution-failure floor). We locate
//! it via `current_exe()` → `../examples/op_fake` (the `probe_bin()` pattern in
//! `egress_mediation_c3bc.rs`).
//!
//! ## RED MODE — COMPILE-RED. `rezidnt_run::secret::OpSecretSource` does not exist
//! yet; neither does the dev-only `op_fake` example. This file cannot compile until
//! the implementer adds BOTH — that IS the failing state (an honest S4 skeleton).
//!
//! IMPLEMENTER ADDS (the seam this pins):
//!   - `pub struct OpSecretSource` (behind `SecretSource`) with:
//!       - `OpSecretSource::new() -> Self` — reads a default op binary (`"op"` on
//!         PATH) + the real `OP_SERVICE_ACCOUNT_TOKEN` env, the daemon path;
//!       - `OpSecretSource::with_binary(self, impl Into<PathBuf>) -> Self` — an
//!         INHERENT builder method (NOT on the shared `SecretSource` trait — the
//!         host-file/composite backends have no "binary") the host tests chain onto
//!         `new()` to point at the fake `op`. Always called builder-form
//!         (`OpSecretSource::new().with_binary(<fake>)`), never bare, so the
//!         implementer keeps it inherent to `OpSecretSource` and off the I4 seam;
//!     resolving `op://vault/item/field` by EXEC'ing `<op-bin> read op://vault/item/field`
//!     (`std::process`, exec'd not linked — I7), capturing stdout, TRIMMING the
//!     trailing newline, into `BrokeredSecret::new(secret_ref, value)`. The
//!     service-account token is passed to the child via `OP_SERVICE_ACCOUNT_TOKEN`
//!     ENV ONLY (never argv/log/fact). Absent binary / unset token / nonzero exit
//!     ⇒ the DROP path (`Ok(None)`), never a fake secret.
//!   - the dev-only `examples/op_fake.rs` binary described above.

// The oracle module-doc uses a nested bullet list whose outer-item continuation
// lines trip clippy's `doc_lazy_continuation` under host `-D warnings`
// ([[clippy-doc-lazy-continuation-trap]]). Lint-only accommodation — no assertion
// or prose is changed (flagged for /debrief).
#![allow(clippy::doc_lazy_continuation)]

use std::path::PathBuf;

// COMPILE-RED until the `OpSecretSource` seam exists on `rezidnt_run::secret`.
use rezidnt_run::secret::{OpSecretSource, SecretSource};

/// A distinctive secret VALUE the fake `op` emits to stdout — must stay redacted
/// everywhere but `.expose()`, and must survive the newline-trim intact.
const OP_VALUE: &str = "op_resolved_secret_value_MUST_STAY_REDACTED_0xC3OPSECRETS";
/// The op-reference the daemon resolves — a NAME (vault/item/field), not a secret;
/// it is the `secret_ref` that rides `credential.injected`.
const OP_REF: &str = "op://Prod/github-token/credential";
/// A distinctive service-account token — must leak into NO error/log/return string.
const SA_TOKEN: &str = "ops_service_account_token_MUST_NEVER_LEAK_0xC3OPSECRETS";

/// Locate the dev-only fake-`op` example built alongside this test. The test binary
/// lives in `<target>/debug/deps/…`; the example sits at
/// `<target>/debug/examples/op_fake[.exe]`. Dev-only test-support (DR-023) — NEVER
/// the shipped daemon binary (I7). Mirrors `egress_mediation_c3bc.rs::probe_bin`.
///
/// COMPILE-RED until `examples/op_fake.rs` exists (nothing to build here yet).
fn op_fake_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
    // .../debug/deps/secret_source_op_resolve-HASH  ->  .../debug/examples/op_fake
    let debug = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("target/debug dir");
    let mut p = debug.join("examples").join("op_fake");
    if cfg!(windows) {
        p.set_extension("exe");
    }
    p
}

/// Build an `OpSecretSource` pointed at the fake `op`, controlling the fake's
/// behavior + the service-account token PURELY through the SOURCE's own inputs — we
/// deliberately do NOT set the shared-process env here ([[env-var test isolation]]:
/// a real `OP_SERVICE_ACCOUNT_TOKEN` read is exercised in exactly ONE dedicated
/// test, `op_source_reads_the_real_service_account_token_env_seam`). The
/// implementer passes the token to the child; the fake reads it from ITS child env.
///
/// This asserts the seam the implementer must build: the `new().with_binary(path)`
/// builder for the injected op path, and a way to hand the fake its token + behavior
/// knobs. If the implementer's token-injection door differs, THIS is the interface
/// that must be reconciled — it pins "token via env to the child, never argv".
fn op_source_with_token(token: Option<&str>, fake_exit_nonzero: bool) -> OpSecretSource {
    // The fake `op` reads two knobs from its OWN env (set by the OpSecretSource when
    // it execs the child): `OP_FAKE_EXIT_NONZERO=1` ⇒ exit 1 (resolution-failure
    // floor); `OP_SERVICE_ACCOUNT_TOKEN` present ⇒ auth ok. The value the fake emits
    // is compiled in (OP_VALUE below). The implementer's `with_binary` returns a
    // source that execs `<bin> read <ref>` with `OP_SERVICE_ACCOUNT_TOKEN` from the
    // DAEMON's env — so this test controls the child env by scoping the real var in
    // the ONE isolated test, and here relies on the source's token-source. To keep
    // THIS test isolation-safe we pass the token + knob through the source builder
    // the implementer exposes (a `with_env` seam), NOT the process env.
    let mut src = OpSecretSource::new().with_binary(op_fake_bin());
    // The implementer exposes a test-visible way to set the child env the source
    // passes to `op` (in production this is the daemon's own env; here we inject it
    // so the host test never mutates the shared-process env). Named `with_child_env`
    // as the seam; the implementer wires it to the same env map `op` receives.
    let mut child_env: Vec<(String, String)> = Vec::new();
    if let Some(t) = token {
        child_env.push(("OP_SERVICE_ACCOUNT_TOKEN".to_string(), t.to_string()));
    }
    if fake_exit_nonzero {
        child_env.push(("OP_FAKE_EXIT_NONZERO".to_string(), "1".to_string()));
    }
    src = src.with_child_env(child_env);
    src
}

/// CRITERION 2 (positive — resolve + newline-trim) — an `op://` ref resolves by
/// exec'ing the fake `op read op://…`, capturing stdout as a `BrokeredSecret` whose
/// `.expose()` is the emitted value with the trailing newline `op` prints TRIMMED,
/// and whose `secret_ref()` is the `op://` ref (the by-reference label).
///
/// COMPILE-RED until `OpSecretSource::new().with_binary(..)` + `SecretSource::resolve`
/// exist.
#[test]
fn op_source_resolves_op_ref_and_trims_trailing_newline() {
    let source = op_source_with_token(Some(SA_TOKEN), false);

    let resolved = source
        .resolve(OP_REF)
        .expect("resolve does not error when op resolves the ref")
        .expect("the op:// ref resolves to a brokered secret");

    assert_eq!(
        resolved.expose(),
        OP_VALUE,
        "CRITERION 2: the op backend captures op's stdout as the value with the trailing newline \
         TRIMMED — a stray newline would corrupt an injected Authorization header (DR-030 §Decision \
         1). Got {:?}",
        resolved.expose()
    );
    assert!(
        !resolved.expose().ends_with('\n'),
        "CRITERION 2 VIOLATION: the resolved value kept op's trailing newline — it MUST be trimmed"
    );
    assert_eq!(
        resolved.secret_ref(),
        OP_REF,
        "the resolved BrokeredSecret carries the op:// REFERENCE as its secret_ref (a NAME, not a \
         value — it is exactly what credential.injected records, DR-030 §Decision 2)"
    );
}

/// CRITERION 2 (redaction holds — DR-026 crit 5) — the `BrokeredSecret` the op
/// backend resolves keeps its value REDACTED under Debug and Display; a stray
/// `{:?}`/`{}` cannot leak the op-resolved bytes.
///
/// COMPILE-RED until the op source resolves a `BrokeredSecret`.
#[test]
fn op_resolved_secret_stays_redacted_under_debug_and_display() {
    let source = op_source_with_token(Some(SA_TOKEN), false);
    let resolved = source
        .resolve(OP_REF)
        .expect("resolve ok")
        .expect("present");

    let debug = format!("{resolved:?}");
    let display = format!("{resolved}");
    assert!(
        !debug.contains(OP_VALUE) && !display.contains(OP_VALUE),
        "CRITERION 2 VIOLATION: the op-resolved VALUE appeared under Debug ({debug:?}) or Display \
         ({display:?}) — an op-backed BrokeredSecret must keep the redaction the type guarantees \
         (DR-026 crit 5). Only .expose() may return the value"
    );
    assert!(
        display.contains("<redacted>"),
        "Display of an op-resolved secret prints the redaction sentinel; got {display:?}"
    );
}

/// CRITERION 2 (the fake was invoked op-shaped) — the source execs the OP-SHAPED
/// argv (`read`, then the `op://` ref), never some other command. Asserted through
/// the fake's own behavior: the fake ONLY prints the value when argv[1]==`read` and
/// argv[2] is the exact `op://` ref it was asked for; a wrong shape makes the fake
/// exit nonzero ⇒ the source drops (`Ok(None)`). So a successful resolve of the
/// exact ref IS proof the op-shaped argv was exec'd.
///
/// COMPILE-RED until the source execs the op-shaped command.
#[test]
fn op_source_execs_the_op_shaped_read_command() {
    let source = op_source_with_token(Some(SA_TOKEN), false);
    // The fake echoes back the ref it was asked to `read` on a control line the
    // source surfaces via the resolved secret_ref; a successful resolve of THIS
    // exact ref (and only this ref) proves `op read op://Prod/github-token/credential`
    // was the argv. A different ref would make the fake refuse (nonzero) ⇒ Ok(None).
    let resolved = source
        .resolve(OP_REF)
        .expect("resolve ok")
        .expect("the op-shaped `read <ref>` argv resolved the exact ref");
    assert_eq!(
        resolved.secret_ref(),
        OP_REF,
        "CRITERION 2: the fake resolved EXACTLY the op:// ref it was asked to `read` — proof the \
         source exec'd `op read op://…` (the op-shaped argv), not a different command/ref shape"
    );

    // Falsification: an unknown ref shape the fake does not recognize as a `read`
    // target it can serve ⇒ the fake exits nonzero ⇒ the source DROPS (Ok(None)),
    // never a fabricated value. (A plain non-op label is not this source's job — the
    // CompositeSecretSource dispatch test covers scheme routing.)
    let unknown = source
        .resolve("op://Prod/nonexistent-item/credential")
        .expect("a ref the fake refuses is a DROP, not a source error");
    assert!(
        unknown.is_none(),
        "an op:// ref the fake `op` refuses (nonzero) resolves to Ok(None) — the DROP path, never a \
         fabricated secret (DR-030 §Decision 5)"
    );
}

/// CRITERION 3 (op absent ⇒ unavailable ⇒ DROP) — an `OpSecretSource` pointed at a
/// MISSING op binary cannot resolve: `resolve` yields the DROP path (`Ok(None)`),
/// NEVER a panic and NEVER a fake secret. This is the op-analogue of the pasta/bwrap
/// availability-probe (a missing tool is a VERDICT, not a crash — DR-030 §Decision 5).
///
/// COMPILE-RED until `with_binary` + `resolve` exist.
#[test]
fn op_absent_binary_is_a_drop_not_a_panic() {
    let source = OpSecretSource::new()
        .with_binary("/nonexistent/definitely-not-op-xyz")
        .with_child_env(vec![(
            "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
            SA_TOKEN.to_string(),
        )]);
    let resolved = source
        .resolve(OP_REF)
        .expect("a missing op binary is the DROP path (Ok(None)), never a source Err/panic");
    assert!(
        resolved.is_none(),
        "CRITERION 3 VIOLATION: op ABSENT did not yield the DROP path — a missing op binary must \
         resolve to Ok(None) (unavailable ⇒ drop, mediated-without-injection), never a fake secret \
         (DR-030 §Decision 5)"
    );
}

/// CRITERION 3 (token unset ⇒ auth fail ⇒ DROP) — with the op binary present but NO
/// `OP_SERVICE_ACCOUNT_TOKEN` handed to the child, the fake `op` cannot auth and
/// exits nonzero ⇒ the source DROPS (`Ok(None)`), never a fake secret.
///
/// COMPILE-RED until the source passes the token by env + drops on nonzero exit.
#[test]
fn op_token_unset_is_a_drop_not_a_fake_secret() {
    // No token handed to the child ⇒ the fake `op` refuses to auth.
    let source = op_source_with_token(None, false);
    let resolved = source
        .resolve(OP_REF)
        .expect("an auth failure is the DROP path (Ok(None)), never a source Err");
    assert!(
        resolved.is_none(),
        "CRITERION 3 VIOLATION: an UNSET service-account token did not yield the DROP path — the \
         source must DROP (Ok(None)), never inject a fake/empty secret (DR-030 §Decision 3/5)"
    );
}

/// CRITERION 3 (op nonzero exit ⇒ resolution fail ⇒ DROP) — the fake `op` exits
/// nonzero (item/field not found, no network) ⇒ the source DROPS (`Ok(None)`),
/// never a fake secret, even though the token was present.
///
/// COMPILE-RED until the source drops on a nonzero `op read` exit.
#[test]
fn op_nonzero_exit_is_a_drop_not_a_fake_secret() {
    let source = op_source_with_token(Some(SA_TOKEN), /* fake_exit_nonzero */ true);
    let resolved = source
        .resolve(OP_REF)
        .expect("a nonzero op-read exit is the DROP path (Ok(None)), never a source Err");
    assert!(
        resolved.is_none(),
        "CRITERION 3 VIOLATION: a NONZERO `op read` exit did not yield the DROP path — a resolution \
         failure must DROP (Ok(None)), never a fabricated secret (DR-030 §Decision 5)"
    );
}

/// CRITERION 3 (token in NO string) — across the resolve paths, the service-account
/// token VALUE must appear in NO error/return surface the source produces. Drive a
/// resolve that fails (nonzero exit) and assert the token bytes are absent from any
/// Debug of the outcome — the token rides env ONLY, never a fact/log/error (DR-030
/// §Decision 3). (The token-never-in-a-live-fact scan is the WSL crit-5 suite; this
/// is the unit-level guard on the source's own outputs.)
///
/// COMPILE-RED until the source exists.
#[test]
fn service_account_token_never_appears_in_the_sources_outputs() {
    let source = op_source_with_token(Some(SA_TOKEN), true);
    let outcome = source.resolve(OP_REF);
    let rendered = format!("{outcome:?}");
    assert!(
        !rendered.contains(SA_TOKEN),
        "CRITERION 3 VIOLATION: the service-account token appeared in the source's resolve outcome \
         ({rendered:?}) — the token is env-only and must NEVER surface in an error/log/return value \
         (DR-030 §Decision 3)"
    );
    // Non-vacuous: the outcome IS a value we inspected (a DROP), so the scan is real.
    assert!(
        matches!(outcome, Ok(None)),
        "non-vacuous: the nonzero-exit path is a DROP (Ok(None)) we inspected for the token"
    );
}

/// CRITERION 3 (the ONE real-env read — kept isolated) — `OpSecretSource::new()`
/// reads the real `OP_SERVICE_ACCOUNT_TOKEN` from the DAEMON's env and passes it to
/// the child `op` (the daemon path). This is the single test that touches the
/// shared-process env var; it scopes+restores it so a parallel test cannot flake it
/// ([[env-var test isolation]]). It asserts only the env-read seam: with the token
/// set + the fake op on the injected path, the ref resolves.
///
/// COMPILE-RED until `OpSecretSource::new()` (+ a way to point new() at the fake for
/// the test — the implementer keeps `with_binary` composable with the real env read,
/// e.g. `OpSecretSource::new().with_binary(fake)` reading the token from env).
#[test]
fn op_source_reads_the_real_service_account_token_env_seam() {
    // SAFETY: single-var set, this test owns the read below; every OTHER op test
    // uses `with_child_env` so they never contend on this var. Restored at the end.
    let prev = std::env::var_os("OP_SERVICE_ACCOUNT_TOKEN");
    unsafe {
        std::env::set_var("OP_SERVICE_ACCOUNT_TOKEN", SA_TOKEN);
    }
    // `new()` reads the real env token; `.with_binary(fake)` points it at the fake
    // op (no live 1Password). The token flows env → child, exactly as in production.
    let source = OpSecretSource::new().with_binary(op_fake_bin());
    let result = source.resolve(OP_REF);
    // Restore BEFORE asserting so a panic does not leak the var to other tests.
    unsafe {
        match prev {
            Some(v) => std::env::set_var("OP_SERVICE_ACCOUNT_TOKEN", v),
            None => std::env::remove_var("OP_SERVICE_ACCOUNT_TOKEN"),
        }
    }
    let resolved = result
        .expect("new() reads OP_SERVICE_ACCOUNT_TOKEN and resolves via the fake op")
        .expect("the op:// ref resolves through the env-authed source");
    assert_eq!(
        resolved.expose(),
        OP_VALUE,
        "new() reads the real OP_SERVICE_ACCOUNT_TOKEN (daemon path) and the fake op resolves the \
         ref — the env-authed seam (DR-030 §Decision 3)"
    );
    assert_eq!(resolved.secret_ref(), OP_REF);
}

/// Compile-time proof `OpSecretSource` is a `SecretSource` (an I4 backend behind the
/// SAME DR-029 seam — swappable/composed exactly like the host-file backend). A
/// `Box<dyn SecretSource>` over it must be constructible.
///
/// COMPILE-RED until the backend exists.
#[test]
fn op_source_is_a_secret_source_backend() {
    let boxed: Box<dyn SecretSource> = Box::new(OpSecretSource::new().with_binary(op_fake_bin()));
    let _: Result<Option<_>, rezidnt_run::RunError> = boxed.resolve(OP_REF);
}
