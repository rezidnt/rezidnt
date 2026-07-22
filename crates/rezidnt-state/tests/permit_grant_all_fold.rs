//! DR-035 sub-slice 2 (`escalation-grant-all`) ORACLE — the grant-all BROADENED
//! MATCH on `PermitRunState::resolution_for`, the pure deterministic judge. The
//! warden minted `scope?: "run_tool"` on `permit.resolved` v1 (an optional
//! single-value string enum) and STAGED IT INERT: `PermitResolution.scope:
//! Option<String>` exists (`#[serde(default)]`) but the reducer arm hardcodes
//! `scope: None` (fold not wired) and `resolution_for` does NOT read it. Wiring
//! the fold AND broadening the `resolution_for` predicate is this sub-slice's
//! implementer obligation and DOES NOT EXIST YET, so these tests are RED until it
//! lands. Criteria: DR-035 §Decision 2 (minimal single-axis wildcard) +
//! §Consequences "Test/criterion honesty" (the grant-all axis).
//!
//! ## The semantics under test (DR-035 §Decision 2)
//! A resolution with `scope == Some("run_tool")` is BROAD: it matches ANY incoming
//! action sharing the same `run` + `target.tool`, wildcarding the action/target
//! axis while holding `run` + `tool` fixed. A resolution with `scope == None`
//! stays DR-033's EXACT `(action, tool)` match — a sibling action returns `None`.
//! The broadening COMPOSES over the sub-slice-1 TTL filter (a broad resolution
//! still expires) and over last-matching-wins.
//!
//! ## Why this is RED today (state it plainly, per the socket-oracle precedent)
//! Two independent reasons, either sufficient:
//!   1. FOLD-INERT — the reducer arm sets `scope: None` unconditionally
//!      (`crates/rezidnt-state/src/lib.rs:977`), discarding the fact's `scope`
//!      key. So even a `permit.resolved` payload carrying `"scope":"run_tool"`
//!      folds to a resolution with `scope == None`. The implementer must fold the
//!      VERBATIM value (`payload["scope"].as_str().map(String::from)`).
//!   2. PREDICATE-EXACT — `resolution_for`'s `find` predicate
//!      (`crates/rezidnt-state/src/lib.rs:535-544`) matches on `r.action ==
//!      action` (exact), never consulting `r.scope`. So even a correctly-folded
//!      broad resolution would NOT match a sibling action. The implementer must
//!      widen the predicate: when `r.scope == Some("run_tool")` the action/target
//!      axis is wildcarded (tool still exact), else the DR-033 exact match holds.
//!
//! Because BOTH gaps stand, the load-bearing sibling test (criterion 1) is
//! BEHAVIOR-RED: it asserts `Some` and gets `None`. The failure NAMES the absent
//! broadening (a sibling action is not granted by a broad resolution), not an
//! unrelated error.
//!
//! ## Controlled-ULID helpers — REUSED verbatim from sub-slice-1's
//! `permit_ttl_fold.rs` so the TTL-composition test (criterion 4) pins the exact
//! deadline with NO `sleep` and NO wall-clock (the vet-concurrency flake note).
//! `ev_at` mints an event whose envelope ULID timestamp is EXACTLY `at`;
//! `envelope_ms` reads it back for the incoming-request side of the deadline.

use rezidnt_state::{PermitResolution, fold};
use rezidnt_types::{Event, EventParts, SourceId, Subject};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use ulid::Ulid;

const RUN: &str = "01DR035GRANTALL0RES0LVE00R";

/// A fixed base instant so every test's timeline is anchored and deterministic
/// (no `now()`). 2026-01-01T00:00:00Z — same anchor sub-slice-1 uses.
fn base() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("valid fixed base instant")
}

