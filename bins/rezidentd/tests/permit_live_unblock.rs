//! DR-034 oracle — operator-live-unblock: a bounded server-assisted long-poll
//! that RESUMES the currently-stalled agent when a matching `permit.resolved`
//! lands within the live-unblock deadline (`REZIDNT_UNBLOCK_TIMEOUT_MS`),
//! LAYERED ON TOP of DR-033's "honored on next ask" ledger-check.
//!
//! ## What the daemon does today (why every test here is RED)
//! `bins/rezidentd/src/main.rs` services `Request::RequestPermission` by calling
//! `decide_permit` EXACTLY ONCE and writing the outcome frame immediately —
//! there is NO hold, NO long-poll (main.rs ~445-500). On an empty permit set the
//! decision is `ask` (DR-011 §3), so a held `request_permission` returns `ask`
//! AT ONCE, before any operator resolution can land. The resume tests assert the
//! held reply is `allow`/`deny`; they fail on that immediate `ask`. This is a
//! CRISP behavior-red (a named `decision` assertion), not a timeout: the daemon
//! answers fast, just with the wrong frame — it never held the request.
//!
//! ## The end-to-end wake is driven through the REAL operator door
//! The resolution is NOT a synthetic log poke: it is a real
//! `resolve_permit { badge, run, request_id, decision }` `tools/call` over the
//! loopback-HTTP MCP surface (the same door `rezidnt operator resolve-permit`
//! drives), carrying the operator badge from the 0600 lockfile. So a green suite
//! proves the daemon woke the HELD socket request from an operator's resolution
//! landing on the log — the whole DR-034 mechanism, not a fragment. One daemon
//! serves BOTH doors: the bare UDS holds the PEP's request; the loopback-HTTP
//! surface takes the operator resolve.
//!
//! ## The match key (DR-034 §Decision 3)
//! The held PEP still carries the ORIGINAL escalated `request_id` on its open
//! connection, so the daemon wakes it by that exact id. Every resume test
//! asserts the woken reply's `request_id` EQUALS the original held id — never a
//! re-minted one. A resolution for a DIFFERENT escalation must NOT wake this
//! held request (it must still expire to `ask`).
//!
//! ## Fallback stays green (DR-033, regression guard)
//! DR-034 WEAKENS no DR-033 test. The DR-033 "next ask" ledger-check is proven
//! by `crates/rezidnt-mcp/tests/permit_resolved_pdp.rs` and
//! `resolve_permit_interrogability.rs` — this board references them, does not
//! duplicate or lower them. The non-live regression guard here confirms the
//! SEPARATE, un-held path: an agent that does NOT hold a live connection still
//! gets the resolution honored on its next fresh ask (the DR-033 fallback the
//! live path layers on top of).
//!
//! Subject/Reply verdict (asked by the work order): encoding these needed NO new
//! subject and NO new `Reply` variant. The held reply reuses
//! `Reply::PermitDecision`; the applied fact is DR-033's unchanged
//! `permit.granted`/`permit.denied`. See the closing note in the module for the
//! precise finding.

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use common::{
    connect, mcp_tool_call, read_reply_line, read_until, send_line,
    start_daemon_with_mcp_and_unblock, wait_for_lockfile,
};
use serde_json::{Value, json};

const LOCK_DEADLINE: Duration = Duration::from_secs(10);
const TAIL_DEADLINE: Duration = Duration::from_secs(20);

/// A LONG live-unblock budget for the resume tests: the operator resolution has
/// time to land while the request is held. 8s comfortably outlasts the
/// open→escalate→resolve round-trip on CI without dragging the suite.
const UNBLOCK_LONG_MS: u64 = 8_000;
/// A SHORT live-unblock budget for the expiry test: the held request degrades to
/// `ask` quickly (no resolution ever lands), keeping the suite fast.
const UNBLOCK_SHORT_MS: u64 = 500;

