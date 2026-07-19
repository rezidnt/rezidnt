//! SP3 oracle — policy-as-exec-verifier on the permit axis (DR-015 ACCEPTED;
//! design `docs/design/permit-exec-verifier-sp3.md` §7/§8). These pin the
//! GATE-CRATE half of SP3: the permit aggregator dispatching an EXEC entry
//! through the existing `ExecVerifier` (§8 stdin→stdout, network-off + scrubbed
//! env, nonzero/malformed/timeout → inconclusive), interleaved IN CONFIGURED
//! ORDER with natives so first-`Fail`→Deny short-circuits across both kinds
//! (DR-015 §Decision 1/2; sketch §3-A).
//!
//! ============================ THE DELIBERATE JUDGE =========================
//! The external policy is a TINY REFERENCE PROGRAM, NOT a vendored engine (I7,
//! DR-015 §Decision 4). It lives at `spec/fixtures/policies/permit_tool_policy.sh`
//! — a committed, few-line POSIX-sh argv that reads the §8 `VerifierInput` on
//! stdin and emits a §8 `VerifierOutput`: `params.tool == "Bash"` → deny
//! (`fail`), else allow (`pass`). It stands in for an OPA/Rego or Cedar policy
//! WITHOUT bundling either. The three never-coerce policies
//! (`permit_policy_nonzero_exit.sh` / `_malformed.sh` / `_slow.sh`) are the
//! §8 traps. No `opa`/`cedar` binary enters the build (asserted in
//! `permit_exec_no_vendored_engine.rs`).
//! ==========================================================================
//!
//! RED MODE (both legs, honest — feature ABSENT, not a harness defect):
//!   1. COMPILE-RED: the exec dispatch API does not exist yet —
//!      `PermitVerifierSpec::exec(name, argv, params)` (the exec kind, DR-015
//!      §Decision 1) and an ASYNC aggregator `permit::aggregate_async(...)`
//!      (option A, DR-015 §Decision 2 — an exec verifier cannot be `await`ed
//!      from the sync `aggregate`, so SP3 adds an async orchestration over the
//!      heterogeneous native+exec set). Neither symbol exists → the crate fails
//!      to compile until the implementer adds them.
//!   2. ASSERT-RED (once they compile): the assertions below pin the SP3
//!      verdict→decision map and ordered short-circuit across kinds.
//!
//! Unix-only: the reference policies are POSIX-sh, dispatched as `sh <abspath>`
//! (interpreter explicit — the committed +x bit is not portable across a
//! Windows checkout, so the argv names the interpreter, mirroring how a
//! cross-platform harness dispatches a scripting-engine policy).

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use rezidnt_cas::Cas;
use rezidnt_gate::VerifierInput;
use rezidnt_gate::permit::{self, PermitDecision, PermitVerifierSpec};
use serde_json::{Value, json};

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

/// Absolute path to a committed reference policy program under
/// `spec/fixtures/policies/`. Resolved from `CARGO_MANIFEST_DIR` so it is
/// checkout-relative, never machine-specific.
fn policy_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/policies")
        .join(name)
}

/// The permit request axis as a `VerifierInput` — the requested `tool` rides
/// `params.tool`, exactly what the reference policy inspects on stdin.
fn permit_input(params: Value, timeout_ms: u64) -> VerifierInput {
    VerifierInput {
        gate: permit::LIFECYCLE_POINT.to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params,
        timeout_ms,
    }
}

/// An EXEC permit entry: a display name + argv (interpreter + committed policy
/// path) + the verifier's own pinned params. THE API SHAPE THE IMPLEMENTER MUST
/// MATCH — `PermitVerifierSpec::exec(name, argv, params)` (DR-015 §Decision 1:
/// the exec kind carries argv + display name + params).
fn exec_policy(name: &str, script: &str, params: Value) -> PermitVerifierSpec {
    PermitVerifierSpec::exec(
        name,
        vec![
            "/bin/sh".to_string(),
            policy_path(script).display().to_string(),
        ],
        params,
    )
}

/// A native permit entry (`PermitVerifierSpec::native(name, params)`) — the
/// constructor the implementer adds alongside `::exec`, migrating the existing
/// struct-literal sites in `permit_aggregate.rs` (compile-only, no assertion
/// change).
fn native(name: &str, params: Value) -> PermitVerifierSpec {
    PermitVerifierSpec::native(name, params)
}

/// CRITERION 1 (headline) — an EXTERNAL policy decides a permit: a `[gates.permit]`
/// set carrying an exec policy that DENIES a forced-breach request (tool `Bash`)
/// yields a permit `Deny`. The verdict is the reference program's, dispatched
/// through the exec seam — no native hardcode decided it.
///
/// COMPILE-RED (`::exec` / `aggregate_async` absent) then ASSERT-RED.
#[tokio::test]
async fn exec_policy_denies_forced_breach_yields_deny() {
    let (_dir, cas) = empty_cas();
    // Forced breach: the request tool is Bash — the reference policy denies it.
    let input = permit_input(json!({ "tool": "Bash" }), rezidnt_gate::DEFAULT_TIMEOUT_MS);
    let set = vec![exec_policy(
        "reference-policy",
        "permit_tool_policy.sh",
        json!({}),
    )];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the EXTERNAL exec policy denied the forced-breach tool → Deny (CRITERION 1 headline)"
    );
    assert_eq!(
        outcome.deciding_verifier, "reference-policy",
        "the deciding verifier is the EXEC policy, not a hardcoded native (CRITERION 1)"
    );
}

