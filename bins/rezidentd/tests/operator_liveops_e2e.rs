//! LIVE-OP END-TO-END DEMO — every operator action driven against ONE real
//! daemon through the REAL `rezidnt` CLI an operator types, not a synthetic log
//! poke. This is the "does the whole operator surface actually work live" proof
//! the DR-032/033/034/035 arc earned: each test stands up `rezidentd`, opens a
//! real run that escalates a permit, and drives one operator action end to end,
//! asserting the LOGGED outcome (I3: the log is truth). The scenarios are
//! serialized by a process-wide lock (`SERIAL`), so they are safe under the
//! default multi-threaded `cargo test` (they run one-at-a-time regardless);
//! `--test-threads=1 --nocapture` is only needed to read the ORDERED narrated
//! transcript:
//!
//! ```text
//! cargo test -p rezidentd --test operator_liveops_e2e -- --test-threads=1 --nocapture
//! ```
//!
//! ## Why the CLI, not the socket/MCP helper
//! The existing per-feature oracles (`permit_live_unblock.rs`, the state and mcp
//! fold/interrogability boards) drive the loopback-HTTP door directly. This demo
//! instead shells out to the actual `rezidnt operator …` binary (located via
//! `rezidnt_testkit::cli_bin`), reading the 0600 lockfile through `REZIDNT_LOCKFILE`
//! exactly as a human operator's shell would. So a green run proves the operator's
//! REAL tool — exit codes, badge read, loopback POST, daemon derive, fold, PDP —
//! works front to back. In particular it is the FIRST live-daemon exercise of the
//! `rezidnt operator resolve-permit --scope`/`--ttl-ms` flags (elsewhere only
//! stub-backed at the CLI edge and pure-fold at the state edge).
//!
//! ## The six operator actions demonstrated (one test each)
//!   1. resolve `allow` — DR-033 honored-on-next-ask (via CLI).
//!   2. resolve `deny` — the recorded human override (via CLI).
//!   3. live-unblock — DR-034: a HELD escalation resumes on a landing resolve.
//!   4. TTL-boxed — DR-035 §Decision 1: `--ttl-ms` applies, then re-escalates.
//!   5. broad grant — DR-035 §Decision 2: `--scope run_tool` grants a sibling action on the same tool (and is bounded to that tool).
//!   6. kill-run — DR-032: terminate a live run's process (via CLI).
//!
//! Plus the DR-035 §Decision 3 coupling guard: `--scope` WITHOUT `--ttl-ms` is
//! refused (broad-and-permanent is unmintable).
//!
//! Cross-platform: unix-only (the socket transport + the DR-034 `#[cfg(unix)]`
//! live-unblock bodies). Host `/vet` cannot reach these; lint/run on WSL.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use common::{
    cli_bin, connect, read_reply_line, read_until, send_line, start_daemon_with_mcp_and_unblock,
    stub_harness, wait_for_lockfile,
};
use serde_json::{Value, json};

const LOCK_DEADLINE: Duration = Duration::from_secs(10);
const TAIL_DEADLINE: Duration = Duration::from_secs(20);
/// A LONG live-unblock budget: the held request only needs to stay OPEN until the
/// open→escalate→CLI-shellout→resolve round-trip lands, and the daemon releases
/// the instant the resolution arrives — so a generous budget is FREE on the happy
/// path (the test never waits it out) and buys robustness against a cold `rezidnt`
/// process spawn + loopback POST under a loaded/parallel runner (auditor's timing
/// note). Only the never-resolved case would pay it, and no test here leaves the
/// held request unresolved.
const UNBLOCK_LONG_MS: u64 = 20_000;
/// A SHORT live-unblock budget for the scenarios that only need an escalation to
/// resolve: an escalated ask degrades to `ask` quickly (the daemon holds every
/// escalated request for the budget before degrading — DR-034), so the "first
/// ask" that seeds the escalation returns promptly instead of being held.
const UNBLOCK_SHORT_MS: u64 = 500;
/// The TTL window for the time-boxed scenario. Wide enough that the WITHIN-window
/// ask (fired right after the CLI resolve returns) comfortably lands inside it
/// even under load (a cold CLI spawn is well under this), so the WITHIN leg is not
/// wall-clock-tight; the AFTER leg sleeps past it (`TTL_MS + TTL_SLACK_MS`).
const TTL_MS: u64 = 5_000;
/// Extra sleep past the TTL deadline before the AFTER-window ask, so expiry is
/// unambiguous even if the resolve/ask ULIDs are minted a little apart.
const TTL_SLACK_MS: u64 = 1_500;

