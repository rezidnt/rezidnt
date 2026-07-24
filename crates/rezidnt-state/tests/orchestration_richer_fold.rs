//! DR-042 ORACLE (richer-fold-from-EXISTING-facts leg) — deepens the
//! `orchestration_graph` projection with fields DERIVABLE from facts that
//! ALREADY EXIST on the folded `AgentRunState`, minting NO new event subject and
//! NO ontology change (DR-042 §Decision 6: derive from existing facts first; a
//! new subject is a warden `/subject`, forbidden here). Read-side only — no live
//! fan-out, no spawn (Phase-3-gated, DR-042 Decision 5).
//!
//! ## What this pins (all from EXISTING folds — verified against the reducers)
//! Per sub (already on `AgentRunState`, folded by shipped reducers):
//! - `cost_usd: Option<f64>` — the sub's recorded cost, folded VERBATIM from
//!   `agent.completed.cost.total_usd` (S1, `AgentRunState::total_usd`). DR-042
//!   §Consequences names "recorded cost" as part of the folded orchestration
//!   evidence; a running/incomplete sub has `None` (honest absence, never 0.0).
//! - `killed_by: Option<String>` — the loggable operator badge id if a human
//!   KILLED this sub, folded VERBATIM from `agent.signaled.operator_badge_id`
//!   (DR-032, `AgentRunState::killed_by`). `None` = not operator-killed.
//!
//! Per lead (PURE derivation over the ALREADY-matched subs — no new state):
//! - `verdict_rollup: VerdictRollup { passed, failed, inconclusive, pending }` —
//!   the folded verdict tally across the lead's subs (DR-042 §Decision 2 "folded
//!   verdicts"; §Invariant I6 "the orchestrator folds verdicts"). A sub counts:
//!   `passed` iff EVERY gate on it is `pass` and it has ≥1 gate; `failed` iff ANY
//!   gate is `fail`; `inconclusive` iff ANY gate is `inconclusive` and none
//!   failed; `pending` iff it has NO terminal gate verdict yet (no gates, or only
//!   `entered`). I6 load-bearing: an inconclusive sub is counted as
//!   `inconclusive`, NEVER folded up into `passed` (never coerced).
//!
//! NO new subject is minted: every input is a field the shipped reducers already
//! fold. The one field the graph genuinely CANNOT fold without a new subject —
//! a sub's worktree linkage — stays DEFERRED to a warden `/subject` (the
//! projection oracle already recorded this; DR-042 §Decision 6).
//!
//! RE-CUT 2026-07-24 (DR-046 §Decision 4/5): the synthetic logs below built the
//! lead→sub edge as a lead-keyed `permit.delegated` fact. That emit is WITHDRAWN
//! — a fan-out attenuates nothing — so the edge is now `agent.spawned.lead_run`
//! on each SUB's own spawn. Every assertion in this file is unchanged; only the
//! shape of the log that produces the edge moved.

use std::path::PathBuf;

use rezidnt_state::{fold, orchestration_graph};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

fn load(name: &str) -> Vec<Event> {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: bad line ({e}): {l}")))
        .collect()
}

const LEAD_RUN: &str = "01RCHRN00000000000000EAD01";
const SUB_A_RUN: &str = "01RCHRN00000000000000SBA01";
const SUB_B_RUN: &str = "01RCHRN00000000000000SBB01";

/// CRITERION (richer per-sub fold) — the committed fan-out fixture folds each
/// sub's RECORDED COST and operator-kill attribution from EXISTING facts. Sub A
/// completed with a recorded cost; sub B is mid-flight (no `agent.completed`
/// yet), so its cost is honestly `None` — never synthesized to 0.0.
#[test]
fn subs_carry_recorded_cost_and_kill_attribution_from_existing_facts() {
    let events = load("dr042_orchestration_fanout.jsonl");
    let view = orchestration_graph(&fold(events.iter()));
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == LEAD_RUN)
        .expect("the lead surfaces");

    let sub_a = lead
        .subs
        .iter()
        .find(|s| s.sub_run == SUB_A_RUN)
        .expect("sub A");
    let sub_b = lead
        .subs
        .iter()
        .find(|s| s.sub_run == SUB_B_RUN)
        .expect("sub B");

    // Sub A completed: cost folds VERBATIM from agent.completed.cost.total_usd
    // (the fixture records 0.002). Recorded cost, not re-derived (I3).
    assert_eq!(
        sub_a.cost_usd,
        Some(0.002),
        "sub A's recorded cost folds verbatim from agent.completed.cost.total_usd (I3): {sub_a:#?}"
    );
    // Sub B is running (no agent.completed): cost is honestly absent, NEVER 0.0.
    assert_eq!(
        sub_b.cost_usd, None,
        "a mid-flight sub has no recorded cost yet — honest None, never synthesized to 0.0: {sub_b:#?}"
    );

    // Neither sub was operator-killed in the golden fan-out.
    assert_eq!(
        sub_a.killed_by, None,
        "sub A ran to completion — not operator-killed (honest None)"
    );
    assert_eq!(sub_b.killed_by, None, "sub B is mid-flight — not killed");
}

