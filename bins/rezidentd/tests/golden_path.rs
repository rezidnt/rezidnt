//! S4 oracle — the slice exit: an agent spawned under rezidnt gates produces
//! a VERIFIED merged diff with replayable `debrief` and recorded cost. The
//! golden path completes here (constitution: the BINDING demo contract).
//! Plus the `debrief` / `gate why` CLI verbs with the DR-004 exit codes and
//! the replay INTEGRITY ALARM (doc §8, the compliance sentence).
//!
//! RED MODES (verified at board time):
//! - `golden_path_…`: assert-red — no gate engine exists; the tail deadline
//!   expires before any `gate.entered(pre_merge)`/`diff.merged` appears.
//! - CLI tests: assert-red — `debrief` and `gate why` are unknown
//!   subcommands today; clap exits 2 where the board pins 0/3/5.
//!
//! Oracle scoping for the exit (stated for the work order): diff-scope and
//! forbidden-path run as REAL natives (cheap, deterministic, replayable from
//! log + CAS); the test-suite runner is EXEC-STUBBED (`tests-pass` speaks
//! the §8 contract from a script) — a real cargo-test verifier adds minutes
//! of wall clock and zero new contract surface to this board.
#![cfg(unix)]

mod common;

use std::process::Command;
use std::time::Duration;

use common::{
    connect, make_gated_project, read_until, run_cli, seed_db_from_fixture, send_line,
    start_daemon, start_daemon_prepared,
};
use serde_json::json;