/// Mint an event whose ENVELOPE ULID timestamp is exactly `at` — the controlled
/// clock the expiry fold reads. `Event::from_parts` + `Ulid::from_datetime` so
/// T0 and the incoming-request time are exact, with NO `sleep` and NO wall-clock
/// (reused verbatim from `permit_ttl_fold.rs`).
fn ev_at(subject: &str, payload: Value, at: OffsetDateTime) -> Event {
    let id = Ulid::from_datetime(at.into());
    Event::from_parts(EventParts {
        id,
        ts: at,
        v: 1,
        source: SourceId::new("rezidnt-mcp"),
        workspace: None,
        subject: Subject::new(subject),
        correlation: id,
        causation: None,
        payload,
    })
    .expect("test event under 32KiB")
}

/// The envelope-ULID timestamp an event carries — the incoming-request side of
/// the deadline comparison (derived from `event.id`, per DR-035 §Decision 1).
fn envelope_ms(event: &Event) -> u64 {
    event.id.timestamp_ms()
}

/// A `permit.resolved` payload carrying `scope="run_tool"` (the just-minted
/// grant-all wildcard) AND a `ttl_ms` (the coupling: a broad grant is bounded,
/// DR-035 §Decision 3). The state fold never sees the coupling guard (that is the
/// tool-boundary's job, criterion 5); it folds whatever lands. Here we hand it a
/// well-formed broad-AND-bounded fact — the only shape the tool would ever emit.
fn resolved_broad(action: &str, tool: &str, decision: &str, ttl_ms: u64) -> Value {
    json!({
        "run": RUN,
        "request_id": "01DR035GRANTALLESCREQ0000R1",
        "action": action,
        "target": { "tool": tool },
        "decision": decision,
        "reason": "operator approved any action on this tool, time-boxed",
        "operator_badge_id": "0badc0de",
        "ttl_ms": ttl_ms,
        "scope": "run_tool",
    })
}

/// A `permit.resolved` payload WITHOUT `scope` — DR-033's exact request-scoped
/// match. Permanent (no `ttl_ms`), so the exact-match regression is clean of any
/// TTL interaction.
fn resolved_exact(action: &str, tool: &str, decision: &str) -> Value {
    json!({
        "run": RUN,
        "request_id": "01DR035GRANTALLESCREQ0000R2",
        "action": action,
        "target": { "tool": tool },
        "decision": decision,
        "operator_badge_id": "0badc0de",
    })
}

// A generous ttl so the grant-all match tests (1-3) are unambiguously WITHIN the
// window — the axis under test there is the ACTION wildcard, not the deadline.
const WIDE_TTL_MS: u64 = 3_600_000; // one hour

// ---------------------------------------------------------------------------
// CRITERION 1 — Broad grants a SIBLING action (same run+tool, different action).
// The load-bearing assertion: this is the whole feature (DR-035 §Decision 2).
// ---------------------------------------------------------------------------