/// Serialize the scenarios so this file's seven real daemons (+ CLI shell-outs +
/// stub-harness processes) do not all spin up at once under the default
/// multi-threaded `cargo test` — core contention is the documented flake vector
/// for the spawn-heavy daemon tests. Held for each test's whole body. Poisoning is
/// ignored (a panicking test still releases the lock for the rest).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------------------------------------------------------------------------
// Narration — each test prints an ordered transcript to stderr so a `--nocapture`
// run reads as a live-op session, not just pass/fail. `step` numbers the beats.
// ---------------------------------------------------------------------------

fn banner(title: &str) {
    eprintln!("\n════════════════════════════════════════════════════════════");
    eprintln!("  LIVE-OP: {title}");
    eprintln!("════════════════════════════════════════════════════════════");
}

fn step(msg: &str) {
    eprintln!("  → {msg}");
}

// ---------------------------------------------------------------------------
// Fixtures + socket helpers (an empty-permit project that escalates to `ask`,
// the natural "held/escalated" starting state). Mirrors the private builder in
// `permit_live_unblock.rs`; inlined because it is test-file-private there.
// ---------------------------------------------------------------------------

/// An empty-permit-gate project: `gates = ["permit"]`, verifier set EMPTY → the
/// aggregator escalates (DR-011 §3) → a `request_permission` for any tool
/// escalates to `ask`. The stub harness holds the spawned run's process alive for
/// `gap_ms` so a permission can be asked mid-run and the run has a live pid to
/// kill.
fn make_empty_permit_project(dir: &Path, gap_ms: u64) -> String {
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());
    let script = stub_harness(dir, gap_ms);
    format!(
        r#"[project]
name = "liveops-empty-permit"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["permit"]
bin_override = "{script}"

[gates.permit]
verifiers = []
"#,
        repo = repo.display(),
        script = script.display(),
    )
}

/// Open the spec over the bare socket and tail until `agent.spawned`; return the
/// spawned run's ulid — the live handle every operator action targets.
fn open_and_get_run(socket: &Path, spec: &str) -> String {
    let mut opener = connect(socket);
    send_line(
        &mut opener,
        &serde_json::to_string(&json!({"op": "open", "spec_toml": spec})).unwrap(),
    );
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "agent.spawned"
    });
    lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .expect("agent.spawned on the fabric")["payload"]["run"]
        .as_str()
        .expect("agent.spawned carries the run ulid")
        .to_string()
}

/// Fire ONE `request_permission` on its own connection and return the single
/// reply frame. `reply_ms` bounds the reader; set it LONGER than the daemon's
/// unblock deadline when the request may be held so the test observes the
/// daemon's own release (resolve-applied OR expiry-`ask`), not a client cutoff.
fn ask(
    socket: &Path,
    run: &str,
    action: &str,
    tool: &str,
    request_id: &str,
    reply_ms: u64,
) -> thread::JoinHandle<Value> {
    let socket = socket.to_path_buf();
    let (run, action, tool, request_id) = (
        run.to_string(),
        action.to_string(),
        tool.to_string(),
        request_id.to_string(),
    );
    thread::spawn(move || {
        let mut conn = connect(&socket);
        send_line(
            &mut conn,
            &serde_json::to_string(&json!({
                "op": "request_permission",
                "run": run,
                "request_id": request_id,
                "action": action,
                "tool": tool,
            }))
            .unwrap(),
        );
        read_reply_line(&mut conn, Duration::from_millis(reply_ms))
    })
}

/// Wait (via a fresh tail) until a `permit.escalated` carrying `request_id`
/// lands — proof the ask reached the daemon and escalated, so the operator has a
/// real escalation to resolve.
fn await_escalation(socket: &Path, run: &str, request_id: &str) {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "permit.escalated"
            && v["payload"]["run"] == json!(run)
            && v["payload"]["request_id"] == json!(request_id)
    });
}

// ---------------------------------------------------------------------------
// The operator's REAL tool — shell out to `rezidnt operator …`, reading the
// lockfile via `REZIDNT_LOCKFILE` exactly as a human's shell does.
// ---------------------------------------------------------------------------

