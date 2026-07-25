//! TRIALS-SLICE-B ENTRY ORACLE — criterion (b) of DR-050 §Decision 2:
//! `AdapterError::ContractViolated` (`crates/rezidnt-run/src/adapter.rs`,
//! `AdapterError::ContractViolated`) must be EXCLUDED from the stream loop's
//! tolerant garbage-line warn branch (LANDED — see below) and SURFACE as a
//! fact-worthy failure: the ratified `run.contract.violated` v1 fact
//! (`spec/ontology.md` "### DR-050 set").
//!
//! STATE OF THE CRITERION (truth pass, session 31): HALF-MET.
//! - The EXCLUSION arm landed in commit 462dee1: `drive_run` discriminates
//!   the variant from `BadLine` and stops mapping after the refusal. The
//!   first guard below (`the_stream_loop_names_contract_violated_…`) was
//!   ASSERT-RED when written and is GREEN since that commit — it stays as
//!   the containment pin.
//! - The SURFACING arm is UNBUILT: the refusal reaches `tracing` and the
//!   fallback completion's `error.message`, but no `run.contract.violated`
//!   fact is minted, and the fallback routing (`contract_violation
//!   .or(last_line)`) puts rezidnt-authored refusal text into a field the
//!   ontology ratifies as harness-authored ONLY (`agent.completed.error?`
//!   authorship boundary). The three new guards below are ASSERT-RED today.
//!
//! ## Why these judges are SOURCE-TEXT guards (disclosure, house style of
//! `registry_convergence_structure.rs`)
//!
//! No behavioral red test can reach the emitter on this tree: `drive_run`
//! constructs `ClaudeCodeAdapter` CONCRETELY, `ContractViolated`'s sole
//! construction site is `CodexAdapter::map_run_completed` (the
//! `terminal_turns > 1` guard), and `SUPPORTED_HARNESSES` still refuses
//! `codex` at open time — so no stream the daemon can run today produces
//! the error, and the exclusion arm cannot fire on any daemon path. Naming
//! the construct is necessary, not sufficient; the behavioral judge lands
//! with substrate selection at the `AgentSubstrate` seam (a daemon-drivable
//! stream that actually produces the variant). Each guard's residual window
//! is disclosed on the guard, not hidden.
//!
//! The arms of the work order that ARE behaviorally reachable today are
//! judged for real elsewhere, not here:
//! - the REDUCER fold (a pure function over events):
//!   `crates/rezidnt-state/tests/dr050_contract_violated_fold.rs` and the
//!   golden fixture `spec/fixtures/dr050_contract_violated_first_wins.jsonl`;
//! - the DEBRIEF dossier surface (seeded log + real CLI):
//!   `bins/rezidnt/tests/dr050_contract_violated_debrief.rs`;
//! - BadLine stays tolerated: `tests/dr050_badline_tolerated_e2e.rs` (unix);
//! - the fallback carrying the LAST STREAM LINE (the survivor of the
//!   routing removal): `tests/dr051_fallback_completion_fidelity_e2e.rs`
//!   (unix) already pins it behaviorally — the dying-stub sentinel rides
//!   `error.message` with no contract violation in play.
//!
//! ## RED MODE (stated plainly, per test)
//!
//! - `the_stream_loop_names_contract_violated_…` — GREEN since 462dee1
//!   (kept: containment for the exclusion arm).
//! - `the_daemon_mints_the_run_contract_violated_fact` — ASSERT-RED:
//!   `runs.rs` never names the subject.
//! - `harness_and_detail_are_lifted_structurally_not_reparsed` — ASSERT-RED:
//!   the only variant match is `ContractViolated { .. }`, binding neither
//!   field; the emitter cannot be lifting what it never binds.
//! - `the_fallback_reason_is_the_last_line_alone_not_the_refusal` —
//!   ASSERT-RED: the routing `contract_violation.or(last_line)` is present.

use std::path::PathBuf;

fn runs_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runs.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The source with all whitespace stripped, so rustfmt line-wrapping can
/// never flip a guard.
fn stripped() -> String {
    runs_rs().chars().filter(|c| !c.is_whitespace()).collect()
}

/// DR-050 §Decision 2(b), EXCLUSION arm: the daemon must discriminate
/// `ContractViolated` from garbage-line errors instead of whispering both
/// through the same warn-and-continue arm.
///
/// GREEN since commit 462dee1 (was ASSERT-RED when written) — kept as the
/// containment pin that the consumer stays non-blind to the variant.
#[test]
fn the_stream_loop_names_contract_violated_so_it_can_stop_whispering_it() {
    let source = runs_rs();
    assert!(
        source.contains("ContractViolated"),
        "DR-050 §Decision 2(b): `bins/rezidentd/src/runs.rs` handles every `map_line` error \
         with the same tolerant warn-and-continue, and it names `ContractViolated` nowhere — \
         so the daemon's loudest adapter failure mode (\"the contract this adapter's facts \
         rest on is false\", adapter.rs `AdapterError::ContractViolated`) would be silenced \
         by a branch designed for garbage lines. The stream loop must match the variant and \
         surface it as a fact-worthy failure, while `BadLine` stays tolerated (judged by \
         dr050_badline_tolerated_e2e.rs). Disclosure: this is a containment backstop — \
         naming the variant is necessary, not sufficient; the behavioral judge lands with \
         substrate selection."
    );
}

