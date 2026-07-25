//! TRIALS-SLICE-B ENTRY ORACLE — criterion (b) of DR-050 §Decision 2:
//! `AdapterError::ContractViolated` (`crates/rezidnt-run/src/adapter.rs`)
//! must be EXCLUDED from the stream loop's tolerant garbage-line warn branch
//! and SURFACE as a fact-worthy failure: the ratified `run.contract.violated`
//! v1 fact (`spec/ontology.md` "### DR-050 set").
//!
//! STATE OF THE CRITERION (truth pass, session 32): BUILT. Every arm of the
//! surfacing work order is in `drive_run`: the exclusion arm discriminates
//! the variant from `BadLine` (462dee1); the emitter mints the fact with
//! `harness`/`detail` lifted structurally from the variant's fields; the
//! `contract_violation.or(last_line)` routing is REMOVED (`agent.completed
//! .error?` is harness-authored again); and a refused loop keeps DRAINING
//! stdout while it stops MAPPING. Every guard below is a GREEN regression
//! pin. The four originals were ASSERT-RED when written; the two drain
//! guards were added by the session-32 remediation AFTER the arm landed and
//! were proven able to go red by mutation (construct removed or reordered,
//! guard red, source restored byte-identical) before being reported green.
//!
//! ## These guards match CODE, not prose (the session-32 debrief FAIL)
//!
//! `runs.rs` documents this arm heavily, and its COMMENTS name the subject
//! and the variant independently of the code. A bare `contains` over the raw
//! source was therefore satisfiable by prose alone — delete the publish,
//! keep the comment, nothing goes red: the fan-out silent-wrong class,
//! caught by the auditor. Every guard now matches over the source with
//! comments REMOVED (string literals preserved — [`strip_comments`]) and
//! anchors on code-shaped tokens (`Subject::new("run.contract.violated")`,
//! `AdapterError::ContractViolated {`), never on a bare string that prose
//! can carry.
//!
//! ## Why these judges are SOURCE-TEXT guards at all (disclosure, house
//! style of `registry_convergence_structure.rs`)
//!
//! No behavioral red test can reach the emitter on this tree: `drive_run`
//! constructs `ClaudeCodeAdapter` CONCRETELY, `ContractViolated`'s sole
//! construction site is `CodexAdapter::map_run_completed` (the
//! `terminal_turns > 1` guard), and `SUPPORTED_HARNESSES` still refuses
//! `codex` at open time — so no stream the daemon can run today produces the
//! error, and the exclusion arm cannot fire on any daemon path. Naming the
//! construct is necessary, not sufficient; the behavioral judge lands with
//! substrate selection at the `AgentSubstrate` seam (a daemon-drivable
//! stream that actually produces the variant). Each guard's residual window
//! is disclosed on the guard, not hidden.
//!
//! The arms of the work order that ARE behaviorally reachable are judged for
//! real elsewhere, not here:
//! - the REDUCER fold (a pure function over events):
//!   `crates/rezidnt-state/tests/dr050_contract_violated_fold.rs` and the
//!   golden fixture `spec/fixtures/dr050_contract_violated_first_wins.jsonl`;
//! - the DEBRIEF dossier surface, JSON and human (seeded log + real CLI):
//!   `bins/rezidnt/tests/dr050_contract_violated_debrief.rs`;
//! - BadLine stays tolerated: `tests/dr050_badline_tolerated_e2e.rs` (unix);
//! - the fallback carrying the LAST STREAM LINE (the survivor of the
//!   routing removal): `tests/dr051_fallback_completion_fidelity_e2e.rs`
//!   (unix) pins it behaviorally — the dying-stub sentinel rides
//!   `error.message` with no contract violation in play.

use std::path::PathBuf;

