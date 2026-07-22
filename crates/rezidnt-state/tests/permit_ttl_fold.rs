//! DR-035 sub-slice 1 (`escalation-ttl`) ORACLE — the log-derived expiry filter
//! on `PermitRunState::resolution_for`, the pure deterministic judge. The warden
//! this session minted `ttl_ms?: u64` on `permit.resolved` v1 (an optional
//! millisecond DURATION relative to the resolution's OWN envelope ULID); the
//! EXPIRY FILTER is this sub-slice's implementer obligation and DOES NOT EXIST
//! YET, so these tests are RED until it lands. Criteria: DR-035 §Consequences
//! "Test/criterion honesty" (the TTL axis) + §Decision 1 (the I3 crux).
//!
//! ## The semantics under test (DR-035 §Decision 1)
//! A resolution's expiry deadline is its ENVELOPE-ULID timestamp `+ ttl_ms`. A
//! resolution is applied to an incoming `permit.requested` ONLY IF that request's
//! OWN envelope-ULID timestamp is at or before the deadline; past it the
//! resolution is skipped and the request re-escalates. BOTH timestamps come from
//! event ULIDs already on the log (`crates/rezidnt-types/src/lib.rs:173`) — NO
//! `SystemTime::now()` at decision time. That is what makes expiry
//! replay-deterministic: the same log always folds to the same decision (I3).
//! Absent `ttl_ms` = permanent (DR-033's behavior — never filtered).
//!
//! ## Why this is RED today (state it plainly, per the socket-oracle precedent)
//! Two independent reasons, either sufficient:
//!   1. SIGNATURE / COMPILE-RED — `resolution_for` today takes `(action, tool)`
//!      with NO time awareness (`crates/rezidnt-state/src/lib.rs:482`). These
//!      tests call it with a THIRD parameter, the incoming request's envelope-ULID
//!      timestamp. That parameter does not exist yet, so the crate fails to
//!      compile until the implementer threads it in. This is the honest red for a
//!      signature-changing slice.
//!   2. FIELD / COMPILE-RED — `PermitResolution` has no `ttl_ms` field and the
//!      fold discards the resolution event's envelope timestamp
//!      (`crates/rezidnt-state/src/lib.rs:866-886` builds the record WITHOUT
//!      `event.id`/`event.ts`), so the deadline cannot be computed. The
//!      implementer must fold `ttl_ms` AND capture the resolution's own
//!      envelope-ULID timestamp so the deadline `T0 + ttl_ms` is a pure fold.
//!
//! When the signature/field land but the FILTER is a no-op (returns the match
//! regardless of the timestamp), the `expired_*` tests below flip to BEHAVIOR-RED
//! — they assert `None` and will get `Some`. Either way the failure NAMES the
//! absent expiry filter, not an unrelated error.
//!
//! ## The controlled-ULID minting helper (implementer: REUSE this)
//! `resolution_for`'s time awareness must be exercised with EXACT, deterministic
//! timestamps and ZERO `sleep` (that is the flake the vet-concurrency note warns
//! against). `Event::new` mints its own ULID at wall-clock `now()`, so it CANNOT
//! pin T0 or the incoming-request time. `Event::from_parts` +
//! `Ulid::from_datetime(at)` injects an envelope ULID at a CHOSEN instant, giving
//! exact control over "before vs after the deadline". `ev_at` below is that
//! helper — the implementer builds the same controlled fixtures.

use rezidnt_state::{PermitResolution, fold};
use rezidnt_types::{Event, EventParts, SourceId, Subject};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use ulid::Ulid;

const RUN: &str = "01DR035TTL0RES0LVE0000000R";

/// A fixed base instant so every test's timeline is anchored and deterministic
/// (no `now()`). T0 (the resolution's envelope time) and the incoming-request
/// times are all expressed as offsets from this. 2026-01-01T00:00:00Z.
fn base() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("valid fixed base instant")
}

/// Mint an event whose ENVELOPE ULID timestamp is exactly `at` — the controlled
/// clock the expiry fold reads. Uses `Event::from_parts` +
/// `Ulid::from_datetime` so T0 and the incoming-request time are exact, with NO
/// `sleep` and NO wall-clock. The implementer REUSES this to build both the
/// resolution (at T0) and to derive the incoming-request timestamp it threads
/// into `resolution_for`.
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