/// CRITERION (folded verdict rollup, I6 non-coercion) — the lead's folded verdict
/// tally across its subs. In the golden fixture sub A passed its `vet` gate and
/// sub B's `pre_merge` is INCONCLUSIVE, so the rollup is `passed=1,
/// inconclusive=1` — the inconclusive sub is NEVER folded up into `passed` (I6,
/// the load-bearing assertion). Pure derivation over the already-folded gate
/// state — no new subject.
#[test]
fn lead_folds_a_verdict_rollup_never_coercing_inconclusive() {
    let events = load("dr042_orchestration_fanout.jsonl");
    let view = orchestration_graph(&fold(events.iter()));
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == LEAD_RUN)
        .expect("the lead surfaces");

    let rollup = &lead.verdict_rollup;
    assert_eq!(
        rollup.passed, 1,
        "exactly one sub (A) has all-pass gates: {rollup:#?}"
    );
    assert_eq!(
        rollup.inconclusive, 1,
        "the inconclusive sub (B) is counted as INCONCLUSIVE, verbatim: {rollup:#?}"
    );
    // I6 — the load-bearing non-coercion guard. An inconclusive sub must NOT be
    // rolled up into passed OR failed.
    assert_eq!(
        rollup.failed, 0,
        "no sub failed; an inconclusive verdict is not a fail (I6, never coerced): {rollup:#?}"
    );
    assert_eq!(
        rollup.passed + rollup.failed + rollup.inconclusive + rollup.pending,
        lead.fan_out,
        "the rollup partitions EVERY sub exactly once (conservation): {lead:#?}"
    );
}

// --- synthetic-log coverage of every rollup bucket (existing subjects only) ----

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

const LEAD: &str = "01ORCHRICH00LEAD00000RL01";
const LEAD_BADGE: &str = "1eadrich00000001";

/// Every rollup bucket is exercised from EXISTING facts: a `failed` sub
/// (`gate.failed`), an `inconclusive` sub, a `passed` sub, and a `pending` sub
/// (spawned, no terminal gate). I6: the inconclusive sub is its own bucket, and
/// a fail dominates (a sub with any failing gate is `failed`, never `passed`).
#[test]
fn rollup_covers_pass_fail_inconclusive_pending_from_existing_subjects() {
    // The four subs, each with a distinct badge the lead delegates to.
    let subs = [
        ("01ORCHRICH00SUB0000PASS01", "5ubrichpass00001", "pass"),
        ("01ORCHRICH00SUB0000FAIL01", "5ubrichfail00001", "fail"),
        (
            "01ORCHRICH00SUB0000INCO01",
            "5ubrichinco00001",
            "inconclusive",
        ),
        ("01ORCHRICH00SUB0000PEND01", "5ubrichpend00001", "pending"),
    ];

    let mut events = vec![ev(
        "agent.spawned",
        json!({"run": LEAD, "agent": "lead", "harness": "claude-code", "badge_id": LEAD_BADGE}),
    )];
    for (sub_run, sub_badge, kind) in subs {
        // The lead→sub edge: the SUB's own spawn names its lead (DR-046
        // §Decision 5). No `permit.delegated` — a fan-out attenuates nothing.
        events.push(ev(
            "agent.spawned",
            json!({
                "run": sub_run, "agent": "sub", "harness": "claude-code",
                "badge_id": sub_badge, "lead_run": LEAD,
            }),
        ));
        // Terminal gate verdict per kind — all EXISTING subjects (gate.*).
        match kind {
            "pass" => events.push(ev(
                "gate.passed",
                json!({"run": sub_run, "gate": "vet", "verifiers": []}),
            )),
            "fail" => events.push(ev(
                "gate.failed",
                json!({"run": sub_run, "gate": "pre_merge", "verifier": "tests-pass", "evidence": []}),
            )),
            "inconclusive" => events.push(ev(
                "gate.inconclusive",
                json!({"run": sub_run, "gate": "pre_merge", "verifier": "tests-pass", "reason": "timeout", "evidence": []}),
            )),
            // "pending": spawned, no terminal gate (or only `entered`).
            _ => events.push(ev(
                "gate.entered",
                json!({"run": sub_run, "gate": "vet"}),
            )),
        }
    }

    let view = orchestration_graph(&fold(events.iter()));
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == LEAD)
        .expect("the lead surfaces once a sub names it");

    assert_eq!(lead.fan_out, 4, "four subs name this lead: {lead:#?}");
    let r = &lead.verdict_rollup;
    assert_eq!(r.passed, 1, "one all-pass sub: {r:#?}");
    assert_eq!(r.failed, 1, "one failed sub: {r:#?}");
    assert_eq!(
        r.inconclusive, 1,
        "one inconclusive sub, its own bucket — never coerced into pass/fail (I6): {r:#?}"
    );
    assert_eq!(
        r.pending, 1,
        "one pending sub (entered-only, no terminal verdict): {r:#?}"
    );
    assert_eq!(
        r.passed + r.failed + r.inconclusive + r.pending,
        4,
        "the rollup partitions every sub exactly once (conservation): {r:#?}"
    );
}

