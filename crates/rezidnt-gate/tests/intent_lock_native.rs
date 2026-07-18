//! SP-intent oracle — the `intent-lock` native permit-verifier (design
//! `docs/design/intent-lock.md` §4; DR-010 §8, C7). The `permit` gate's
//! run-intent native: a deterministic Rust check binding an agent's tool use
//! to the run's DECLARED, content-pinned intent allowlist so an off-task
//! request (a prompt injection, or a benign scope expansion — the verifier
//! does not know which) is surfaced to a human, never silently allowed.
//!
//! ## The load-bearing honesty forks (DR-010 §8 criterion 2, I6)
//!   - requested tool ∈ the run's intent allowlist → **Pass** (→ allow).
//!   - off-task tool (∉ allowlist), DEFAULT knob → **Inconclusive** (→ escalate
//!     / ask-a-human) — NEVER coerced to Pass or Fail (I6). This is the whole
//!     point of C7: off-task ≠ malicious, so the honest default routes to a
//!     human. Evidence names the off-task tool AND the declared intent so the
//!     escalation is interrogable (I6), CAS-ref carried (I2).
//!   - off-task under the hardened knob `on_off_task = deny` → **Fail**
//!     (→ deny), for high-assurance runs. Default knob value is `escalate`.
//!   - intent state ABSENT (no allowlist pinned) → **Inconclusive**, NEVER a
//!     synthesized Pass (cannot-run discipline, same as `SpendCap` with missing
//!     caps — a native that emits `pass` on garbage is broken, testing-oracles).
//!
//! RED MODE: **compile-red** — these reference `rezidnt_gate::IntentLock`, the
//! SP-intent native, which does not exist yet, so the crate fails to compile
//! until the implementer adds it and registers it in `builtin_natives()`. Once
//! it compiles, the assertions pin the three-valued behavior above.
//!
//! ## Determinism BINDING — the pinned-input param shape (pinned HERE, load-bearing)
//! The verifier MUST read the intent allowlist, the requested tool, and the
//! `on_off_task` knob from the content-hash-pinned `inputs.params` — NEVER live
//! mutable state, NEVER a re-derivation of intent (DR-010 §3, design §3; a live
//! LLM inference at decision time would be non-deterministic and break I6 /
//! debrief replay). The daemon folds `AgentRunState.intent` from the log (I3)
//! and passes the pinned allowlist in verbatim, exactly as `SpendCap` receives
//! its folded `PermitAccumulators` snapshot. Params shape pinned by this file
//! (matching the SP1 native idiom: `ToolAllowlist` reads the requested tool from
//! `params.tool`):
//!   `{ "tool": "<requested tool>",          // the action's tool (permit.requested.target.tool)
//!      "allowed_tools": ["<name>", ...],    // the PINNED intent allowlist (folded from the log)
//!      "on_off_task": "escalate" | "deny" } // the knob; ABSENT ⇒ escalate (default)`
//! Same params ⇒ same verdict AND same evidence, every time (I6).

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_gate::{IntentLock, NativeVerifier, Verdict, VerifierInput};
use serde_json::{Value, json};

/// A permit-gate `VerifierInput` with `params` (no CAS blob needed — the
/// requested tool and the pinned intent allowlist both ride inline in `params`,
/// per ontology `permit.requested.target` + the DR-010 pinned-input rule). A
/// fresh empty CAS is passed since this native decides from params alone.
fn permit_input(params: Value) -> VerifierInput {
    VerifierInput {
        gate: "permit".to_string(),
        workspace: None,
        refs: BTreeMap::new(),
        params,
        timeout_ms: 120_000,
    }
}

fn empty_cas() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

// --- in-intent → Pass -------------------------------------------------------

/// DR-010 §8 crit 2a — a requested tool that IS in the run's intent allowlist →
/// Pass (→ permit.granted / allow). On-task tool use is authorized.
#[test]
fn in_intent_tool_passes() {
    let (_d, cas) = empty_cas();
    let out = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "Read",
                "allowed_tools": ["Read", "Grep", "Glob"],
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Pass,
        "a tool in the declared intent allowlist is on-task → allow"
    );
}

// --- off-task, default knob → Inconclusive (escalate), NEVER coerced --------

/// DR-010 §8 crit 2b — THE load-bearing honesty test. An off-task tool (∉ the
/// intent allowlist) under the DEFAULT knob → **Inconclusive** (→ escalate /
/// ask-a-human). NEVER coerced to Pass and NEVER auto-denied by default (I6,
/// DR-010 §3). Off-task ≠ malicious, so the honest default surfaces it for a
/// human — the escalate path is what the slice exit rests on.
#[test]
fn off_task_tool_default_is_inconclusive_never_coerced() {
    let (_d, cas) = empty_cas();
    let out = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "Bash",
                "allowed_tools": ["Read", "Grep", "Glob"],
            })),
            &cas,
        )
        .expect("cannot-decide/escalate is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "off-task tool under the default knob → escalate-to-a-human, NEVER coerced to pass or deny (I6, DR-010 §3)"
    );
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "the load-bearing honesty guard — an off-task tool is never synthesized to a pass"
    );
}

