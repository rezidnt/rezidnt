//! c3-op-secrets oracle (DR-030) — CRITERION 3 (HOST-provable): the HONEST DEGRADE
//! TAXONOMY at the FOLD level. Three distinct op-backend floors — (a) op binary
//! ABSENT ⇒ unavailable; (b) `OP_SERVICE_ACCOUNT_TOKEN` UNSET ⇒ auth fail; (c) the
//! fake `op` exits NONZERO ⇒ resolution fail — EACH drives `fold_egress_policy` to
//! DROP the `op://` mapping (the host stays allowlisted, mediated-WITHOUT-injection)
//! with a `CredentialDrop` carrying a DISTINGUISHING reason, never a fake/empty
//! secret. The three reasons DIFFER, and the service-account token VALUE appears in
//! NO drop/reason/error string (DR-030 §Decision 3/5).
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE. Folds a `[egress]` spec through
//! `fold_egress_policy` with a `CompositeSecretSource` whose op arm points at the
//! compiled fake `op` (dev-only `examples/op_fake.rs` — the DR-023 precedent,
//! cross-platform so host /vet on Windows runs it). No netns, no live 1Password.
//!
//! ## RED MODE — COMPILE-RED (the `OpSecretSource`/`CompositeSecretSource` seams +
//! the fake `op` example do not exist yet) + BEHAVIOR-RED (the DISTINGUISHING drop
//! reason: `CredentialDrop`/`dropped_fact` today carry ONE hard-coded reason —
//! `egress.rs:475-485`; this slice must make the reason DISTINGUISH the three op
//! floors so "why dropped" is interrogable per floor, I6, DR-030 §Decision 5).
//!
//! IMPLEMENTER ADDS (the seams this pins):
//!   - `OpSecretSource` + `CompositeSecretSource` (see the two secret_source_op_*
//!     host suites for their full seam) and the dev-only `examples/op_fake.rs`.
//!   - a DISTINGUISHING drop reason: `CredentialDrop` gains a `reason()` (or the
//!     fold otherwise carries a per-floor reason) so an op-absent drop, a token-unset
//!     drop, and a nonzero-exit drop are TELLABLE APART (three different reason
//!     strings). The exact carrier is the implementer's oracle-first call; this file
//!     pins the property (three drops, three DISTINCT reasons), not the field bytes.

use std::path::PathBuf;

use rezidnt_run::egress::{CredentialDrop, fold_egress_policy};
use rezidnt_run::secret::{
    CompositeSecretSource, HostFileSecretSource, OpSecretSource, SecretSource,
};
use rezidnt_run::spec::ProjectSpec;

/// The op ref the spec declares for github.com — a NAME, never a value.
const OP_REF: &str = "op://Prod/github-token/credential";
/// The distinctive service-account token — must leak into NO drop/reason/error.
const SA_TOKEN: &str = "ops_service_account_token_MUST_NEVER_LEAK_0xC3TAXONOMY";

/// Locate the dev-only fake-`op` example (the `probe_bin()` pattern). COMPILE-RED
/// until `examples/op_fake.rs` exists.
fn op_fake_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
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

/// A spec declaring github.com allowlisted with an `op://` secret_ref — the folded
/// authority the op backend resolves (or drops).
fn spec_with_op_secret() -> ProjectSpec {
    ProjectSpec::from_toml_str(&format!(
        r#"
[project]
name = "acme"
repo = "."

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"

[egress]
allowlist = ["github.com"]

[egress.secrets]
"github.com" = "{OP_REF}"
"#
    ))
    .expect("spec with an op:// [egress.secrets] ref parses")
}

/// Build a `CompositeSecretSource` whose op arm points at `op_bin` with the given
/// child env (token + knobs), and whose host-file arm is empty (no plain labels in
/// this suite). Isolation-safe: env flows through the source, not the process env.
fn composite_op(op_bin: PathBuf, child_env: Vec<(String, String)>) -> CompositeSecretSource {
    let op = OpSecretSource::new()
        .with_binary(op_bin)
        .with_child_env(child_env);
    let host = HostFileSecretSource::from_path(None).expect("empty host-file source");
    CompositeSecretSource::new(Box::new(op) as Box<dyn SecretSource>, Box::new(host))
}

/// Fold the op-secret spec through the composite and return the drops. The host
/// stays allowlisted regardless (mediated-without-injection on a drop); this
/// helper focuses the assertions on the DROP + its reason.
fn fold_drops(source: &CompositeSecretSource) -> Vec<CredentialDrop> {
    let spec = spec_with_op_secret();
    let (policy, drops) =
        fold_egress_policy(&spec.egress, source).expect("the fold proceeds past an op drop");
    // A drop never removes the allowlist entry — the host stays mediated.
    assert!(
        policy.allows("github.com"),
        "an op DROP keeps the host allowlisted (mediated-without-injection), never removes it \
         (DR-030 §Decision 5)"
    );
    // ...and carries NO secret (never a fake/empty injection).
    assert!(
        policy.secret_for("github.com").is_none(),
        "an op DROP leaves NO secret mapped — never a fake/empty secret (DR-030 §Decision 5)"
    );
    drops
}

/// The reason string for the github.com op drop — the implementer's DISTINGUISHING
/// per-floor reason. COMPILE-RED against `CredentialDrop::reason()` until the
/// implementer adds the per-floor reason carrier.
fn drop_reason(drops: &[CredentialDrop]) -> String {
    let d = drops
        .iter()
        .find(|d| d.dest() == "github.com" && d.secret_ref() == OP_REF)
        .expect("the github.com op:// mapping was dropped (reported so the loud fact rides it)");
    // COMPILE-RED: `reason()` is the per-floor distinguishing reason the implementer adds.
    d.reason().to_string()
}