// ---------------------------------------------------------------------------
// Fixtures — an empty-permit project that ESCALATES to `ask` (DR-011 §3), the
// natural "held" starting state for live-unblock. Mirrors
// `permit_socket_decision.rs::make_empty_permit_project`; inlined because that
// builder is private to that test file and the testkit's project builders don't
// cover the `[gates.permit] verifiers = []` shape.
// ---------------------------------------------------------------------------

/// An empty-permit-gate project: `gates = ["permit"]` declared, verifier set
/// EMPTY → the aggregator escalates (DR-011 §3) → a `request_permission` for any
/// tool escalates to `ask`. The stub harness holds the run open `gap_ms` so a
/// permission can be asked (and held) mid-run.
fn make_empty_permit_project(dir: &Path, gap_ms: u64) -> String {
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git init");
    assert!(git.success());
    let script = common::stub_harness(dir, gap_ms);
    format!(
        r#"[project]
name = "dr034-empty-permit"
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
/// spawned run's ulid — the live handle the held permission request targets.
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

/// Spawn a thread that HOLDS a socket `request_permission` open (the PEP's live
/// connection) and returns the single reply frame it eventually reads. `reply_ms`
/// bounds the reader — set it LONGER than the daemon's unblock deadline so the
/// test observes the daemon's own release (resolve-applied OR expiry-`ask`), not
/// a client-side cutoff. The request carries the ORIGINAL `request_id` the wake
/// must key on.
fn hold_request(
    socket: &Path,
    run: &str,
    tool: &str,
    request_id: &str,
    reply_ms: u64,
) -> thread::JoinHandle<Value> {
    let socket = socket.to_path_buf();
    let run = run.to_string();
    let tool = tool.to_string();
    let request_id = request_id.to_string();
    thread::spawn(move || {
        let mut conn = connect(&socket);
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
        read_reply_line(&mut conn, Duration::from_millis(reply_ms))
    })
}

/// Wait (via a fresh tail) until `permit.escalated` carrying `request_id` lands —
/// proof the held request reached the daemon and escalated, so the operator has a
/// real escalation to resolve. Returns once the escalation is on the log.
fn await_escalation(socket: &Path, run: &str, request_id: &str) {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "permit.escalated"
            && v["payload"]["run"] == json!(run)
            && v["payload"]["request_id"] == json!(request_id)
    });
}

/// Drive a REAL operator `resolve_permit` over loopback-HTTP MCP (the operator
/// door, badge from the 0600 lockfile). `request_id` is the ESCALATED ask's id;
/// the daemon derives `(action, target)` from the folded `permit.requested`.
fn operator_resolve(url: &str, badge: &str, run: &str, request_id: &str, decision: &str) {
    let result = mcp_tool_call(
        url,
        50,
        "resolve_permit",
        json!({
            "badge": badge,
            "run": run,
            "request_id": request_id,
            "decision": decision,
            "reason": "operator approved after review (DR-034 live-unblock oracle)",
        }),
    );
    assert_ne!(
        result["isError"],
        json!(true),
        "the operator resolve_permit must succeed so a real permit.resolved lands: {result:#}"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 1 — Resolve-while-held RESUMES (allow, then deny). The load-bearing
// DR-034 headline: a held request wakes and returns the applied decision keyed
// by the ORIGINAL request_id — the agent resumes with NO re-ask.
// ---------------------------------------------------------------------------

/// CRITERION 1 (allow) — a held `request_permission` that escalated, when a
/// MATCHING `permit.resolved(allow)` lands within the unblock deadline, returns
/// `Reply::PermitDecision { decision: "allow", request_id: <ORIGINAL id> }`.
/// The agent resumes without re-prompting.
///
/// RED today: the daemon answers the held request with `ask` IMMEDIATELY (no
/// hold), so `decision` is `ask` (never `allow`) and it returns before the
/// resolve lands. The `decision == "allow"` assertion fails on that `ask`.
#[test]
fn held_request_resumes_allow_on_matching_resolution() {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_LONG_MS);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile url");
    let badge = lock["badge"].as_str().expect("operator badge");

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 4_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    const REQ: &str = "01DR034HELDALLOWREQ0000001";
    // Hold the request open; reply reader outlasts the 8s unblock budget.
    let held = hold_request(&daemon.socket, &run, "Bash", REQ, UNBLOCK_LONG_MS + 4_000);

    // Confirm the held request escalated, THEN resolve it through the real door.
    await_escalation(&daemon.socket, &run, REQ);
    operator_resolve(url, badge, &run, REQ, "allow");

    let reply = held.join().expect("held request thread");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "a woken held request answers a permit_decision frame (reuses Reply::PermitDecision, \
         DR-034 §Design 'no new Reply variant'): {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("allow"),
        "the held request RESUMED with the operator's allow — the agent proceeds WITHOUT a \
         re-ask (DR-034 §Decision 1). An `ask` here means the daemon never held the request: {reply:#}"
    );
    assert_eq!(
        reply["request_id"],
        json!(REQ),
        "the woken reply carries the ORIGINAL held request_id, not a re-minted one — the wake \
         is keyed by the id on the open connection (DR-034 §Decision 3): {reply:#}"
    );
}