fn runs_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runs.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The guard substrate: Rust source with every `//` line comment and
/// `/* */` block comment removed, string literals preserved — a subject
/// name inside `Subject::new("…")` is code; the same name inside a comment
/// is prose, and a guard satisfiable by prose is not a judge (the
/// session-32 debrief FAIL).
///
/// Handles the constructs `runs.rs` actually uses, and NO more. Verified
/// against the file as it stands: one non-nested block comment, no raw
/// strings, no `'"'` char literal.
///
/// TWO CONSTRUCTS WOULD BREAK IT TOWARD GREEN — i.e. toward re-admitting
/// prose as guard substrate, the unsafe direction (session-32 debrief
/// finding B; an earlier version of this note claimed the only failure mode
/// was dropping code, which is false):
///   1. **Nested block comments** (legal in Rust). The scanner stops at the
///      first `*/`, so the outer comment's remaining prose lands in `out`.
///   2. **Any unpaired `"` earlier in the file** — a `'"'` char literal, or
///      a raw string containing `"`. String parity flips, and the scanner
///      then swallows following comments as if they were string content.
///
/// Either would let a guard pass on prose alone, which is exactly the defect
/// this helper exists to close. If `runs.rs` ever grows one, this stripper
/// must be replaced with a real lexer, not patched.
///
/// `the_comment_stripper_strips_prose_and_keeps_code` asserts that precondition
/// over `runs.rs` — soundly for nesting (it bounds `/*` at one, and nesting
/// needs two), and for the TWO LIKELY SPELLINGS of the parity hazard (`r#"` and
/// `'"'`). It is not exhaustive: an escaped-quote char literal (`'\"'`) or a
/// hash-less `r"…\"` would flip parity uncaught. Both absent today.
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                out.push_str(&source[start..i]);
            }
            _ => {
                let start = i;
                i += 1;
                while i < bytes.len() && !matches!(bytes[i], b'/' | b'"') {
                    i += 1;
                }
                out.push_str(&source[start..i]);
            }
        }
    }
    out
}

