//! c3-op-secrets oracle (DR-030) — CRITERION 1 (HOST-provable): SCHEME-DISPATCH.
//! An `op://…` `secret_ref` routes to `OpSecretSource`; a plain label routes to
//! `HostFileSecretSource`; a `CompositeSecretSource` resolves BOTH in one project
//! (a mixed `[egress.secrets]`). Dispatch is BY SCHEME (the `op://` prefix), NOT by
//! trying both backends blindly — asserted by observing which backend each ref hits.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE (pure trait dispatch; no netns, no live op —
//! the op arm is driven by a recording fake below, not exec). Windows /vet included.
//!
//! ## RED MODE — COMPILE-RED. `rezidnt_run::secret::CompositeSecretSource` does not
//! exist yet. This file cannot compile until the implementer adds it — the failing
//! state (an honest S4 skeleton).
//!
//! IMPLEMENTER ADDS (the seam this pins):
//!   - `pub struct CompositeSecretSource` (behind `SecretSource`) that DISPATCHES BY
//!     SCHEME: a `secret_ref` starting with `op://` → an `OpSecretSource`; else → a
//!     `HostFileSecretSource`. Construction shape is the implementer's oracle-first
//!     call (e.g. `CompositeSecretSource::new(op, host_file)`); this file pins the
//!     DISPATCH property (which backend a given ref reaches), not the ctor bytes.
//!     Both backends coexist so one project's mixed `[egress.secrets]` resolves.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rezidnt_run::egress::BrokeredSecret;
use rezidnt_run::secret::SecretSource;
// COMPILE-RED until the composite dispatch backend exists.
use rezidnt_run::secret::CompositeSecretSource;

/// A recording `SecretSource` that counts the refs it was asked to resolve and
/// resolves exactly the ones it was seeded with. Two of these stand in for the op
/// backend and the host-file backend so the test OBSERVES which backend each ref
/// reaches (dispatch-by-scheme, not try-both-blindly). Its recorded calls are the
/// falsifier: an `op://` ref that reaches the host-file recorder (or vice-versa) is
/// a dispatch bug.
#[derive(Clone)]
struct RecordingSource {
    label: &'static str,
    resolves: Arc<Vec<(String, String)>>,
    seen: Arc<AtomicUsize>,
    last_ref: Arc<std::sync::Mutex<Option<String>>>,
}

impl RecordingSource {
    fn new(label: &'static str, resolves: &[(&str, &str)]) -> Self {
        Self {
            label,
            resolves: Arc::new(
                resolves
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            seen: Arc::new(AtomicUsize::new(0)),
            last_ref: Arc::new(std::sync::Mutex::new(None)),
        }
    }
    fn calls(&self) -> usize {
        self.seen.load(Ordering::SeqCst)
    }
    fn last(&self) -> Option<String> {
        self.last_ref.lock().unwrap().clone()
    }
    /// The backend-identity label (`"op"` / `"host_file"`) — asserted by the
    /// dispatch tests so a routed probe self-describes WHICH backend the scheme
    /// dispatched to (dispatch-observability, DR-030 §Decision 2). This makes
    /// `label` load-bearing rather than a dead field.
    fn label(&self) -> &'static str {
        self.label
    }
}

impl SecretSource for RecordingSource {
    fn resolve(&self, secret_ref: &str) -> Result<Option<BrokeredSecret>, rezidnt_run::RunError> {
        self.seen.fetch_add(1, Ordering::SeqCst);
        *self.last_ref.lock().unwrap() = Some(secret_ref.to_string());
        Ok(self
            .resolves
            .iter()
            .find(|(k, _)| k == secret_ref)
            .map(|(_, v)| BrokeredSecret::new(secret_ref, v)))
    }
}

