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

// ===========================================================================
// DR-006 — replay-divergence lands a DURABLE `integrity.alarm` fact.
//
// Today `debrief` is a standalone CLI read of SQLite + CAS (main.rs::debrief,
// NO fabric writer handle): it prints report.alarms[] and exits, emitting
// NOTHING (the §8/§14 I3 gap the DR closes). DR-006 ratifies: divergence lands
// a durable `integrity.alarm` fact on the log so it is rebuild-visible and
// queryable — and the append MUST go through the daemon's SINGLE writer (I3:
// never a second writer racing the append-only log / WAL). The existing
// `cli_debrief_divergence_raises_integrity_alarm` pin above (report + exit 3)
// stays correct and is now ADDITIVE-to, never replaced.
//
// RED MODE: assert-red. Every test below runs against today's binaries: the
// divergence debrief prints its report and exits 3 (that half is green), but
// NO `integrity.alarm` fact ever reaches the log, so `tail` never sees it and
// `rebuild` never folds it — the durable-fact assertions fail. `debrief` has
// no socket path today, so the daemon-routing observable is red by
// construction.
//
// DAEMON-ROUTING OBSERVABLE (what the oracle pins vs. leaves open): the pin is
// that the fact appears on the DAEMON's live `tail` broadcast — only events
// appended through the daemon's Fabric are broadcast, so a CLI that appended
// directly to SQLite (a forbidden second writer) would NOT produce this frame.
// The EXACT mechanism (a new socket op that asks the daemon to record the
// alarm, or debrief-with-append becoming a daemon operation) is left to the
// implementer; the observable — legitimate-writer emission + rebuild survival
// — is pinned.
//
// IDEMPOTENCY / AT-LEAST-ONCE (the pinned honest choice, see the oracle
// deliverable): debrief is replayable and re-runnable. The WIRE is
// at-least-once (each detection may append a fact — the log records each
// check), but the daemon DEDUPS BY (run, gate, verifier) against alarms
// already on the log before appending, so re-running debrief on an
// already-alarmed divergence does NOT append a second fact. This keeps the
// append-only log honest and the derived dossier free of a growing duplicate
// pile. The dedup is log-derived (I3): the daemon reads existing
// integrity.alarm facts, it does not consult a side table.

