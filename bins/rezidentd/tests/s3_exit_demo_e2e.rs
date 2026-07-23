//! S3 PHASE-1-EXIT DEMO, END-TO-END THROUGH THE `rezidnt mcp` STDIO PROXY
//! (§16 S3; slice `s3-exit-demo`). One take, one real `rezidentd`, one real
//! `rezidnt mcp` subprocess: a local MCP client (Claude Code's transport) opens a
//! project, spawns an agent, reads its dossier, and receives a `gate_explain` for a
//! forced (LIVE-generated) non-pass — every beat driven as line-delimited JSON-RPC
//! over the proxy's stdin/stdout, so the WHOLE path is exercised in one shot:
//!
//! ```text
//!   client stdin  →  `rezidnt mcp` (stdio↔loopback-HTTP proxy)  →  rezidentd /mcp
//!                     (log append → fold → permit PDP)  →  response  →  client stdout
//! ```
//!
//! This is the S3 exit criterion made LIVE over the connection path an operator
//! actually uses (`bins/rezidnt/src/main.rs::mcp_serve`), not the direct-HTTP board
//! (`mcp_http.rs`) nor a bare-socket poke. The direct-HTTP oracle already pins the
//! daemon surface; THIS pins that the shipped stdio proxy relays initialize /
//! open_project / spawn_agent / resources-read / request_permission / gate_explain to
//! the resident daemon and back, badge-injected, in one session.
//!
//! ## What each beat pins (the CONTRACT under test)
//!   1. `initialize` — the proxy relays a well-formed JSON-RPC result (the transport
//!      is up end to end, lockfile → loopback → daemon → back).
//!   2. `tools/call open_project` — the proxy AUTO-INJECTS the operator badge (§12;
//!      the client never handles the token), so an un-badged client request still
//!      materializes: the result is not `isError` and names a workspace ULID.
//!   3. `tools/call spawn_agent` — returns a 26-char run ULID (the dossier +
//!      gate_explain target), badge injected by the proxy.
//!   4. `resources/read rezidnt://run/<ulid>/dossier` — the dossier is the FOLDED run
//!      state (status + accounting + gates), derived from the log, proving I3 (a
//!      derived read, never a side store).
//!   5. `tools/call request_permission` for an action the run's permit gate does not
//!      grant → a LIVE `permit.escalated` non-pass verdict on the log (NO seeded
//!      fixture, NO S4 verifier engine); then `tools/call gate_explain` returns the
//!      real interrogation: the `permit` gate, the failing verdict `ask`, the exact
//!      `request_id` that was escalated, and a resolvable `policy_ref` — and the
//!      non-pass is NOT coerced to `allow`/`pass` (I6 interrogability, never coerce).
//!
//! ## The forced-failure mechanism (and why THIS one)
//! The work order prefers a real DENY; S4's verifier engine does not exist, but the
//! permit PDP is LIVE. A deterministic in-test DENY needs a denying verifier wired
//! into the spec (an exec/role policy plus a role on the agent) — more moving parts,
//! an extra committed policy script, and a slower/less-deterministic path under a
//! loaded WSL runner. This demo instead uses the EMPTY permit gate
//! (`gates = ["permit"]`, `verifiers = []`), whose aggregator ESCALATES (DR-011 §3):
//! a `request_permission` for any tool produces a LIVE `permit.escalated` fact. An
//! escalation is a real, blocking, interrogable non-pass — `gate_explain` resolves it
//! generically by subject (crates/rezidnt-mcp/src/lib.rs `call_gate_explain` matches
//! `permit.escalated`) and surfaces the failing verdict `ask` (never coerced to
//! allow, I6) with its `policy_ref`/`request_id`. This is the fallback the work order
//! explicitly sanctions for when a deterministic deny is not cleanly reachable in a
//! pure test: it is LIVE-generated, not seeded, and `gate_explain` returns a real
//! interrogation. (A true deny would need a denying verifier config; the escalation is
//! the honest reachable non-pass here.)
//!
//! ## Honesty
//! RED until the demo passes end to end. Written before the whole path was proven
//! green over the proxy: if any relay leg is broken (badge not injected, loopback not
//! reached, gate_explain not resolving the live escalation) a beat's assertion fails.
//! No assertion states a present-tense "works today" claim; each states the contract
//! it pins so it does not go stale.
//!
//! Cross-platform: `#![cfg(unix)]` — a real daemon over a Unix domain socket + a
//! loopback-HTTP hop, run on WSL, not host Windows. Host `/vet` cannot reach this;
//! lint/run on WSL per the project's host-vs-WSL rule.
//!
//! Timing robustness (mirrors `operator_liveops_e2e`): generous deadlines and a
//! process-wide `SERIAL` guard, so this spawn-heavy daemon+proxy test is safe under
//! the default multi-threaded `cargo test` alongside its siblings on a loaded runner.

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{
    cli_bin, connect, read_until, send_line, start_daemon_with_mcp_and_unblock, stub_harness,
    wait_for_lockfile,
};
use serde_json::{Value, json};

