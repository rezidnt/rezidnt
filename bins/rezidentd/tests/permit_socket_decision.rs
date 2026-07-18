//! SP2 oracle — the socket-side PDP path (the enforcing slice). The harness PEP
//! speaks the socket, not loopback-HTTP: it sends ONE
//! `Request::RequestPermission` line and reads ONE `Reply::PermitDecision`.
//! Today `bins/rezidentd/src/main.rs:355` answers `request_permission` with an
//! honest `op.not_served` error frame — SP1 routed the decision through MCP
//! (I5). SP2 un-stubs it: the socket handler builds a transport-neutral
//! `PermitRequest`, runs the SAME `decide_permit` PDP flow that
//! `McpCore::call_request_permission` runs (design §2/§3, DR-013 decision 1),
//! and maps the outcome to `Reply::PermitDecision`.
//!
//! RED MODE: **assert-red** — the socket still returns
//! `{"reply":"error","op":"request_permission","code":"op.not_served"}` today,
//! so every `decision`/fact assertion here fails on that error frame. The wire
//! variants (`Request::RequestPermission`, `Reply::PermitDecision`) already
//! exist (SP1 proto pin), so these are behavior-red, not compile-red. The
//! un-stub is the wire-behavior change DR-013 §Consequences names; the oracle
//! encodes the NEW expectation rather than the implementer quietly deleting the
//! `op.not_served` branch.
//!
//! Judge: a REAL end-to-end `open` with a `[gates.permit]` `tool-allowlist`
//! config, spawn the run, then ask over the socket. No log seeding — the deny /
//! allow / ask verdicts come from the live PDP folding the live log (I3).

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{connect, read_reply_line, read_until, send_line, start_daemon, stub_harness};
use serde_json::json;

const REPLY_DEADLINE: Duration = Duration::from_secs(10);
const TAIL_DEADLINE: Duration = Duration::from_secs(20);

/// A temp project whose agent runs under a `[gates.permit]` gate with a
/// `tool-allowlist` native scoped to `allow`. The `gates = ["permit"]` on the
/// agent is what wires the permit config into the opened-workspace registry
/// (`permit_config_for`, DR-011). The harness holds the run open long enough to
/// ask a permission mid-run.
fn make_permit_project(gap_ms: u64, allow: &[&str]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());

    let script = stub_harness(dir.path(), gap_ms);
    let allow_list = allow
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let spec = format!(
        r#"[project]
name = "sp2-permit"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["permit"]
bin_override = "{script}"

[gates.permit]
verifiers = [
  {{ native = "tool-allowlist", params = {{ allow = [{allow_list}] }} }},
]
"#,
        repo = repo.display(),
        script = script.display(),
        allow_list = allow_list,
    );
    (dir, spec)
}

/// An empty-permit-gate project: `gates = ["permit"]` is declared but the
/// verifier set is EMPTY. An empty configured set aggregates to Inconclusive
/// (DR-011 §3) — the PDP must escalate, NEVER synthesize an allow (I6).
fn make_empty_permit_project(gap_ms: u64) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());
    let script = stub_harness(dir.path(), gap_ms);
    let spec = format!(
        r#"[project]
name = "sp2-empty-permit"
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
    );
    (dir, spec)
}

/// Open the spec, tail until `agent.spawned`, return the spawned run's ulid
/// (the PEP's `run`) — the live handle the socket permission request targets.
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
    let spawned = lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .expect("agent.spawned on the fabric");
    spawned["payload"]["run"]
        .as_str()
        .expect("agent.spawned carries the run ulid")
        .to_string()
}

/// Send one `request_permission` line on a fresh connection and read the single
/// reply frame. The socket is the transport the PEP speaks (design §3).
fn ask_permission(socket: &Path, run: &str, tool: &str, request_id: &str) -> serde_json::Value {
    let mut conn = connect(socket);
    send_line(
        &mut conn,
        &serde_json::to_string(&json!({
            "op": "request_permission",
            "run": run,
            "request_id": request_id,
            "action": "tool.invoke",
            "tool": tool,
        }))
        .unwrap(),
    );
    read_reply_line(&mut conn, REPLY_DEADLINE)
}

/// Collect every tail line up to and including the last permit fact for `run`.
fn tail_permit_facts(socket: &Path, run: &str, until_subject: &str) -> Vec<serde_json::Value> {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == until_subject && v["payload"]["run"] == json!(run)
    })
}