/// DR-010 §8 crit 2b — the escalation is INTERROGABLE: the evidence names both
/// the off-task tool AND the declared intent (the allowlist it violated), so
/// `gate_explain` can tell a human WHY the request was surfaced (I6). The
/// evidence msg carries the offending tool name at minimum.
#[test]
fn off_task_escalation_evidence_names_the_tool_and_intent() {
    let (_d, cas) = empty_cas();
    let out = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "WebFetch",
                "allowed_tools": ["Read", "Edit"],
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(out.verdict, Verdict::Inconclusive);
    assert!(!out.evidence.is_empty(), "escalation carries evidence (I6)");
    // Pin the EXACT evidence msg — the canonical string the fixtures and the
    // `gate_explain` MCP test assert character-for-character, so native →
    // permit.escalated → gate_explain is proven end-to-end on ONE wording and
    // this contract can never silently diverge again (audit remediation).
    // The exact string still names BOTH the off-task tool (WebFetch) AND the
    // declared intent it violated (Read, Edit) — WHY, interrogable (I6, DR-010 §8).
    assert_eq!(
        out.evidence[0].msg, "off-task tool WebFetch not in declared intent [Read, Edit]",
        "the escalation names the OFF-TASK tool AND the DECLARED intent, exact wording pinned so \
         native/fixtures/MCP cannot diverge (I6, DR-010 §8); got {:?}",
        out.evidence
    );
}

// --- off-task under the hardened knob → Fail (deny) -------------------------

/// DR-010 §8 crit 2c — under the hardened knob `on_off_task = deny`, an off-task
/// tool hardens from escalate to **Fail** (→ permit.denied) for high-assurance
/// runs. The knob is read from the PINNED params (never live state; DR-010 §3).
#[test]
fn off_task_under_deny_knob_fails() {
    let (_d, cas) = empty_cas();
    let out = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "Bash",
                "allowed_tools": ["Read", "Grep"],
                "on_off_task": "deny",
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        out.verdict,
        Verdict::Fail,
        "off-task under the hardened `on_off_task = deny` knob → deny (DR-010 §8 crit 2c)"
    );
    assert!(
        !out.evidence.is_empty() && out.evidence[0].msg.contains("Bash"),
        "the denial names the off-task tool (I6); got {:?}",
        out.evidence
    );
}

/// DR-010 §3 — the knob DEFAULT is `escalate`: an off-task tool with the knob
/// set to `escalate` explicitly behaves identically to the knob absent
/// (Inconclusive), and NEITHER is a deny. Pins that `deny` is opt-in, not the
/// default (so a mis-read knob that defaulted to deny is caught).
#[test]
fn off_task_escalate_knob_matches_default_and_is_not_deny() {
    let (_d, cas) = empty_cas();
    let explicit = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "Bash",
                "allowed_tools": ["Read"],
                "on_off_task": "escalate",
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        explicit.verdict,
        Verdict::Inconclusive,
        "the explicit `escalate` knob escalates (never denies) — deny is opt-in only (DR-010 §3)"
    );
    assert_ne!(
        explicit.verdict,
        Verdict::Fail,
        "the DEFAULT posture never hard-denies an off-task tool (DR-010 §3, I6)"
    );
}

// --- intent state absent → Inconclusive, NEVER a synthesized Pass -----------

/// DR-010 §8 crit 2d — THE cannot-run honesty test. With NO intent allowlist
/// pinned (intent state absent), the verifier cannot decide whether a tool is
/// on-task → **Inconclusive** (escalate), NEVER a synthesized Pass. Same
/// discipline as `SpendCap` with missing caps: a native that emits `pass` on an
/// absent policy is broken (testing-oracles, I6).
#[test]
fn intent_absent_is_inconclusive_not_pass() {
    let (_d, cas) = empty_cas();
    let out = IntentLock
        .verify(&permit_input(json!({"tool": "Bash"})), &cas)
        .expect("cannot-decide is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "no intent allowlist pinned is undecidable → escalate, NEVER a synthesized pass (I6, DR-010 §8 crit 2d)"
    );
    assert_ne!(
        out.verdict,
        Verdict::Pass,
        "intent-absent is NEVER coerced to a pass — the cannot-run guard (I6)"
    );
}

/// DR-010 §3, I6 determinism BINDING — same PINNED params ⇒ same verdict AND
/// same evidence (refs included). The intent-lock native is bound to the same
/// determinism rule as every SP1 native: it reads only the pinned inputs, so a
/// replay reproduces the verdict from log + CAS (debrief replay, I6).
#[test]
fn intent_lock_is_deterministic() {
    let (_d, cas) = empty_cas();
    let input = permit_input(json!({
        "tool": "Bash",
        "allowed_tools": ["Read", "Grep"],
    }));
    let a = IntentLock.verify(&input, &cas).expect("engine ok");
    let b = IntentLock.verify(&input, &cas).expect("engine ok");
    assert_eq!(a.verdict, b.verdict, "same pinned params ⇒ same verdict");
    assert_eq!(a.evidence, b.evidence, "evidence is deterministic (I6)");
}

// --- registration -----------------------------------------------------------

/// DR-010 §8 crit 1 — the `intent-lock` native is registered in
/// `builtin_natives()` by its CANONICAL name `intent-lock` (the ontology/design
/// spelling) — the same registry the engine dispatches on and `replay`
/// re-executes against. Without registration, no gate can invoke it and no
/// debrief can replay it.
///
/// COMPILE-RED until `IntentLock` exists AND is added to `builtin_natives()`.
#[test]
fn intent_lock_is_registered_by_canonical_name() {
    let names: Vec<&'static str> = rezidnt_gate::builtin_natives()
        .iter()
        .map(|n| n.name())
        .collect();
    assert!(
        names.contains(&"intent-lock"),
        "builtin_natives() must register the SP-intent native \"intent-lock\"; got {names:?}"
    );
}

/// The native's own `name()` is the canonical `intent-lock` — pins the exact
/// spelling the daemon's PDP dispatches on and `gate_explain` names.
#[test]
fn intent_lock_name_is_canonical() {
    assert_eq!(
        IntentLock.name(),
        "intent-lock",
        "the native's canonical name is exactly \"intent-lock\" (DR-010 §8 crit 1)"
    );
}
