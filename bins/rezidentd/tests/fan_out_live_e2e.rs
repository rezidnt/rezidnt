//! DR-044 ORACLE (live fan-out, end-to-end) — guard (b) of DR-044
//! §Consequences plus the §Decision 2b lead-parented-edge pin and the
//! §Decision 1 per-task idempotency contract. Drives the REAL daemon over the
//! loopback-HTTP MCP transport, so it follows the house `#[cfg(unix)]` +
//! `*_e2e.rs` convention and is WSL/unix-only; it is NOT part of the host
//! clippy surface.
//!
//! ## Why this file exists at all
//!
//! `crates/rezidnt-state/tests/orchestration_rebuild_equivalence.rs` and
//! `orchestration_rebuild_from_log.rs` already prove rebuild-equivalence — but
//! over `spec/fixtures/dr042_orchestration_fanout.jsonl`, a HAND-AUTHORED log
//! whose lead-keyed cross-run edge no shipped emitter produces (DR-044
//! §Context). The read side is green against a shape production has never
//! emitted. DR-044 §Consequences (b) therefore owes the same property over a
//! log an ACTUAL fan-out produced. That distinction is this file's entire job.
//!
//! ## RED MODE
//!
//! ASSERT-RED: `fan_out` is not dispatched, so the tool call returns a JSON-RPC
//! error / unknown-tool result and the first assertion fires. Once the tool
//! exists but the projection guard does not, the cross-run assertions fire on
//! the self-edge instead. Both are red for the right reason.
//!
//! ## The lead's badge is the REAL one
//!
//! DR-044 §Decision 1 authorizes fan-out with the LEAD's own badge through the
//! existing §12 door under the existing verb `spawn`. So this board does not
//! substitute the operator badge: the lead agent's stub harness dumps the
//! `REZIDNT_BADGE` macaroon the daemon injected (`bins/rezidentd/src/runs.rs:749`)
//! and the test presents THAT. It is the only way the daemon can resolve which
//! run is the lead (`agent.spawned.badge_id == lead_badge_id`, log-derived — no
//! session object, I3).
//!
//! The spec declares `role`, so every run here — the lead AND each sub — also
//! carries its own DR-017 role-attenuation `permit.delegated` self-edge, exactly
//! as production emits. The graph assertions below therefore ride a log that
//! contains BOTH kinds of edge, which is the honest live counterpart of the
//! pure-logic guard in `crates/rezidnt-state/tests/orchestration_self_edge_guard.rs`.
//!
//! ## Ontology posture (DR-044 §Decision 6 — warden session now CLOSED)
//!
//! When this board was first cut, the `worktree.allocated.allocator` v1 value
//! vocabulary was mid-widening in a parallel warden session, so every test here
//! deliberately asserted only the PRESENCE or ABSENCE of `worktree.allocated`
//! and never its VALUE — the warden's outcome could not invalidate the board.
//!
//! The warden has since ratified `"rezidnt" | "run:<ULID>"`
//! (`spec/ontology.md:215`), which retired that isolation and left the value
//! itself machine-unchecked. `ordinary_allocations_stay_rezidnt_and_fan_out_allocations_name_the_lead`
//! closes that hole and is the ONLY test in this file that reads the allocator
//! value; its doc comment carries the honesty note about being a
//! regression pin over already-shipped behavior rather than a failing-first
//! oracle.
#![cfg(unix)]

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{
    DaemonGuard, make_project, mcp_post, mcp_tool_call, restart_daemon_with_mcp, rpc,
    start_daemon_with_mcp, tool_payload, wait_for_lockfile,
};
use rezidnt_fabric::EventLog;
use rezidnt_state::{Materializer, OrchestrationView, fold, orchestration_graph};
use rezidnt_types::Event;
use serde_json::{Value, json};

const LOCK_DEADLINE: Duration = Duration::from_secs(10);
const FACT_DEADLINE: Duration = Duration::from_secs(30);

// --- harness plumbing --------------------------------------------------------

fn initialize(url: &str) {
    let response = mcp_post(
        url,
        &rpc(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "dr044-oracle", "version": "0"}
            }),
        ),
    );
    assert!(
        response.get("error").is_none(),
        "initialize must succeed: {response:#}"
    );
}