/// Run `rezidnt operator resolve-permit <run> <request_id> <decision> [extra…]`
/// against the live daemon whose lockfile is at `lockfile`. Returns (exit,
/// stderr). `extra` carries `--ttl-ms`/`--scope`/`--reason` as an operator would.
fn cli_resolve(
    lockfile: &Path,
    run: &str,
    request_id: &str,
    decision: &str,
    extra: &[&str],
) -> (Option<i32>, String) {
    let out = Command::new(cli_bin())
        .arg("operator")
        .arg("resolve-permit")
        .arg(run)
        .arg(request_id)
        .arg(decision)
        .args(extra)
        .env("REZIDNT_LOCKFILE", lockfile)
        .output()
        .expect("spawn rezidnt operator resolve-permit");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Run `rezidnt operator kill-run <run>` against the live daemon.
fn cli_kill(lockfile: &Path, run: &str) -> (Option<i32>, String) {
    let out = Command::new(cli_bin())
        .arg("operator")
        .arg("kill-run")
        .arg(run)
        .env("REZIDNT_LOCKFILE", lockfile)
        .output()
        .expect("spawn rezidnt operator kill-run");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Boot one daemon with the given unblock budget + wait for its lockfile; return
/// (guard, lockfile-path). Kept as a one-liner so each scenario reads as a script.
fn boot(unblock_ms: u64) -> (common::DaemonGuard, std::path::PathBuf) {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(unblock_ms);
    // Force the lockfile to have appeared/parsed before the scenario drives the CLI.
    let _ = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    (daemon, lock_path)
}

/// Tail until a fact with `subject` carrying `payload.run == run` lands; return
/// it. Proof-on-the-log for an operator action (I3: on the log = it happened).
fn await_fact(socket: &Path, subject: &str, run: &str) -> Value {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == json!(subject) && v["payload"]["run"] == json!(run)
    });
    lines
        .into_iter()
        .find(|v| v["subject"] == json!(subject) && v["payload"]["run"] == json!(run))
        .expect("the awaited fact is present in the tail")
}

// ===========================================================================
// 1. resolve ALLOW — DR-033 honored-on-next-ask, via the real CLI.
// ===========================================================================

/// An operator approves an escalated permit with `rezidnt operator resolve-permit
/// … allow`; the agent's NEXT ask for that action is honored `allow` (the DR-033
/// ledger-check), driven end to end through the real binary.
#[test]
fn resolve_allow_honored_next_ask_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("resolve ALLOW → honored on the agent's next ask (DR-033)");
    let (daemon, lock_path) = boot(UNBLOCK_SHORT_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);
    step(&format!("opened run {run}"));

    const ESC: &str = "01LIVEOPALLOWESCREQ0000001";
    let first = ask(&daemon.socket, &run, "tool.invoke", "Bash", ESC, 4_000);
    await_escalation(&daemon.socket, &run, ESC);
    let first_reply = first.join().expect("first ask thread");
    assert_eq!(
        first_reply["decision"],
        json!("ask"),
        "the empty policy escalates the first ask to `ask`: {first_reply:#}"
    );
    step("agent asked to run Bash → escalated to the operator (ask)");

    let (code, stderr) = cli_resolve(
        &lock_path,
        &run,
        ESC,
        "allow",
        &["--reason", "approved in demo"],
    );
    assert_eq!(
        code,
        Some(0),
        "operator resolve-permit allow exits 0; stderr: {stderr}"
    );
    step("operator: `rezidnt operator resolve-permit <run> <req> allow` → exit 0");

    const NEXT: &str = "01LIVEOPALLOWNEXTREQ0000001";
    let next = ask(&daemon.socket, &run, "tool.invoke", "Bash", NEXT, 4_000);
    let next_reply = next.join().expect("next ask thread");
    assert_eq!(
        next_reply["decision"],
        json!("allow"),
        "the fresh ask is HONORED from the folded resolution (DR-033 next-ask): {next_reply:#}"
    );
    step("agent re-asks (fresh request_id) → daemon honors it: ALLOW ✓");
}

// ===========================================================================
// 2. resolve DENY — the recorded human override, via the real CLI.
// ===========================================================================

