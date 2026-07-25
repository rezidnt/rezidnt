//! BOUNDED_REASON boundary oracle — closes the auditor-found coverage gap on
//! `crate::runs::bounded_reason` (`bins/rezidentd/src/runs.rs:1773`), the
//! daemon-side enforcement of the `agent.completed.error?.message` length
//! bound (I2; `spec/ontology.md` `agent.completed` `error?` clause, cap +
//! marker semantics recorded 2026-07-25).
//!
//! ## Honesty: written AFTER the implementation, expected GREEN on write
//!
//! This is judge-strengthening on shipped code, not a pre-implementation
//! criterion — the same inversion the parent module discloses for its two
//! post-Stage-B guards. Every test here was expected to pass when written; a
//! failure would be a real bug in the bound, not a red-mode checkpoint.
//!
//! ## Why the file lives HERE
//!
//! `bounded_reason` is private to the `runs` module, and `runs.rs` is frozen
//! under review, so no new `mod` declaration can be added there and no
//! visibility can be widened. The one test module `runs.rs` already declares
//! (`registry_convergence_tests`, `runs.rs:47-48`) is therefore the sole seam:
//! a descendant module of `runs` sees its private items, so this child reaches
//! `crate::runs::bounded_reason` without touching the file under review. Same
//! WSL-only caveat as the parent: the daemon modules are `#[cfg(unix)]`, so
//! host `/vet` never compiles this file — run it under WSL.
//!
//! ## The contract being pinned (implementation and ontology AGREE)
//!
//! Text of `ERROR_MESSAGE_CAP` (1024) bytes or fewer rides VERBATIM and
//! unmarked — including text that happens to end in the harness's own `…`.
//! Longer text is cut at the last char boundary at or below the cap and a
//! single `…` (U+2026, 3 bytes) is appended AFTER the cut, so the marker does
//! NOT count toward the cap: an elided value always measures `cap`..=`cap + 3`
//! bytes and ends with the marker, while a complete value never exceeds `cap`
//! bytes. That length band, not the character alone, is what disambiguates an
//! elision from a verbatim harness ellipsis.
//!
//! The final test pins the daemon's copy of the algorithm against the adapter
//! crate's private `elide` THROUGH ITS PUBLIC SURFACE (a synthetic codex
//! `turn.failed` line driven into `CodexAdapter::map_line`, the same seam
//! `codex_adapter_guards.rs` uses), because the cap CONSTANT is shared but the
//! ALGORITHM is duplicated — marker choice, boundary rule and band semantics
//! could drift with no shared code to stop them. `elide` itself stays private;
//! nothing here needed it exported.

use rezidnt_run::RunId;
use rezidnt_run::adapter::{CodexAdapter, ERROR_MESSAGE_CAP};
use ulid::Ulid;

use crate::runs::bounded_reason;

/// The marker is U+2026 HORIZONTAL ELLIPSIS, 3 bytes in UTF-8. Every length
/// assertion below leans on this, so pin it once where a failure names it.
const MARKER_BYTES: usize = 3;

/// The ontology names the NUMBER, not just the constant: "`ERROR_MESSAGE_CAP
/// = 1024` bytes". A change to the cap is a DEFAULT change and legal, but it
/// must route through the ontology prose too — this assertion makes the drift
/// loud instead of silent.
#[test]
fn the_cap_is_the_1024_bytes_the_ontology_records() {
    assert_eq!('…'.len_utf8(), MARKER_BYTES, "the marker is 3 bytes");
    assert_eq!(
        ERROR_MESSAGE_CAP, 1024,
        "spec/ontology.md's agent.completed.error? clause names 1024; \
         changing the constant without the ontology prose is silent drift"
    );
}

/// One byte UNDER the cap: verbatim, unmarked, byte-identical.
#[test]
fn a_reason_under_the_cap_rides_verbatim_and_unmarked() {
    let text = "x".repeat(ERROR_MESSAGE_CAP - 1);
    assert_eq!(
        bounded_reason(text.clone()),
        text,
        "cap - 1 bytes is complete text and must not be touched"
    );
}

/// EXACTLY the cap: still complete — the bound is `<=`, and an off-by-one
/// here would start marking complete reasons as elided.
#[test]
fn a_reason_of_exactly_the_cap_is_complete_and_untouched() {
    let text = "x".repeat(ERROR_MESSAGE_CAP);
    assert_eq!(
        bounded_reason(text.clone()),
        text,
        "exactly cap bytes is within the bound and must ride verbatim"
    );
}

/// One byte OVER the cap, all-ASCII: the cut lands exactly at the cap (every
/// byte index is a char boundary), the marker is appended after it, and the
/// result sits at the band CEILING, cap + 3 bytes.
#[test]
fn one_byte_past_the_cap_is_cut_at_the_cap_and_marked() {
    let out = bounded_reason("x".repeat(ERROR_MESSAGE_CAP + 1));
    assert_eq!(
        out,
        format!("{}…", "x".repeat(ERROR_MESSAGE_CAP)),
        "the largest whole prefix at the cap, plus a single marker"
    );
    assert_eq!(
        out.len(),
        ERROR_MESSAGE_CAP + MARKER_BYTES,
        "the marker rides OUTSIDE the cap: band ceiling is cap + 3"
    );
}