/// CRITERION 1 (deny) — symmetric: a MATCHING `permit.resolved(deny)` wakes the
/// held request with `decision: "deny"`, request_id = the ORIGINAL held id.
///
/// RED today: immediate `ask`, no hold — `decision == "deny"` fails.
#[test]
fn held_request_resumes_deny_on_matching_resolution() {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_LONG_MS);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile url");
    let badge = lock["badge"].as_str().expect("operator badge");

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 4_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    const REQ: &str = "01DR034HELDDENYREQ00000001";
    let held = hold_request(&daemon.socket, &run, "Bash", REQ, UNBLOCK_LONG_MS + 4_000);

    await_escalation(&daemon.socket, &run, REQ);
    operator_resolve(url, badge, &run, REQ, "deny");

    let reply = held.join().expect("held request thread");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "a woken held request answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("deny"),
        "the held request RESUMED with the operator's deny — a recorded human override, not a \
         silent proceed (DR-034 §Decision 1, I6): {reply:#}"
    );
    assert_eq!(
        reply["request_id"],
        json!(REQ),
        "the woken deny carries the ORIGINAL held request_id (DR-034 §Decision 3): {reply:#}"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 2 — Deadline expiry degrades to `ask` (fail-closed). No resolution
// lands within a SHORT unblock budget → the held request returns `ask`. The
// load-bearing negative assertion is `decision != "allow"`.
// ---------------------------------------------------------------------------

/// CRITERION 2 — a held request with a SHORT `REZIDNT_UNBLOCK_TIMEOUT_MS` and NO
/// resolution returns `decision: "ask"` after ~the deadline — fail-closed, the
/// DR-033 fallback. The `!= "allow"` assertion is load-bearing: a held request
/// must NEVER silently proceed on expiry.
///
/// RED today: this is the ONE criterion the current daemon already satisfies for
/// the wrong reason (it returns `ask` immediately, never having held). It stays
/// here as the fail-closed floor a green live-unblock implementation must
/// preserve — the implementer must keep it `ask`, never let a hold decay to
/// `allow`. The timing assertion (`elapsed >= most of the budget`) is what turns
/// red today: a real hold waits ~the deadline before degrading; the current
/// no-hold path returns near-instantly. If the implementer adds a hold, this
/// test confirms it fails CLOSED on expiry.
#[test]
fn held_request_expires_to_ask_never_allows() {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_SHORT_MS);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    // No operator resolve is driven — this held request is left to expire.
    let _ = lock;

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 4_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    const REQ: &str = "01DR034EXPIRYREQ0000000001";
    let started = Instant::now();
    // Reply reader outlasts the short unblock budget by a wide margin.
    let held = hold_request(&daemon.socket, &run, "Bash", REQ, UNBLOCK_SHORT_MS + 5_000);
    let reply = held.join().expect("held request thread");
    let elapsed = started.elapsed();

    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "an expired held request still answers a permit_decision frame (not an error): {reply:#}"
    );
    assert_ne!(
        reply["decision"],
        json!("allow"),
        "LOAD-BEARING: a held request with no matching resolution NEVER silently proceeds to \
         allow on expiry (DR-034 §Decision 2, fail-closed): {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("ask"),
        "on deadline expiry the held request degrades to `ask` — the DR-033 fallback (the agent \
         re-asks): {reply:#}"
    );
    assert!(
        elapsed >= Duration::from_millis(UNBLOCK_SHORT_MS / 2),
        "the daemon HELD the request ~the unblock deadline before degrading to `ask` (a real \
         bounded long-poll), rather than returning `ask` near-instantly with no hold — held for \
         {elapsed:?}, unblock budget {UNBLOCK_SHORT_MS}ms. RED today: the current handler answers \
         at once, so `elapsed` is far below half the budget."
    );
}