/// `runs.rs`, comments stripped, then all whitespace removed so rustfmt
/// line-wrapping can never flip a guard.
fn stripped() -> String {
    strip_comments(&runs_rs())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// The stripper is itself load-bearing for every guard below, so it gets its
/// own judge: prose (line comments, block comments, trailing comments) must
/// vanish; code — including string literals carrying the very same tokens —
/// must survive.
#[test]
fn the_comment_stripper_strips_prose_and_keeps_code() {
    let sample = "// prose names Subject::new(\"run.contract.violated\") here\n\
                  /* and here: contract_refused = true */\n\
                  let s = Subject::new(\"run.contract.violated\"); // trailing: ContractViolated\n";
    let stripped: String = strip_comments(sample)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        stripped.contains(r#"lets=Subject::new("run.contract.violated");"#),
        "code and its string literals survive stripping — got {stripped:?}"
    );
    assert!(
        !stripped.contains("prose") && !stripped.contains("trailing"),
        "line and trailing comments are removed — got {stripped:?}"
    );
    assert!(
        !stripped.contains("contract_refused=true"),
        "block comments are removed — got {stripped:?}"
    );
    assert!(
        !stripped.contains("ContractViolated"),
        "a token that exists ONLY in prose does not survive — got {stripped:?}"
    );

    // THE PRECONDITION, ASSERTED (session-32 debrief finding F4). Everything
    // above tests the stripper on a sample; this tests the SUBSTRATE. The
    // stripper is not a lexer, and its two known holes both break toward GREEN
    // — they re-admit prose, silently restoring the exact defect every guard in
    // this file exists to close. Documenting them is not enough when the thing
    // keeping them hypothetical is an unchecked claim about another file.
    let runs = runs_rs();
    for (needle, what) in [
        (
            "/*",
            "a nested block comment would end the strip at the first `*/`",
        ),
        (
            "r#\"",
            "a raw string containing `\"` would flip the scanner's string parity",
        ),
        (
            "'\"'",
            "a double-quote char literal would flip the scanner's string parity",
        ),
    ] {
        let count = runs.matches(needle).count();
        // `/*` legitimately appears once (`/* egress_active */`); the hazard is
        // NESTING, which needs a second `/*` before the matching `*/`. The other
        // two must not appear at all.
        let allowed = usize::from(needle == "/*");
        assert!(
            count <= allowed,
            "`runs.rs` now contains {count} occurrence(s) of `{needle}` (at most \
             {allowed} is safe): {what}, re-admitting comment prose as guard \
             substrate and turning every guard in this file green-by-default. \
             The CHEAPEST correct response to this going red is to use `//` \
             comments in `runs.rs` instead; the next-cheapest is to replace \
             `strip_comments` with a real lexer. Do not patch the stripper, and \
             do not relax this assertion"
        );
    }
}

/// DR-050 §Decision 2(b), EXCLUSION arm: the daemon discriminates
/// `ContractViolated` from garbage-line errors instead of whispering both
/// through the same warn-and-continue arm.
///
/// GREEN pin (ASSERT-RED when written; landed in 462dee1). Session-32
/// hardening: matches the comment-stripped source for the code-shaped
/// `AdapterError::ContractViolated {` match — the raw file names the variant
/// in prose independently of the code, and the old bare `contains` was
/// satisfiable by comments alone. Disclosure: containment backstop — naming
/// the variant is necessary, not sufficient; the behavioral judge lands with
/// substrate selection. `BadLine` staying tolerated is judged behaviorally
/// by `dr050_badline_tolerated_e2e.rs`.
#[test]
fn the_stream_loop_names_contract_violated_so_it_can_stop_whispering_it() {
    assert!(
        stripped().contains("AdapterError::ContractViolated{"),
        "DR-050 §Decision 2(b): `drive_run`'s stream loop must match \
         `AdapterError::ContractViolated` as its own arm — in CODE, not comments (this \
         guard strips prose before matching). Without the arm, the daemon's loudest \
         adapter failure mode (\"the contract this adapter's facts rest on is false\", \
         adapter.rs `AdapterError::ContractViolated`) is silenced by the branch \
         designed for garbage lines."
    );
}

/// SURFACING arm, guard 1: the daemon mints the ratified fact. The ontology
/// names the emitter — "the daemon run supervisor (`drive_run`'s
/// `ContractViolated` exclusion arm)" — so `runs.rs` must CONSTRUCT the
/// subject: `Subject::new("run.contract.violated")`, a code-shaped token the
/// comments cannot carry once stripped.
///
/// GREEN pin (ASSERT-RED when written). Session-32 hardening: the old guard
/// matched the bare subject string over the RAW source, which two comments
/// satisfied independently of the code — delete the publish, keep the prose,
/// and the ONLY judge of the emitter stayed green (the fan-out silent-wrong
/// class). Proven red-by-mutation: subject construction removed with the
/// prose kept, this guard red, source restored.
///
/// Disclosure: containment backstop — constructing the subject is necessary,
/// not sufficient; a daemon that publishes it with the wrong payload,
/// envelope, or timing slips past a text guard. The payload/fold semantics
/// are pinned behaviorally in rezidnt-state; the envelope ruling (causation =
/// the run's published `agent.completed` id when held, else the spawn fact
/// id) is ratified in the ontology block and left to the auditor until a
/// stream can drive the emitter.
#[test]
fn the_daemon_mints_the_run_contract_violated_fact() {
    assert!(
        stripped().contains(r#"Subject::new("run.contract.violated")"#),
        "DR-050 set (surfacing arm): the adapter's refusal must become a durable \
         `run.contract.violated` fact on the fabric, and `drive_run`'s exclusion arm is \
         the ratified emitter — so `runs.rs` must construct \
         `Subject::new(\"run.contract.violated\")` in CODE (this guard strips comments \
         before matching; prose naming the subject does not count). A refusal that \
         reaches tracing and nothing else is a write-only variable, not a fact."
    );
}

/// SURFACING arm, guard 2: `harness` and `detail` are lifted STRUCTURALLY
/// from `AdapterError::ContractViolated { harness, detail }` — never parsed
/// back out of the prose `Display` text (which wraps detail in
/// "{harness}: recorded stream contract violated — {detail}; refusing …").
/// A publisher can only lift the fields structurally if it BINDS them, so
/// the comment-stripped source must destructure the variant's fields
/// somewhere, not merely match `{ .. }`.
///
/// GREEN pin (ASSERT-RED when written: the sole occurrence was the
/// field-blind `matches!(e, AdapterError::ContractViolated { .. })`).
/// Disclosure: containment backstop — binding the fields is necessary, not
/// sufficient (a publisher could bind them and still log `e.to_string()`);
/// the byte-exact judge is behavioral at the debrief surface
/// (`surfaced_detail_is_the_structural_field_not_the_display_wrapping`) and
/// at the fold.
#[test]
fn harness_and_detail_are_lifted_structurally_not_reparsed() {
    let source = stripped();
    let destructures = source.match_indices("ContractViolated{").any(|(i, _)| {
        let window = &source[i..source.len().min(i + 160)];
        let head = &window[..window.find('}').unwrap_or(window.len())];
        head.contains("harness") && head.contains("detail")
    });
    assert!(
        destructures,
        "DR-050 set: the fact's `harness` and `detail` are the VARIANT'S FIELDS, lifted \
         structurally — `runs.rs` must destructure `ContractViolated {{ harness, \
         detail }}` (a field-blind `{{ .. }}` match cannot lift anything), and any \
         payload built without the bindings would have to re-parse the prose Display \
         text, the exact move the ontology forbids."
    );
}

/// SURFACING arm, guard 3 (the routing REMOVAL): `agent.completed.error?`
/// carries harness-authored text ONLY (ratified authorship boundary; the
/// two fields "partition failure text by author" — adapter-composed
/// refusals ride `run.contract.violated.detail`). The fallback's reason is
/// `last_line` alone; the `contract_violation.or(last_line)` routing that
/// let rezidnt-authored refusal text win the harness-authored field is
/// REMOVED and must stay removed.
///
/// GREEN pin (ASSERT-RED when written: the routing was present). Matching
/// over the comment-stripped source keeps a comment that merely NAMES the
/// dead routing from flipping this negative guard. Disclosure: containment
/// backstop against this exact expression — a renamed variable routing the
/// same refusal text into `error.message` slips past; the behavioral judge
/// (a codex stream refusing mid-run, then asserting the fallback's
/// error.message excludes the refusal text) lands with substrate selection.
/// The SURVIVOR half — the last stream line still rides `error.message` —
/// is pinned behaviorally by `dr051_fallback_completion_fidelity_e2e.rs`
/// (unix), which stayed green through the removal.
#[test]
fn the_fallback_reason_is_the_last_line_alone_not_the_refusal() {
    assert!(
        !stripped().contains("contract_violation.or(last_line)"),
        "DR-050 set (authorship boundary): the fallback completion must not route the \
         adapter's refusal into `agent.completed.error.message` via \
         `contract_violation.or(last_line)` — that field is harness-authored ONLY \
         (spec/ontology.md, the `error?` clause); the rezidnt-authored refusal rides \
         `run.contract.violated.detail`. The reason is `last_line` alone."
    );
}

/// The DRAIN GUARD (session-32 auditor finding: previously judged by
/// NOTHING). After a refusal the loop keeps DRAINING stdout — an unread pipe
/// blocks the child against the reap's `child.wait()` and deadlocks — but
/// stops MAPPING: every later fact would rest on the premise the adapter
/// just withdrew (I3). Pins both halves structurally: the
/// `if contract_refused { continue; }` short-circuit exists (CONTINUE, so
/// the drain keeps running — never a break), and it sits BEFORE the loop's
/// `map_line` call. An implementer who moves the guard below `map_line`, or
/// drops it, goes red here.
///
/// GREEN pin added post-landing (session-32 remediation), proven able to go
/// red by mutation (guard deleted → red; guard moved below `map_line` →
/// red) before being reported. Disclosure: structural — a short-circuit that
/// skips `map_line` but leaves some new fact-producing call above itself
/// would slip past; the behavioral judge (a refusing stream, then asserting
/// no post-refusal facts) lands with substrate selection.
#[test]
fn after_a_refusal_the_loop_drains_without_mapping() {
    let source = stripped();
    let guard_at = source
        .find("ifcontract_refused{continue;}")
        .unwrap_or_else(|| {
            panic!(
                "DR-050 set: `drive_run`'s stream loop must short-circuit with \
             `if contract_refused {{ continue; }}` once the adapter has refused — \
             CONTINUE, keeping the drain alive (an unread pipe blocks the child \
             against `child.wait()` and hangs the reap; the bytes stay evidence), \
             while mapping stops (facts resting on a withdrawn premise, I3). The \
             token is absent from the comment-stripped source."
            )
        });
    let map_at = source.find(".map_line(").unwrap_or_else(|| {
        panic!(
            "premise of this guard: the stream loop maps lines via `.map_line(` — \
             the call is absent from the comment-stripped source, so the guard \
             cannot locate the mapping it protects; re-anchor it"
        )
    });
    assert!(
        guard_at < map_at,
        "the refusal short-circuit must run BEFORE `map_line` — placed after it, a \
         refused stream keeps minting facts on a withdrawn premise for the rest of \
         the run (I3). Short-circuit at byte {guard_at}, `.map_line(` at byte \
         {map_at} of the comment-stripped source"
    );

    // AND `last_line` is captured ABOVE the short-circuit (session-32 debrief
    // finding F3 — this position was changed, then reverted, with nothing
    // pinning it either way). It is NOT covered by the guard above: the drain
    // guard stops adapter MAPPING, while `last_line` feeds the fallback's
    // `agent.completed.error.message`, and the ontology's `run.contract.violated`
    // Timing bullet rules that a pre-completion refusal still lets the DR-051
    // fallback carry "the last stream line". Below the short-circuit, the
    // fallback would carry the last line up to and INCLUDING the refusal
    // instead — a narrowing of ratified semantics, and one no behavioral test
    // can reach while the emitter is unreachable. So it is pinned here.
    //
    // RESIDUAL, disclosed: both anchors are `str::find` — FIRST occurrence over
    // the whole stripped file. This pin holds against a MOVE (the sole capture
    // relocating below the guard) but not against an ADDITION (a SECOND
    // `last_line = Some(` added below it leaves the first one, and this
    // assertion, untouched). One occurrence today.
    let last_line_at = source.find("last_line=Some(").unwrap_or_else(|| {
        panic!(
            "premise of this guard: `drive_run` records the child's last stream \
             line as `last_line = Some(...)` — absent from the comment-stripped \
             source, so the guard cannot locate what it pins; re-anchor it"
        )
    });
    assert!(
        last_line_at < guard_at,
        "`last_line` must be captured BEFORE the refusal short-circuit. Post-refusal \
         lines are still HARNESS-authored, and the ontology's Timing bullet ratifies \
         that a pre-completion refusal leaves the DR-051 fallback carrying the last \
         stream line while the refusal itself rides `run.contract.violated.detail` — \
         the two facts partition failure text by AUTHOR. Moving this below the \
         short-circuit narrows that ratified semantics and needs a `/dr`, not an \
         edit. Capture at byte {last_line_at}, short-circuit at byte {guard_at}"
    );
}

/// The FLAG SET-POINT (session-32 auditor finding, the other unjudged half):
/// `contract_refused` is set in exactly ONE place — inside the
/// `ContractViolated` arm, and only AFTER the `run.contract.violated`
/// publish has been `?`-propagated (`publish(…).await?`). A failed append
/// propagates out and the flag is never set, so the daemon can never sit in
/// the state "silently draining, nothing recorded" — the exact write-only
/// state the DR-050 mint exists to make impossible.
///
/// GREEN pin added post-landing (session-32 remediation), proven able to go
/// red by mutation (set-site moved above the publish → red) before being
/// reported. Disclosure: structural and ordering-only — a second set-site
/// outside the arm, or the same arm rewritten so publish success no longer
/// gates the flag through `?`, is judged here only as far as source order
/// carries; the behavioral judge lands with substrate selection.
#[test]
fn the_refusal_flag_is_set_once_and_only_after_the_durable_publish() {
    let source = stripped();
    assert_eq!(
        source.matches("contract_refused=true").count(),
        1,
        "exactly ONE set-site for `contract_refused` — the ContractViolated arm. A \
         second writer would decouple \"stopped mapping\" from \"the refusal is \
         durably on the fabric\", the invariant the flag encodes"
    );
    let arm_at = source
        .find("AdapterError::ContractViolated{")
        .unwrap_or_else(|| {
            panic!(
                "premise of this guard: the exclusion arm exists (judged by \
                 the_stream_loop_names_contract_violated_so_it_can_stop_whispering_it)"
            )
        });
    let arm = &source[arm_at..];
    let flag_at = arm.find("contract_refused=true").unwrap_or_else(|| {
        panic!(
            "the single `contract_refused = true` set-site must live INSIDE (after \
             the start of) the ContractViolated arm — found only before it"
        )
    });
    let before_flag = &arm[..flag_at];
    assert!(
        before_flag.contains(r#"Subject::new("run.contract.violated")"#)
            && before_flag.contains(").await?"),
        "the flag is set only AFTER the `run.contract.violated` fact is durably on \
         the fabric: between the arm's start and the set-site, the comment-stripped \
         source must construct the subject AND `?`-propagate a publish \
         (`.await?`). Set before the publish, a failed append would leave the \
         daemon draining silently with NOTHING recorded — the write-only-refusal \
         state the mint exists to make impossible"
    );
}
