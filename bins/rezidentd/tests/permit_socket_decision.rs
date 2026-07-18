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
use std::path::Path;
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
// The fail-posture (DR-014 §Decision 3: unreachable/timed-out PDP → fail closed
// to `ask`, never a silent proceed) lives in the PEP hook's connect/timeout
// handling — there is no daemon-side judge for "the socket points nowhere",
// because by definition the daemon is not there. DR-014 §Decision 1 settled
// WHERE the PEP lives — the `rezidnt permit-hook` CLI subcommand — so the honest
// judge now EXISTS and is written: `bins/rezidnt/tests/permit_hook.rs` points
// the subcommand at a dead socket (`REZIDNT_SOCKET` + a low
// `REZIDNT_PERMIT_TIMEOUT_MS`) and asserts it resolves to `ask`, never `allow`.
//
// The `#[ignore]`/`unimplemented!()` stub that used to sit here is DELETED: now
// that DR-014 settles where the hook lives, an ignored placeholder for a
// criterion with a real judge would be dishonest coverage. The daemon-side half
// of the same honesty — "an EMPTY/unreachable POLICY never synthesizes allow" —
// is judged above by `socket_permission_escalates_an_empty_policy_never_allows`.

// ---------------------------------------------------------------------------
// §7 / CRITERION 6 — Path parity: the socket carries `paths`, so `path-scope`
// decides IDENTICALLY over socket and MCP (DR-014 §Decision 4; design §7). Today
// the socket op has no `paths` axis, so `path-scope` degrades to escalate over
// the socket while the MCP path can DENY — the asymmetry the auditor flagged on
// `bb7afe3`. These tests pin that the socket, once it carries `paths`, DENIES a
// path outside the allowed scope (where it previously escalated).
// ---------------------------------------------------------------------------

/// A permit project whose gate is a `path-scope` native scoped to `allow` globs
/// (the PathScope verifier reads `params.paths` against `params.allow`; a path
/// outside → Deny, paths ABSENT → escalate). This is the transport-parity judge:
/// the same `paths` must decide the same on socket and MCP.
fn make_path_scope_project(gap_ms: u64, allow_glob: &str) -> (tempfile::TempDir, String) {
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
name = "sp2-path-scope"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["permit"]
bin_override = "{script}"

[gates.permit]
verifiers = [
  {{ native = "path-scope", params = {{ allow = ["{allow_glob}"] }} }},
]
"#,
        repo = repo.display(),
        script = script.display(),
    );
    (dir, spec)
}

/// Send one `request_permission` line CARRYING a `paths` axis and read the reply.
/// COMPILE-NOTE: the `paths` field on `Request::RequestPermission` does not exist
/// yet (DR-014 §Decision 4 adds `paths: Option<Value>`); this raw JSON carries it
/// on the wire so the test is behavior-red on the socket handler that ignores it
/// today (`bins/rezidentd/src/main.rs` hardcodes `paths: None`), not blocked on
/// the type. The parity pin is the DECISION, not the struct field.
fn ask_permission_with_paths(
    socket: &Path,
    run: &str,
    tool: &str,
    request_id: &str,
    paths: &[&str],
) -> serde_json::Value {
    let mut conn = connect(socket);
    send_line(
        &mut conn,
        &serde_json::to_string(&json!({
            "op": "request_permission",
            "run": run,
            "request_id": request_id,
            "action": "tool.invoke",
            "tool": tool,
            "paths": paths,
        }))
        .unwrap(),
    );
    read_reply_line(&mut conn, REPLY_DEADLINE)
}

/// §7 / CRITERION 6 (the parity headline) — a `path-scope` verifier DENIES a path
/// outside the allowed scope OVER THE SOCKET, exactly as it would over MCP. This
/// is the asymmetry DR-014 §Decision 4 closes: the socket can now deny where it
/// previously degraded to escalate.
///
/// RED today: the socket op carries no `paths` axis (main.rs `paths: None`), so
/// `path-scope` sees no paths → cannot-run → escalate → `ask`. The assertion
/// `decision == "deny"` fails on that `ask` until the wire + handler thread
/// `paths` through to the PDP.
#[test]
fn socket_path_scope_denies_a_path_outside_allowed_scope() {
    let daemon = start_daemon();
    // allow only src/**; ask about a path OUTSIDE it → deny.
    let (_project, spec) = make_path_scope_project(600, "src/**");
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission_with_paths(
        &daemon.socket,
        &run,
        "Edit",
        "01SP2PATHDENYREQ0000000001",
        &["/etc/passwd"],
    );
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("deny"),
        "a path outside the allowed scope is DENIED over the socket — parity with MCP \
         (DR-014 §Decision 4; the socket now denies where it previously escalated): {reply:#}"
    );
}

/// §7 / CRITERION 6 (the parity floor) — a path INSIDE the allowed scope is
/// allowed over the socket. Paired with the deny test, this pins that the socket
/// `paths` axis is actually READ (not merely accepted-and-ignored): an ignored
/// `paths` would leave `path-scope` at cannot-run → `ask` for BOTH, collapsing
/// the two outcomes. Distinct outcomes prove the axis reaches the verifier.
///
/// RED today: `paths` is ignored on the socket (main.rs `paths: None`), so this
/// is `ask`, not `allow`.
#[test]
fn socket_path_scope_allows_a_path_inside_allowed_scope() {
    let daemon = start_daemon();
    let (_project, spec) = make_path_scope_project(600, "src/**");
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission_with_paths(
        &daemon.socket,
        &run,
        "Edit",
        "01SP2PATHALLOWREQ000000001",
        &["src/main.rs"],
    );
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("allow"),
        "a path inside the allowed scope is ALLOWED over the socket — the paths axis \
         reaches the verifier, not silently dropped (DR-014 §Decision 4): {reply:#}"
    );
}

// A tiny compile-time guard so the imports above (BufReader/UnixStream/BufRead)
// stay honestly used.
#[allow(dead_code)]
fn _touch_imports(sock: &Path) {
    if let Ok(stream) = UnixStream::connect(sock) {
        let mut r = BufReader::new(stream);
        let mut s = String::new();
        let _ = r.read_line(&mut s);
    }
}
