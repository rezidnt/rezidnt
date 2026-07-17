//! S4 oracle — `vet` enforcement PRE-SPAWN (slice criterion: bare-mode /
//! pinned-version / allowedTools checks refuse a non-conforming agent spec
//! BEFORE spawn) and the `rezidnt vet` CLI verb with the DR-004 exit codes.
//!
//! RED MODES (verified at board time):
//! - daemon tests: assert-red — today the daemon spawns without consulting
//!   gates, so `agent.spawned` appears where the board demands a refusal
//!   (fast failure), and no `gate.*` fact ever hits the tail.
//! - CLI tests: assert-red — `vet` is an unknown subcommand today, clap
//!   exits 2 where the board pins 5/0.
//!
//! DR-004 (BINDING): gate-fail exits 5; `inconclusive` exits 3, never
//! coerced toward pass or fail; local input errors stay clap's 2.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{
    connect, make_gated_project, open_request, read_until, run_cli, send_line, start_daemon,
};

/// Strip the `bare = true` line from the gated spec — the resulting agent
/// spec must FAIL the vet gate's bare-mode check.
fn unbared(spec: &str) -> String {
    let out: String = spec
        .lines()
        .filter(|l| !l.trim_start().starts_with("bare = "))
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(!out.contains("bare = "), "test bug: bare line survived");
    out
}

/// THE criterion: a non-conforming agent spec is refused AT THE VET GATE,
/// pre-spawn — NO `agent.spawned` ever reaches the log; what does is
/// `gate.entered {gate: "vet"}` and `gate.failed` naming `bare-mode`. The
/// refusal is a machine-readable fact, not a log-less error string.
#[test]
fn vet_refuses_unbared_spec_before_spawn() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);
    let spec = unbared(&spec);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
            || (v["subject"] == "gate.failed" && v["payload"]["gate"] == "vet")
    });

    let spawned: Vec<_> = lines
        .iter()
        .filter(|v| v["subject"] == "agent.spawned")
        .collect();
    assert!(
        spawned.is_empty(),
        "vet is PRE-SPAWN enforcement: a spec without bare = true must never \
         mint agent.spawned; observed {spawned:?}"
    );

    let failed = lines
        .iter()
        .find(|v| v["subject"] == "gate.failed")
        .expect("read_until stopped on gate.failed");
    assert_eq!(failed["payload"]["gate"], "vet");
    assert_eq!(
        failed["payload"]["verifier"], "bare-mode",
        "the FAILING verifier is named on the fact (ontology gate.failed v1)"
    );
    assert!(
        failed["payload"]["inputs"]["refs"]["spec"]
            .as_str()
            .is_some_and(|r| r.starts_with("cas:blake3:")),
        "vet inputs pin the agent spec by content hash (§8 determinism): {failed:#}"
    );
    assert!(
        lines
            .iter()
            .any(|v| v["subject"] == "gate.entered" && v["payload"]["gate"] == "vet"),
        "the gate lifecycle is on the log: gate.entered precedes the verdict"
    );
}

/// The conforming spec passes vet, and the ORDER is the criterion:
/// `gate.entered(vet)` < `gate.passed(vet)` < `agent.spawned` — the gate ran
/// BEFORE the spawn, with all three vet natives on the passed record. The
/// spawned fact records the governed fields (DR-001: allowedTools and
/// version pinning recorded in events).
#[test]
fn vet_pass_is_ordered_before_spawn_and_records_governed_fields() {
    let daemon = start_daemon();
    let (_project, spec) = make_gated_project(100);

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, Duration::from_secs(20), |v| {
        v["subject"] == "agent.spawned"
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
    let entered = pos("gate.entered", "vet");
    let passed = pos("gate.passed", "vet");
    let spawned = pos("agent.spawned", "");
    assert!(
        entered < passed && passed < spawned,
        "vet runs BEFORE the spawn: {subjects:?}"
    );

    let passed_ev = &lines[passed];
    let verifiers: Vec<&str> = passed_ev["payload"]["verifiers"]
        .as_array()
        .expect("gate.passed carries per-verifier records (proposed v1 shape)")
        .iter()
        .filter_map(|v| v["verifier"].as_str())
        .collect();
    for name in ["bare-mode", "pinned-version", "allowed-tools"] {
        assert!(
            verifiers.contains(&name),
            "vet native {name} missing from {verifiers:?}"
        );
    }

    let spawned_ev = &lines[spawned];
    assert_eq!(spawned_ev["payload"]["bare"], true);
    assert_eq!(spawned_ev["payload"]["harness_version"], "2.1.191");
    assert_eq!(
        spawned_ev["payload"]["allowed_tools"],
        serde_json::json!(["Read", "Edit"]),
        "permission composition recorded in events (DR-001)"
    );
}

/// DR-004: `rezidnt vet <agent-spec>` on a non-conforming spec is a
/// gate-fail — exit 5 (NOT 2, NOT 3) — with a machine-readable verdict on
/// stdout naming the failing verifier.
#[test]
fn cli_vet_fail_exits_5_with_machine_readable_verdict() {
    let daemon = start_daemon();
    let (project, spec) = make_gated_project(100);
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, unbared(&spec)).expect("write spec");

    let out = run_cli(
        &daemon,
        &["vet", spec_path.to_str().expect("utf8"), "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(5),
        "gate-fail exits 5 (DR-004, BINDING); stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let verdict: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("--json means machine-readable stdout");
    assert_eq!(
        verdict["verdict"], "fail",
        "the verdict rides the output verbatim (I6)"
    );
    assert_eq!(
        verdict["verifier"], "bare-mode",
        "the failing verifier is named"
    );
}

/// DR-004: a conforming spec vets clean — exit 0, verdict `pass`.
#[test]
fn cli_vet_pass_exits_0() {
    let daemon = start_daemon();
    let (project, spec) = make_gated_project(100);
    let spec_path = project.path().join("rezidnt.toml");
    std::fs::write(&spec_path, &spec).expect("write spec");

    let out = run_cli(
        &daemon,
        &["vet", spec_path.to_str().expect("utf8"), "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "a passing vet exits 0; stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let verdict: serde_json::Value = serde_json::from_slice(&out.stdout).expect("--json stdout");
    assert_eq!(verdict["verdict"], "pass");
}