/// CRITERION 3 (op ABSENT ⇒ unavailable ⇒ drop) — folding an `op://` mapping with a
/// MISSING op binary DROPS the mapping (never a fake secret) with an
/// unavailability-flavored reason.
///
/// COMPILE-RED until the op/composite seams + the per-floor `reason()` exist.
#[test]
fn op_absent_floor_drops_with_an_unavailability_reason() {
    let source = composite_op(
        PathBuf::from("/nonexistent/definitely-not-op-xyz"),
        vec![("OP_SERVICE_ACCOUNT_TOKEN".to_string(), SA_TOKEN.to_string())],
    );
    let drops = fold_drops(&source);
    let reason = drop_reason(&drops);
    assert!(
        !reason.trim().is_empty(),
        "CRITERION 3: an op-absent drop carries a LOGGABLE reason (I6) — got empty"
    );
    assert!(
        !reason.contains(SA_TOKEN),
        "CRITERION 3 VIOLATION: the service-account token leaked into the drop reason ({reason:?}) \
         — the token is env-only, never in a fact/log (DR-030 §Decision 3)"
    );
}

/// CRITERION 3 (token UNSET ⇒ auth fail ⇒ drop) — folding with the op binary present
/// but NO service-account token DROPS the mapping with an auth-flavored reason.
///
/// COMPILE-RED until the seams + per-floor reason exist.
#[test]
fn op_token_unset_floor_drops_with_an_auth_reason() {
    // No OP_SERVICE_ACCOUNT_TOKEN handed to the child ⇒ the fake op refuses to auth.
    let source = composite_op(op_fake_bin(), vec![]);
    let drops = fold_drops(&source);
    let reason = drop_reason(&drops);
    assert!(
        !reason.trim().is_empty(),
        "CRITERION 3: a token-unset drop carries a LOGGABLE reason (I6)"
    );
    assert!(
        !reason.contains(SA_TOKEN),
        "the (absent) token value must not appear in the reason — {reason:?}"
    );
}

/// CRITERION 3 (op NONZERO exit ⇒ resolution fail ⇒ drop) — folding with the fake op
/// exiting nonzero (item/field not found, no network) DROPS the mapping with a
/// resolution-flavored reason, even though the token was present.
///
/// COMPILE-RED until the seams + per-floor reason exist.
#[test]
fn op_nonzero_exit_floor_drops_with_a_resolution_reason() {
    let source = composite_op(
        op_fake_bin(),
        vec![
            ("OP_SERVICE_ACCOUNT_TOKEN".to_string(), SA_TOKEN.to_string()),
            ("OP_FAKE_EXIT_NONZERO".to_string(), "1".to_string()),
        ],
    );
    let drops = fold_drops(&source);
    let reason = drop_reason(&drops);
    assert!(
        !reason.trim().is_empty(),
        "CRITERION 3: a resolution-failure drop carries a LOGGABLE reason (I6)"
    );
    assert!(
        !reason.contains(SA_TOKEN),
        "the token value must not appear in the reason — {reason:?}"
    );
}

/// CRITERION 3 (the centerpiece — the three reasons DISTINGUISH the floors) — the
/// op-absent, token-unset, and nonzero-exit drops carry THREE DIFFERENT reasons, so
/// "why was this credential dropped?" is answerable per floor (I6 interrogability,
/// DR-030 §Decision 5). A single hard-coded reason across all three (the DR-029
/// `dropped_fact` default) is BEHAVIOR-RED here.
///
/// COMPILE-RED until the seams + per-floor reason exist; BEHAVIOR-RED until the
/// reason distinguishes the floors.
#[test]
fn the_three_op_floors_carry_distinguishing_reasons() {
    let absent = composite_op(
        PathBuf::from("/nonexistent/definitely-not-op-xyz"),
        vec![("OP_SERVICE_ACCOUNT_TOKEN".to_string(), SA_TOKEN.to_string())],
    );
    let unset = composite_op(op_fake_bin(), vec![]);
    let nonzero = composite_op(
        op_fake_bin(),
        vec![
            ("OP_SERVICE_ACCOUNT_TOKEN".to_string(), SA_TOKEN.to_string()),
            ("OP_FAKE_EXIT_NONZERO".to_string(), "1".to_string()),
        ],
    );

    let r_absent = drop_reason(&fold_drops(&absent));
    let r_unset = drop_reason(&fold_drops(&unset));
    let r_nonzero = drop_reason(&fold_drops(&nonzero));

    assert_ne!(
        r_absent, r_unset,
        "CRITERION 3 VIOLATION: the op-ABSENT and token-UNSET drops carry the SAME reason \
         ({r_absent:?}) — the taxonomy must DISTINGUISH unavailable vs auth-fail (DR-030 §Decision 5)"
    );
    assert_ne!(
        r_unset, r_nonzero,
        "CRITERION 3 VIOLATION: the token-UNSET and nonzero-EXIT drops carry the SAME reason \
         ({r_unset:?}) — the taxonomy must DISTINGUISH auth-fail vs resolution-fail (DR-030 §Decision 5)"
    );
    assert_ne!(
        r_absent, r_nonzero,
        "CRITERION 3 VIOLATION: the op-ABSENT and nonzero-EXIT drops carry the SAME reason \
         ({r_absent:?}) — the taxonomy must DISTINGUISH unavailable vs resolution-fail (DR-030 \
         §Decision 5)"
    );

    // And NONE of the three reasons carries the service-account token value.
    for reason in [&r_absent, &r_unset, &r_nonzero] {
        assert!(
            !reason.contains(SA_TOKEN),
            "CRITERION 3 VIOLATION: the service-account token leaked into a drop reason ({reason:?})"
        );
    }
}