const LOCK_DEADLINE: Duration = Duration::from_secs(10);
const TAIL_DEADLINE: Duration = Duration::from_secs(20);
/// The stdio proxy is a request/response relay; a cold `rezidnt mcp` spawn + loopback
/// POST + daemon fold must complete inside this per-request read budget even under a
/// loaded parallel runner. Generous by design — the happy path never waits it out.
const RPC_DEADLINE: Duration = Duration::from_secs(20);
/// A SHORT live-unblock budget: the escalation only needs to LAND and be interrogated
/// (this demo never holds a request open waiting for a resolve), so a small budget
/// keeps the run's first ask returning promptly.
const UNBLOCK_SHORT_MS: u64 = 500;
/// Hold the spawned run's process open comfortably longer than the whole beat
/// sequence (open → spawn → dossier → ask → gate_explain), so the run is live and its
/// state stable while every proxy call lands.
const HARNESS_GAP_MS: u64 = 30_000;

/// Serialize the scenario: this file stands up a real daemon + a `rezidnt mcp`
/// subprocess + a stub-harness process, and core contention is the documented flake
/// vector for spawn-heavy daemon tests (see `operator_liveops_e2e`). Poisoning is
/// ignored so a panicking test still releases the lock.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn banner(title: &str) {
    eprintln!("\n════════════════════════════════════════════════════════════");
    eprintln!("  S3 EXIT DEMO (via `rezidnt mcp` proxy): {title}");
    eprintln!("════════════════════════════════════════════════════════════");
}

fn step(msg: &str) {
    eprintln!("  → {msg}");
}

// ---------------------------------------------------------------------------
// Fixture: an empty-permit project (the natural LIVE non-pass source). Mirrors the
// private builder in `operator_liveops_e2e.rs`; inlined because it is test-private
// there. The absolute `repo` path is spliced into the spec so the daemon materializes
// THIS scaffolded tempdir repo regardless of its own cwd (the sibling e2e pattern).
// ---------------------------------------------------------------------------

/// `gates = ["permit"]` with an EMPTY verifier set → the aggregator escalates
/// (DR-011 §3): a `request_permission` for any tool escalates to `ask`. The stub
/// harness holds the spawned run's process alive `gap_ms` so the ask lands mid-run.
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
name = "s3-exit-demo"
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

// ---------------------------------------------------------------------------
// The proxy driver: spawn `rezidnt mcp`, write line-delimited JSON-RPC to its stdin,
// read one response line from its stdout. This is the CLIENT SIDE of the S3 connection
// path — everything a beat asserts about open/spawn/dossier/gate_explain goes through
// here, so a green run proves the shipped proxy relays end to end.
// ---------------------------------------------------------------------------

/// A live `rezidnt mcp` subprocess with piped stdio. Kills the proxy on drop so a
/// failing test never leaks it.
struct Proxy {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Proxy {
    /// Spawn `rezidnt mcp` pointed (via `REZIDNT_LOCKFILE`) at the daemon's 0600
    /// lockfile — exactly as a local client launches the transport. The proxy reads
    /// the lockfile (loopback port + operator badge) at startup; if it is missing the
    /// proxy exits 4, which surfaces as a driver read failure on the first request.
    fn spawn(lockfile: &Path) -> Proxy {
        let mut child = Command::new(cli_bin())
            .arg("mcp")
            .env("REZIDNT_LOCKFILE", lockfile)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn `rezidnt mcp` stdio proxy");
        let stdin = child.stdin.take().expect("proxy stdin pipe");
        let stdout = BufReader::new(child.stdout.take().expect("proxy stdout pipe"));
        Proxy {
            child,
            stdin,
            stdout,
        }
    }