/// CRITERION 1 (headline, allow leg) — the SAME reference policy ALLOWS a
/// non-breach request (tool `Read`) → the permit `Grant`s. Proves the exec
/// verdict maps `pass → allow` verbatim through the existing path (design §7).
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_policy_allows_non_breach_yields_grant() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" }), rezidnt_gate::DEFAULT_TIMEOUT_MS);
    let set = vec![exec_policy(
        "reference-policy",
        "permit_tool_policy.sh",
        json!({}),
    )];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Grant,
        "the external exec policy allowed the non-breach tool → Grant (CRITERION 1, pass→allow)"
    );
}

/// CRITERION 2 (un-filtered + dispatched, gate-crate leg) — an exec entry is
/// actually EXECUTED by the aggregator (not silently skipped): its verdict
/// reaches the decision. A lone exec-deny yields Deny — impossible unless the
/// aggregator ran the subprocess. (The un-FILTER half, `permit_config_for`, is
/// pinned in `crates/rezidnt-mcp/tests/permit_exec_live.rs`.)
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_entry_is_executed_not_skipped() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Bash" }), rezidnt_gate::DEFAULT_TIMEOUT_MS);
    let set = vec![exec_policy(
        "reference-policy",
        "permit_tool_policy.sh",
        json!({}),
    )];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.verifiers_run, 1,
        "the exec verifier RAN (an unrun exec would leave verifiers_run at 0 / never reach a verdict) — CRITERION 2"
    );
    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the exec verdict reached the decision — dispatched, not skipped (CRITERION 2)"
    );
}

/// CRITERION 3 (ordered short-circuit across kinds, leg A) — a native `Fail`
/// BEFORE an exec entry short-circuits to Deny WITHOUT running the exec. The
/// exec policy here would ALLOW (tool `Read`), so if it ran and combined we'd
/// still Deny — the load-bearing proof is `verifiers_run == 1`: the exec
/// subprocess never spawned.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn native_fail_before_exec_short_circuits_without_running_exec() {
    let (_dir, cas) = empty_cas();
    // tool "Read" is NOT in the allowlist → the FIRST (native) verifier fails.
    let input = permit_input(
        json!({ "tool": "Read", "allow": ["Edit"] }),
        rezidnt_gate::DEFAULT_TIMEOUT_MS,
    );
    let set = vec![
        native("tool-allowlist", json!({ "allow": ["Edit"] })),
        // Would ALLOW (Read ≠ Bash) — must never run.
        exec_policy("reference-policy", "permit_tool_policy.sh", json!({})),
    ];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the native Fail decides Deny (CRITERION 3, native-before-exec short-circuit)"
    );
    assert_eq!(
        outcome.deciding_verifier, "tool-allowlist",
        "the native Fail is the deciding verifier (CRITERION 3)"
    );
    assert_eq!(
        outcome.verifiers_run, 1,
        "the exec policy must NOT have run — the native Fail short-circuited across kinds (CRITERION 3)"
    );
}

/// CRITERION 3 (ordered short-circuit across kinds, leg B) — an exec `Fail`
/// short-circuits a LATER native. The exec denies (tool `Bash`) in position 1;
/// a native that would PASS sits in position 2 and must never run. Proof:
/// deciding verifier is the exec policy and `verifiers_run == 1`.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_fail_short_circuits_a_later_native() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(
        json!({ "tool": "Bash", "allow": ["Bash"] }),
        rezidnt_gate::DEFAULT_TIMEOUT_MS,
    );
    let set = vec![
        // Exec DENIES Bash in position 1 → short-circuit.
        exec_policy("reference-policy", "permit_tool_policy.sh", json!({})),
        // Would PASS (Bash is allowlisted) — must never run.
        native("tool-allowlist", json!({ "allow": ["Bash"] })),
    ];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the exec Fail decides Deny before the later native (CRITERION 3, exec-before-native short-circuit)"
    );
    assert_eq!(
        outcome.deciding_verifier, "reference-policy",
        "the exec policy is the deciding verifier (CRITERION 3)"
    );
    assert_eq!(
        outcome.verifiers_run, 1,
        "the later native must NOT have run — the exec Fail short-circuited across kinds (CRITERION 3)"
    );
}