/// SURFACING arm, guard 1: the daemon mints the ratified fact. The ontology
/// names the emitter — "the daemon run supervisor (`drive_run`'s
/// `ContractViolated` exclusion arm)" — so `runs.rs` must reference the
/// subject `run.contract.violated` at all.
///
/// ASSERT-RED today: the subject string appears nowhere in `runs.rs`.
/// Disclosure: containment backstop — a daemon that names the subject but
/// publishes it with the wrong payload, envelope, or timing slips past this
/// text guard; the behavioral judge lands with substrate selection. The
/// payload/fold semantics are pinned behaviorally in rezidnt-state; the
/// envelope ruling (causation = the run's published `agent.completed` id
/// when held, else the spawn fact id) is ratified in the ontology block and
/// left to the auditor until a stream can drive it.
#[test]
fn the_daemon_mints_the_run_contract_violated_fact() {
    let source = runs_rs();
    assert!(
        source.contains("run.contract.violated"),
        "DR-050 set (surfacing arm): the adapter's refusal must become a durable \
         `run.contract.violated` fact on the fabric — today the refusal reaches tracing \
         and the fallback's error.message and NOTHING ELSE (a write-only variable, not a \
         fact; the auditor finding that forced the mint). `drive_run`'s exclusion arm is \
         the ratified emitter; `runs.rs` never names the subject, so no publish site can \
         exist. Disclosure: containment backstop — naming the subject is necessary, not \
         sufficient; the behavioral judge lands with substrate selection."
    );
}

/// SURFACING arm, guard 2: `harness` and `detail` are lifted STRUCTURALLY
/// from `AdapterError::ContractViolated { harness, detail }` — never parsed
/// back out of the prose `Display` text (which wraps detail in
/// "{harness}: recorded stream contract violated — {detail}; refusing …").
/// A publisher can only lift the fields structurally if it BINDS them, so
/// the source must destructure the variant's fields somewhere, not merely
/// match `{ .. }`.
///
/// ASSERT-RED today: the sole occurrence is
/// `matches!(e, AdapterError::ContractViolated { .. })` — no bindings.
/// Disclosure: containment backstop — binding the fields is necessary, not
/// sufficient (a publisher could bind them and still log `e.to_string()`);
/// the byte-exact judge is behavioral at the debrief surface
/// (`surfaced_detail_is_the_structural_field_not_the_display_wrapping`)
/// once the emitter is drivable, and structural at the fold until then.
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
         structurally — but `runs.rs` never destructures `ContractViolated {{ harness, \
         detail }}` (the only match is the field-blind `{{ .. }}`), so any payload it \
         could build today would have to re-parse the prose Display text, the exact move \
         the ontology forbids. Bind the fields at the exclusion arm and build the payload \
         from them."
    );
}

/// SURFACING arm, guard 3 (the routing REMOVAL): `agent.completed.error?`
/// carries harness-authored text ONLY (ratified authorship boundary; the
/// two fields "partition failure text by author" — adapter-composed
/// refusals ride `run.contract.violated.detail`). The fallback's reason
/// must therefore be `last_line` alone; the `contract_violation
/// .or(last_line)` routing that lets rezidnt-authored refusal text win the
/// harness-authored field must go.
///
/// ASSERT-RED today: the routing is present in `drive_run`'s fallback.
/// Disclosure: containment backstop against this exact expression — a
/// renamed variable routing the same refusal text into `error.message`
/// slips past; the behavioral judge (a codex stream refusing mid-run, then
/// asserting the fallback's error.message excludes the refusal text) lands
/// with substrate selection. The SURVIVOR half — the last stream line still
/// rides `error.message` — is already pinned behaviorally by
/// `dr051_fallback_completion_fidelity_e2e.rs` (unix), which this removal
/// must keep green.
#[test]
fn the_fallback_reason_is_the_last_line_alone_not_the_refusal() {
    let source = stripped();
    assert!(
        !source.contains("contract_violation.or(last_line)"),
        "DR-050 set (authorship boundary): the fallback completion routes the adapter's \
         refusal into `agent.completed.error.message` via `contract_violation.or(last_line)` \
         — but that field is harness-authored ONLY (spec/ontology.md, the `error?` clause); \
         the rezidnt-authored refusal now has its own ratified home \
         (`run.contract.violated.detail`). Remove the routing: the reason becomes \
         `last_line` alone, and the refusal rides the new fact instead."
    );
}