    /// Send one JSON-RPC request line and read the single response line the proxy
    /// relays back. The proxy answers one line per request (a request carries an
    /// `id`, so a response body is always relayed). Panics on a closed pipe or the
    /// deadline — the caller's assertion message is the failure.
    fn request(&mut self, request: &Value, deadline: Duration) -> Value {
        let line = serde_json::to_string(request).expect("encode JSON-RPC request");
        self.stdin
            .write_all(line.as_bytes())
            .expect("write request to proxy stdin");
        self.stdin
            .write_all(b"\n")
            .expect("write newline to proxy stdin");
        self.stdin.flush().expect("flush proxy stdin");
        self.read_response(deadline)
    }

    /// Read the next non-empty stdout line as a JSON-RPC response, bounded by
    /// `deadline`. A read that yields nothing before the deadline (or a closed pipe)
    /// is a hard failure — the proxy either could not reach the daemon or died.
    fn read_response(&mut self, deadline: Duration) -> Value {
        let until = std::time::Instant::now() + deadline;
        let mut line = String::new();
        // The proxy's stdout is a blocking pipe; a stalled daemon would block the
        // read. To keep a generous-but-bounded budget without threads, we rely on the
        // proxy answering promptly on the happy path and on the daemon's own timeouts
        // (permit hot-path/unblock budgets) bounding the worst case well inside
        // `deadline`. If nothing arrives, the read returns 0 (pipe closed) or the loop
        // trips the deadline.
        loop {
            line.clear();
            let n = self
                .stdout
                .read_line(&mut line)
                .expect("read response line from proxy stdout");
            if n == 0 {
                panic!(
                    "the `rezidnt mcp` proxy closed its stdout before answering — it \
                     could not reach the daemon (bad lockfile / dead loopback port), \
                     or exited"
                );
            }
            if line.trim().is_empty() {
                if std::time::Instant::now() >= until {
                    panic!("deadline: proxy produced only blank lines, no JSON-RPC response");
                }
                continue;
            }
            return serde_json::from_str(line.trim())
                .unwrap_or_else(|e| panic!("proxy response must be JSON-RPC ({e}): {line}"));
        }
    }
}

/// A `tools/call` JSON-RPC request. The proxy injects the operator badge for mutating
/// tools, so `arguments` here NEVER carries a badge (the point of the S3 path: the
/// client does not handle the token, §12).
fn tools_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments},
    })
}

/// The machine-readable tool payload: `result.content[0].text` parsed as JSON (the
/// same shape `rezidnt_testkit::tool_payload` reads off the HTTP board).
fn tool_payload(response: &Value) -> Value {
    let result = &response["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool result must carry content[0].text: {response:#}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("content[0].text must be JSON ({e}): {text}"))
}

/// Wait (via a fresh bare-socket tail) until a `permit.escalated` carrying
/// `request_id` for `run` lands — proof the proxy's `request_permission` reached the
/// daemon PDP and produced the LIVE non-pass. The bare socket is used ONLY to CONFIRM
/// the fact landed (the work order sanctions this); the DRIVING went through the
/// proxy.
fn await_escalation(socket: &Path, run: &str, request_id: &str) {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "permit.escalated"
            && v["payload"]["run"] == json!(run)
            && v["payload"]["request_id"] == json!(request_id)
    });
}

