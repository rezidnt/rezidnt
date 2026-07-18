//! SP2 oracle — §5 criterion 4: the PEP hook contract, decision→enforcement
//! mapping. The claude-code `PreToolUse` hook reads ONE `Reply::PermitDecision`
//! off the socket and maps its `decision` word to the harness's own
//! block/allow/ask output (design §3):
//!
//!   - `allow` → let the tool PROCEED,
//!   - `deny`  → BLOCK / non-proceed (reason surfaced to the agent),
//!   - `ask`   → route to the human ESCALATE surface (a client, I1).
//!
//! WHAT HAS A REAL JUDGE HERE: the wire-word → enforcement-class mapping is a
//! pure, total function over the three decision words, and it is judgeable
//! against `rezidnt_proto` today. WHAT DOES NOT: the full hook SCRIPT (stdin
//! tool descriptor → claude-code PreToolUse stdout/exit code) needs the hook
//! binary that DR-013 defers to the impl slice ("Deferred to the impl slice"
//! (b)). That leg is an #[ignore]-with-reason stub below, named for where it
//! belongs — NOT faked here.
//!
//! RED MODE: **assert-red** — `Enforcement::for_decision` (the pure
//! decision→enforcement mapping this criterion pins) does not exist on
//! `rezidnt_proto` yet, so the mapping tests fail to compile/assert until the
//! SP2 PEP contract lands. Encoded as a wire-adjacent pure judge so the mapping
//! is nailed down before the script that depends on it.

use rezidnt_proto::{Reply, decode_reply};

/// The three enforcement classes a PEP must produce from a decision. The hook
/// maps each to the harness's concrete PreToolUse output; this enum is the
/// transport-neutral contract the mapping is pinned against.
///
/// NOTE FOR THE IMPLEMENTER: if the SP2 impl names these differently (e.g. a
/// method on a `Decision` type, or a `pep` module), rename here to match — the
/// LOAD-BEARING assertion is the total, never-coerced mapping, not the type
/// path. What must NOT change: three distinct classes, `ask` is its own class
/// (never folded into allow), `deny` never proceeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedEnforcement {
    /// The tool call proceeds.
    Proceed,
    /// The tool call is blocked; the agent sees the reason.
    Block,
    /// Routed to the human escalate surface.
    Escalate,
}

/// §5 criterion 4: the decision word maps to exactly one enforcement class, and
/// the mapping is TOTAL and never-coercing — `deny` never proceeds, `ask` is its
/// own class and is never folded into `allow`. This is the honesty the whole
/// permit axis rests on (I6): the PEP cannot silently upgrade an `ask`/`deny`
/// into a proceed.
///
/// ASSERT/COMPILE-RED until `rezidnt_proto::Enforcement::for_decision` (the
/// pure mapping) exists.
#[test]
fn decision_word_maps_to_the_right_enforcement_class() {
    // The SP2 mapping under test. Compile-red anchor: `Enforcement::for_decision`
    // is the pure function DR-013's §3 contract implies but does not exist yet.
    let cases = [
        ("allow", ExpectedEnforcement::Proceed),
        ("deny", ExpectedEnforcement::Block),
        ("ask", ExpectedEnforcement::Escalate),
    ];
    for (word, expected) in cases {
        let got = rezidnt_proto::Enforcement::for_decision(word);
        let got_expected = match got {
            rezidnt_proto::Enforcement::Proceed => ExpectedEnforcement::Proceed,
            rezidnt_proto::Enforcement::Block => ExpectedEnforcement::Block,
            rezidnt_proto::Enforcement::Escalate => ExpectedEnforcement::Escalate,
        };
        assert_eq!(
            got_expected, expected,
            "decision {word:?} must map to {expected:?} enforcement (criterion 4)"
        );
    }
}

/// §5 criterion 4 (the never-coerce corner): `ask` and `deny` must NOT map to
/// Proceed. This is the assertion that makes a silent-allow regression a test
/// failure rather than a shrug — the PEP is forbidden from turning an
/// unresolved/denied decision into a proceed (I6).
///
/// ASSERT/COMPILE-RED until the mapping exists.
#[test]
fn ask_and_deny_never_map_to_proceed() {
    for word in ["ask", "deny"] {
        assert_ne!(
            rezidnt_proto::Enforcement::for_decision(word),
            rezidnt_proto::Enforcement::Proceed,
            "decision {word:?} must NEVER enforce as Proceed — no silent allow (I6, criterion 4)"
        );
    }
}

/// §5 criterion 4 (end-to-end over the wire): a decoded `Reply::PermitDecision`
/// drives the enforcement mapping — the PEP reads the frame off the socket and
/// enforces the word it carries. This ties the pure mapping to the actual reply
/// the daemon writes (the frame the SP2 socket handler produces).
///
/// ASSERT/COMPILE-RED until the mapping exists.
#[test]
fn a_decoded_deny_reply_enforces_as_block() {
    let line = r#"{"reply":"permit_decision","request_id":"01SP2PEPWIRE00000000000001","decision":"deny","reason":"tool Bash not in allowlist"}"#;
    let reply = decode_reply(line).expect("decode permit_decision reply");
    let Reply::PermitDecision { decision, .. } = reply else {
        panic!("expected a PermitDecision reply, got {reply:?}");
    };
    assert_eq!(
        rezidnt_proto::Enforcement::for_decision(&decision),
        rezidnt_proto::Enforcement::Block,
        "a deny reply off the socket enforces as Block/non-proceed (criterion 4)"
    );
}

// §5 criterion 4 (script-level leg) — the stdin→PreToolUse-output SCRIPT
// contract. DR-013 deferred this "to the impl slice"; DR-014 (ACCEPTED) settles
// where the hook lives — the `rezidnt permit-hook` CLI subcommand (§Decision 1)
// — so the honest judge now EXISTS and is written, not ignored. It cannot live
// in this proto crate (the subcommand is in the `rezidnt` binary, which does not
// depend on this crate's test harness), so it is the subcommand-level
// integration board `bins/rezidnt/tests/permit_hook.rs` (spawn the binary with a
// stdin tool descriptor + REZIDNT_SOCKET, capture stdout). The `#[ignore]` stub
// that used to sit here is DELETED: now that the design is settled, an ignored
// placeholder for a criterion with a real judge would be dishonest coverage.
//
// The pure decision→enforcement mapping this crate DOES own is judged above by
// `decision_word_maps_to_the_right_enforcement_class` and
// `ask_and_deny_never_map_to_proceed` — those pin the never-coerce contract the
// script's output mapping is built on.