/// CRITERION 1 (the load-bearing one) — a `permit.resolved` with
/// `scope="run_tool"` resolving action A1 (`tool.invoke`) on `(RUN, Bash)` IS
/// returned by `resolution_for` for a DIFFERENT action A2 (`tool.exec`) on the
/// SAME tool. The action/target axis is wildcarded; `run` + `tool` held fixed.
///
/// RED today: BEHAVIOR-RED. The fold discards `scope` (sets `None`,
/// `lib.rs:977`) AND `resolution_for` matches `r.action == action` exactly
/// (`lib.rs:536`), so the sibling action finds no resolution. This asserts `Some`
/// and gets `None` — the failure names the absent broadening.
#[test]
fn broad_resolution_grants_a_sibling_action_on_the_same_tool() {
    let t0 = base();
    let events = [ev_at(
        "permit.resolved",
        resolved_broad("tool.invoke", "Bash", "allow", WIDE_TTL_MS),
        t0,
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a resolved fact creates the run entry (I3)");

    // A DIFFERENT action on the SAME tool, well within the ttl window.
    let incoming = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.exec", "target": { "tool": "Bash" } }),
        t0 + Duration::milliseconds((WIDE_TTL_MS / 2) as i64),
    );
    let incoming_ms = envelope_ms(&incoming);

    let res = run.resolution_for("tool.exec", "Bash", incoming_ms).expect(
        "a BROAD (scope=\"run_tool\") resolution for one action on (run, Bash) grants \
             ANY action on the same (run, tool) — a sibling action MUST find it (DR-035 \
             §Decision 2: the action/target axis is wildcarded, run+tool held fixed)",
    );
    assert_eq!(
        res.decision, "allow",
        "the applied broad resolution's human decision folds verbatim (unchanged from DR-033)"
    );
    assert_eq!(
        res.scope.as_deref(),
        Some("run_tool"),
        "the matched resolution carries its broadening scope VERBATIM — the fold must \
         thread `scope` (today it hardcodes None, lib.rs:977), so this pins the fold too"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 2 — Broad does NOT grant OUT-OF-SCOPE (a DIFFERENT tool).
// The blast-radius bound: broad != unlimited (DR-035 §Decision 2).
// ---------------------------------------------------------------------------

/// CRITERION 2 — the SAME broad `scope="run_tool"` resolution for `(RUN, Bash)`
/// is NOT returned for a request on a DIFFERENT tool (`Grep`). The wildcard is on
/// the action/target axis ONLY; `tool` stays fixed. A broad grant on Bash never
/// authorizes Grep. This bounds the blast radius — broad is not unlimited.
///
/// RED-OR-GREEN today (the honest note): with the fold inert (`scope: None`) and
/// the predicate exact, a Grep request already returns `None` — so this could
/// PASS trivially before implementation. That is the negative-control trap the
/// oracle skill warns against. To keep it RED-FOR-THE-RIGHT-REASON, this test
/// PAIRS the out-of-scope `None` with a same-tool sibling `Some` in ONE test: the
/// sibling assertion is the one that fails today (behavior-red), and it PROVES
/// the resolution genuinely broadened, so the Grep `None` is a real bound and not
/// the trivial "nothing matches anything" of the un-broadened impl.
#[test]
fn broad_resolution_does_not_grant_a_different_tool() {
    let t0 = base();
    let events = [ev_at(
        "permit.resolved",
        resolved_broad("tool.invoke", "Bash", "allow", WIDE_TTL_MS),
        t0,
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];

    let incoming = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.invoke", "target": { "tool": "Grep" } }),
        t0 + Duration::milliseconds((WIDE_TTL_MS / 2) as i64),
    );
    let incoming_ms = envelope_ms(&incoming);

    // The bound: a DIFFERENT tool is out of scope even for the granted action.
    assert!(
        run.resolution_for("tool.invoke", "Grep", incoming_ms)
            .is_none(),
        "a broad grant on (run, Bash) does NOT authorize (run, Grep) — the wildcard is on \
         the ACTION axis only, `tool` stays fixed; broad is bounded, not unlimited \
         (DR-035 §Decision 2 blast-radius bound)"
    );

    // The paired positive — a same-tool sibling DOES match, so the `None` above is
    // a genuine tool bound and not the trivial 'un-broadened impl matches nothing'.
    // THIS is the behavior-red assertion today (the fold/predicate are not wired).
    assert!(
        run.resolution_for("tool.exec", "Bash", incoming_ms)
            .is_some(),
        "same broad resolution DOES grant a sibling action on the SAME tool — this proves \
         the resolution actually broadened, so the Grep `None` is a real bound (RED today: \
         the fold discards scope and the predicate is exact, so this returns None)"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 3 — Absent scope = EXACT match (DR-033 regression guard).
// Do NOT weaken any DR-033 test; this REFERENCES DR-033 request-scoped behavior
// (also pinned by `permit_resolved_fold.rs`), extending it to the sibling case.
// ---------------------------------------------------------------------------

/// CRITERION 3 — a `permit.resolved` with NO `scope` (`scope == None`) matches
/// ONLY its exact `(action, tool)`. A SIBLING action on the same tool returns
/// `None` — DR-033's request-scoped guard is unchanged: a resolution for one
/// action never grants another. This is a regression GUARD, not a new bar; it
/// references DR-033 §Decision 3 (request-scoped), which `permit_resolved_fold.rs`
/// pins and which stays untouched.
///
/// RED-OR-GREEN today: like criterion 2's bound, the exact-match `None` for a
/// sibling ALREADY holds under the un-broadened impl, so the `None` half could
/// pass trivially. To keep it honest, this is PAIRED: the exact resolution's OWN
/// action must still match (`Some`) — the DR-033 behavior — AND the sibling must
/// NOT (`None`). Under a CORRECT implementation both hold; under a BUGGY over-
/// broadening (a fix that wildcards even absent-scope) the sibling `None` would
/// FAIL, catching the regression. Today the exact half passes (DR-033 shipped);
/// the guard's value is post-implementation, so it is documented as a GUARD, not
/// a red-before-impl assertion — it must stay green through the whole slice.
#[test]
fn absent_scope_is_exact_match_no_sibling_grant() {
    let t0 = base();
    let events = [ev_at(
        "permit.resolved",
        resolved_exact("tool.invoke", "Bash", "allow"),
        t0,
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];

    // Any incoming time is fine — this resolution is permanent (no ttl).
    let incoming_ms = envelope_ms(&ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.invoke", "target": { "tool": "Bash" } }),
        t0 + Duration::seconds(1),
    ));

    // DR-033 exact behavior: the resolution's OWN action still matches.
    assert!(
        run.resolution_for("tool.invoke", "Bash", incoming_ms)
            .is_some(),
        "an absent-scope resolution still matches its exact (action, tool) — DR-033 \
         request-scoped behavior unchanged (regression guard, do NOT weaken)"
    );

    // The guard: a SIBLING action must NOT be granted — request-scoped, not broad.
    // A fix that over-broadens (wildcards even when scope==None) would fail here.
    assert!(
        run.resolution_for("tool.exec", "Bash", incoming_ms)
            .is_none(),
        "an absent-scope resolution does NOT grant a sibling action — the broadening \
         fires ONLY when scope==Some(\"run_tool\"); an over-eager wildcard that ignores \
         `scope` would leak a request-scoped grant to a sibling (DR-033 §Decision 3)"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 4 — Broad COMPOSES with TTL (expiry still applies to a sibling).
// The sub-slice-1 filter and the broadening compose (DR-035 §Decision 3): a
// broad resolution is bounded, so an EXPIRED broad resolution grants nothing —
// not even a matching sibling.
// ---------------------------------------------------------------------------

/// CRITERION 4 — a `scope="run_tool"` resolution WITH a `ttl_ms` that has LAPSED
/// is NOT returned even for a matching SIBLING action. `incoming_ms > T0 +
/// ttl_ms` → `None`, exactly as for an exact resolution. The sub-slice-1 expiry
/// filter and the sub-slice-2 broadening COMPOSE: broadening the action match
/// never bypasses the deadline.
///
/// RED today: BEHAVIOR-RED on the WITHIN-window half. Even before composing with
/// TTL, the sibling is not granted (fold/predicate not wired), so the "live broad
/// resolution grants the sibling" assertion fails. The past-deadline `None` half
/// would pass trivially under the un-broadened impl; pairing them keeps the test
/// red-for-the-right-reason (the live-grant assertion is the one that must flip
/// green when the broadening lands) AND pins the composition once it does.
#[test]
fn expired_broad_resolution_does_not_grant_a_sibling() {
    let t0 = base();
    let ttl_ms: u64 = 60_000; // one minute
    let events = [ev_at(
        "permit.resolved",
        resolved_broad("tool.invoke", "Bash", "allow", ttl_ms),
        t0,
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];

    // A sibling action WITHIN the window — a live broad resolution grants it.
    let within = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.exec", "target": { "tool": "Bash" } }),
        t0 + Duration::milliseconds((ttl_ms / 2) as i64),
    );
    // The SAME sibling action PAST the deadline — the broad resolution has expired.
    let past = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.exec", "target": { "tool": "Bash" } }),
        t0 + Duration::milliseconds(ttl_ms as i64) + Duration::seconds(1),
    );

    // Live broad resolution grants the sibling (the broadening under test).
    assert!(
        run.resolution_for("tool.exec", "Bash", envelope_ms(&within))
            .is_some(),
        "a LIVE broad resolution grants a sibling action within its ttl window (RED today: \
         the broadening is not wired, so this returns None)"
    );

    // Past the deadline the SAME broad resolution grants NOTHING — expiry composes.
    assert!(
        run.resolution_for("tool.exec", "Bash", envelope_ms(&past))
            .is_none(),
        "an EXPIRED broad resolution does NOT grant even a matching sibling — the sub-slice-1 \
         expiry filter and the sub-slice-2 broadening COMPOSE; broadening never bypasses the \
         deadline (DR-035 §Decision 3: broad OR permanent, and a broad one is bounded)"
    );
}

// ---------------------------------------------------------------------------
// Determinism — the broadened match is a pure fold (I3). Same log + same
// incoming timestamp => same outcome across repeated folds, for BOTH a sibling
// and an out-of-scope tool. No wall-clock in the predicate.
// ---------------------------------------------------------------------------

/// The broadened match is a pure fold: folding the same broad-resolution log
/// twice yields the SAME `resolution_for` outcome for a sibling (Some) and an
/// out-of-scope tool (None). No `SystemTime::now()` in the predicate — the same
/// log always folds to the same decision (I3, DR-035 §Invariants "Grant-all is
/// I3-neutral").
///
/// RED today: BEHAVIOR-RED — the sibling side is `None` (un-broadened) rather
/// than the `Some` this pins; both folds agree on `None`, so the equality holds
/// but the `Some` anchor at the end fails. The failure names the absent
/// broadening, not a determinism defect.
#[test]
fn broadened_match_is_a_pure_fold() {
    let t0 = base();
    let log = [ev_at(
        "permit.resolved",
        resolved_broad("tool.invoke", "Bash", "allow", WIDE_TTL_MS),
        t0,
    )];
    let incoming_ms = envelope_ms(&ev_at(
        "permit.requested",
        json!({}),
        t0 + Duration::seconds(30),
    ));

    let g1 = fold(log.iter());
    let r1 = &g1.agent_runs[RUN];
    let sibling_1 = r1
        .resolution_for("tool.exec", "Bash", incoming_ms)
        .map(|r| r.decision.clone());
    let cross_tool_1 = r1
        .resolution_for("tool.invoke", "Grep", incoming_ms)
        .is_some();

    let g2 = fold(log.iter());
    let r2 = &g2.agent_runs[RUN];
    let sibling_2 = r2
        .resolution_for("tool.exec", "Bash", incoming_ms)
        .map(|r| r.decision.clone());
    let cross_tool_2 = r2
        .resolution_for("tool.invoke", "Grep", incoming_ms)
        .is_some();

    assert_eq!(
        sibling_1, sibling_2,
        "the sibling-grant outcome is identical across re-folds — the broadened match is a \
         pure fold, no wall-clock (I3, DR-035 §Invariants)"
    );
    assert_eq!(
        cross_tool_1, cross_tool_2,
        "the out-of-scope outcome is identical across re-folds (I3)"
    );
    // And the two axes actually differ, so determinism is not the trivial
    // 'always None': the sibling IS granted and the cross-tool is NOT.
    assert_eq!(
        sibling_1.as_deref(),
        Some("allow"),
        "a broad resolution grants the sibling (the broadening under test)"
    );
    assert!(
        !cross_tool_1,
        "a broad resolution does not cross the tool bound"
    );
}

// Suppress `unused import` on the plain `PermitResolution` re-export used only
// via the folded records above — it documents the API this board pins.
#[allow(unused_imports)]
use PermitResolution as _PinnedResolutionType;