/// I6 dominance edge — a sub with a MIX of a pass and a fail gate is `failed`,
/// not `passed` (any fail dominates); a sub with a pass and an inconclusive (no
/// fail) is `inconclusive`, not `passed` (inconclusive is never coerced up).
/// Both derived from EXISTING gate facts.
#[test]
fn rollup_fail_and_inconclusive_dominate_a_partial_pass() {
    let mixed_fail = "01ORCHRICH00SUBMIXFAIL01";
    let mixed_inco = "01ORCHRICH00SUBMIXINC01";
    let badge_f = "5ubmixfail000001";
    let badge_i = "5ubmixinco000001";

    let events = [
        ev(
            "agent.spawned",
            json!({"run": LEAD, "agent": "lead", "harness": "claude-code", "badge_id": LEAD_BADGE}),
        ),
        // Sub with a PASS then a FAIL gate → failed dominates. Each sub names its
        // lead on its OWN spawn (DR-046 §Decision 5).
        ev(
            "agent.spawned",
            json!({"run": mixed_fail, "agent": "sub", "harness": "claude-code", "badge_id": badge_f, "lead_run": LEAD}),
        ),
        ev(
            "gate.passed",
            json!({"run": mixed_fail, "gate": "vet", "verifiers": []}),
        ),
        ev(
            "gate.failed",
            json!({"run": mixed_fail, "gate": "pre_merge", "verifier": "tests-pass", "evidence": []}),
        ),
        // Sub with a PASS then an INCONCLUSIVE gate (no fail) → inconclusive.
        ev(
            "agent.spawned",
            json!({"run": mixed_inco, "agent": "sub", "harness": "claude-code", "badge_id": badge_i, "lead_run": LEAD}),
        ),
        ev(
            "gate.passed",
            json!({"run": mixed_inco, "gate": "vet", "verifiers": []}),
        ),
        ev(
            "gate.inconclusive",
            json!({"run": mixed_inco, "gate": "pre_merge", "verifier": "tests-pass", "reason": "timeout", "evidence": []}),
        ),
    ];

    let view = orchestration_graph(&fold(events.iter()));
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == LEAD)
        .expect("lead");
    let r = &lead.verdict_rollup;

    assert_eq!(
        r.failed, 1,
        "a sub with any failing gate is FAILED even with a passing gate (fail dominates): {r:#?}"
    );
    assert_eq!(
        r.inconclusive, 1,
        "a sub with a pass + an inconclusive (no fail) is INCONCLUSIVE — never coerced to pass (I6): {r:#?}"
    );
    assert_eq!(r.passed, 0, "neither mixed sub is all-pass: {r:#?}");
    assert_eq!(r.pending, 0, "both subs reached a terminal verdict: {r:#?}");
}