/// Symmetric to (1): `… deny` is honored as a recorded `deny` on the next ask —
/// a human override, never a silent proceed (I6).
#[test]
fn resolve_deny_honored_next_ask_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("resolve DENY → recorded override on the next ask (DR-033, I6)");
    let (daemon, lock_path) = boot(UNBLOCK_SHORT_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);
    step(&format!("opened run {run}"));

    const ESC: &str = "01LIVEOPDENYESCREQ00000001";
    let first = ask(&daemon.socket, &run, "tool.invoke", "Bash", ESC, 4_000);
    await_escalation(&daemon.socket, &run, ESC);
    let _ = first.join().expect("first ask thread");
    step("agent asked to run Bash → escalated (ask)");

    let (code, stderr) = cli_resolve(&lock_path, &run, ESC, "deny", &[]);
    assert_eq!(
        code,
        Some(0),
        "operator resolve-permit deny exits 0; stderr: {stderr}"
    );
    step("operator: `… resolve-permit <run> <req> deny` → exit 0");

    const NEXT: &str = "01LIVEOPDENYNEXTREQ00000001";
    let next = ask(&daemon.socket, &run, "tool.invoke", "Bash", NEXT, 4_000);
    let next_reply = next.join().expect("next ask thread");
    assert_eq!(
        next_reply["decision"],
        json!("deny"),
        "the fresh ask is DENIED from the folded override (recorded, not silent): {next_reply:#}"
    );
    step("agent re-asks → daemon honors the override: DENY ✓");
}

// ===========================================================================
// 3. live-unblock — DR-034: a HELD escalation resumes on a landing resolve.
// ===========================================================================

/// The operator resolves WHILE the agent's request is still held open; the held
/// connection wakes and resumes with the operator's decision, keyed by its
/// ORIGINAL request_id — no re-ask (DR-034 §Decision 1/3).
#[test]
fn live_unblock_resumes_held_request_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("live-unblock → resolve WHILE held resumes the stalled agent (DR-034)");
    // LONG budget: the request stays HELD open long enough for the operator's
    // resolve to land and resume it.
    let (daemon, lock_path) = boot(UNBLOCK_LONG_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);
    step(&format!("opened run {run}"));

    const HELD: &str = "01LIVEOPUNBLOCKHELDREQ00001";
    // Reply reader outlasts the unblock budget so we observe the daemon's release.
    let held = ask(
        &daemon.socket,
        &run,
        "tool.invoke",
        "Bash",
        HELD,
        UNBLOCK_LONG_MS + 4_000,
    );
    await_escalation(&daemon.socket, &run, HELD);
    step("agent's Bash ask is HELD open (stalled), escalated to the operator");

    let (code, stderr) = cli_resolve(
        &lock_path,
        &run,
        HELD,
        "allow",
        &["--reason", "live unblock in demo"],
    );
    assert_eq!(
        code,
        Some(0),
        "operator resolve-permit (held) exits 0; stderr: {stderr}"
    );
    step("operator resolves ALLOW while the request is still held");

    let reply = held.join().expect("held request thread");
    assert_eq!(
        reply["decision"],
        json!("allow"),
        "the HELD request resumed with allow — no re-ask (DR-034 §Decision 1): {reply:#}"
    );
    assert_eq!(
        reply["request_id"],
        json!(HELD),
        "the woken reply carries the ORIGINAL held request_id (DR-034 §Decision 3): {reply:#}"
    );
    step("held connection woke and RESUMED: allow, original request_id ✓");
}

// ===========================================================================
// 4. TTL-boxed — DR-035 §Decision 1: `--ttl-ms` applies, then re-escalates.
// ===========================================================================