// ---------------------------------------------------------------------------
// CRITERION 3 — Never a silent proceed / no spurious wake. A resolution for a
// DIFFERENT escalation does NOT wake this held request: it still expires to
// `ask`. Pins the match key — only the ORIGINAL request_id's resolution resumes
// this connection.
// ---------------------------------------------------------------------------

/// CRITERION 3 — a `permit.resolved(allow)` for a DIFFERENT request_id (a
/// different escalation on the same run) must NOT wake THIS held request: with a
/// SHORT unblock budget it still expires to `ask`, never `allow`. This pins that
/// the wake is keyed on the held connection's ORIGINAL request_id (DR-034
/// §Decision 3), not any resolution on the run.
///
/// RED today: the daemon returns `ask` immediately for the held request (no hold
/// at all), so the `ask` outcome is reached for the WRONG reason. Once a hold
/// exists, this test guards that a NON-matching resolve does not spuriously
/// resume it — the `elapsed >= half budget` assertion proves the request was
/// genuinely held-and-expired, not woken by the foreign resolution.
#[test]
fn foreign_resolution_does_not_wake_this_held_request() {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_SHORT_MS);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile url");
    let badge = lock["badge"].as_str().expect("operator badge");

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    // The HELD request (Bash) — the one that must NOT be woken by a foreign resolve.
    const HELD_REQ: &str = "01DR034FOREIGNHELDREQ00001";
    let started = Instant::now();
    let held = hold_request(
        &daemon.socket,
        &run,
        "Bash",
        HELD_REQ,
        UNBLOCK_SHORT_MS + 6_000,
    );
    await_escalation(&daemon.socket, &run, HELD_REQ);

    // A SEPARATE escalation on the SAME run for a DIFFERENT tool/request_id: fire
    // one ask on its own short-lived connection so a real permit.requested +
    // permit.escalated land for OTHER_REQ, then resolve THAT (allow). This
    // resolution matches a different action identity and a different request_id —
    // it must not wake the held Bash request.
    const OTHER_REQ: &str = "01DR034FOREIGNOTHERREQ0001";
    let other = hold_request(
        &daemon.socket,
        &run,
        "Write",
        OTHER_REQ,
        UNBLOCK_SHORT_MS + 2_000,
    );
    await_escalation(&daemon.socket, &run, OTHER_REQ);
    operator_resolve(url, badge, &run, OTHER_REQ, "allow");
    // Let the foreign one settle however it settles — irrelevant to the held one.
    let _ = other.join();

    let reply = held.join().expect("held request thread");
    let elapsed = started.elapsed();
    assert_ne!(
        reply["decision"],
        json!("allow"),
        "LOAD-BEARING: a resolution for a DIFFERENT escalation (OTHER_REQ/Write) must NOT wake \
         this held request (HELD_REQ/Bash) — a spurious resume would be a match-key bug \
         (DR-034 §Decision 3): {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("ask"),
        "the held request, unmatched by any resolution for ITS request_id, expires to `ask`: {reply:#}"
    );
    assert!(
        elapsed >= Duration::from_millis(UNBLOCK_SHORT_MS / 2),
        "the held request was genuinely HELD to the unblock deadline (not woken early by the \
         foreign resolve) before degrading to `ask` — held for {elapsed:?}, budget \
         {UNBLOCK_SHORT_MS}ms."
    );
}