/// The envelope-ULID timestamp an event carries — the value the expiry fold reads
/// for BOTH sides of the deadline comparison (resolution `T0` and incoming
/// request). Derived from `event.id` (the ULID), NOT `event.ts`, because the DR
/// pins expiry to the time-ordered ULID (`§Decision 1`: "Both timestamps come
/// from event ULIDs"). The incoming-request timestamp the implementer threads
/// into `resolution_for` is exactly this, taken from the `permit.requested`
/// envelope.
fn envelope_ms(event: &Event) -> u64 {
    event.id.timestamp_ms()
}

/// A `permit.resolved` payload carrying a `ttl_ms` (the just-minted optional
/// DURATION field). Absent-`ttl_ms` variants build the payload without this key.
fn resolved_with_ttl(action: &str, tool: &str, decision: &str, ttl_ms: u64) -> Value {
    json!({
        "run": RUN,
        "request_id": "01DR035ESCALATEDREQ00000R1",
        "action": action,
        "target": { "tool": tool },
        "decision": decision,
        "reason": "operator approved this Bash invocation, time-boxed",
        "operator_badge_id": "0badc0de",
        "ttl_ms": ttl_ms,
    })
}

/// A `permit.resolved` payload WITHOUT `ttl_ms` — DR-033's permanent behavior.
fn resolved_permanent(action: &str, tool: &str, decision: &str) -> Value {
    json!({
        "run": RUN,
        "request_id": "01DR035ESCALATEDREQ00000R2",
        "action": action,
        "target": { "tool": tool },
        "decision": decision,
        "operator_badge_id": "0badc0de",
    })
}

// ---------------------------------------------------------------------------
// CRITERION 1 — Past-TTL resolution is NOT applied → re-escalates.
// The load-bearing assertion (DR-035 §Consequences, TTL axis).
// ---------------------------------------------------------------------------