// ---------------------------------------------------------------------------
// §5 CRITERION 1 — Socket deny path (the headline).
// ---------------------------------------------------------------------------

/// §5 criterion 1 (headline): a `Request::RequestPermission` for a tool OUTSIDE
/// the run's allowlist returns `Reply::PermitDecision { decision: "deny",
/// reason: Some(_) }`. RED today: the socket answers `op.not_served`, so the
/// reply is `{"reply":"error",...}` and the `decision` assertion fails.
#[test]
fn socket_permission_denies_a_tool_outside_the_allowlist() {
    let daemon = start_daemon();
    // allow only Read; the PEP asks for Bash → deny.
    let (_project, spec) = make_permit_project(400, &["Read"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Bash", "01SP2DENYREQ00000000000001");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame, NOT op.not_served: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("deny"),
        "a tool outside the allowlist is DENIED over the socket (I6, criterion 1): {reply:#}"
    );
    assert!(
        reply["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "a deny carries a non-empty reason so the blocked agent reads WHY (criterion 1): {reply:#}"
    );
}

/// §5 criterion 1 (the log leg): the deny lands `permit.requested` +
/// `permit.denied` on the log, and the deny fact carries a resolvable
/// `policy_ref` and `evidence_ref` (I3, I6, I2 — refs not inline bytes).
/// RED today: no permit facts land because the socket never runs the PDP.
#[test]
fn socket_deny_lands_requested_and_denied_facts_with_refs() {
    let daemon = start_daemon();
    let (_project, spec) = make_permit_project(600, &["Read"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Bash", "01SP2DENYFAX00000000000001");
    // Crisp red FIRST: today this is op.not_served, so the facts never land and
    // the tail below would otherwise fail on a 20s deadline. Assert the decision
    // frame here so the failure names the absent feature, not a timeout.
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket must answer a permit_decision before any facts can land (criterion 1): {reply:#}"
    );

    let lines = tail_permit_facts(&daemon.socket, &run, "permit.denied");
    let requested = lines
        .iter()
        .find(|v| v["subject"] == "permit.requested" && v["payload"]["run"] == json!(run))
        .unwrap_or_else(|| panic!("permit.requested must land on the log (I3); saw {lines:#?}"));
    assert_eq!(
        requested["payload"]["request_id"],
        json!("01SP2DENYFAX00000000000001"),
        "the requested fact carries the caller's request_id"
    );

    let denied = lines
        .iter()
        .find(|v| v["subject"] == "permit.denied" && v["payload"]["run"] == json!(run))
        .unwrap_or_else(|| panic!("permit.denied must land on the log (I3); saw {lines:#?}"));
    assert!(
        denied["payload"]["policy_ref"]["hash"].is_string(),
        "the deny fact carries a resolvable policy_ref (I6, I2): {denied:#}"
    );
    assert!(
        denied["payload"]["evidence_ref"]["hash"].is_string(),
        "the deny fact carries a resolvable evidence_ref (I6, I2): {denied:#}"
    );
}

// ---------------------------------------------------------------------------
// §5 CRITERION 2 — Three-valued honesty (empty → ask, allowlisted → allow).
// ---------------------------------------------------------------------------

/// §5 criterion 2: an allowlisted tool returns `decision: "allow"`.
/// RED today: op.not_served, no decision.
#[test]
fn socket_permission_allows_an_allowlisted_tool() {
    let daemon = start_daemon();
    let (_project, spec) = make_permit_project(400, &["Read", "Grep", "Bash"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Bash", "01SP2ALLOWREQ0000000000001");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("allow"),
        "an allowlisted tool is ALLOWED over the socket (criterion 2): {reply:#}"
    );
}

/// §5 criterion 2 (the honesty headline): an EMPTY / unresolvable permit set
/// returns `decision: "ask"` — escalate, NEVER a synthesized allow (I6, DR-011
/// §3). RED today: op.not_served. The negative assertion (`!= "allow"`) is the
/// load-bearing one — a permit-by-default when policy is empty is exactly the
/// dishonesty the fail-posture stance forbids.
#[test]
fn socket_permission_escalates_an_empty_policy_never_allows() {
    let daemon = start_daemon();
    let (_project, spec) = make_empty_permit_project(400);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Bash", "01SP2ASKREQ000000000000001");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_ne!(
        reply["decision"],
        json!("allow"),
        "an empty/unresolvable policy is NEVER coerced to allow (I6, DR-011 §3): {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("ask"),
        "an empty/unresolvable policy set ESCALATES to ask (criterion 2): {reply:#}"
    );
}

// ---------------------------------------------------------------------------
// §5 CRITERION 3 — request_id fidelity (proto carries it; socket echoes it).
// ---------------------------------------------------------------------------

/// §5 criterion 3: the reply's `request_id` AND the decision fact's request_id
/// EQUAL the caller-supplied id — the PEP's correlation token is echoed, never
/// discarded for a freshly-minted one (DR-013 decision 1). RED today:
/// op.not_served carries no request_id.
#[test]
fn socket_reply_and_fact_echo_the_caller_request_id() {
    let daemon = start_daemon();
    let (_project, spec) = make_permit_project(600, &["Read"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    const REQ: &str = "01SP2FIDELITYREQ0000000001";
    let reply = ask_permission(&daemon.socket, &run, "Bash", REQ);
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["request_id"],
        json!(REQ),
        "the reply echoes the caller-supplied request_id, never a minted one (criterion 3): {reply:#}"
    );

    let lines = tail_permit_facts(&daemon.socket, &run, "permit.denied");
    let denied = lines
        .iter()
        .find(|v| v["subject"] == "permit.denied" && v["payload"]["run"] == json!(run))
        .unwrap_or_else(|| panic!("permit.denied must land (I3); saw {lines:#?}"));
    assert_eq!(
        denied["payload"]["request_id"],
        json!(REQ),
        "the on-log decision fact's request_id equals the caller's token — the PEP's ask and the log share one id (criterion 3): {denied:#}"
    );
}

// ---------------------------------------------------------------------------
// §5 CRITERION 5 — Fail-posture: PEP fails CLOSED to `ask`, never a silent allow.
// ---------------------------------------------------------------------------
//
// The PEP proper is a claude-code hook SCRIPT (design §3) that connects to the
// socket. The fail-posture (DR-013 decision 2: unreachable/timed-out PDP →
// fail closed to `ask`, never a silent proceed) lives in that script's
// connect/timeout handling — there is no daemon-side judge for "the socket
// points nowhere", because by definition the daemon is not there. The honest
// judge is a hook-client-level test: point the client at a dead socket and
// assert it resolves to `ask`, not `allow`. That client does not exist yet
// (deferred with the hook binary per DR-013 "Deferred to the impl slice" (b)),
// so this criterion cannot be given a real judge in THIS crate today.
//
// Encoded as an #[ignore]-with-reason stub so the criterion is tracked and
// visible, not silently dropped. The daemon-side half of the same honesty —
// "an EMPTY/unreachable POLICY never synthesizes allow" — IS judged, above, by
// `socket_permission_escalates_an_empty_policy_never_allows`.

/// §5 criterion 5 (fail-posture) — TRACKING STUB (no honest judge in this
/// crate yet). The PEP hook client, given an unreachable socket or a timeout,
/// must resolve to `ask` (fail CLOSED), never `allow` (DR-013 decision 2).
/// Belongs in the hook-client crate/tests once the hook binary lands (DR-013
/// "Deferred to the impl slice" (b)). Un-ignore only when that client exists.
#[test]
#[ignore = "SP2 criterion 5: PEP-hook fail-closed-to-ask has no daemon-side judge; \
            lives in the hook-client tests once the hook binary lands (DR-013 deferred (b)). \
            The daemon-side empty-policy-never-allows honesty IS judged by \
            socket_permission_escalates_an_empty_policy_never_allows."]
fn pep_fails_closed_to_ask_when_pdp_unreachable() {
    // Intentionally empty: the judge (a hook-client that dials a dead socket)
    // does not exist in this crate. Un-ignoring without that client would be a
    // vibes test. See the module note above.
    let _dead_socket = PathBuf::from("/nonexistent/rezidnt.sock");
    let _ = UnixStream::connect(&_dead_socket); // documents the shape only.
    unimplemented!("hook-client fail-posture judge is deferred to the hook-binary slice (DR-013)");
}

// A tiny compile-time guard so the unused imports above (BufReader/UnixStream/
// BufRead) are honestly used even while the fail-posture judge is a stub.
#[allow(dead_code)]
fn _touch_imports(sock: &Path) {
    if let Ok(stream) = UnixStream::connect(sock) {
        let mut r = BufReader::new(stream);
        let mut s = String::new();
        let _ = r.read_line(&mut s);
    }
}
