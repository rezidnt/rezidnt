//! DR-059 ORACLE — the GATE-TIME emitter's half of `patch?: CasRef`, judged
//! on the real golden path (criteria 1, 2, 3, 4): `gates::summarize_worktree`
//! pins real `git diff` unified bytes as a SECOND CAS blob, the gate-time
//! `diff.ready` emits it as `patch`, `merge_worktree` republishes the SAME
//! ref on `diff.merged`, the summary ref's bytes are unchanged under the
//! corrected mime, and the gate's `refs["diff"]` input still carries the
//! SUMMARY — the patch is NOT wired into the native-verifier input map.
//!
//! EMITTER SYMMETRY (criterion 1): this board and the watcher's
//! (`crates/rezidnt-adapters/git/tests/dr059_patch_ref.rs`) are the two
//! halves of the ontology's both-together rule. Landing one site leaves the
//! other board red.
//!
//! PLATFORM DISCLOSURE: `#![cfg(unix)]` like every daemon e2e (the daemon
//! implementation itself is unix-gated in `main.rs`), so host `/vet` cannot
//! see this judge. The host-side backstop is `dr059_patch_structure.rs`, on
//! the `registry_convergence_structure.rs` precedent.
//!
//! RED MODE (verified against the tree at cut time): ASSERT-RED. The
//! gate-time `diff.ready` payload is built at `runs.rs::run_pre_merge` as
//! `{worktree, diff}` — the quoted literal `"patch"` appears nowhere in
//! `gates.rs` or `runs.rs` (grep-verified this session) — so the patch
//! presence assertion fails first; the mime assertion fails against the old
//! `text/x-diff` literal at `gates.rs::summarize_worktree`.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{connect, make_gated_project, read_until, run_cli, send_line, start_daemon};
use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;
use serde_json::Value;

fn ref_field(payload: &Value, field: &str) -> CasRef {
    let value = &payload[field];
    assert!(
        value.is_object(),
        "payload must carry `{field}` as a CasRef object — got: {payload:#}"
    );
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("`{field}` must parse as a full CasRef ({e}): {value:#}"))
}

/// The whole DR-059 gate-side contract in one golden-path take. One test,
/// deliberately: the criteria are claims about ONE run's facts agreeing with
/// each other (same patch ref on two subjects, refs map pinned against that
/// same run's summary), so splitting them would re-run the multi-second
/// golden path per leg to judge relationships that only exist within a take.
#[test]
fn the_golden_path_pins_and_republishes_the_real_patch() {
    let daemon = start_daemon();
    let (project, spec) = make_gated_project(100);
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, &spec).expect("write spec");

    let out = run_cli(&daemon, &["open", spec_path.to_str().expect("utf8")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "gated open must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(45), |v| {
        v["subject"] == "diff.merged"
    });

    // --- The gate-time diff.ready: the daemon-minted pin, source
    // "rezidnt-adapter-git" (ontology `diff.ready` two-emitter note). Exactly
    // one per pre_merge is already pinned elsewhere; this board reads it.
    let gate_ready = lines
        .iter()
        .find(|v| v["subject"] == "diff.ready" && v["source"] == "rezidnt-adapter-git")
        .expect("the gate-time diff.ready is on the log");
    let summary = ref_field(&gate_ready["payload"], "diff");
    let patch = ref_field(&gate_ready["payload"], "patch");

    // Criterion 3 — the summary ref: content UNCHANGED, mime corrected.
    assert_eq!(
        summary.mime, "text/x-diff-summary",
        "the gate-time summary carries the corrected mime (DR-059 §Decision 4)"
    );
    let cas_root = daemon.db.parent().expect("db has a parent dir").join("cas");
    let cas = Cas::open(&cas_root).expect("open the daemon's CAS root");
    let summary_text =
        String::from_utf8_lossy(&cas.get(&summary).expect("summary blob resolves")).into_owned();
    assert!(
        summary_text
            .lines()
            .all(|l| l.len() > 2 && "AMD".contains(&l[..1]) && l.as_bytes()[1] == b'\t'),
        "the summary bytes are UNCHANGED — still one `<letter>\\t<path>` line \
         per touched file, the exact shape DiffScope/ForbiddenPath parse \
         (widening it in place is the alternative DR-059 REJECTED) — got:\n{summary_text}"
    );

    // Criterion 1 (gate side) — the patch: a second blob of real unified
    // diff bytes under the honest mime, carrying the stub's actual change.
    assert_eq!(
        patch.mime, "text/x-diff",
        "the patch mime is `text/x-diff` — the trusted label finally on real \
         diff bytes (DR-059 §Decision 5)"
    );
    assert_ne!(patch.hash, summary.hash, "a second blob, not a relabel");
    let patch_text =
        String::from_utf8_lossy(&cas.get(&patch).expect("patch blob resolves")).into_owned();
    assert!(
        patch_text.contains("diff --git") && patch_text.contains("cart.rs"),
        "the patch is unified `git diff` output naming the changed file — got:\n{patch_text}"
    );
    assert!(
        patch_text.contains("+oracle-change"),
        "the stub's change rides the patch as an added line — the reviewable \
         content the e2e finding proved missing — got:\n{patch_text}"
    );

    // Criterion 2 — diff.merged republishes the SAME gate-time patch ref,
    // threaded through merge_worktree exactly as `diff` already rides.
    let merged = lines
        .iter()
        .find(|v| v["subject"] == "diff.merged")
        .expect("diff.merged is on the log");
    assert_eq!(
        ref_field(&merged["payload"], "patch"),
        patch,
        "one gate-time product, re-attested on the merge fact — the identical \
         triple, not a re-summarize (ontology `diff.merged` `patch?` bullet)"
    );
    assert_eq!(
        ref_field(&merged["payload"], "diff"),
        summary,
        "and the summary ref rides through unchanged, as it always did"
    );

    // Criterion 4 — the gate's native-verifier input map is UNCHANGED:
    // `refs["diff"]` is the SUMMARY's content hash (what DiffScope/
    // ForbiddenPath parse), and NO `patch` key is wired in. Non-vacuous by
    // construction: the patch assertions above already proved a patch exists
    // on this very run, so "refs carries no patch" is a live exclusion, not
    // the status quo restated.
    let passed = lines
        .iter()
        .find(|v| v["subject"] == "gate.passed" && v["payload"]["gate"] == "pre_merge")
        .expect("the pre_merge pass is on the log");
    let verifiers = passed["payload"]["verifiers"]
        .as_array()
        .expect("per-verifier records on gate.passed");
    assert!(!verifiers.is_empty(), "pre_merge ran verifiers");
    for record in verifiers {
        let refs = record["inputs"]["refs"]
            .as_object()
            .unwrap_or_else(|| panic!("verifier inputs carry a refs map: {record:#}"));
        assert_eq!(
            refs.get("diff").and_then(Value::as_str),
            Some(format!("cas:blake3:{}", summary.hash).as_str()),
            "refs[\"diff\"] is the SUMMARY's hash — DiffScope/ForbiddenPath \
             keep parsing `<letter>\\t<path>` lines, BINDING pre_merge inputs \
             (DR-059 §Decision 3): {record:#}"
        );
        assert!(
            !refs.contains_key("patch"),
            "the patch bytes are NOT wired into the gate refs map — a native \
             wanting real diff bytes is its own record (DR-043's \
             one-ref-per-honest-need pattern): {record:#}"
        );
    }
}
