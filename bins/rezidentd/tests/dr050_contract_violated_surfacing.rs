//! TRIALS-SLICE-B ENTRY ORACLE — criterion (b) of DR-050 §Decision 2:
//! `AdapterError::ContractViolated` (`crates/rezidnt-run/src/adapter.rs:82`)
//! must be EXCLUDED from the stream loop's tolerant garbage-line warn branch
//! and surface as a fact-worthy failure.
//!
//! THE DEFECT (DR-050 §Context finding 2, second trap): `drive_run`'s
//! `map_line` error arm tolerates EVERY adapter error the same way — a
//! `tracing::warn` and `continue`. That tolerance is deliberate and must stay
//! for `BadLine` ("a harness emitting a garbage line must not kill the run");
//! `ContractViolated` is categorically different — the adapter is saying the
//! contract its facts rest on is FALSE, refusing rather than logging a fact
//! the stream does not support (I3). Wired through the unmodified loop, the
//! adapter refuses loudly and the daemon whispers.
//!
//! ## Why the red judge is a SOURCE-TEXT guard (disclosure, house style of
//! `registry_convergence_structure.rs`)
//!
//! No behavioral red test can reach the defect on this tree:
//! `ContractViolated` has NO live producer inside `drive_run` — the loop
//! hardcodes `ClaudeCodeAdapter`, whose `map_line` never returns the variant,
//! and the daemon's `SUPPORTED_HARNESSES` still refuses `codex` at open time,
//! so no stream the daemon can run today produces the error. The guard below
//! is therefore a whole-file containment backstop: it demands `runs.rs`
//! reference `ContractViolated` at all (today it does not — the loop CANNOT be
//! discriminating a variant it never names). A daemon that named the variant
//! and still whispered it into the same warn-continue arm would slip past this
//! text guard — the window is disclosed, not hidden. The behavioral judge for
//! the surfacing arm arrives with the daemon-side codex wiring (a stream that
//! actually produces the variant); until then the crate-side guards
//! (`crates/rezidnt-run/tests/codex_adapter_guards.rs`) pin the producer's
//! behavior and this backstop pins that the consumer stopped being blind.
//!
//! The OTHER arm of criterion (b) — BadLine stays tolerated — is behaviorally
//! reachable today and is judged for real by
//! `tests/dr050_badline_tolerated_e2e.rs` (unix), not here.
//!
//! ## RED MODE (stated plainly)
//!
//! ASSERT-RED today: `bins/rezidentd/src/runs.rs` contains no occurrence of
//! `ContractViolated` anywhere, so the containment assertion fails. Green when
//! the stream loop's error handling names and discriminates the variant.

use std::path::PathBuf;

fn runs_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runs.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// DR-050 §Decision 2(b): the daemon must discriminate `ContractViolated` from
/// garbage-line errors instead of whispering both through the same
/// warn-and-continue arm.
///
/// ASSERT-RED today: `runs.rs` never names the variant.
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
         the daemon-side codex wiring."
    );
}