/// A 3-byte char straddling the cap — the case the boundary walk exists for.
/// 1024 = 3 * 341 + 1, so byte 1024 falls mid-`€` and a raw
/// `text.truncate(ERROR_MESSAGE_CAP)` would PANIC; the walk must back off to
/// 1023 and cut there.
#[test]
fn a_multibyte_char_straddling_the_cap_backs_off_to_a_char_boundary() {
    let out = bounded_reason("€".repeat(342)); // 1026 bytes, over the cap
    assert_eq!(
        out,
        format!("{}…", "€".repeat(341)),
        "the cut is the last char boundary at or below the cap (byte 1023)"
    );
    assert_eq!(out.len(), 1023 + MARKER_BYTES);
}

/// A 4-byte char straddle that needs a MULTI-STEP walk: `🦀` is 4 bytes, and
/// the 2-byte ASCII prefix shifts every boundary to 2 mod 4, so neither 1024
/// nor 1023 is a boundary — the walk must step twice to 1022. A walk that
/// only backs off one byte, or that truncates on the raw index, fails here.
#[test]
fn the_boundary_walk_steps_more_than_once_when_the_straddle_demands_it() {
    let text = format!("ab{}", "🦀".repeat(256)); // 2 + 1024 = 1026 bytes
    let out = bounded_reason(text);
    assert_eq!(
        out,
        format!("ab{}…", "🦀".repeat(255)),
        "boundaries sit at 2 mod 4; the last one at or below 1024 is 1022"
    );
    assert_eq!(out.len(), 1022 + MARKER_BYTES);
}

/// The band FLOOR: a 4-byte char spanning bytes 1021..1025 forces the cut
/// down to 1021, and 1021 + 3 marker bytes lands the elided value at exactly
/// `cap` — the minimum length an elision can have. Together with the ceiling
/// test above this pins the ontology's claim that an elided value always
/// measures `cap`..=`cap + 3` bytes.
#[test]
fn an_elided_value_never_measures_below_the_cap_band_floor() {
    let text = format!("{}{}", "a".repeat(1021), "🦀🦀"); // 1029 bytes
    let out = bounded_reason(text);
    assert_eq!(out, format!("{}…", "a".repeat(1021)));
    assert_eq!(
        out.len(),
        ERROR_MESSAGE_CAP,
        "band floor: the shortest possible elision measures exactly cap bytes"
    );
    assert!(out.ends_with('…'));
}

/// The other half of the length-disambiguation contract: a harness whose OWN
/// reason ends in `…` and fits under the cap rides verbatim — under the band,
/// a trailing marker is the harness's ellipsis, never a cut, and the bound
/// must not touch it.
#[test]
fn a_harness_own_ellipsis_under_the_cap_is_not_a_cut() {
    let text = "upstream provider truncated its own body…".to_string();
    assert!(text.len() < ERROR_MESSAGE_CAP);
    assert_eq!(
        bounded_reason(text.clone()),
        text,
        "a sub-cap value ending in … is complete text carried verbatim"
    );
}

/// Mint the `agent.completed.error.message` the adapter crate's private
/// `elide` produces for `message`, through the public seam: a synthetic codex
/// `turn.failed` line driven into `CodexAdapter::map_line` (the exact shape
/// `codex_adapter_guards.rs` uses).
fn adapter_carried_message(message: &str, entropy: u128) -> String {
    let line = serde_json::json!({
        "type": "turn.failed",
        "error": { "message": message },
    })
    .to_string();
    let mut substrate = CodexAdapter::new(RunId::new(Ulid::from_parts(7, entropy)));
    substrate
        .map_line(&line)
        .expect("a turn.failed line maps cleanly")
        .into_iter()
        .find(|f| f.subject == "agent.completed")
        .expect("turn.failed yields a completion fact")
        .payload["error"]["message"]
        .as_str()
        .expect("the failure reason rides the fact")
        .to_string()
}

/// The duplication guard. `bounded_reason` restates the adapter crate's
/// private `elide` because `Completion` is crate-private and the daemon
/// fallback cannot go through `into_fact`; the shared constant stops the
/// NUMBER drifting, but nothing structural stops the ALGORITHM drifting.
/// This pins byte-for-byte agreement across every boundary class above, so a
/// divergence in marker choice, boundary rule or band semantics turns a
/// silent fork into a red test naming the input that split them.
#[test]
fn the_daemon_fallback_and_the_adapter_elide_agree_byte_for_byte() {
    let cases: Vec<String> = vec![
        "x".repeat(ERROR_MESSAGE_CAP - 1),
        "x".repeat(ERROR_MESSAGE_CAP),
        "x".repeat(ERROR_MESSAGE_CAP + 1),
        "€".repeat(342),
        format!("ab{}", "🦀".repeat(256)),
        format!("{}{}", "a".repeat(1021), "🦀🦀"),
        "upstream provider truncated its own body…".to_string(),
    ];
    for (i, case) in cases.into_iter().enumerate() {
        let daemon_side = bounded_reason(case.clone());
        let adapter_side = adapter_carried_message(&case, i as u128 + 1);
        assert_eq!(
            daemon_side,
            adapter_side,
            "bounded_reason and the adapter's elide diverged on case {i} \
             ({} input bytes): the duplicated algorithm has forked",
            case.len()
        );
    }
}