/// CRITERION 3 (interleave, no short-circuit) — with an allowing exec BETWEEN
/// two passing natives, ALL THREE run in configured order and the aggregate is
/// Grant. Proves the aggregator interleaves native+exec in order (not "all
/// natives then all exec"), and that a passing exec does not stop the scan.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn interleaved_native_exec_native_all_run_and_grant() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(
        json!({
            "tool": "Read",
            "paths": ["src/checkout/lib.rs"],
        }),
        rezidnt_gate::DEFAULT_TIMEOUT_MS,
    );
    let set = vec![
        native("tool-allowlist", json!({ "allow": ["Read", "Edit"] })),
        // Allows Read (≠ Bash) — passes, scan continues.
        exec_policy("reference-policy", "permit_tool_policy.sh", json!({})),
        native("path-scope", json!({ "allow": ["src/checkout/**"] })),
    ];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Grant,
        "all three (native, exec, native) passed → Grant (CRITERION 3 interleave)"
    );
    assert_eq!(
        outcome.verifiers_run, 3,
        "the aggregator ran the whole interleaved set in order — a passing exec does not short-circuit (CRITERION 3)"
    );
}

/// CRITERION 4 (never-coerce, I6) — an exec policy that EXITS NONZERO maps to
/// ESCALATE, NEVER allow — even though its stdout says `pass`. The load-bearing
/// negative is `decision != Grant`.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_nonzero_exit_escalates_never_allows() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" }), rezidnt_gate::DEFAULT_TIMEOUT_MS);
    let set = vec![exec_policy(
        "nonzero-policy",
        "permit_policy_nonzero_exit.sh",
        json!({}),
    )];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_ne!(
        outcome.decision,
        PermitDecision::Grant,
        "a nonzero-exit policy is NEVER coerced to allow, even when stdout says pass (I6, CRITERION 4)"
    );
    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "nonzero exit → inconclusive → Escalate (CRITERION 4)"
    );
}

/// CRITERION 4 (never-coerce, I6) — an exec policy that emits MALFORMED stdout
/// (prose, not a §8 VerifierOutput) maps to ESCALATE, never allow.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_malformed_stdout_escalates_never_allows() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(json!({ "tool": "Read" }), rezidnt_gate::DEFAULT_TIMEOUT_MS);
    let set = vec![exec_policy(
        "malformed-policy",
        "permit_policy_malformed.sh",
        json!({}),
    )];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_ne!(
        outcome.decision,
        PermitDecision::Grant,
        "malformed stdout is NEVER coerced to allow (I6, CRITERION 4)"
    );
    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "malformed output → inconclusive → Escalate (CRITERION 4)"
    );
}

/// CRITERION 4 (never-coerce, I6) — an exec policy that OVERRUNS `timeout_ms`
/// maps to ESCALATE in bounded time, never allow. A short 300 ms timeout kills
/// the 30 s sleeper; the decision is Escalate.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_timeout_escalates_never_allows() {
    use std::time::{Duration, Instant};
    let (_dir, cas) = empty_cas();
    // 300 ms wall-clock timeout rides the request axis; the sleeper overruns it.
    let input = permit_input(json!({ "tool": "Read" }), 300);
    let set = vec![exec_policy(
        "slow-policy",
        "permit_policy_slow.sh",
        json!({}),
    )];

    let started = Instant::now();
    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_ne!(
        outcome.decision,
        PermitDecision::Grant,
        "a timed-out policy is NEVER coerced to allow (I6, CRITERION 4)"
    );
    assert_eq!(
        outcome.decision,
        PermitDecision::Escalate,
        "timeout → inconclusive → Escalate (CRITERION 4)"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the 300 ms timeout must actually kill the policy child; waited {:?}",
        started.elapsed()
    );
}

/// CRITERION 4 (precedence) — an exec Escalate does NOT short-circuit, and a
/// LATER native Fail still decides Deny (Fail > Escalate across kinds, I6). The
/// exec malformed-policy escalates in position 1; a denying native in position 2
/// decides. Proves inconclusive-from-exec never stops the scan (only Fail does),
/// mirroring the native-only precedence in `permit_aggregate.rs`.
///
/// COMPILE-RED then ASSERT-RED.
#[tokio::test]
async fn exec_inconclusive_does_not_short_circuit_later_native_fail_denies() {
    let (_dir, cas) = empty_cas();
    let input = permit_input(
        json!({ "tool": "Bash", "allow": ["Edit"] }),
        rezidnt_gate::DEFAULT_TIMEOUT_MS,
    );
    let set = vec![
        // Malformed exec → Inconclusive (does NOT short-circuit).
        exec_policy("malformed-policy", "permit_policy_malformed.sh", json!({})),
        // Bash not allowlisted → native Fail in position 2 decides Deny.
        native("tool-allowlist", json!({ "allow": ["Edit"] })),
    ];

    let outcome = permit::aggregate_async(&set, &input, &cas)
        .await
        .expect("aggregate runs");

    assert_eq!(
        outcome.decision,
        PermitDecision::Deny,
        "the exec Inconclusive did NOT short-circuit; the later native Fail decides Deny (Fail > Escalate, CRITERION 4/3)"
    );
    assert_eq!(
        outcome.deciding_verifier, "tool-allowlist",
        "the native Fail is the deciding verifier even though an exec Inconclusive preceded it (CRITERION 4/3)"
    );
    assert_eq!(
        outcome.verifiers_run, 2,
        "both ran — an exec Inconclusive never short-circuits, only a Fail does (CRITERION 4/3)"
    );
}