/// A time-boxed resolution: `--ttl-ms N` is honored while a fresh ask lands
/// within N ms of the resolution, then re-escalates once the window lapses. The
/// deadline is a pure fold of two event-ULID timestamps, but over a LIVE daemon
/// the ULIDs are minted in real time, so a real sleep past the window expires it.
#[test]
fn ttl_boxed_resolution_applies_then_expires_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("TTL-boxed resolve → applies inside the window, re-escalates after (DR-035 §1)");
    let (daemon, lock_path) = boot(UNBLOCK_SHORT_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 8_000);
    let run = open_and_get_run(&daemon.socket, &spec);
    step(&format!("opened run {run}"));

    const ESC: &str = "01LIVEOPTTLESCREQ000000001";
    let first = ask(&daemon.socket, &run, "tool.invoke", "Bash", ESC, 4_000);
    await_escalation(&daemon.socket, &run, ESC);
    let _ = first.join().expect("first ask thread");

    // Time-box the grant (TTL_MS window, comfortably wider than a cold CLI spawn).
    let (code, stderr) = cli_resolve(
        &lock_path,
        &run,
        ESC,
        "allow",
        &[
            "--ttl-ms",
            &TTL_MS.to_string(),
            "--reason",
            "time-boxed grant in demo",
        ],
    );
    assert_eq!(
        code,
        Some(0),
        "operator resolve-permit --ttl-ms exits 0; stderr: {stderr}"
    );
    step(&format!(
        "operator: `… resolve-permit … allow --ttl-ms {TTL_MS}` → exit 0"
    ));

    // Within the window: a fresh ask fired immediately is honored.
    const WITHIN: &str = "01LIVEOPTTLWITHINREQ0000001";
    let within = ask(&daemon.socket, &run, "tool.invoke", "Bash", WITHIN, 4_000);
    let within_reply = within.join().expect("within ask thread");
    assert_eq!(
        within_reply["decision"],
        json!("allow"),
        "a fresh ask WITHIN the ttl window is honored allow: {within_reply:#}"
    );
    step("fresh ask inside the window → ALLOW ✓");

    // Past the window: the same action re-escalates (fail-closed to ask).
    thread::sleep(Duration::from_millis(TTL_MS + TTL_SLACK_MS));
    const AFTER: &str = "01LIVEOPTTLAFTERREQ00000001";
    let after = ask(&daemon.socket, &run, "tool.invoke", "Bash", AFTER, 4_000);
    let after_reply = after.join().expect("after ask thread");
    assert_eq!(
        after_reply["decision"],
        json!("ask"),
        "past the ttl deadline the same action RE-ESCALATES (DR-035 §Decision 1): {after_reply:#}"
    );
    step("after sleeping past the deadline → RE-ESCALATES to ask ✓");
}

// ===========================================================================
// 5. broad grant — DR-035 §Decision 2: `--scope run_tool` grants a sibling
//    action on the same tool (and is bounded to that tool). FIRST live-daemon
//    exercise of the `--scope` flag.
// ===========================================================================

/// A broad, time-boxed grant: resolving one escalated action with
/// `--scope run_tool --ttl-ms N` grants ANY action on the same `(run, tool)` — a
/// DIFFERENT action (`tool.exec` vs the resolved `tool.invoke`) on the same tool
/// is honored — while staying bounded to that tool (a different tool still
/// escalates).
#[test]
fn broad_grant_scope_run_tool_grants_sibling_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner(
        "broad grant `--scope run_tool` → sibling action honored, other tool still asks (DR-035 §2)",
    );
    let (daemon, lock_path) = boot(UNBLOCK_SHORT_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 8_000);
    let run = open_and_get_run(&daemon.socket, &spec);
    step(&format!("opened run {run}"));

    const ESC: &str = "01LIVEOPSCOPEESCREQ00000001";
    let first = ask(&daemon.socket, &run, "tool.invoke", "Bash", ESC, 4_000);
    await_escalation(&daemon.socket, &run, ESC);
    let _ = first.join().expect("first ask thread");
    step("agent's `tool.invoke` on Bash escalated");

    // Broad AND time-boxed (the coupling: broad requires a ttl).
    let (code, stderr) = cli_resolve(
        &lock_path,
        &run,
        ESC,
        "allow",
        &[
            "--scope",
            "run_tool",
            "--ttl-ms",
            "60000",
            "--reason",
            "grant any action on this tool, time-boxed",
        ],
    );
    assert_eq!(
        code,
        Some(0),
        "operator resolve-permit --scope run_tool --ttl-ms exits 0; stderr: {stderr}"
    );
    step("operator: `… resolve-permit … allow --scope run_tool --ttl-ms 60000` → exit 0");

    // A DIFFERENT action on the SAME tool — granted by the broad resolution.
    const SIBLING: &str = "01LIVEOPSCOPESIBLINGREQ0001";
    let sibling = ask(&daemon.socket, &run, "tool.exec", "Bash", SIBLING, 4_000);
    let sibling_reply = sibling.join().expect("sibling ask thread");
    assert_eq!(
        sibling_reply["decision"],
        json!("allow"),
        "a SIBLING action (tool.exec) on the same tool is honored by the broad grant: {sibling_reply:#}"
    );
    step("agent asks a DIFFERENT action (tool.exec) on Bash → ALLOW (broadened) ✓");

    // The blast-radius bound: a DIFFERENT tool still escalates — broad ≠ unlimited.
    const OTHER_TOOL: &str = "01LIVEOPSCOPEOTHERTOOLREQ01";
    let other = ask(
        &daemon.socket,
        &run,
        "tool.invoke",
        "Grep",
        OTHER_TOOL,
        4_000,
    );
    let other_reply = other.join().expect("other-tool ask thread");
    assert_eq!(
        other_reply["decision"],
        json!("ask"),
        "a DIFFERENT tool (Grep) is NOT covered — broad is bounded to the tool (DR-035 §2): {other_reply:#}"
    );
    step("agent asks on a different tool (Grep) → still ASKS (bounded) ✓");
}