/// Poll `tail_events` until `pred` matches some envelope; return the snapshot.
fn tail_until(url: &str, deadline: Duration, mut pred: impl FnMut(&Value) -> bool) -> Vec<Value> {
    let until = Instant::now() + deadline;
    loop {
        let result = mcp_tool_call(url, 40, "tail_events", json!({}));
        let events = tool_payload(&result)["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if events.iter().any(&mut pred) {
            return events;
        }
        assert!(
            Instant::now() < until,
            "deadline: tail_events never showed the expected event; last saw {} envelopes",
            events.len()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The file each run's stub drops its injected badge into, INSIDE its own
/// worktree. It has to be the worktree: the harness spawns through the DR-028
/// composed confinement path (`bins/rezidentd/src/runs.rs:809`), so a write to
/// an arbitrary path outside the sandbox's binds is silently denied. The
/// worktree is bound writable because that is where the agent works.
const BADGE_DROP: &str = "rezidnt-injected-badge.txt";

/// A stub harness that FIRST drops the injected `REZIDNT_BADGE` macaroon into
/// its own worktree (cwd), then emits the same stream-json a normal stub emits
/// so the run completes honestly.
///
/// The token never touches stdout, so it never reaches the capture stream and
/// never reaches the log — the badge TOKEN is not a fabric value (I2/§12). It
/// travels test-side only, through the filesystem, exactly as a real lead
/// receives it: in its environment.
fn badge_dumping_harness(dir: &Path) -> std::path::PathBuf {
    let script = dir.join("lead-harness.sh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$REZIDNT_BADGE" > "$PWD/{BADGE_DROP}"
echo '{{"type":"system","subtype":"init","session_id":"dr044-session","claude_code_version":"2.1.191","tools":[]}}'
sleep 0.05
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"duration_ms":5,"total_cost_usd":0.001,"usage":{{"input_tokens":1,"output_tokens":1}},"session_id":"dr044-session"}}'
"#
        ),
    )
    .expect("write badge-dumping harness stub");
    let mut perms = std::fs::metadata(&script).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");
    script
}

/// Rewrite `make_project`'s spec to use the badge-dumping harness and to declare
/// a `role`, so every run carries the production DR-017 self-edge alongside any
/// genuine fan-out edge.
fn with_badge_dump_and_role(spec: &str, harness: &Path) -> String {
    let anchor = "worktree = \"auto\"\n";
    assert!(
        spec.contains(anchor),
        "test bug: the project spec lost its worktree anchor"
    );
    let bin_line = spec
        .lines()
        .find(|l| l.trim_start().starts_with("bin_override"))
        .expect("test bug: the project spec lost its bin_override line")
        .to_string();
    spec.replace(anchor, &format!("{anchor}role = \"lead\"\n"))
        .replace(
            &bin_line,
            &format!("bin_override = \"{}\"", harness.display()),
        )
}