/// CRITERION 1 (the load-bearing one) — a resolution folded from an event whose
/// envelope ULID is T0, carrying `ttl_ms = N`, does NOT apply to an incoming
/// request whose envelope-ULID timestamp is > T0 + N. `resolution_for` returns
/// `None`, so the PDP re-escalates rather than granting.
///
/// RED today: `resolution_for` takes no incoming-timestamp param (compile-red);
/// once threaded, a no-op filter returns `Some` here (behavior-red). Either
/// failure names the absent expiry filter.
#[test]
fn expired_resolution_is_not_applied() {
    let t0 = base();
    let ttl_ms: u64 = 60_000; // one minute

    let events = [ev_at(
        "permit.resolved",
        resolved_with_ttl("tool.invoke", "Bash", "allow", ttl_ms),
        t0,
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a resolved fact creates the run entry (I3)");

    // The incoming request arrives ONE SECOND PAST the deadline (T0 + ttl + 1s).
    let past_deadline = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.invoke", "target": { "tool": "Bash" } }),
        t0 + Duration::milliseconds(ttl_ms as i64) + Duration::seconds(1),
    );
    let incoming_ms = envelope_ms(&past_deadline);

    assert!(
        run.resolution_for("tool.invoke", "Bash", incoming_ms)
            .is_none(),
        "a request PAST the resolution's deadline (T0 + ttl_ms) must find NO \
         resolution — it re-escalates, it is NOT silently granted (DR-035 \
         §Decision 1: past the deadline the resolution is skipped)"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 2 — Within-TTL resolution IS applied.
// Paired with #1 so the two outcomes prove the DEADLINE is read, not that
// everything expires.
// ---------------------------------------------------------------------------

/// CRITERION 2 — the SAME `ttl_ms = N` resolution DOES apply to an incoming
/// request at time <= T0 + N. `resolution_for` returns the resolution (its human
/// decision intact). Paired with `expired_resolution_is_not_applied` so the two
/// prove the deadline is genuinely read.
///
/// RED today: compile-red on the missing incoming-timestamp param. Once the
/// signature lands, this test PINS that within-TTL is not over-filtered.
#[test]
fn within_ttl_resolution_is_applied() {
    let t0 = base();
    let ttl_ms: u64 = 60_000;

    let events = [ev_at(
        "permit.resolved",
        resolved_with_ttl("tool.invoke", "Bash", "allow", ttl_ms),
        t0,
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];

    // The incoming request arrives WELL WITHIN the window (T0 + half the ttl).
    let within = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.invoke", "target": { "tool": "Bash" } }),
        t0 + Duration::milliseconds((ttl_ms / 2) as i64),
    );
    let incoming_ms = envelope_ms(&within);

    let res = run
        .resolution_for("tool.invoke", "Bash", incoming_ms)
        .expect(
            "a request WITHIN the resolution's deadline (T0 + ttl_ms) must find \
             the resolution — expiry must not filter a live resolution (DR-035 \
             §Decision 1)",
        );
    assert_eq!(
        res.decision, "allow",
        "the applied resolution's human decision folds verbatim (unchanged from DR-033)"
    );
}

/// CRITERION 2 (boundary — AT the deadline) — an incoming request whose envelope
/// timestamp is EXACTLY T0 + ttl_ms IS applied. DR-035 §Decision 1: "at or
/// before that deadline" — the boundary is inclusive. Pins the off-by-one the
/// implementer must get right (`<=`, not `<`).
///
/// RED today: compile-red on the missing incoming-timestamp param.
#[test]
fn resolution_at_exact_deadline_is_applied() {
    let t0 = base();
    let ttl_ms: u64 = 60_000;

    let events = [ev_at(
        "permit.resolved",
        resolved_with_ttl("tool.invoke", "Bash", "allow", ttl_ms),
        t0,
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];

    let at_deadline = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.invoke", "target": { "tool": "Bash" } }),
        t0 + Duration::milliseconds(ttl_ms as i64),
    );
    let incoming_ms = envelope_ms(&at_deadline);

    assert!(
        run.resolution_for("tool.invoke", "Bash", incoming_ms)
            .is_some(),
        "a request EXACTLY at the deadline (T0 + ttl_ms) is still applied — the \
         deadline is inclusive (`at or before`, DR-035 §Decision 1); the boundary \
         is `<=`, not `<`"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 4 — Absent ttl_ms = permanent (DR-033 regression guard).
// Do NOT weaken any DR-033 test; this REFERENCES DR-033's permanent behavior
// (also pinned by `permit_resolved_fold.rs::a_new_resolution_overrides_...`).
// ---------------------------------------------------------------------------

/// CRITERION 4 — a `permit.resolved` with NO `ttl_ms` is applied REGARDLESS of
/// how late the incoming request's timestamp is. DR-033's permanent behavior is
/// unchanged: absent `ttl_ms` means never-filtered. This is a regression GUARD,
/// not a new bar — it references DR-033 §Decision 2 (permanent-until-overridden),
/// which `permit_resolved_fold.rs` also pins and which stays untouched.
///
/// RED today: compile-red on the missing incoming-timestamp param (the third arg
/// exists on NO variant yet). Once the signature lands, this test PROVES the
/// filter only fires when `ttl_ms` is present — an over-eager filter that expires
/// permanent resolutions would fail here.
#[test]
fn absent_ttl_is_permanent_regardless_of_incoming_time() {
    let t0 = base();

    let events = [ev_at(
        "permit.resolved",
        resolved_permanent("tool.invoke", "Bash", "allow"),
        t0,
    )];
    let graph = fold(events.iter());
    let run = &graph.agent_runs[RUN];

    // An absurdly late request — a full year past the resolution.
    let much_later = ev_at(
        "permit.requested",
        json!({ "run": RUN, "action": "tool.invoke", "target": { "tool": "Bash" } }),
        t0 + Duration::days(365),
    );
    let incoming_ms = envelope_ms(&much_later);

    let res = run
        .resolution_for("tool.invoke", "Bash", incoming_ms)
        .expect(
            "a resolution WITHOUT `ttl_ms` is permanent — it applies no matter how \
             late the incoming request (DR-033 §Decision 2, unchanged by DR-035); \
             the expiry filter fires ONLY when `ttl_ms` is present",
        );
    assert_eq!(
        res.decision, "allow",
        "the permanent resolution's decision is intact"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 3 — Replay-determinism of expiry.
// Expiry is a pure fold of on-log timestamps, no wall-clock. Same log +
// same incoming timestamp => same outcome across repeated folds.
// ---------------------------------------------------------------------------

/// CRITERION 3 — folding the SAME log twice yields the SAME `resolution_for`
/// outcome for BOTH an expired and a live incoming timestamp. Expiry is a pure
/// fold of on-log timestamps; there is no `SystemTime::now()` in the decision, so
/// repeated folds are invariant (I3 — the same log always folds to the same
/// decision). Checks both sides of the deadline so it pins that the DETERMINISM
/// holds regardless of outcome, not just "always None" or "always Some".
///
/// RED today: compile-red on the missing incoming-timestamp param.
#[test]
fn expiry_outcome_is_replay_deterministic() {
    let t0 = base();
    let ttl_ms: u64 = 60_000;
    let log = [ev_at(
        "permit.resolved",
        resolved_with_ttl("tool.invoke", "Bash", "allow", ttl_ms),
        t0,
    )];

    // Two fixed incoming timestamps: one live (within), one expired (past).
    let within_ms =
        (t0 + Duration::milliseconds((ttl_ms / 2) as i64)).unix_timestamp_nanos() / 1_000_000;
    let past_ms = (t0 + Duration::milliseconds(ttl_ms as i64) + Duration::seconds(1))
        .unix_timestamp_nanos()
        / 1_000_000;
    let within_ms = within_ms as u64;
    let past_ms = past_ms as u64;

    // Fold #1.
    let g1 = fold(log.iter());
    let r1 = &g1.agent_runs[RUN];
    let within_1 = r1
        .resolution_for("tool.invoke", "Bash", within_ms)
        .map(|r| r.decision.clone());
    let past_1 = r1.resolution_for("tool.invoke", "Bash", past_ms).is_some();

    // Fold #2 — same log, re-folded from zero.
    let g2 = fold(log.iter());
    let r2 = &g2.agent_runs[RUN];
    let within_2 = r2
        .resolution_for("tool.invoke", "Bash", within_ms)
        .map(|r| r.decision.clone());
    let past_2 = r2.resolution_for("tool.invoke", "Bash", past_ms).is_some();

    assert_eq!(
        within_1, within_2,
        "the within-TTL outcome is identical across re-folds — expiry is a pure \
         fold of on-log timestamps, no wall-clock (I3, DR-035 §Decision 1)"
    );
    assert_eq!(
        past_1, past_2,
        "the past-TTL outcome is identical across re-folds — replay-determinism \
         of expiry (I3)"
    );
    // And the two sides actually differ, so the determinism is not the trivial
    // 'always the same because always None' — the deadline is genuinely read.
    assert_eq!(within_1.as_deref(), Some("allow"), "within-TTL applies");
    assert!(!past_1, "past-TTL does not apply");
}

// ---------------------------------------------------------------------------
// Property — expiry is invariant across repeated folds for arbitrary
// (ttl, incoming-offset) pairs. The pure-fold guarantee, generalized.
// ---------------------------------------------------------------------------

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// For an ARBITRARY `ttl_ms` and an arbitrary incoming-request offset from
        /// T0, folding the same single-resolution log twice yields the SAME
        /// `resolution_for` outcome (applied-or-not AND, when applied, the same
        /// decision). No wall-clock is read; the expiry judge is a pure fold, so
        /// the outcome is invariant across repeated folds (I3, DR-035 §Decision 1).
        ///
        /// RED today: compile-red on the missing incoming-timestamp param.
        #[test]
        fn expiry_is_a_pure_fold(
            ttl_ms in 1_000u64..3_600_000,
            offset_ms in -3_600_000i64..7_200_000,
        ) {
            let t0 = base();
            let log = [ev_at(
                "permit.resolved",
                resolved_with_ttl("tool.invoke", "Bash", "allow", ttl_ms),
                t0,
            )];
            let incoming_ms = ((t0 + Duration::milliseconds(offset_ms))
                .unix_timestamp_nanos()
                / 1_000_000) as u64;

            let a = fold(log.iter());
            let ra = &a.agent_runs[RUN];
            let out_a = ra
                .resolution_for("tool.invoke", "Bash", incoming_ms)
                .map(|r| r.decision.clone());

            let b = fold(log.iter());
            let rb = &b.agent_runs[RUN];
            let out_b = rb
                .resolution_for("tool.invoke", "Bash", incoming_ms)
                .map(|r| r.decision.clone());

            prop_assert_eq!(out_a, out_b, "expiry outcome invariant across re-folds (pure fold, I3)");
        }
    }
}

// Suppress `unused import` on the plain `PermitResolution`/`Ulid` re-exports when
// only some arms reference them directly — they document the API this board pins.
#[allow(unused_imports)]
use PermitResolution as _PinnedResolutionType;