// ===========================================================================
// 6. kill-run — DR-032: terminate a live run's process, via the real CLI.
// ===========================================================================

/// The operator kills a live run: `rezidnt operator kill-run <run>` drives the
/// reaper against the run's log-derived pid, exits 0, and lands the
/// `agent.signaled` attribution fact on the log (I3: on the log = it happened).
/// The fact names the terminal signal — the interrogable "a human killed this
/// run", distinct from a daemon-timeout stop.
#[test]
fn kill_run_terminates_live_run_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("kill-run → terminate a live run's process, attributed on the log (DR-032)");
    let (daemon, lock_path) = boot(UNBLOCK_SHORT_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    // A long gap so the run's process is comfortably alive when we kill it.
    let spec = make_empty_permit_project(dir.path(), 30_000);
    let run = open_and_get_run(&daemon.socket, &spec);
    step(&format!("opened run {run} (process alive)"));

    let (code, stderr) = cli_kill(&lock_path, &run);
    assert_eq!(
        code,
        Some(0),
        "operator kill-run on a live run exits 0; stderr: {stderr}"
    );
    step("operator: `rezidnt operator kill-run <run>` → exit 0 (reaper signalled)");

    // The honest proof it happened: an `agent.signaled` fact for this run lands
    // on the log (the single-writer attribution DR-032 records).
    let signaled = await_fact(&daemon.socket, "agent.signaled", &run);
    let sig = signaled["payload"]["signal"].as_str().unwrap_or("");
    assert!(
        sig == "SIGTERM" || sig == "SIGKILL",
        "the kill fact names the terminal signal (SIGTERM/SIGKILL): {signaled:#}"
    );
    step(&format!(
        "`agent.signaled` landed on the log (signal={sig}): the kill is recorded + attributed ✓"
    ));
}

// ===========================================================================
// Coupling guard — DR-035 §Decision 3: `--scope` WITHOUT `--ttl-ms` is refused.
// ===========================================================================

/// The structural guarantee, live: a broad grant with NO ttl is UNMINTABLE. The
/// daemon refuses `--scope run_tool` without `--ttl-ms` (`scope.requires_ttl`) —
/// the CLI surfaces it as a tool-refusal (exit 5), and NO `permit.resolved` lands.
#[test]
fn broad_and_permanent_is_refused_via_cli() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("coupling guard → `--scope` without `--ttl-ms` is REFUSED (DR-035 §3)");
    let (daemon, lock_path) = boot(UNBLOCK_SHORT_MS);
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    const ESC: &str = "01LIVEOPCOUPLINGESCREQ00001";
    let first = ask(&daemon.socket, &run, "tool.invoke", "Bash", ESC, 4_000);
    await_escalation(&daemon.socket, &run, ESC);
    let _ = first.join().expect("first ask thread");
    step("escalated; operator attempts a BROAD grant with no ttl…");

    // Broad but permanent — the forbidden quadrant.
    let (code, stderr) = cli_resolve(&lock_path, &run, ESC, "allow", &["--scope", "run_tool"]);
    assert_eq!(
        code,
        Some(5),
        "a broad-and-permanent resolve is refused (tool-refused, exit 5); stderr: {stderr}"
    );
    step("operator: `… resolve-permit … allow --scope run_tool` (no ttl) → REFUSED (exit 5) ✓");

    // And it left NO grant: a fresh ask still escalates (nothing was minted).
    const AFTER: &str = "01LIVEOPCOUPLINGAFTERREQ001";
    let after = ask(&daemon.socket, &run, "tool.invoke", "Bash", AFTER, 4_000);
    let after_reply = after.join().expect("after ask thread");
    assert_eq!(
        after_reply["decision"],
        json!("ask"),
        "the refused broad-and-permanent grant minted NOTHING — the next ask still escalates: {after_reply:#}"
    );
    step("no fact landed: a fresh ask still ASKS — broad-and-permanent is unmintable ✓");
}
