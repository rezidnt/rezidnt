//! SP3 oracle — determinism / replay of an exec permit policy (DR-015
//! §Decision 3, BINDING I6; design §4 / §8 crit 5). The compliance sentence for
//! the permit axis: an exec policy is content-pinned and re-executing the SAME
//! recorded §8 stdin against the SAME policy bytes yields the SAME verdict; a
//! divergence is a DR-006 integrity alarm, never silently reconciled.
//!
//! WHAT HAS AN HONEST JUDGE NOW vs WHAT IS DEFERRED
//! ------------------------------------------------
//! HONEST, PINNED RED HERE (through the SP3 seam, NOT the raw `ExecVerifier`):
//!   - the exec policy dispatched THROUGH THE PERMIT AGGREGATOR is DETERMINISTIC:
//!     the same request axis against the same reference policy produces the same
//!     permit outcome across runs (the property I6 rests on). Driving it through
//!     `permit::aggregate_async` (not `ExecVerifier::run` directly) is what makes
//!     this RED-until-SP3 rather than test theater — the raw exec runner already
//!     exists, so a direct-`run` determinism check would pass before SP3 and
//!     test nothing. The seam under test is the SP3 permit-exec dispatch.
//!
//! DEFERRED (implementer scope, `#[ignore]`-gated with the missing piece named):
//!   - the FULL debrief-replay-from-LOG re-execution for an EXEC verifier and
//!     its divergence alarm. Today `rezidnt_gate::replay` reports exec verifiers
//!     as `replayed: None` (v1 policy — the exec argv/policy is not on the v1
//!     verdict payload, so re-execution would be a guess, not evidence; see
//!     `crates/rezidnt-gate/tests/replay.rs`). SP3's `policy_ref` pins the
//!     policy bytes, which is the PREREQUISITE for exec replay — but threading
//!     the pinned policy back through `replay()` to re-execute the recorded
//!     stdin and raise an `IntegrityAlarm` on divergence is NEW machinery with
//!     no in-crate judge yet. The `#[ignore]`d test names exactly that gap.
//!
//! Unix-only: the reference policy is POSIX-sh.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use rezidnt_cas::Cas;
use rezidnt_gate::permit::{self, PermitDecision, PermitVerifierSpec};
use rezidnt_gate::{Verdict, VerifierInput};
use serde_json::json;

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

fn policy_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/policies")
        .join(name)
}

/// An EXEC permit entry for the reference policy — the SP3 API shape
/// (`PermitVerifierSpec::exec`). Dispatched as `sh <abspath>` (interpreter
/// explicit — the committed +x bit is not portable across a Windows checkout).
fn reference_policy_spec() -> PermitVerifierSpec {
    PermitVerifierSpec::exec(
        "reference-policy",
        vec![
            "/bin/sh".to_string(),
            policy_path("permit_tool_policy.sh").display().to_string(),
        ],
        json!({}),
    )
}

/// The pinned §8 stdin axis for a forced-breach request (tool `Bash`). This is
/// the replay preimage: recording it and re-dispatching the same policy against
/// it is the debrief-replay contract (I6).
fn forced_breach_input() -> VerifierInput {
    VerifierInput {
        gate: permit::LIFECYCLE_POINT.to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params: json!({ "tool": "Bash" }),
        timeout_ms: rezidnt_gate::DEFAULT_TIMEOUT_MS,
    }
}

/// CRITERION 5 (determinism, the property I6 rests on) — the exec policy
/// dispatched THROUGH THE PERMIT AGGREGATOR is deterministic: the same request
/// axis against the same policy yields the same permit outcome across runs. A
/// non-deterministic policy is a verifier bug; this is the judge that catches it.
///
/// COMPILE-RED (`aggregate_async` / `::exec` absent) then ASSERT-RED. Driven
/// through the SP3 seam so it cannot pass before SP3 exists (a raw
/// `ExecVerifier::run` determinism check would — that would be theater).
#[tokio::test]
async fn exec_permit_dispatch_is_deterministic_across_runs() {
    let (_dir, cas) = empty_cas();
    let input = forced_breach_input();
    let set = vec![reference_policy_spec()];

    let first = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");
    let second = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        first.decision,
        PermitDecision::Deny,
        "the exec policy denies the forced breach (tool Bash), deterministically (CRITERION 5)"
    );
    assert_eq!(
        first.decision, second.decision,
        "same request axis + same policy → same decision (I6 determinism through the seam, CRITERION 5)"
    );
    assert_eq!(
        first.verdict, second.verdict,
        "same aggregate verdict across runs (CRITERION 5)"
    );
    assert_eq!(
        first.verdict,
        Verdict::Fail,
        "the deciding exec verdict is Fail → Deny (CRITERION 5)"
    );
}

/// CRITERION 5 (the deciding exec policy is content-pinnable) — a deny through
/// the exec seam names the exec verifier as the decider and carries its evidence,
/// so the emit path can pin a `policy_ref` to the REAL external policy (the
/// prerequisite the full replay re-execution builds on; the live `policy_ref`
/// presence is pinned in `crates/rezidnt-mcp/tests/permit_exec_live.rs`).
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_deny_names_the_deciding_policy_for_pinning() {
    let (_dir, cas) = empty_cas();
    let set = vec![reference_policy_spec()];
    let outcome = permit::aggregate_async(&set, &forced_breach_input(), &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(outcome.decision, PermitDecision::Deny);
    assert_eq!(
        outcome.deciding_verifier, "reference-policy",
        "the deciding verifier is the EXTERNAL exec policy — the thing `policy_ref` pins (CRITERION 5)"
    );
    let ev = outcome
        .evidence
        .first()
        .unwrap_or_else(|| panic!("the exec deny carries its §8 evidence: {outcome:?}"));
    assert!(
        ev.msg.contains("Bash"),
        "the exec policy's evidence names the denied tool so the deny stays interrogable (I6): {ev:?}"
    );
}

/// CRITERION 5 (DEFERRED — full debrief replay re-execution + divergence alarm
/// for an EXEC permit verifier). `#[ignore]`d with the missing piece named:
/// `rezidnt_gate::replay` reports exec verifiers as `replayed: None` (v1 policy)
/// because the exec argv/policy is not recoverable from the v1 verdict payload.
/// SP3's `policy_ref` pins the policy bytes — the PREREQUISITE — but threading
/// that pinned policy back through `replay()` to re-execute the recorded §8
/// stdin and raise a DR-006 `IntegrityAlarm` on divergence is NEW machinery with
/// no in-crate judge yet. Un-ignore ONLY when the exec-replay wiring lands
/// (argv/policy_ref recoverable on the payload + `replay()` extended to re-run
/// exec verifiers). This body is NOT a passing stand-in — it fails loudly to
/// prevent the ignore from ever silently masking real (missing) coverage.
#[tokio::test]
#[ignore = "SP3-deferred: exec-verifier debrief-replay re-execution + IntegrityAlarm is not wired; \
            replay() reports exec as replayed:None (v1). policy_ref pins the policy (prerequisite) \
            but re-running the recorded §8 stdin through replay() has no in-crate judge yet. \
            Tracking: the exec-replay follow-on to SP3 dispatch."]
async fn exec_permit_replay_divergence_alarms_deferred() {
    panic!(
        "DEFERRED: no exec-verifier replay re-execution surface exists to assert against \
         (replay() reports exec as replayed:None). A recorded exec permit verdict re-executed \
         from the log over its pinned policy_ref should match (no alarm) or diverge (DR-006 alarm \
         naming the verifier + both verdicts) — that surface is not built. See the ignore note."
    );
}
