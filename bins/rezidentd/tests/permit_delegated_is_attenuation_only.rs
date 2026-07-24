//! DR-046 §Decision 4 / §Consequences (c) — STRUCTURAL guard: the daemon emits
//! `permit.delegated` for a genuine ROLE ATTENUATION and for nothing else. The
//! lead-parented emit DR-044 §Decision 2b introduced is WITHDRAWN, because a
//! fan-out is not an attenuation and that subject cannot express one without
//! asserting a narrowing that did not occur.
//!
//! HOST-RUNNABLE: reads one source file by path, with no `#[cfg(unix)]` gate, so
//! host `/vet` executes it. That is the entire reason it exists in this form —
//! see the coverage note below.
//!
//! ## Why a source guard, stated plainly (test honesty)
//!
//! This asserts on SOURCE TEXT, not on runtime behavior, which is normally the
//! shape to refuse. It is here because the alternatives cannot cover the claim:
//!
//! The fold-side guard
//! (`crates/rezidnt-state/tests/orchestration_graph_projection.rs`) folds a
//! committed fixture. A fixture is static, so NO fold-based test can notice a
//! change in what the daemon emits — restoring the withdrawn emit in
//! `runs.rs` leaves every fixture-based assertion green. That guard now carries
//! a positive control proving its zero is a fact about the log rather than a
//! dead observable, which is the most a fold-side test can honestly claim.
//!
//! The runtime guard (`fan_out_live_e2e.rs`, leg 2) DOES catch a restored emit
//! against a real daemon — but it is `#![cfg(unix)]` and therefore invisible to
//! the host gauntlet. Before this file, a reintroduced false emit could not be
//! caught on the host at all.
//!
//! So the three legs are complementary and none is redundant: this one fails on
//! the host when the EMITTER changes; the e2e leg fails on WSL when the emitted
//! LOG changes; the projection leg fails when the REDUCER changes. House
//! precedent for a host-runnable structural guard covering what a WSL-only test
//! cannot: `bench/harness/tests/testkit_dev_only.rs`.
//!
//! ## What this guard can and cannot see
//!
//! It reads `bins/rezidentd/src/runs.rs` — the only file that has ever emitted
//! this subject. It cannot see an emit added to a DIFFERENT file, and it is not
//! a substitute for the runtime leg. It is a tripwire on the one location the
//! withdrawal actually removed code from, which is where a revert or a bad merge
//! would put it back.

use std::path::PathBuf;

/// The one daemon source file that emits `permit.delegated`.
fn runs_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runs.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("daemon source {} must be readable: {e}", path.display()))
}

/// Every line index that constructs the `permit.delegated` subject.
fn emit_sites(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(r#"Subject::new("permit.delegated")"#))
        .map(|(i, _)| i)
        .collect()
}

/// DR-046 §Decision 4 — there is EXACTLY ONE `permit.delegated` emit in the
/// daemon, the DR-017 role-attenuation self-edge. The withdrawn lead-parented
/// emit was a SECOND construction of this subject in this same file, so its
/// return moves this count to 2 and trips the guard on the host.
#[test]
fn the_daemon_emits_permit_delegated_exactly_once() {
    let source = runs_rs();
    let sites = emit_sites(&source);
    assert_eq!(
        sites.len(),
        1,
        "expected EXACTLY ONE `permit.delegated` emit in bins/rezidentd/src/runs.rs — the DR-017 \
         role-attenuation self-edge. DR-046 §Decision 4 WITHDREW the lead-parented fan-out emit: \
         a fan-out is not an attenuation, and re-adding one would put a known-false narrowing \
         back on the dossier `debrief` reads. Found {} emit site(s) at 1-based lines {:?}.",
        sites.len(),
        sites.iter().map(|i| i + 1).collect::<Vec<_>>()
    );
}

/// DR-046 §Decision 4 — and the surviving emit is the ATTENUATION one: it sits
/// under the `role_delegation` guard (the branch taken only when the injected
/// badge is a genuine role-narrowed child of the run's base badge) and carries
/// NOTHING from a fan-out lead.
///
/// This is the half that matters if someone REPLACES rather than ADDS: swapping
/// the surviving emit's keying from the spawning run to a lead would keep the
/// count at 1 and still reintroduce the false fact.
#[test]
fn the_surviving_emit_is_the_role_attenuation_edge_and_names_no_lead() {
    let source = runs_rs();
    let lines: Vec<&str> = source.lines().collect();
    let sites = emit_sites(&source);
    let site = *sites
        .first()
        .expect("the role-attenuation emit exists (see the sibling count test)");

    // The ENCLOSING GUARD: look back for the `role_delegation` branch this emit
    // must sit under.
    let guard_window = lines[site.saturating_sub(40)..site].join("\n");
    assert!(
        guard_window.contains("role_delegation"),
        "the surviving `permit.delegated` emit must sit under the DR-017 `role_delegation` guard \
         — it is emitted only when the spawn genuinely attenuated the base badge with a Role \
         caveat (DR-046 §Decision 6, which leaves that path untouched). Looked back at:\n\
         {guard_window}"
    );

    // The EMIT'S OWN PAYLOAD: forward from the subject line only.
    //
    // Scoped deliberately. An earlier cut of this guard scanned the 40 lines
    // BEFORE the emit too and tripped on `lead.lead_run` — the LEGITIMATE
    // `agent.spawned.lead_run` insertion that DR-046 §Decision 5 put a few lines
    // above. That was a false positive in the test, not a fault in the daemon,
    // and it is exactly the failure mode a source guard must not have: the
    // withdrawn emit is identified by what ITS OWN payload names, not by what
    // happens to sit near it.
    let payload_window = lines[site..(site + 16).min(lines.len())].join("\n");
    for forbidden in ["lead.lead_run", "lead_badge_id", "lead."] {
        assert!(
            !payload_window.contains(forbidden),
            "the `permit.delegated` emit's own payload must name NO fan-out lead — found \
             {forbidden:?}. The lead→sub edge is `agent.spawned.lead_run` (DR-046 §Decision 5); \
             expressing it as a delegation asserts a narrowing that did not occur \
             (§Decision 4). Payload:\n{payload_window}"
        );
    }

    // Positive control on the same window: the surviving emit keys `run` to the
    // SPAWNING run and carries the attenuation's own two badge ends. Without
    // this, the forbidden-token scan above would pass over an empty or
    // mislocated window and prove nothing.
    for expected in [
        "\"run\": run,",
        "parent_badge_id",
        "child_badge_id",
        "added_caveats",
    ] {
        assert!(
            payload_window.contains(expected),
            "the surviving emit is the attenuation edge, keyed on the SPAWNING run and carrying \
             both sig-derived badge ends — expected {expected:?} in its payload. If this fails \
             the window is mislocated and the scan above is vacuous. Payload:\n{payload_window}"
        );
    }
}