/// Build a `CompositeSecretSource` from two recording backends so the test observes
/// the dispatch. The implementer's composite takes an op backend + a host-file
/// backend; to make dispatch OBSERVABLE (rather than needing a live op / a real
/// file), the composite must accept ANY `SecretSource` for each arm — a
/// `Box<dyn SecretSource>` per scheme. THIS pins that seam: the composite dispatches
/// by scheme to two injected backends. If the implementer hard-wires concrete
/// `OpSecretSource`/`HostFileSecretSource` with no injection point, this test names
/// the seam the composite must expose for its dispatch to be falsifiable.
fn composite(
    op_arm: RecordingSource,
    host_arm: RecordingSource,
) -> (CompositeSecretSource, RecordingSource, RecordingSource) {
    let op_probe = op_arm.clone();
    let host_probe = host_arm.clone();
    let comp = CompositeSecretSource::new(
        Box::new(op_arm) as Box<dyn SecretSource>,
        Box::new(host_arm) as Box<dyn SecretSource>,
    );
    (comp, op_probe, host_probe)
}

/// CRITERION 1 (op:// routes to the op backend ONLY) — an `op://…` ref reaches the
/// OP arm and NOT the host-file arm. Dispatch by scheme: the host-file backend is
/// never even consulted for an `op://` ref.
///
/// COMPILE-RED until `CompositeSecretSource` exists.
#[test]
fn op_scheme_ref_routes_to_the_op_backend_only() {
    let op_ref = "op://Prod/github-token/credential";
    let (comp, op_probe, host_probe) = composite(
        RecordingSource::new("op", &[(op_ref, "op_resolved_value_do_not_leak")]),
        RecordingSource::new("host-file", &[]),
    );

    let resolved = comp
        .resolve(op_ref)
        .expect("resolve ok")
        .expect("the op:// ref resolves via the op backend");
    assert_eq!(resolved.secret_ref(), op_ref);

    assert_eq!(
        op_probe.calls(),
        1,
        "CRITERION 1 VIOLATION: an op:// ref did NOT reach the op backend — dispatch must route \
         `op://…` to OpSecretSource (DR-030 §Decision 2)"
    );
    assert_eq!(op_probe.last().as_deref(), Some(op_ref));
    // The consulted probe self-describes as the OP backend — the op:// scheme routed
    // to the op-identity arm, making the dispatch assertion self-describing.
    assert_eq!(
        op_probe.label(),
        "op",
        "the backend that resolved the op:// ref is the OP-identity arm (scheme-routed)"
    );
    assert_eq!(
        host_probe.calls(),
        0,
        "CRITERION 1 VIOLATION: an op:// ref ALSO hit the host-file backend — dispatch is BY SCHEME, \
         not try-both-blindly; the host-file arm must not be consulted for an op:// ref (DR-030 \
         §Decision 2)"
    );
}

/// CRITERION 1 (a plain label routes to the host-file backend ONLY) — a plain
/// (non-`op://`) label reaches the HOST-FILE arm and NOT the op arm; the op backend
/// (which would exec `op`) is never invoked for a plain label.
///
/// COMPILE-RED until `CompositeSecretSource` exists.
#[test]
fn plain_label_routes_to_the_host_file_backend_only() {
    let plain = "gh_token";
    let (comp, op_probe, host_probe) = composite(
        RecordingSource::new("op", &[]),
        RecordingSource::new("host-file", &[(plain, "hostfile_value_do_not_leak")]),
    );

    let resolved = comp
        .resolve(plain)
        .expect("resolve ok")
        .expect("the plain label resolves via the host-file backend");
    assert_eq!(resolved.secret_ref(), plain);

    assert_eq!(
        host_probe.calls(),
        1,
        "CRITERION 1 VIOLATION: a plain label did NOT reach the host-file backend — dispatch must \
         route a non-op:// label to HostFileSecretSource (DR-030 §Decision 2)"
    );
    assert_eq!(host_probe.last().as_deref(), Some(plain));
    // The consulted probe self-describes as the HOST-FILE backend — a plain label
    // routed to the host-file-identity arm.
    assert_eq!(
        host_probe.label(),
        "host-file",
        "the backend that resolved the plain label is the HOST-FILE-identity arm (scheme-routed)"
    );
    assert_eq!(
        op_probe.calls(),
        0,
        "CRITERION 1 VIOLATION: a plain label hit the OP backend — a plain label must NEVER cause an \
         `op` exec; dispatch is by scheme (DR-030 §Decision 2)"
    );
}