/// Wait until the run whose worktree is `worktree` has dropped its injected
/// macaroon there, and return it.
fn injected_badge(worktree: &Path, deadline: Duration) -> String {
    let drop = worktree.join(BADGE_DROP);
    let until = Instant::now() + deadline;
    loop {
        if let Ok(text) = std::fs::read_to_string(&drop)
            && let Some(first) = text.lines().find(|l| !l.trim().is_empty())
        {
            return first.trim().to_string();
        }
        assert!(
            Instant::now() < until,
            "deadline: the agent in {} never dropped its REZIDNT_BADGE at {}",
            worktree.display(),
            drop.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Open the project and return `(workspace, lead_run, lead_badge)`.
///
/// The lead's badge is the macaroon the daemon INJECTED into it — recovered
/// from the lead's own worktree, which the log names via `worktree.allocated`.
/// Only the ABSENCE/PRESENCE and `path` of that fact are read here; its
/// `allocator` VALUE is never asserted, so the parallel warden `/subject`
/// widening the allocator vocabulary (DR-044 §Decision 6) cannot affect this.
fn open_and_capture_lead(url: &str, operator: &str, spec: &str) -> (String, String, String) {
    let opened = mcp_tool_call(
        url,
        2,
        "open_project",
        json!({"badge": operator, "spec_toml": spec}),
    );
    assert_ne!(
        opened["isError"],
        json!(true),
        "open_project must succeed: {opened:#}"
    );
    let workspace = tool_payload(&opened)["workspace"]
        .as_str()
        .expect("open ack names the workspace ulid")
        .to_string();

    // The lead's spawn must be on the log before we can fan out from it.
    let events = tail_until(url, FACT_DEADLINE, |e| e["subject"] == "agent.spawned");
    let lead_run = events
        .iter()
        .find(|e| e["subject"] == "agent.spawned")
        .and_then(|e| e["payload"]["run"].as_str())
        .expect("the lead run's agent.spawned names its run")
        .to_string();

    // The lead is the only run that exists at this point, so the single
    // allocated worktree is its own.
    let lead_worktree = events
        .iter()
        .find(|e| e["subject"] == "worktree.allocated")
        .and_then(|e| e["payload"]["path"].as_str())
        .expect("the lead's worktree.allocated names its path")
        .to_string();

    (
        workspace,
        lead_run,
        injected_badge(Path::new(&lead_worktree), FACT_DEADLINE),
    )
}

/// `fan_out` sugar — the lead's OWN badge, one workspace, N tasks.
fn fan_out(url: &str, id: u64, lead_badge: &str, workspace: &str, keys: &[&str]) -> Value {
    let tasks: Vec<Value> = keys
        .iter()
        .map(|k| json!({"agent": "impl", "idempotency_key": k}))
        .collect();
    mcp_tool_call(
        url,
        id,
        "fan_out",
        json!({"badge": lead_badge, "workspace": workspace, "tasks": tasks}),
    )
}

/// The per-task outcome vector of an admitted `fan_out`.
fn outcome_runs(result: &Value) -> Vec<String> {
    assert_ne!(
        result["isError"],
        json!(true),
        "fan_out must be admitted with the LEAD's own badge (DR-044 §Decision 1: the existing \
         §12 door, existing verb `spawn`, no new badge kind): {result:#}"
    );
    let payload = tool_payload(result);
    payload["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("fan_out returns a per-task outcome vector: {payload:#}"))
        .iter()
        .map(|o| {
            o["run"]
                .as_str()
                .unwrap_or_else(|| panic!("each admitted task's outcome names its run: {o:#}"))
                .to_string()
        })
        .collect()
}

/// Stop the daemon, then read its persisted log cold — the real
/// `rezidnt rebuild` read path (`EventLog::open` then `read_from(1)`), with no
/// live daemon and no retained materializer anywhere.
fn cold_read(daemon: &mut DaemonGuard) -> Vec<Event> {
    let _ = daemon.child.kill();
    let _ = daemon.child.wait();
    let log = EventLog::open(&daemon.db).expect("re-open the daemon's persisted log cold");
    log.read_from(1)
        .expect("read the persisted log from seq 1")
        .into_iter()
        .map(|row| row.event)
        .collect()
}

fn project(events: &[Event]) -> OrchestrationView {
    orchestration_graph(&fold(events.iter()))
}

// --- CRITERION (b): I3 fold-equivalence over a REAL fanned-out log -----------

/// CRITERION (b), DR-044 §Consequences — the orchestration graph over a log an
/// ACTUAL fan-out produced rebuilds from that log alone:
/// `orchestration_graph(fold(log)) == orchestration_graph(fold(replay(log)))`,
/// through the real `rezidnt rebuild` read path.
///
/// The non-vacuity legs are what make this different from the shipped
/// fixture-based rebuild tests: the compared view must carry ONE lead, that lead
/// must be the run that CALLED `fan_out`, its subs must be TWO DISTINCT OTHER
/// runs, and the lead must not appear among its own subs — despite the log also
/// carrying every run's DR-017 role self-edge. A matching pair of empty or
/// self-edged views is an oracle failure, not a pass.
#[test]
fn fan_out_graph_rebuilds_from_the_real_fanned_out_log() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"]
        .as_str()
        .expect("lockfile carries url")
        .to_string();
    let operator = lock["badge"]
        .as_str()
        .expect("lockfile carries badge")
        .to_string();
    initialize(&url);

    let (project_dir, base_spec) = make_project(20);
    let harness = badge_dumping_harness(project_dir.path());
    let spec = with_badge_dump_and_role(&base_spec, &harness);

    let (workspace, lead_run, lead_badge) = open_and_capture_lead(&url, &operator, &spec);

    // The fan-out itself: one call, two tasks, the lead's own badge.
    let result = fan_out(
        &url,
        10,
        &lead_badge,
        &workspace,
        &["dr044-sub-a", "dr044-sub-b"],
    );
    let sub_runs = outcome_runs(&result);
    assert_eq!(sub_runs.len(), 2, "two tasks, two outcomes: {result:#}");
    assert_ne!(
        sub_runs[0], sub_runs[1],
        "two tasks mint two DISTINCT runs: {sub_runs:?}"
    );
    for sub in &sub_runs {
        assert_ne!(
            sub, &lead_run,
            "a sub is a DIFFERENT run from its lead — this is the cross-run edge no shipped \
             emitter produced before DR-044 (§Context): lead {lead_run}, subs {sub_runs:?}"
        );
    }

    // Both subs' spawns must be on the log before the graph can fold them.
    for sub in &sub_runs {
        tail_until(&url, FACT_DEADLINE, |e| {
            e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(sub)
        });
    }

    // Cold-read the persisted log through the real rebuild path.
    let log = cold_read(&mut daemon);
    assert!(
        !log.is_empty(),
        "the daemon's persisted log must carry the fan-out (I3)"
    );

    // Path A: one-shot fold-from-zero, projected.
    let from_fold = project(&log);
    // Path B: incremental live materialization replaying from seq 0, projected.
    let mut live = Materializer::new();
    for event in &log {
        live.apply(event);
    }
    let from_rebuild = orchestration_graph(&live.snapshot());

    assert_eq!(
        from_fold, from_rebuild,
        "orchestration_graph(fold(log)) MUST EQUAL orchestration_graph(fold(replay(log))) over a \
         log a REAL fan-out produced — the graph rebuilds from the log alone, with no in-daemon \
         orchestration session (I3, DR-044 §Decision 5 / §Consequences (b))"
    );

    // --- non-vacuity: the compared view is the real cross-run fan-out --------
    assert_eq!(
        from_fold.leads.len(),
        1,
        "exactly ONE lead surfaces — the fanning-out run. Every run on this log also carries a \
         DR-017 role self-edge, so a projection without the `sub_run != lead_run` guard reports \
         one self-lead per run instead (DR-044 §Decision 2a): {from_fold:#?}"
    );
    let lead = &from_fold.leads[0];
    assert_eq!(
        lead.lead_run, lead_run,
        "the lead row is keyed on the run that CALLED fan_out: {from_fold:#?}"
    );
    assert_eq!(
        lead.fan_out, 2,
        "fan_out is the DERIVED count of the two cross-run subs — never a stored fact, and never \
         inflated by the lead's own self-edge: {lead:#?}"
    );
    let mut folded: Vec<&str> = lead.subs.iter().map(|s| s.sub_run.as_str()).collect();
    folded.sort_unstable();
    let mut expected: Vec<&str> = sub_runs.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(
        folded, expected,
        "the folded subs are exactly the two runs fan_out minted: {lead:#?}"
    );
    assert!(
        !lead.subs.iter().any(|s| s.sub_run == lead.lead_run),
        "the lead is never its own sub (DR-044 §Decision 2a): {lead:#?}"
    );
}

// --- CRITERION: Decision 2b, the lead-parented edge --------------------------

/// CRITERION (DR-044 §Decision 2b) — the emitted `permit.delegated` for each sub
/// is keyed `run` = the LEAD's run, and its `child_badge_id` EQUALS that sub's
/// folded `agent.spawned.badge_id`.
///
/// DR-044 warns that if those two diverge the graph silently reports
/// `fan_out: 0` — a SILENT-WRONG, the exact class of defect that made the
/// shipped read side a mirage. A graph-level assertion alone would not localize
/// it (a missing edge and a mismatched badge look identical downstream), so the
/// two ends are pinned directly on the log.
#[test]
fn each_sub_edge_is_lead_keyed_and_its_child_badge_matches_the_sub_spawn() {
    let (daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let operator = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    let (project_dir, base_spec) = make_project(20);
    let harness = badge_dumping_harness(project_dir.path());
    let spec = with_badge_dump_and_role(&base_spec, &harness);

    let (workspace, lead_run, lead_badge) = open_and_capture_lead(&url, &operator, &spec);
    let result = fan_out(&url, 11, &lead_badge, &workspace, &["dr044-edge-a"]);
    let sub_runs = outcome_runs(&result);
    let sub_run = sub_runs.first().expect("one task, one run").clone();

    let events = tail_until(&url, FACT_DEADLINE, |e| {
        e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(sub_run)
    });

    // The sub's own spawn badge — the id the graph matches on.
    let sub_badge = events
        .iter()
        .find(|e| e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(sub_run))
        .and_then(|e| e["payload"]["badge_id"].as_str())
        .expect("the sub's agent.spawned carries badge_id (ontology v1 REQUIRED)")
        .to_string();

    // A LEAD-KEYED delegation whose child badge is exactly that id.
    let lead_keyed: Vec<&Value> = events
        .iter()
        .filter(|e| e["subject"] == "permit.delegated" && e["payload"]["run"] == json!(lead_run))
        .collect();
    assert!(
        lead_keyed
            .iter()
            .any(|e| e["payload"]["child_badge_id"] == json!(sub_badge)),
        "the fan-out edge MUST be keyed `run` = the LEAD ({lead_run}) with `child_badge_id` \
         EQUAL to the sub's folded agent.spawned.badge_id ({sub_badge}). If those diverge the \
         graph silently reports fan_out: 0 (DR-044 §Decision 2b). Lead-keyed delegations seen: \
         {lead_keyed:#?}"
    );

    // And the edge is genuinely CROSS-RUN: a delegation keyed on the SUB's own
    // run carrying the sub's own badge is the DR-017 capability-chain fact, a
    // different axis — it must not be what satisfies the assertion above.
    assert_ne!(
        lead_run, sub_run,
        "precondition: the lead and the sub are different runs"
    );

    drop(daemon);
}

// --- CRITERION: Decision 1, per-task idempotency -----------------------------

/// CRITERION (DR-044 §Decision 1) — "a retry with the same keys re-returns the
/// same runs and spawns nothing new". Idempotency composes PER TASK, resolving
/// through the EXISTING per-workspace `spawn_keys` map
/// (`bins/rezidentd/src/runs.rs:149`, `:229`, `:287`) — no new dedup mechanism.
///
/// The "spawns nothing new" leg is asserted on the LOG, by counting
/// `agent.spawned` facts per run — not on the response, which a broken
/// implementation could echo correctly while double-spawning.
///
/// ## Scope: SAME-PROCESS, and that is not a weakening
///
/// An earlier cut of this test also asserted the retry across a daemon restart.
/// That assertion was **unsatisfiable by construction**, and no implementation
/// could ever have made it green: DR-045 admits ONLY a verified agent macaroon,
/// and DR-017 §Decision 6 mints the macaroon root key PER PROCESS and never
/// persists it (`bins/rezidentd/src/runs.rs:187`). After a restart the daemon
/// holds a different root key, so a lead's badge cannot verify and NO `fan_out`
/// call is admissible at all — the door refuses before idempotency is ever
/// reached. The owner ruled the ephemeral root key stays (persisting it would
/// turn it into a long-lived stealable secret), so the test moved, not the code.
///
/// The cross-restart requirement was never in DR-044 either: §Decision 1 asks
/// only that a retry re-return the same runs. What DOES survive a restart — the
/// log-derived `spawn_keys` map itself — is proven by the sibling test below,
/// through a path whose badge survives.
#[test]
fn fan_out_is_idempotent_per_task_within_one_process() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let operator = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    let (project_dir, base_spec) = make_project(20);
    let harness = badge_dumping_harness(project_dir.path());
    let spec = with_badge_dump_and_role(&base_spec, &harness);

    let (workspace, _lead_run, lead_badge) = open_and_capture_lead(&url, &operator, &spec);
    let keys = ["dr044-idem-a", "dr044-idem-b"];

    let first = outcome_runs(&fan_out(&url, 12, &lead_badge, &workspace, &keys));
    assert_eq!(first.len(), 2, "two tasks, two runs");
    for run in &first {
        tail_until(&url, FACT_DEADLINE, |e| {
            e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(run)
        });
    }

    // Same call, same keys, same process: the same runs, nothing new.
    let retry = outcome_runs(&fan_out(&url, 13, &lead_badge, &workspace, &keys));
    assert_eq!(
        retry, first,
        "a same-key retry re-returns the SAME runs in the same order — idempotency composes PER \
         TASK (DR-044 §Decision 1)"
    );

    // The log is the judge on "spawns nothing new": exactly one agent.spawned
    // per run, after two fan_out calls carrying the same two keys.
    let log = cold_read(&mut daemon);
    for run in &first {
        let spawns = log
            .iter()
            .filter(|e| e.subject.as_str() == "agent.spawned" && e.payload()["run"] == json!(run))
            .count();
        assert_eq!(
            spawns, 1,
            "run {run} was spawned EXACTLY ONCE despite two fan_out calls with its key — a \
             keyed retry spawns nothing new (DR-044 §Decision 1)"
        );
    }
}

/// CRITERION (DR-044 §Decision 1, the half a restart CAN prove) — a `fan_out`
/// task's idempotency key lands in the SAME per-workspace `spawn_keys` map that
/// `spawn_agent` uses, and that map is genuinely LOG-DERIVED: it survives a
/// daemon restart.
///
/// This is the honest cross-restart proof. `fan_out` itself cannot be re-called
/// after a restart (the lead's macaroon cannot verify against a freshly minted
/// root key — DR-017 §Decision 6), but `spawn_agent` rides the OPERATOR badge,
/// which the lockfile re-announces. So: fan out with key K in process 1, restart,
/// then present key K to `spawn_agent` in process 2 and demand the SAME run back.
///
/// It proves two things at once, neither of which the sibling same-process test
/// can reach. **Shared map** — DR-044 §Decision 1 says fan-out resolves "through
/// the *existing* per-workspace `spawn_keys` map ... no new dedup mechanism"; a
/// separate fan-out-only key space would return a fresh run here. **Log-derived**
/// — the map is a fold of `agent.spawned.idempotency_key`, not process memory,
/// so it survives the restart that destroys the root key.
///
/// It is NOT a duplicate of `mcp_workspace_recovery::
/// spawn_key_idempotency_survives_daemon_restart`: that test writes and reads the
/// key through `spawn_agent` on both sides. This one writes through `fan_out` and
/// reads through `spawn_agent` — the cross-mechanism claim is the whole point.
#[test]
fn a_fan_out_key_joins_the_shared_spawn_key_map_and_survives_restart() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let operator = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    let (project_dir, base_spec) = make_project(20);
    let harness = badge_dumping_harness(project_dir.path());
    let spec = with_badge_dump_and_role(&base_spec, &harness);

    let (workspace, _lead_run, lead_badge) = open_and_capture_lead(&url, &operator, &spec);
    const SHARED_KEY: &str = "dr044-shared-key";

    // Process 1: the key enters the map through FAN_OUT.
    let via_fan_out = outcome_runs(&fan_out(&url, 15, &lead_badge, &workspace, &[SHARED_KEY]));
    let run = via_fan_out.first().expect("one task, one run").clone();
    tail_until(&url, FACT_DEADLINE, |e| {
        e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(run)
    });

    // Restart. The macaroon root key dies here (DR-017 §6) — which is exactly
    // why the read-back below goes through the operator-badged `spawn_agent`.
    restart_daemon_with_mcp(&mut daemon, &lock_path);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url after restart").to_string();
    let operator = lock["badge"]
        .as_str()
        .expect("badge after restart")
        .to_string();
    initialize(&url);

    // Process 2: the SAME key, presented to SPAWN_AGENT, must resolve to the run
    // fan_out minted before the restart.
    let result = mcp_tool_call(
        &url,
        16,
        "spawn_agent",
        json!({
            "badge": operator,
            "workspace": workspace,
            "agent": "impl",
            "idempotency_key": SHARED_KEY,
        }),
    );
    assert_ne!(
        result["isError"],
        json!(true),
        "a keyed spawn_agent retry after restart is the §9 contract, not an error: {result:#}"
    );
    let resolved = tool_payload(&result)["run"]
        .as_str()
        .expect("spawn_agent names the run ulid")
        .to_string();

    assert_eq!(
        resolved, run,
        "the key fan_out used in process 1 resolves, in process 2, to the run fan_out minted — \
         so fan_out writes into the SHARED per-workspace spawn_keys map (DR-044 §Decision 1: \
         \"no new dedup mechanism\") and that map is LOG-DERIVED, not process memory (I3). A \
         fresh ULID here means either a fan-out-private key space or an in-memory map."
    );

    // And nothing new was spawned: still exactly one agent.spawned for that run.
    let log = cold_read(&mut daemon);
    let spawns = log
        .iter()
        .filter(|e| e.subject.as_str() == "agent.spawned" && e.payload()["run"] == json!(run))
        .count();
    assert_eq!(
        spawns, 1,
        "run {run} was spawned EXACTLY ONCE across the fan_out and the post-restart keyed \
         spawn_agent — a keyed retry spawns nothing new (DR-044 §Decision 1)"
    );
}

// --- CRITERION: the allocator PRINCIPAL, both directions ---------------------

/// CRITERION (ontology `worktree.allocated.allocator` v1, widened by the warden
/// for DR-044 §Decision 3) — the recorded allocating principal is correct in
/// BOTH directions, on one log:
///
/// an ORDINARY (non-fan-out) allocation emits `allocator: "rezidnt"` VERBATIM —
/// the daemon on its own initiative, with no delegating lead; and a FAN-OUT
/// allocation emits the scheme-tagged `allocator: "run:<lead ULID>"`, naming the
/// lead it allocated on behalf of.
///
/// ## Why both directions, in one test
///
/// The warden ratified the widening with an explicit warning: repointing
/// ordinary allocations at a delegating value would make "the daemon allocated
/// this on its own initiative" UNEXPRESSIBLE (`spec/ontology.md:215`). The two
/// assertions are mutually exclusive by construction, so no single-value
/// implementation can satisfy both — that is this test's non-vacuity, and it is
/// why they are pinned together rather than in separate files.
///
/// ## Honesty: this is a REGRESSION pin, not a failing-first oracle
///
/// The value plumbing already shipped in lane 2 (`bins/rezidentd/src/runs.rs`
/// selects the principal from the `LeadDelegation`), so this test is expected to
/// be GREEN on arrival. It is written because NOTHING machine-checked the value
/// before it: both existing boards assert only the PRESENCE or ABSENCE of
/// `worktree.allocated`, deliberately, so that the warden session widening the
/// vocabulary could not invalidate them. That isolation was correct then and
/// leaves exactly this hole now, which the implementer flagged. A green-on-
/// arrival pin over shipped behavior is a regression guard; it is not evidence
/// that the oracle drove the behavior, and it is not presented as such.
///
/// The tagged form is asserted as a WHOLE STRING (`run:<ULID>`), never by
/// prefix-only or by "contains the ULID": the ontology fixes the scheme prefix
/// precisely so a consumer never has to guess whether a value is a sentinel or
/// an id, and a bare ULID is explicitly NOT legal there.
#[test]
fn ordinary_allocations_stay_rezidnt_and_fan_out_allocations_name_the_lead() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let operator = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    let (project_dir, base_spec) = make_project(20);
    let harness = badge_dumping_harness(project_dir.path());
    let spec = with_badge_dump_and_role(&base_spec, &harness);

    // The lead's OWN allocation happens on the open chain — an ordinary spawn
    // with no delegating lead.
    let (workspace, lead_run, lead_badge) = open_and_capture_lead(&url, &operator, &spec);

    let sub_runs = outcome_runs(&fan_out(
        &url,
        17,
        &lead_badge,
        &workspace,
        &["dr044-alloc-a", "dr044-alloc-b"],
    ));
    assert_eq!(sub_runs.len(), 2, "two tasks, two subs");
    for sub in &sub_runs {
        tail_until(&url, FACT_DEADLINE, |e| {
            e["subject"] == "agent.spawned" && e["payload"]["run"] == json!(sub)
        });
    }

    let log = cold_read(&mut daemon);
    let allocations: Vec<String> = log
        .iter()
        .filter(|e| e.subject.as_str() == "worktree.allocated")
        .map(|e| {
            e.payload()["allocator"]
                .as_str()
                .unwrap_or_else(|| {
                    panic!(
                        "worktree.allocated.allocator is a REQUIRED v1 field: {:#}",
                        e.payload()
                    )
                })
                .to_string()
        })
        .collect();

    // 1 lead (ordinary, on the open chain) + 2 subs (fan-out).
    assert_eq!(
        allocations.len(),
        3,
        "one ordinary allocation for the lead plus one per sub: {allocations:?}"
    );

    let delegating = format!("run:{lead_run}");
    let ordinary = allocations.iter().filter(|a| *a == "rezidnt").count();
    let delegated = allocations.iter().filter(|a| **a == delegating).count();

    assert_eq!(
        ordinary, 1,
        "the LEAD's own allocation — an ordinary spawn on the open chain, with no delegating \
         lead — records `rezidnt` VERBATIM. Repointing ordinary allocations at a delegating \
         value would make \"the daemon allocated this on its own initiative\" unexpressible \
         (ontology `worktree.allocated.allocator` v1, warden 2026-07-24): {allocations:?}"
    );
    assert_eq!(
        delegated, 2,
        "each SUB's allocation records the scheme-tagged `{delegating}` — the daemon allocated \
         on behalf of the named lead run, so the log ALONE answers \"which lead allocated this \
         worktree\" (DR-044 §Decision 3): {allocations:?}"
    );

    // Nothing outside the ratified vocabulary, and specifically never a BARE
    // ULID (the ontology fixes the `run:` scheme prefix so a consumer never has
    // to guess sentinel-vs-id) and never the reserved `human` sentinel, which
    // rezidnt does not emit on this subject.
    for allocator in &allocations {
        assert!(
            allocator == "rezidnt" || *allocator == delegating,
            "allocator {allocator:?} is outside the ratified v1 vocabulary \
             {{\"rezidnt\", \"run:<ULID>\"}} — a bare ULID is NOT legal and `human` is reserved \
             for out-of-band observation, never emitted by rezidnt here: {allocations:?}"
        );
        assert_ne!(
            allocator, &lead_run,
            "the delegating form is scheme-TAGGED `run:<ULID>`, never a bare ULID: {allocations:?}"
        );
    }
}
// --- CRITERION (DR-044 §Decision 3): a refused sub is not a failed sub -------

/// Make the workspace's worktree base UN-WRITABLE so every subsequent
/// allocation fails, without touching git internals or the lead's already-live
/// worktree. `create_dir_all` still succeeds (the dir exists), but
/// `git worktree add <base>/<name>` cannot create its subdir.
///
/// This is the only lever available: worktree paths are ULID-derived
/// (`<base>/<agent>-<run ULID>`), so a test cannot predict — and therefore
/// cannot pre-claim — the path a given task will take. See the honesty note on
/// the test below for what that costs.
fn block_allocations(repo: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let base = repo.join(".rezidnt").join("worktrees");
    assert!(
        base.is_dir(),
        "precondition: the lead's own allocation created {}",
        base.display()
    );
    let mut perms = std::fs::metadata(&base).expect("stat base").permissions();
    perms.set_mode(0o555); // r-xr-xr-x: traversable, not writable
    std::fs::set_permissions(&base, perms).expect("chmod worktree base read-only");
}

fn unblock_allocations(repo: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let base = repo.join(".rezidnt").join("worktrees");
    if let Ok(meta) = std::fs::metadata(&base) {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&base, perms);
    }
}

/// CRITERION (DR-044 §Decision 3 / I6, §Consequences (e)) — a task whose
/// worktree CANNOT be allocated is a REFUSED SUB, not a failed run:
///
/// no run is minted, so nothing folds as `fail` (there is no run to fail); the
/// task's outcome carries a machine-readable refusal code; the call itself is
/// NOT a whole-call error; and the refused task appears in NO `VerdictRollup`
/// bucket — not passed, not failed, not inconclusive, not pending. Plus the
/// liveness half of §Decision 3: NO in-daemon retry loop, so the call RETURNS
/// rather than livelocking against a sole allocator.
///
/// ## Honesty: what this does and does not cover
///
/// DR-044 §Decision 3 names a WORKTREE CONFLICT (a registry double-claim) as the
/// refusal trigger. This test forces a different allocation failure, because a
/// registry double-claim has NO deterministic black-box seam today: worktree
/// paths are ULID-derived, so a test cannot pre-claim the path a task will take,
/// and two fan-out tasks can never collide with each other. What is under test
/// here is the SEMANTICS the record fixes — refused-is-not-failed, remaining
/// subs stand, never tallied — which are identical for any allocation refusal.
/// The conflict-SPECIFIC obligations (exactly one `worktree.conflict`, the
/// allocation errors rather than taking over) are already pinned at the level
/// where a deterministic oracle DOES exist:
/// `crates/rezidnt-adapters/git/tests/worktree_conflict.rs` and
/// `restart_and_discovery.rs`.
///
/// What is genuinely uncovered, and stays uncovered until someone builds the
/// seam: that a conflict raised INSIDE a live fan-out surfaces as this refusal
/// shape. Making it testable needs an injectable allocation seam or a
/// test-visible worktree path — a change to the daemon, not to this test.
#[test]
fn an_unallocatable_task_is_a_refused_sub_and_never_a_tallied_one() {
    let (mut daemon, lock_path) = start_daemon_with_mcp(None);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("url").to_string();
    let operator = lock["badge"].as_str().expect("badge").to_string();
    initialize(&url);

    let (project_dir, base_spec) = make_project(20);
    let repo = project_dir.path().join("repo");
    let harness = badge_dumping_harness(project_dir.path());
    let spec = with_badge_dump_and_role(&base_spec, &harness);

    // The lead allocates normally; only the SUBS are blocked.
    let (workspace, lead_run, lead_badge) = open_and_capture_lead(&url, &operator, &spec);
    block_allocations(&repo);

    let started = Instant::now();
    let result = fan_out(
        &url,
        18,
        &lead_badge,
        &workspace,
        &["dr044-refused-a", "dr044-refused-b"],
    );
    let elapsed = started.elapsed();
    unblock_allocations(&repo);

    // §Decision 3 liveness — "no in-daemon retry loop against a sole-allocator
    // registry: that is a livelock". A generous bound; the claim is that the
    // call terminates at all, not that it is fast.
    assert!(
        elapsed < Duration::from_secs(60),
        "fan_out must RETURN on an unallocatable task — DR-044 §Decision 3 forbids an in-daemon \
         retry loop (a livelock against a sole allocator); re-issuing the call with the same \
         keys is the honest retry. Took {elapsed:?}"
    );

    // A per-task failure is NOT a whole-call failure.
    assert_ne!(
        result["isError"],
        json!(true),
        "an unallocatable TASK does not fail the CALL — the report is the per-task outcome \
         vector (DR-044 §Decision 1): {result:#}"
    );

    let payload = tool_payload(&result);
    let outcomes = payload["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("fan_out returns a per-task outcome vector: {payload:#}"));
    assert_eq!(outcomes.len(), 2, "one outcome per task: {outcomes:#?}");
    for outcome in outcomes {
        // Pinned to the ALLOCATION failure specifically, not merely "some code":
        // a task refused for an unrelated reason (an unknown agent, a bad key)
        // would otherwise satisfy this test without exercising the criterion.
        assert_eq!(
            outcome["code"],
            json!("spawn.failed"),
            "a refused task's outcome carries a machine-readable refusal code: {outcome:#}"
        );
        assert!(
            outcome["message"]
                .as_str()
                .unwrap_or_default()
                .contains("worktree"),
            "and the refusal is the WORKTREE ALLOCATION failing, which is the DR-044 §Decision 3              trigger class under test — not some unrelated per-task refusal: {outcome:#}"
        );
        assert!(
            outcome.get("run").is_none() || outcome["run"].is_null(),
            "a refused task mints NO run — there is nothing to fail, so nothing folds as a \
             failed sub (DR-044 §Decision 3): {outcome:#}"
        );
    }

    // The log is the judge. Fold it and project: the refused tasks must be
    // invisible to the orchestration graph — no sub row, and therefore no
    // bucket in the lead's VerdictRollup.
    let log = cold_read(&mut daemon);
    let view = project(&log);

    let lead = view.leads.iter().find(|l| l.lead_run == lead_run);
    let (fan_out_width, rollup_total) = match lead {
        Some(lead) => (
            lead.fan_out,
            lead.verdict_rollup.passed
                + lead.verdict_rollup.failed
                + lead.verdict_rollup.inconclusive
                + lead.verdict_rollup.pending,
        ),
        // No lead row at all is the correct projection of "delegated to nobody".
        None => (0, 0),
    };
    assert_eq!(
        fan_out_width, 0,
        "both tasks were refused, so the lead has NO subs — a refused task never inflates \
         fan_out (DR-044 §Decision 3): {view:#?}"
    );
    assert_eq!(
        rollup_total, 0,
        "a refused task is counted in NO VerdictRollup bucket — not passed, not failed, not \
         inconclusive, and not pending (I6, DR-044 §Consequences (e)): {view:#?}"
    );

    // And nothing folded as a failed RUN either: no agent.spawned carrying
    // either refused key ever reached the log.
    for key in ["dr044-refused-a", "dr044-refused-b"] {
        assert!(
            log.iter().all(|e| {
                e.subject.as_str() != "agent.spawned"
                    || e.payload()["idempotency_key"] != json!(key)
            }),
            "no agent.spawned was emitted for refused task {key} — a refused sub is not a \
             spawned-then-failed sub (DR-044 §Decision 3)"
        );
    }
}