// ---------------------------------------------------------------------------
// CRITERION 4 — DR-033 fallback stays green (regression guard). The non-live
// path is unchanged: an agent that does NOT hold a live connection still gets
// "honored on next ask" from the ledger-check. This does NOT duplicate or lower
// the DR-033 PDP tests (`crates/rezidnt-mcp/tests/permit_resolved_pdp.rs`); it
// confirms the SEPARATE un-held socket path the live layer sits on top of.
// ---------------------------------------------------------------------------

/// CRITERION 4 — the DR-033 fallback: escalate (ask) → operator resolves →
/// a FRESH, non-held ask (new request_id) is honored as `allow`. No live hold is
/// involved — the request is a normal one-shot socket ask AFTER the resolution
/// already landed. Proves the live layer did not disturb the ledger-check.
///
/// RED today: the pre-verifier ledger-check that applies a folded resolution on
/// the next ask is itself part of the DR-033/DR-034 permit substrate over the
/// socket; if it is absent the fresh ask escalates (`ask`) instead of applying
/// the resolution. (DR-033's PDP-level board proves the ledger-check in
/// isolation; this asserts it over the SOCKET transport end-to-end.) A green
/// implementation keeps this `allow`.
#[test]
fn non_held_next_ask_is_honored_dr033_fallback() {
    let (daemon, lock_path) = start_daemon_with_mcp_and_unblock(UNBLOCK_SHORT_MS);
    let lock = wait_for_lockfile(&lock_path, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile url");
    let badge = lock["badge"].as_str().expect("operator badge");

    let dir = tempfile::tempdir().expect("tempdir");
    let spec = make_empty_permit_project(dir.path(), 6_000);
    let run = open_and_get_run(&daemon.socket, &spec);

    // FIRST ask (one-shot, NOT held long — a short reply budget). It escalates to
    // `ask` and lands a permit.requested + permit.escalated the operator resolves.
    const ESC_REQ: &str = "01DR034FALLBACKESCREQ00001";
    let first = hold_request(
        &daemon.socket,
        &run,
        "Bash",
        ESC_REQ,
        UNBLOCK_SHORT_MS + 3_000,
    );
    await_escalation(&daemon.socket, &run, ESC_REQ);
    let first_reply = first.join().expect("first ask thread");
    assert_eq!(
        first_reply["decision"],
        json!("ask"),
        "the first ask escalates (empty policy) — the escalation the operator will resolve: \
         {first_reply:#}"
    );

    // Operator resolves the escalation (allow).
    operator_resolve(url, badge, &run, ESC_REQ, "allow");

    // A FRESH, non-held ask for the SAME action with a DIFFERENT request_id — the
    // DR-033 "next ask" path (not a live hold). It must be honored as `allow`.
    const NEXT_REQ: &str = "01DR034FALLBACKNEXTREQ0001";
    let next = hold_request(
        &daemon.socket,
        &run,
        "Bash",
        NEXT_REQ,
        UNBLOCK_SHORT_MS + 3_000,
    );
    let next_reply = next.join().expect("next ask thread");
    assert_eq!(
        next_reply["decision"],
        json!("allow"),
        "the fresh non-held ask is HONORED from the folded resolution (DR-033 'next ask'), proving \
         the live layer did not disturb the ledger fallback: {next_reply:#}"
    );
    assert_eq!(
        next_reply["request_id"],
        json!(NEXT_REQ),
        "the honored next-ask reply echoes ITS OWN fresh request_id (the match was by action \
         identity, not request_id — DR-033 §Decision 3): {next_reply:#}"
    );
}

// A tiny compile-time guard so the imports above (BufReader/UnixStream/BufRead)
// stay honestly used regardless of which tests compile in (mirrors the
// `_touch_imports` pattern in `permit_socket_decision.rs`).
#[allow(dead_code)]
fn _touch_imports(sock: &Path) {
    if let Ok(stream) = UnixStream::connect(sock) {
        let mut r = BufReader::new(stream);
        let mut s = String::new();
        let _ = r.read_line(&mut s);
    }
}