/// CRITERION 1 (the centerpiece — a mixed project resolves BOTH) — a single
/// `CompositeSecretSource` resolves an `op://` ref AND a plain label in ONE run
/// (a mixed `[egress.secrets]`), each via its own backend. Both backends coexist;
/// one project can mix them (DR-030 §Decision 2).
///
/// COMPILE-RED until `CompositeSecretSource` exists.
#[test]
fn a_mixed_project_resolves_both_backends_in_one_composite() {
    let op_ref = "op://Prod/github-token/credential";
    let plain = "gitlab_token";
    let (comp, op_probe, host_probe) = composite(
        RecordingSource::new("op", &[(op_ref, "op_value_do_not_leak")]),
        RecordingSource::new("host-file", &[(plain, "hostfile_value_do_not_leak")]),
    );

    let op_secret = comp
        .resolve(op_ref)
        .expect("op ref resolves")
        .expect("op ref present");
    let plain_secret = comp
        .resolve(plain)
        .expect("plain label resolves")
        .expect("plain label present");

    assert_eq!(op_secret.secret_ref(), op_ref);
    assert_eq!(plain_secret.secret_ref(), plain);
    // Each backend saw EXACTLY its own ref — the mix dispatched cleanly.
    assert_eq!(
        (op_probe.calls(), host_probe.calls()),
        (1, 1),
        "CRITERION 1 VIOLATION: a mixed [egress.secrets] did not dispatch cleanly — the op:// ref \
         must reach the op backend and the plain label the host-file backend, each exactly once \
         (DR-030 §Decision 2). op-calls={}, host-calls={}",
        op_probe.calls(),
        host_probe.calls()
    );
    assert_eq!(op_probe.last().as_deref(), Some(op_ref));
    assert_eq!(host_probe.last().as_deref(), Some(plain));
    // Each ref reached its own backend-identity arm — the mix dispatched by scheme.
    assert_eq!(
        (op_probe.label(), host_probe.label()),
        ("op", "host-file"),
        "the op:// ref resolved via the OP arm and the plain label via the HOST-FILE arm — the mixed \
         [egress.secrets] dispatched by scheme, each ref to its identity backend (DR-030 §Decision 2)"
    );
}

/// CRITERION 1 (an unresolvable ref still DROPS through the right backend) — an
/// `op://` ref the op backend cannot resolve returns `Ok(None)` (the DROP signal)
/// WITHOUT falling through to the host-file backend. Dispatch-by-scheme means a
/// DROP on the op arm is a DROP, not a silent retry against the wrong backend
/// (which could resolve a same-named plain label to the wrong secret).
///
/// COMPILE-RED until `CompositeSecretSource` exists.
#[test]
fn an_unresolvable_op_ref_drops_without_falling_through_to_host_file() {
    let op_ref = "op://Prod/absent-item/credential";
    let (comp, op_probe, host_probe) = composite(
        RecordingSource::new("op", &[]), // op resolves nothing
        // A host-file arm that WOULD resolve the same string — proving no fallthrough.
        RecordingSource::new(
            "host-file",
            &[(op_ref, "WRONG_backend_must_not_serve_this")],
        ),
    );

    let resolved = comp
        .resolve(op_ref)
        .expect("an unresolvable op ref is a DROP, not an error");
    assert!(
        resolved.is_none(),
        "CRITERION 1 VIOLATION: an unresolvable op:// ref FELL THROUGH to the host-file backend — \
         dispatch is by scheme and a DROP on the op arm must stay a DROP, never a retry against the \
         wrong backend (DR-030 §Decision 2). A fallthrough could serve the WRONG secret"
    );
    assert_eq!(
        op_probe.calls(),
        1,
        "the op arm WAS consulted (it owns the op:// scheme)"
    );
    assert_eq!(
        host_probe.calls(),
        0,
        "the host-file arm was NEVER consulted for an op:// ref — no fallthrough"
    );
    // The arm that WAS consulted (and dropped) is the OP-identity arm — the drop
    // stayed on the op backend, never crossed to the host-file identity.
    assert_eq!(
        op_probe.label(),
        "op",
        "the DROP happened on the OP-identity arm (the op:// scheme's owner), never the host-file arm"
    );
}