/// Read the daemon's log from seq 0 by opening a fresh `tail` and draining the
/// replay until `stop`, returning every frame seen. Used to observe a durable
/// fact AFTER the CLI action that should have produced it (the frame is on the
/// log, so a fresh tail's replay carries it).
fn tail_snapshot(
    socket: &std::path::Path,
    deadline: Duration,
    stop: impl FnMut(&serde_json::Value) -> bool,
) -> Vec<serde_json::Value> {
    let mut reader = connect(socket);
    send_line(&mut reader, r#"{"op":"tail"}"#);
    read_until(&mut reader, deadline, stop)
}

/// Seed the divergence scenario (CAS preimage + fixture) into a prepared
/// daemon dir — the committed `s4_replay_divergence_alarm.jsonl` records
/// `fail` for diff-scope, but its CAS preimage (`M src/checkout/cart.rs`)
/// replays to `pass` under the `src/checkout/**` allow-glob.
fn seed_divergence(dir: &std::path::Path) {
    let cas = rezidnt_cas::Cas::open(&dir.join("cas")).expect("open cas");
    let put = cas
        .put(b"M\tsrc/checkout/cart.rs\n", "text/plain")
        .expect("seed diff");
    assert_eq!(
        put.hash, "1d50030ca17af09eb6fad0eadfb3492275bfc76635d0965260cde6bc685d785e",
        "oracle hash bug"
    );
    seed_db_from_fixture(&dir.join("events.db"), "s4_replay_divergence_alarm.jsonl");
}

const DIVERGE_RUN: &str = "01S4D1VERGE000000000000R01";

/// CRITERION 2 (durable fact on divergence, daemon single-writer): a debrief
/// over the committed divergence fixture lands an `integrity.alarm` fact on
/// the log carrying run + verifier + recorded(fail) + replayed(pass),
/// OBSERVABLE on the daemon's `tail` broadcast — which only the legitimate
/// daemon writer can produce. The exit stays 3 (CRITERION 6, DR-004: the fact
/// is additive, it does not change the class).
#[test]
fn dr006_divergence_lands_durable_integrity_alarm_via_daemon_writer() {
    let daemon = start_daemon_prepared(seed_divergence);

    let out = run_cli(&daemon, &["debrief", DIVERGE_RUN, "--json"]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "DR-004 unchanged: divergence still exits 3 (the fact is additive); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The durable fact is on the daemon-owned log: a fresh tail's replay carries
    // an integrity.alarm for this run. (Emission via the daemon's single writer
    // is what puts it on the broadcast/log at all — a direct-SQLite second
    // writer is forbidden by I3 and is not what this observable accepts.)
    let frames = tail_snapshot(&daemon.socket, Duration::from_secs(10), |v| {
        v["subject"] == "integrity.alarm" && v["payload"]["run"] == json!(DIVERGE_RUN)
    });
    let alarm = frames
        .iter()
        .rfind(|v| v["subject"] == "integrity.alarm")
        .expect("an integrity.alarm fact must be on the log after a divergence debrief (DR-006)");
    assert_eq!(alarm["payload"]["run"], json!(DIVERGE_RUN));
    assert_eq!(
        alarm["payload"]["verifier"], "diff-scope",
        "the diverging verifier is named on the durable fact (§8)"
    );
    assert_eq!(alarm["payload"]["recorded"], "fail", "recorded verdict");
    assert_eq!(alarm["payload"]["replayed"], "pass", "replayed verdict");
    assert_eq!(
        alarm["v"], 1,
        "integrity.alarm minted at v = 1 (taxonomy v0 discipline)"
    );
}

/// CRITERION 5 (rebuild-visible + queryable fold): after the divergence
/// debrief, `rezidnt rebuild` (fold from seq 0 — the log is truth, I3)
/// reproduces the alarm as queryable state on the run's dossier. This is the
/// rebuild-survival half of DR-006: the fact is not a wire-only blip, it folds
/// into the graph and rebuild reproduces it.
#[test]
fn dr006_rebuild_reproduces_the_integrity_alarm_in_state() {
    let daemon = start_daemon_prepared(seed_divergence);

    let debrief = run_cli(&daemon, &["debrief", DIVERGE_RUN, "--json"]);
    assert_eq!(debrief.status.code(), Some(3), "divergence debrief exits 3");

    // Rebuild folds the whole log from seq 0; the alarm must be in the run's
    // queryable state (dossier.integrity_alarms), reproduced from the durable
    // fact debrief appended.
    let rebuild = run_cli(
        &daemon,
        &["rebuild", "--db", daemon.db.to_str().expect("utf8")],
    );
    assert_eq!(
        rebuild.status.code(),
        Some(0),
        "rebuild folds the log; stderr: {}",
        String::from_utf8_lossy(&rebuild.stderr)
    );
    let graph: serde_json::Value =
        serde_json::from_slice(&rebuild.stdout).expect("rebuild --json is machine-readable");
    let alarms = graph["agent_runs"][DIVERGE_RUN]["integrity_alarms"]
        .as_array()
        .expect("the run's dossier carries integrity_alarms after rebuild (DR-006 fold)");
    assert_eq!(
        alarms.len(),
        1,
        "rebuild reproduces exactly one alarm for the diverging verifier"
    );
    assert_eq!(alarms[0]["verifier"], "diff-scope");
    assert_eq!(alarms[0]["recorded"], "fail");
    assert_eq!(alarms[0]["replayed"], "pass");
}

/// CRITERION 3 (NO alarm on agreement, symmetric to I6 "never coerce"): a
/// debrief whose replay AGREES with the record (the committed
/// `s4_replay_verified.jsonl`, recorded `pass` that replays to `pass`) emits
/// NO integrity.alarm — a spurious alarm on an honest log is as wrong as
/// coercing a verdict. Exit 0 (clean replay).
///
/// ORACLE HONESTY NOTE (this test is GREEN today, by absence): today NO alarm
/// is ever emitted, so "agreement emits none" holds trivially — it tests
/// nothing until the emit path lands. It CANNOT be made red pre-implementation
/// (the honest behavior and the unimplemented behavior coincide: both emit
/// nothing). It is retained as the REGRESSION GUARD that becomes load-bearing
/// the moment the emit path exists: it pins that emission is conditioned on
/// DIVERGENCE, so an implementer who emits on every debrief turns this red.
/// Its RED partner is `dr006_divergence_lands_durable_integrity_alarm_via_daemon_writer`
/// (the emit-on-divergence pin). Flagged for the auditor: green-by-absence, not
/// green-by-satisfaction.
#[test]
fn dr006_agreement_emits_no_integrity_alarm() {
    const VERIFIED_RUN: &str = "01S4REP1AYED00000000000R01";
    let daemon = start_daemon_prepared(|dir| {
        let cas = rezidnt_cas::Cas::open(&dir.join("cas")).expect("open cas");
        cas.put(b"M\tsrc/checkout/cart.rs\n", "text/plain")
            .expect("seed diff");
        seed_db_from_fixture(&dir.join("events.db"), "s4_replay_verified.jsonl");
    });

    let out = run_cli(&daemon, &["debrief", VERIFIED_RUN, "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "an honest log replays clean — exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // No integrity.alarm anywhere on the log. Drain a bounded window: the log
    // is tiny (2 seeded facts), so a short tail that never hits the stop
    // predicate returns the whole replay — we assert the subject is absent.
    let mut reader = connect(&daemon.socket);
    send_line(&mut reader, r#"{"op":"tail"}"#);
    // Read the replay of the fixture (2 facts); allow a brief window for any
    // erroneously-emitted alarm to also arrive, then assert none did.
    let frames = read_until(&mut reader, Duration::from_secs(3), |v| {
        // Stop when we've observed the last seeded fact (gate.passed) — by then
        // any same-run alarm the debrief wrongly emitted would already be on the
        // replayed log or the live tail.
        v["subject"] == "gate.passed" && v["payload"]["run"] == json!(VERIFIED_RUN)
    });
    assert!(
        !frames.iter().any(|v| v["subject"] == "integrity.alarm"),
        "agreement must NOT emit an integrity.alarm (I6-symmetric: never a spurious alarm); saw {frames:?}"
    );
}

/// CRITERION 4 (idempotency — DEDUP BY (run, gate, verifier)): running
/// `debrief` TWICE over the same divergent log does NOT append a second
/// `integrity.alarm` fact. The daemon reads existing alarm facts off the log
/// (log-derived, I3) and dedups by (run, gate, verifier) before appending, so
/// a re-run of a replayable operation stays a no-op on the append-only log.
/// (The honest-choice reasoning is in the oracle deliverable; this pins the
/// observable.)
#[test]
fn dr006_rerunning_debrief_does_not_duplicate_the_alarm() {
    let daemon = start_daemon_prepared(seed_divergence);

    for _ in 0..2 {
        let out = run_cli(&daemon, &["debrief", DIVERGE_RUN, "--json"]);
        assert_eq!(
            out.status.code(),
            Some(3),
            "each divergence debrief exits 3"
        );
    }

    // Count integrity.alarm facts ON THE LOG directly (not via the dedup-safe
    // fold, which would mask a second append). The reducer collapses duplicates
    // by (run, gate, verifier), so a rebuild can't distinguish one durable fact
    // from two — we must read the raw log. Two seeded facts + one alarm = three
    // rows; a broken (non-deduping) daemon would append a second alarm on the
    // re-run, giving two. The daemon has fully written by the time each debrief
    // returned (the CLI blocks on the daemon's ack), so a post-hoc read is
    // race-free.
    let log = rezidnt_fabric::EventLog::open(&daemon.db).expect("open log");
    let alarm_count = log
        .read_from(1)
        .expect("read log")
        .into_iter()
        .filter(|row| {
            row.event.subject.as_str() == "integrity.alarm"
                && row.event.payload()["run"].as_str() == Some(DIVERGE_RUN)
        })
        .count();
    assert_eq!(
        alarm_count, 1,
        "two debriefs, ONE durable integrity.alarm on the log — dedup by (run, gate, verifier)"
    );
}

/// DR-006 DAEMON-DOWN COMPLEMENT (the gap the auditor FAILED the DR-006 diff
/// on): when a divergence is found AND the durable append cannot complete
/// (daemon unreachable, or the RecordAlarms op errors), the primary signal —
/// the divergence VERDICT and its `report.alarms[]` — must survive, and the
/// DR-004 exit class must stay 3.
///
/// The divergence verdict is computed from the CLI's OWN local log + CAS read
/// (main.rs::debrief replays before it appends); the durable append (DR-006) is
/// an ADDITIVE audit improvement. An additive improvement's failure must
/// degrade LOUDLY — never destroy the finding it decorates. Concretely:
///   1. the CLI STILL prints `report.alarms[]` on stdout (the finding is not
///      suppressed by an append failure), and
///   2. the CLI exits 3 — the SAME integrity-alarm/inconclusive class the
///      daemon-UP path exits (DR-004; `cli_debrief_divergence_raises_integrity_alarm`
///      pins exit 3 with the daemon up). NOT 1 (catch-all crash), NOT 4.
///   3. (left to the implementer, unasserted here to avoid over-constraining
///      the wording) the durability failure is surfaced LOUDLY on STDERR — a
///      best-effort append that warns it could not durably record the alarm
///      because the daemon was unreachable. The implementer adds the warning
///      text; this test does not pin a substring so it does not dictate the
///      exact phrasing.
///
/// RED TODAY: main.rs::debrief runs `record_alarms(&report.alarms)?` with a
/// HARD `?` placed BEFORE the report print and before `std::process::exit(3)`.
/// With no daemon listening, `connect_and_request` fails, the `?` propagates to
/// `main()`, whose `Cmd::Debrief` failure class is 1 — so the CLI exits **1**,
/// prints NO report on stdout, and records no durable fact. A real integrity
/// divergence is misclassified as a crash and its report suppressed.
///
/// Setup mirrors `cli_debrief_divergence_raises_integrity_alarm` (same fixture,
/// same run ULID, same CAS preimage via `seed_divergence`), but runs the CLI
/// with NO daemon started and `REZIDNT_SOCKET` pointed at a dead path, so the
/// append attempt fails. Does NOT touch the daemon-UP pins.
#[test]
fn dr006_divergence_debrief_with_daemon_down_still_reports_and_exits_3() {
    // Seed the divergence scenario (CAS preimage + fixture) into a temp dir,
    // exactly as the daemon-UP divergence pins do — but never start a daemon.
    let dir = tempfile::tempdir().expect("tempdir");
    seed_divergence(dir.path());

    // A socket path that will never be listened on: the daemon append MUST
    // fail. (The dir has no `rezidnt.sock` because no daemon ran here.)
    let dead_socket = dir.path().join("no-daemon.sock");
    assert!(
        !dead_socket.exists(),
        "the daemon-down setup requires an unbound socket path"
    );

    let out = Command::new(common::cli_bin())
        .args(["debrief", DIVERGE_RUN, "--json"])
        .env("REZIDNT_SOCKET", &dead_socket)
        .env("REZIDNT_DB", dir.path().join("events.db"))
        .output()
        .expect("run rezidnt CLI");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // (2) The DR-004 exit class is unchanged by an append failure: still 3.
    // RED today: the propagated `?` makes main() exit 1 (Debrief failure class).
    assert_eq!(
        out.status.code(),
        Some(3),
        "a divergence is exit 3 whether or not the durable append lands (DR-004); \
         a daemon-down append failure must NOT become a catch-all crash (1). \
         stdout: {stdout}\nstderr: {stderr}"
    );

    // (1) The finding is computed locally and must not be suppressed: the
    // machine-readable report still carries the diverging verifier + both
    // verdicts. RED today: stdout is EMPTY (main() exited before the print).
    let report: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("--json debrief must still print its report on a daemon-down append failure ({e}); stdout: {stdout:?}\nstderr: {stderr}")
    });
    let alarms = report["alarms"]
        .as_array()
        .expect("report carries alarms[] even when the durable append could not complete");
    assert_eq!(
        alarms.len(),
        1,
        "the locally-computed divergence finding survives the append failure"
    );
    assert_eq!(alarms[0]["verifier"], "diff-scope");
    assert_eq!(alarms[0]["recorded"], "fail");
    assert_eq!(alarms[0]["replayed"], "pass");
    // NOTE (work order): (3) the implementer must ALSO surface the durability
    // failure loudly on stderr (a warning that the integrity alarm could not be
    // durably recorded because the daemon was unreachable). Left unasserted here
    // to avoid pinning the exact wording; the auditor should confirm a loud,
    // non-silent stderr warning exists in the remediation.
}