/// Capture the run ULID the daemon auto-spawned for the opened workspace, off a fresh
/// bare-socket tail (the spec's single agent auto-spawns on open; open-chain spawns
/// are keyless, so this is the honest run→ULID source). Used to cross-check the
/// proxy `spawn_agent` result and to target the dossier/permit/gate_explain beats.
fn await_spawned_run(socket: &Path) -> String {
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

// ===========================================================================
// THE demo — one take, all beats over the proxy.
// ===========================================================================

/// The S3 Phase-1-exit demo, driven end to end through the `rezidnt mcp` stdio proxy
/// against a real `rezidentd`: initialize → open_project → spawn_agent → read dossier
/// → force a LIVE permit escalation → gate_explain the non-pass. Every mutating call
/// is un-badged from the client (the proxy injects the operator badge, §12); a green
/// run proves the whole S3 connection path relays, badge-injected, in one session.
#[test]
fn s3_exit_demo_over_stdio_proxy() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    banner("initialize → open → spawn → dossier → forced non-pass → gate_explain");

    // --- one real daemon, both doors (bare UDS for the tail confirms; loopback-HTTP
    //     MCP for the proxy target); wait for the announced 0600 lockfile. ---
    let (daemon, lockfile) = start_daemon_with_mcp_and_unblock(UNBLOCK_SHORT_MS);
    let lock = wait_for_lockfile(&lockfile, LOCK_DEADLINE);
    assert!(
        lock["port"].as_u64().is_some_and(|p| p > 0),
        "the daemon announced a bound loopback port for the proxy to reach: {lock:#}"
    );
    step("daemon up; MCP lockfile announced (loopback port + operator badge)");

    // --- the client's transport: `rezidnt mcp`, pointed at the daemon's lockfile. ---
    let mut proxy = Proxy::spawn(&lockfile);
    step("`rezidnt mcp` stdio proxy spawned (REZIDNT_LOCKFILE → the daemon's lockfile)");

    // === Beat 1: initialize — the relay is up end to end. ===
    let init = proxy.request(
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "s3-exit-demo", "version": "0"}
            }
        }),
        RPC_DEADLINE,
    );
    assert_eq!(
        init["id"],
        json!(1),
        "the proxy relays the JSON-RPC id back unchanged: {init:#}"
    );
    assert!(
        init["result"]["protocolVersion"].as_str().is_some(),
        "initialize must answer with a well-formed result through the proxy: {init:#}"
    );
    step("initialize → well-formed JSON-RPC result relayed ✓");

    // === Beat 2: open_project — UN-BADGED from the client; the proxy injects the
    //     operator badge (§12), so it still materializes. ===
    // The scaffolded repo + stub harness live in a tempdir held for the whole
    // scenario (the daemon materializes this repo by the absolute path spliced into
    // the spec, so it must outlive the open).
    let scaffold = tempfile::tempdir().expect("scaffold tempdir");
    let spec = make_empty_permit_project(scaffold.path(), HARNESS_GAP_MS);
    let opened = proxy.request(
        &tools_call(2, "open_project", json!({"spec_toml": spec})),
        RPC_DEADLINE,
    );
    assert_ne!(
        opened["result"]["isError"],
        json!(true),
        "open_project must succeed through the proxy (badge injected for the un-badged \
         client, §12): {opened:#}"
    );
    let open_payload = tool_payload(&opened);
    let workspace = open_payload["workspace"]
        .as_str()
        .expect("open_project result names the workspace ulid")
        .to_string();
    step(&format!(
        "open_project (no client badge) → workspace {workspace} materialized ✓"
    ));

    // The spec's single agent auto-spawns on open; capture that run off the bare
    // socket (the honest keyless-spawn source) — the target for the beats below.
    let run = await_spawned_run(&daemon.socket);
    step(&format!("agent auto-spawned on open → run {run}"));

    // === Beat 3: spawn_agent over the proxy — returns a 26-char run ULID. ===
    // Idempotency-keyed so the beat is a clean "spawn returns a run ULID" proof; the
    // dossier/permit/gate_explain beats target the auto-spawned `run` above (the empty
    // agent whose process the stub keeps alive). A green result here proves the proxy
    // relays spawn_agent badge-injected and the daemon answers with a run ULID.
    let spawned = proxy.request(
        &tools_call(
            3,
            "spawn_agent",
            json!({
                "workspace": workspace,
                "agent": "impl",
                "idempotency_key": "s3-exit-demo-proxy-spawn"
            }),
        ),
        RPC_DEADLINE,
    );
    assert_ne!(
        spawned["result"]["isError"],
        json!(true),
        "spawn_agent must succeed through the proxy: {spawned:#}"
    );
    let proxy_run = tool_payload(&spawned)["run"]
        .as_str()
        .expect("spawn_agent result names the run ulid")
        .to_string();
    assert_eq!(
        proxy_run.len(),
        26,
        "spawn_agent returns a 26-char run ULID (the dossier + gate_explain target \
         shape): run={proxy_run:?}"
    );
    step(&format!(
        "spawn_agent (no client badge) → run ULID {proxy_run} ✓"
    ));

    // === Beat 4: resources/read the dossier — the FOLDED run state (I3). ===
    let dossier_resp = proxy.request(
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/read",
            "params": {"uri": format!("rezidnt://run/{run}/dossier")}
        }),
        RPC_DEADLINE,
    );
    let text = dossier_resp["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("dossier read must carry contents[0].text: {dossier_resp:#}"));
    let dossier: Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("dossier must be JSON ({e}): {text}"));
    assert!(
        dossier.get("code") != Some(&json!("run.unknown")),
        "the dossier must resolve the spawned run (not a run.unknown miss): {dossier:#}"
    );
    assert!(
        dossier["status"].as_str().is_some(),
        "the dossier is the FOLDED run state — it carries a status derived from the log \
         (I3, not a side store): {dossier:#}"
    );
    assert!(
        dossier.get("total_usd").is_some(),
        "the dossier carries the folded accounting field (total_usd, I3): {dossier:#}"
    );
    assert!(
        dossier.get("gates").is_some(),
        "the dossier carries the folded gates map (I3): {dossier:#}"
    );
    step(&format!(
        "resources/read dossier → folded run state (status={}, accounting + gates present) ✓",
        dossier["status"]
    ));

    // === Beat 5a: force a LIVE non-pass — request_permission for a tool the empty
    //     permit gate does not grant → a real `permit.escalated` on the log. ===
    const ESC_REQ: &str = "01S3EXITDEMOESCREQ00000001";
    let asked = proxy.request(
        &tools_call(
            5,
            "request_permission",
            json!({
                "run": run,
                "request_id": ESC_REQ,
                "action": "tool.invoke",
                "tool": "Bash"
            }),
        ),
        RPC_DEADLINE,
    );
    assert_ne!(
        asked["result"]["isError"],
        json!(true),
        "request_permission must be answered (not refused) through the proxy — the \
         badge is injected for the un-badged client (§12): {asked:#}"
    );
    let decision = tool_payload(&asked)["decision"].clone();
    assert_eq!(
        decision,
        json!("ask"),
        "the empty permit gate ESCALATES this action (DR-011 §3) — a LIVE non-pass, \
         never coerced to allow (I6): {asked:#}"
    );
    step("request_permission (Bash, ungranted) → LIVE escalation: decision=ask ✓");

    // Confirm the LIVE `permit.escalated` fact landed on the log (I3) for this run +
    // request_id — the honest anchor gate_explain will interrogate.
    await_escalation(&daemon.socket, &run, ESC_REQ);
    step("permit.escalated fact landed on the log (I3): the non-pass is real ✓");

    // === Beat 5b: gate_explain the forced non-pass — the real interrogation (I6). ===
    let explained = proxy.request(
        &tools_call(6, "gate_explain", json!({"run": run})),
        RPC_DEADLINE,
    );
    assert_ne!(
        explained["result"]["isError"],
        json!(true),
        "a run with a LIVE permit non-pass on the log MUST be interrogable — \
         gate_explain must not answer gate.no_verdict: {explained:#}"
    );
    let explain = tool_payload(&explained);
    assert_eq!(
        explain["gate"],
        json!("permit"),
        "gate_explain names the deciding gate: permit ({explain:#})"
    );
    assert_eq!(
        explain["verdict"],
        json!("ask"),
        "the escalation surfaces as the failing verdict `ask` (route-to-a-human), \
         NEVER coerced to allow or pass (I6): {explain:#}"
    );
    assert_ne!(
        explain["verdict"],
        json!("allow"),
        "an escalation is a blocking non-pass — it is never reported as allow (I6)"
    );
    assert_ne!(
        explain["verdict"],
        json!("pass"),
        "an escalation is a blocking non-pass — it is never coerced to pass (I6)"
    );
    assert_eq!(
        explain["request_id"],
        json!(ESC_REQ),
        "gate_explain returns the EXACT request_id that was escalated — the recorded \
         input, so the blocked agent reads WHICH ask blocked (I6 interrogability): \
         {explain:#}"
    );
    let policy_hash = explain["policy_ref"]["hash"].as_str().unwrap_or("");
    assert!(
        !policy_hash.is_empty(),
        "gate_explain surfaces a resolvable deciding policy_ref (a CAS ref, I2 — ref \
         not inline) so the decision is auditable (I6): {explain:#}"
    );
    step(&format!(
        "gate_explain → gate=permit, verdict=ask (not coerced), request_id={ESC_REQ}, \
         policy_ref resolvable ✓"
    ));

    eprintln!(
        "  ✔ S3 exit demo complete over the `rezidnt mcp` proxy: MCP-only open → spawn → \
         dossier → gate_explain(forced non-pass), one take."
    );

    // Keep the daemon alive to the end of the scenario.
    drop(proxy);
    drop(daemon);
}
