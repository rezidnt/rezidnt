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
    // SP-empty (DR-012) — pin the ABSENT leg's cannot-run EVIDENCE so it is
    // distinguishable from the DECLARED-empty off-task case below. Absent must
    // carry the "no intent allowlist pinned" cannot-run message; declared-empty
    // must NOT. This is the discriminator the two new tests assert against.
    assert!(
        out.evidence
            .iter()
            .any(|e| e.msg.contains("no intent allowlist pinned")),
        "intent-ABSENT is cannot-run with the 'no intent allowlist pinned' evidence — the \
         message declared-empty must NOT carry (DR-012 discriminator); got {:?}",
        out.evidence
    );
}

// --- SP-empty (DR-012): DECLARED-empty allowlist is off-task, NOT cannot-run --
//
// DR-012 option B (ACCEPTED) breaks the collapse at lib.rs:784-789: today
// `string_list(p, "allowed_tools")` returns `[]` for BOTH a missing key
// (intent ABSENT) and an explicit `[]` (intent DECLARED-empty), and the
// `if allowed.is_empty() { cannot_run(...) }` guard sends both to cannot-run.
// Per DR-012 the implementer replaces that guard with a KEY-PRESENCE check:
//   - `allowed_tools` key ABSENT  → cannot-run (genuinely no declared intent).
//   - `allowed_tools` key PRESENT but `[]` → "every tool is off-task": fall
//     through into the EXISTING off-task path (the `any(...)==false` branch),
//     honoring `on_off_task` (escalate default / deny under the knob).
// These tests pin the DECLARED-empty half. They are ASSERT-RED against the
// current collapse: with the `is_empty()` guard in place a declared-empty
// allowlist returns cannot-run/Inconclusive REGARDLESS of the knob, so
// `declared_empty_under_deny_knob_fails` (which expects Fail) is red today.

/// DR-012 — an `allowed_tools` key PRESENT but EMPTY (`[]`) means every tool is
/// off-task. Under the DEFAULT knob it routes through the off-task path →
/// **Inconclusive** (escalate), NEVER a synthesized pass and NEVER the
/// cannot-run absent path. The distinction from ABSENT is BEHAVIORAL (it honors
/// the knob) and INTERROGABLE (the evidence is the off-task wording naming the
/// empty declared intent `[]`, NOT the "no intent allowlist pinned" cannot-run
/// message the absent leg carries).
///
/// ASSERT-RED against the current collapse: today declared-empty hits the
/// `is_empty()` cannot-run guard, so the evidence is "no intent allowlist
/// pinned …" (the absent message) — this test demands the off-task wording and
/// fails on the collapsed message.
#[test]
fn declared_empty_allowlist_is_off_task_not_cannot_run() {
    let (_d, cas) = empty_cas();
    let out = IntentLock
        .verify(
            // Key PRESENT (`allowed_tools`), value EMPTY (`[]`) — declared
            // lockdown: the run declared it may use NO tools, so Bash is off-task.
            &permit_input(json!({
                "tool": "Bash",
                "allowed_tools": [],
            })),
            &cas,
        )
        .expect("off-task/escalate is a verdict, not an error");
    assert_eq!(
        out.verdict,
        Verdict::Inconclusive,
        "a DECLARED-empty allowlist means every tool is off-task → escalate under the default \
         knob (DR-012 option B), NOT a synthesized pass"
    );
    assert!(
        !out.evidence.is_empty(),
        "the escalation carries evidence (I6)"
    );
    // The load-bearing DISCRIMINATOR from ABSENT: declared-empty routes through
    // the OFF-TASK path, so its evidence is the off-task wording naming the
    // (empty) declared intent — NOT the cannot-run "no intent allowlist pinned"
    // message. Pin the exact off-task string the existing path produces for an
    // empty allowlist (join of `[]` is the empty string ⇒ trailing `[]`).
    assert_eq!(
        out.evidence[0].msg, "off-task tool Bash not in declared intent []",
        "declared-empty is the OFF-TASK path (empty declared intent named `[]`), NOT the \
         cannot-run absent message — this is the DR-012 discriminator; got {:?}",
        out.evidence
    );
    assert!(
        !out.evidence[0].msg.contains("no intent allowlist pinned"),
        "declared-empty must NOT carry the ABSENT cannot-run message — that would be the \
         collapse DR-012 breaks; got {:?}",
        out.evidence
    );
}

/// DR-012 — THE crux of option B. A DECLARED-empty allowlist under the hardened
/// knob `on_off_task = deny` → **Fail** (deny): a real least-privilege LOCKDOWN
/// ("this run may use NO tools" → deny every tool). This is the observable
/// behavior option A could NEVER express — an ABSENT intent under the deny knob
/// only ever escalates (cannot-run ignores the knob), but declared-empty under
/// deny denies. Pins BOTH sides of that contrast.
///
/// ASSERT-RED against the current collapse: today declared-empty hits the
/// `is_empty()` cannot-run guard and returns Inconclusive REGARDLESS of the
/// knob — so this expects Fail and gets Inconclusive. The red reason is the
/// collapse (cannot-run swallows the knob), not a typo.
#[test]
fn declared_empty_under_deny_knob_fails() {
    let (_d, cas) = empty_cas();
    // Declared-empty (key present, `[]`) + the hardened deny knob → deny.
    let declared_empty = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "Bash",
                "allowed_tools": [],
                "on_off_task": "deny",
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        declared_empty.verdict,
        Verdict::Fail,
        "DECLARED-empty under `on_off_task = deny` is a lockdown → DENY every tool (DR-012 \
         option B crux); the collapse returns Inconclusive here — the RED is the collapse"
    );
    assert!(
        !declared_empty.evidence.is_empty() && declared_empty.evidence[0].msg.contains("Bash"),
        "the denial names the off-task tool (I6); got {:?}",
        declared_empty.evidence
    );

    // The CONTRAST that only option B can express: an ABSENT intent (no
    // `allowed_tools` key) under the SAME deny knob still only ESCALATES —
    // cannot-run ignores the knob because we genuinely do not know the intent.
    let absent = IntentLock
        .verify(
            &permit_input(json!({
                "tool": "Bash",
                "on_off_task": "deny",
            })),
            &cas,
        )
        .expect("engine ok");
    assert_eq!(
        absent.verdict,
        Verdict::Inconclusive,
        "ABSENT intent + deny knob STILL escalates (cannot-run ignores the knob) — the \
         distinction from declared-empty that makes option B honest (DR-012)"
    );
    assert_ne!(
        absent.verdict,
        Verdict::Fail,
        "absent is NEVER coerced to a deny even under the deny knob — only a DECLARED-empty \
         lockdown denies (DR-012); the deny knob does not manufacture an intent"
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