/// THE EXIT. One take, socket + CLI only:
/// open (gated spec) → vet passes pre-spawn → stub harness writes a real
/// change in its worktree → `diff.ready` (CAS-pinned) → pre_merge runs
/// diff-scope + forbidden-path (native) + tests-pass (exec stub) against the
/// CAS ref → `gate.passed` with per-verifier recorded cost → the diff is
/// MERGED (visible in the repo AND as `diff.merged` on the log) → `rezidnt
/// debrief <run>` replays the recorded verdicts clean and reports the cost.
#[test]
fn golden_path_verified_merged_diff_with_replayable_debrief_and_cost() {
    let daemon = start_daemon();
    let (project, spec) = make_gated_project(100);
    let repo = project.path().join("repo");
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, &spec).expect("write spec");

    let out = run_cli(&daemon, &["open", spec_path.to_str().expect("utf8")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "gated open must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let run_id = stdout
        .split_whitespace()
        .last()
        .expect("pinned open shape: `opened <name> run <run-ulid>`")
        .to_string();
    assert_eq!(
        run_id.len(),
        26,
        "the trailing token is the run ULID: {stdout:?}"
    );

    // The whole verified-merge chain lands on the log, in causal order.
    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(45), |v| {
        v["subject"] == "diff.merged"
    });
    let subjects: Vec<(String, String)> = lines
        .iter()
        .map(|v| {
            (
                v["subject"].as_str().unwrap_or_default().to_string(),
                v["payload"]["gate"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect();
    let pos = |sub: &str, gate: &str| {
        subjects
            .iter()
            .position(|(s, g)| s == sub && g == gate)
            .unwrap_or_else(|| panic!("{sub}({gate}) never appeared; saw {subjects:?}"))
    };
    assert!(
        pos("gate.passed", "vet") < pos("agent.spawned", ""),
        "vet before spawn"
    );
    assert!(
        pos("diff.ready", "") < pos("gate.entered", "pre_merge"),
        "pre_merge verifies the CAS-pinned diff, so it enters after diff.ready"
    );
    assert!(
        pos("gate.passed", "pre_merge") < pos("diff.merged", ""),
        "merge happens only after the VERIFIED verdict"
    );

    // The pre_merge pass carries per-verifier records: both real natives and
    // the exec stub, each with its recorded cost (the exit's "recorded
    // cost") and content-hash-pinned inputs.
    let passed = &lines[pos("gate.passed", "pre_merge")];
    let verifiers = passed["payload"]["verifiers"]
        .as_array()
        .expect("per-verifier records on gate.passed (proposed v1 shape)");
    for name in ["diff-scope", "forbidden-path", "tests-pass"] {
        let record = verifiers
            .iter()
            .find(|v| v["verifier"] == json!(name))
            .unwrap_or_else(|| panic!("verifier {name} missing from {verifiers:#?}"));
        assert!(record["cost_ms"].is_u64(), "{name}: cost_ms recorded");
        assert!(
            record["inputs"]["refs"]["diff"]
                .as_str()
                .is_some_and(|r| r.starts_with("cas:blake3:")),
            "{name}: inputs pinned by content hash (§8 BINDING)"
        );
    }

    // diff.merged names this run, and the merge is REAL: the stub's change
    // reached the repo's checked-in history.
    let merged = &lines[pos("diff.merged", "")];
    assert_eq!(merged["payload"]["run"], json!(run_id));
    let show = Command::new("git")
        .args(["show", "HEAD:src/checkout/cart.rs"])
        .current_dir(&repo)
        .output()
        .expect("git show");
    assert!(
        String::from_utf8_lossy(&show.stdout).contains("oracle-change"),
        "the VERIFIED diff must actually be merged into the repo; got: {}",
        String::from_utf8_lossy(&show.stdout)
    );

    // Replayable debrief with recorded cost: exit 0 (verdicts replay clean),
    // zero alarms, the run's dossier cost carried through.
    let debrief = run_cli(&daemon, &["debrief", &run_id, "--json"]);
    assert_eq!(
        debrief.status.code(),
        Some(0),
        "a clean replay debrief exits 0; stderr: {}",
        String::from_utf8_lossy(&debrief.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&debrief.stdout).expect("--json debrief is machine-readable");
    assert_eq!(report["alarms"], json!([]), "an honest log replays clean");
    assert_eq!(report["gates"]["vet"]["verdict"], "pass");
    assert_eq!(report["gates"]["pre_merge"]["verdict"], "pass");
    assert_eq!(
        report["cost"]["total_usd"].as_f64(),
        Some(0.001),
        "recorded cost (the stub harness's stream-json total_cost_usd) rides the debrief"
    );
}

/// DR-004: `debrief` on a run whose recorded verdict is `fail` — and whose
/// replay AGREES — exits 5 (gate-fail), no alarm. The events and CAS blobs
/// are seeded programmatically pre-daemon (log + CAS are the truth, I3).
#[test]
fn cli_debrief_fail_exits_5_when_replay_agrees() {
    let run = "01S4FA1AGREED0000000000R01";
    let daemon = start_daemon_prepared(|dir| {
        let cas = rezidnt_cas::Cas::open(&dir.join("cas")).expect("open cas");
        let diff = cas
            .put(b"M\tsrc/payments/mod.rs\n", "text/plain")
            .expect("seed diff");
        let evidence = cas
            .put(b"scope violation: src/payments/mod.rs\n", "text/plain")
            .expect("seed evidence");
        let mut log = rezidnt_fabric::EventLog::open(&dir.join("events.db")).expect("open db");
        let correlation = ulid::Ulid::new();
        let entered = rezidnt_types::Event::new(
            rezidnt_types::SourceId::new("rezidnt-gate"),
            None,
            rezidnt_types::Subject::new("gate.entered"),
            correlation,
            None,
            1,
            json!({"run": run, "gate": "pre_merge"}),
        )
        .expect("event");
        log.append(&entered).expect("append");
        let failed = rezidnt_types::Event::new(
            rezidnt_types::SourceId::new("rezidnt-gate"),
            None,
            rezidnt_types::Subject::new("gate.failed"),
            correlation,
            Some(entered.id),
            1,
            json!({
                "run": run,
                "gate": "pre_merge",
                "verifier": "diff-scope",
                "evidence": [evidence],
                "inputs": {
                    "gate": "pre_merge",
                    "refs": {"diff": format!("cas:blake3:{}", diff.hash)},
                    "params": {"allow": ["src/checkout/**"]},
                    "timeout_ms": 120000
                }
            }),
        )
        .expect("event");
        log.append(&failed).expect("append");
    });

    let out = run_cli(&daemon, &["debrief", run, "--json"]);
    assert_eq!(
        out.status.code(),
        Some(5),
        "recorded fail, replayed fail: gate-fail exits 5 (DR-004); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("--json debrief");
    assert_eq!(report["alarms"], json!([]), "agreement is not an alarm");
    assert_eq!(report["gates"]["pre_merge"]["verdict"], "fail");
}

/// DR-004 + I6: a recorded `inconclusive` debriefs to exit 3 — NOT 5, NOT 0,
/// never coerced. Per the v1 replay policy it is reported, not re-executed,
/// and it is not an alarm.
#[test]
fn cli_debrief_inconclusive_exits_3_never_coerced() {
    let daemon = start_daemon_prepared(|dir| {
        seed_db_from_fixture(&dir.join("events.db"), "s3_gate_inconclusive.jsonl");
    });

    let out = run_cli(
        &daemon,
        &["debrief", "01S3GATE1NC0NC000000000R02", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(3),
        "inconclusive is 3 (DR-004: never coerced toward pass or fail); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("--json debrief");
    assert_eq!(report["gates"]["pre_merge"]["verdict"], "inconclusive");
    assert_eq!(report["alarms"], json!([]), "inconclusive never alarms");
}

/// The compliance sentence, end to end: the committed divergence fixture
/// (recorded `fail`, CAS preimage that replays to `pass`) raises an
/// INTEGRITY ALARM on `debrief` — named verifier, both verdicts, exit 3
/// (substrate-fault family: the verdict is neither trusted nor re-issued).
#[test]
fn cli_debrief_divergence_raises_integrity_alarm() {
    let daemon = start_daemon_prepared(|dir| {
        let cas = rezidnt_cas::Cas::open(&dir.join("cas")).expect("open cas");
        let put = cas
            .put(b"M\tsrc/checkout/cart.rs\n", "text/plain")
            .expect("seed diff");
        assert_eq!(
            put.hash, "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e",
            "oracle hash bug"
        );
        seed_db_from_fixture(&dir.join("events.db"), "s4_replay_divergence_alarm.jsonl");
    });

    let out = run_cli(
        &daemon,
        &["debrief", "01S4D1VERGE000000000000R01", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(3),
        "an integrity alarm is not a gate verdict — 3, not 5, not 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("--json debrief");
    let alarms = report["alarms"].as_array().expect("alarms array");
    assert_eq!(alarms.len(), 1, "recorded fail vs replayed pass MUST alarm");
    assert_eq!(alarms[0]["verifier"], "diff-scope");
    assert_eq!(alarms[0]["recorded"], "fail");
    assert_eq!(alarms[0]["replayed"], "pass");
}

/// §9: `rezidnt gate why <run>` — interrogability from the CLI. Returns the
/// failing verifier, evidence refs, and the EXACT recorded inputs; exit 0
/// (the interrogation succeeded — the verdict rides the output, not the
/// exit code).
#[test]
fn cli_gate_why_names_verifier_evidence_and_exact_inputs() {
    let daemon = start_daemon_prepared(|dir| {
        seed_db_from_fixture(&dir.join("events.db"), "s3_gate_forced_failure.jsonl");
    });

    let out = run_cli(
        &daemon,
        &["gate", "why", "01S3GATEFA1DED000000000R01", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "gate why answers; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let why: serde_json::Value = serde_json::from_slice(&out.stdout).expect("--json gate why");
    assert_eq!(
        why["verdict"], "fail",
        "recorded verdict verbatim, never a bool (I6)"
    );
    assert_eq!(why["verifier"], "tests-pass");
    assert_eq!(
        why["evidence"][0]["hash"],
        "a0fda6ff40cb5f91bd2d09cbfb839ae91b9b4c9aa0ccfc0981986c10d4d08246",
        "evidence refs exactly as recorded (I2)"
    );
    assert_eq!(
        why["inputs"],
        json!({
            "gate": "pre_merge",
            "refs": {"diff": "cas:blake3:9e5dcbdfd76ce6fcd97070be70be372bf9c59655f07a0a94b1de102ca1ac3921"},
            "params": {},
            "timeout_ms": 120000
        }),
        "the EXACT inputs, verbatim from the log (§8 interrogability)"
    );
}
