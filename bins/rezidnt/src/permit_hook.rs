//! `rezidnt permit-hook` — the permit Policy Enforcement Point (DR-014
//! §Decision 1; design §4). Not a new binary (I7): a subcommand of the `rezidnt`
//! CLI that claude-code's `PreToolUse` hook config invokes.
//!
//! Flow (design §4): read claude-code's `PreToolUse` stdin JSON
//! (`tool_name`, `tool_input`, session/cwd context) → map `tool_name` → `tool`,
//! extract path args → `paths`, run id from `REZIDNT_RUN` (deterministic run
//! discovery, never cwd-guessed) → connect `REZIDNT_SOCKET`, send one
//! `Request::RequestPermission`, read one `Reply::PermitDecision` → emit the
//! claude-code `hookSpecificOutput.permissionDecision` (`allow`/`deny`/`ask`).
//!
//! Honesty posture (I6, DR-014 §Decision 3): the decision word is NEVER coerced
//! (`deny`/`ask` never become proceed). On an unreachable socket, decode error,
//! or a past-timeout round-trip the hook fails CLOSED to `ask` — never a silent
//! proceed. Timeout is `REZIDNT_PERMIT_TIMEOUT_MS` (default 250 ms).
//!
//! I2: a bulky `tool_input` is pinned to the CAS and carried as a `context_ref`
//! string — never inline bytes over the control-plane socket.

use std::io::Write as _;
#[cfg(unix)]
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};

/// Default fail-closed timeout for the whole PDP round-trip (DR-014 §Decision
/// 3); overridable via `REZIDNT_PERMIT_TIMEOUT_MS`. Unix-only: the socket
/// round-trip (`ask_daemon`) speaks a UDS; the non-unix path bails.
#[cfg(unix)]
const DEFAULT_TIMEOUT_MS: u64 = 250;

/// The size above which a `tool_input` descriptor is pinned to the CAS and
/// carried as a `context_ref` rather than sent inline (I2 — the control plane
/// carries refs, not bulk). Well under the 32 KiB envelope ceiling so the
/// on-socket descriptor stays small. Unix-only: only the UDS `ask_daemon` pins.
#[cfg(unix)]
const INLINE_CONTEXT_MAX: usize = 4 * 1024;

/// The `rezidnt permit-hook` entrypoint. Reads stdin, asks the daemon, writes
/// exactly one claude-code `PreToolUse` output frame to stdout. ALWAYS emits a
/// decision (fail-closed to `ask`) — a live tool call is never left
/// un-adjudicated. Defense-in-depth: even an UNREADABLE/UNPARSEABLE stdin
/// resolves to a fail-closed `ask` rather than a bare non-zero exit — fail-
/// closed is the PEP's whole ethos, so a malformed input is escalated, never
/// left to claude-code's own default (I6). Only a stdout WRITE failure (the
/// hook cannot deliver its decision at all) is a hard error.
pub fn run() -> anyhow::Result<()> {
    let output = match read_input() {
        Ok(input) => decide(&input),
        // A stdin read/parse failure is not a proceed: escalate (fail-closed).
        Err(e) => decision_to_output("ask", Some(&format!("permit-hook stdin unreadable: {e:#}"))),
    };
    let mut out = std::io::stdout().lock();
    writeln!(out, "{output}").context("write hook output")?;
    out.flush().context("flush hook output")?;
    Ok(())
}

/// Read + parse the claude-code `PreToolUse` stdin JSON. A failure here funnels
/// to a fail-closed `ask` in [`run`] (defense-in-depth), never a proceed.
fn read_input() -> anyhow::Result<Value> {
    let mut stdin = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)
        .context("read PreToolUse stdin")?;
    serde_json::from_str(stdin.trim()).context("parse claude-code PreToolUse stdin JSON")
}

/// Resolve the full hook output for a PreToolUse `input`: build the request from
/// stdin + env, ask the daemon, map the decision. Any failure to reach a real
/// decision fails CLOSED to `ask` (fail-posture, design §5) — never a proceed.
fn decide(input: &Value) -> Value {
    match ask_daemon(input) {
        Ok((decision, reason)) => decision_to_output(&decision, reason.as_deref()),
        // Unreachable / decode-error / timeout ⇒ escalate, NEVER a silent
        // proceed (I6, DR-014 §Decision 3). The reason names why so the human
        // escalate surface can read it.
        Err(e) => decision_to_output("ask", Some(&format!("permit PDP unreachable: {e:#}"))),
    }
}

/// The claude-code tool name (`tool_name`) mapped to the request's `tool`; the
/// design maps `tool_name` → `tool` verbatim. Unix-only: consumed by the UDS
/// `ask_daemon`.
#[cfg(unix)]
fn extract_tool(input: &Value) -> Option<String> {
    input
        .get("tool_name")
        .and_then(Value::as_str)
        .map(String::from)
}

/// Extract path arguments from `tool_input` → the `paths` axis the native
/// `path-scope` verifier reads (`params.paths`). Best-effort over the common
/// claude-code tool-input shapes: a `file_path`/`path`/`notebook_path` scalar,
/// or an `edits`/`paths` array. Returns `None` when no path axis is present so
/// the socket omits it (additive-evolution: absent = omitted, never null).
/// Unix-only: consumed by the UDS `ask_daemon`.
#[cfg(unix)]
pub fn extract_paths(input: &Value) -> Option<Value> {
    let tool_input = input.get("tool_input")?;
    let mut paths: Vec<Value> = Vec::new();
    for key in ["file_path", "path", "notebook_path"] {
        if let Some(p) = tool_input.get(key).and_then(Value::as_str) {
            paths.push(json!(p));
        }
    }
    // A `paths` array (e.g. a multi-file tool) is threaded through verbatim.
    if let Some(arr) = tool_input.get("paths").and_then(Value::as_array) {
        for p in arr {
            if p.is_string() {
                paths.push(p.clone());
            }
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(Value::Array(paths))
    }
}

/// The fail-closed timeout for the round-trip: `REZIDNT_PERMIT_TIMEOUT_MS` when
/// set to a parseable value, else the 250 ms default (DR-014 §Decision 3).
/// Unix-only: bounds the UDS round-trip in `ask_daemon`.
#[cfg(unix)]
fn timeout() -> Duration {
    let ms = std::env::var("REZIDNT_PERMIT_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    Duration::from_millis(ms)
}

/// Ask the daemon PDP over the socket and return `(decision_word, reason)`. Any
/// step failing is an `Err` the caller turns into a fail-closed `ask`. This is
/// the only impure part; `decide` funnels every failure to escalate.
#[cfg(unix)]
fn ask_daemon(input: &Value) -> anyhow::Result<(String, Option<String>)> {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;

    use rezidnt_proto::{Reply, decode_reply, encode_request, socket_path};

    let request = build_request(input, &crate::cas_dir())?;

    let deadline = timeout();
    let sock = socket_path();
    let stream = UnixStream::connect(&sock)
        .with_context(|| format!("connect daemon at {}", sock.display()))?;
    // Bound every blocking op by the fail-closed timeout — a hung daemon must
    // resolve to `ask`, not block the tool call forever.
    stream
        .set_read_timeout(Some(deadline))
        .context("set read timeout")?;
    stream
        .set_write_timeout(Some(deadline))
        .context("set write timeout")?;

    let mut reader = BufReader::new(stream);
    // Discard the versioned hello (the daemon's first frame on every conn).
    let mut hello = String::new();
    reader.read_line(&mut hello).context("read hello")?;

    let mut frame = encode_request(&request).context("encode request")?;
    frame.push('\n');
    reader
        .get_mut()
        .write_all(frame.as_bytes())
        .context("send request")?;

    let mut line = String::new();
    reader.read_line(&mut line).context("read reply")?;
    match decode_reply(line.trim_end()).context("decode reply frame")? {
        Reply::PermitDecision {
            decision, reason, ..
        } => Ok((decision, reason)),
        // An honest daemon-side error frame is NOT a decision — escalate.
        Reply::Error { code, message, .. } => {
            anyhow::bail!("daemon error {code}: {message}")
        }
        other => anyhow::bail!("unexpected reply to request_permission: {other:?}"),
    }
}

/// Build the transport-neutral `Request::RequestPermission` from the PreToolUse
/// stdin + env + the resolved CAS root. Factored out of `ask_daemon` so the
/// descriptor construction — especially the I2 `context_ref` pin branch — is
/// unit-testable without a socket. `cas_root` is threaded in (not read from env
/// here) so a test can point it at a controlled directory deterministically.
///
/// A CAS-pin FAILURE returns `Err` (never a `context_ref: None` degrade) so it
/// funnels through `decide()` → fail-closed `ask` (I6) rather than deciding the
/// PDP on an incomplete descriptor.
#[cfg(unix)]
fn build_request(
    input: &Value,
    cas_root: &std::path::Path,
) -> anyhow::Result<rezidnt_proto::Request> {
    let run = std::env::var("REZIDNT_RUN").context("REZIDNT_RUN not set (run discovery)")?;
    let tool = extract_tool(input).context("PreToolUse stdin carries no tool_name")?;
    let paths = extract_paths(input);
    // I2: a bulky tool_input is pinned to the CAS as a context_ref, never sent
    // inline over the control-plane socket. A CAS-pin failure propagates (`?`).
    let context_ref = context_ref_for(input, cas_root)?;
    Ok(rezidnt_proto::Request::RequestPermission {
        run,
        request_id: ulid::Ulid::new().to_string(),
        action: "tool.invoke".to_string(),
        tool,
        badge: None,
        context_ref,
        paths,
    })
}

#[cfg(not(unix))]
fn ask_daemon(_input: &Value) -> anyhow::Result<(String, Option<String>)> {
    anyhow::bail!(
        "permit-hook speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

/// Pin a bulky `tool_input` to the CAS at `cas_root` and return a `context_ref`
/// string (I2). A small `tool_input` (≤ [`INLINE_CONTEXT_MAX`]) rides inline
/// (returns `None`), so the descriptor on the socket stays small. A CAS failure
/// is SURFACED (`Err`) so `build_request`/`ask_daemon` fails closed to `ask`
/// rather than silently sending bytes inline or dropping the context. Unix-only:
/// only the UDS `ask_daemon` pins bulk context.
#[cfg(unix)]
fn context_ref_for(input: &Value, cas_root: &std::path::Path) -> anyhow::Result<Option<String>> {
    let Some(tool_input) = input.get("tool_input") else {
        return Ok(None);
    };
    let serialized = serde_json::to_vec(tool_input).context("serialize tool_input")?;
    if serialized.len() <= INLINE_CONTEXT_MAX {
        return Ok(None); // small enough to omit — the descriptor carries the tool
    }
    let cas = rezidnt_cas::Cas::open(cas_root)
        .with_context(|| format!("open cas {}", cas_root.display()))?;
    let cas_ref = cas
        .put(&serialized, "application/json")
        .context("pin bulky tool_input to CAS (I2)")?;
    Ok(Some(format!("cas:blake3:{}", cas_ref.hash)))
}

/// The pure decision → claude-code `PreToolUse` output mapping (design §4),
/// factored so it is unit-testable without a socket. Maps the daemon's decision
/// word through the never-coercing [`rezidnt_proto::Enforcement`] contract:
/// `allow → allow`, `deny → deny` (+ reason), `ask → ask` (+ reason). Any word
/// the PDP does not emit conservatively escalates to `ask` — an unrecognized
/// decision is NEVER upgraded to proceed (I6).
pub fn decision_to_output(decision: &str, reason: Option<&str>) -> Value {
    use rezidnt_proto::Enforcement;

    let permission_decision = match Enforcement::for_decision(decision) {
        Enforcement::Proceed => "allow",
        Enforcement::Block => "deny",
        Enforcement::Escalate => "ask",
    };
    let mut hook_output = json!({
        "hookEventName": "PreToolUse",
        "permissionDecision": permission_decision,
    });
    // A deny/ask carries the daemon's reason so the blocked agent (or the human
    // escalate surface) reads WHY (I6). A trivially-granted allow needs none.
    if permission_decision != "allow"
        && let Some(r) = reason
        && let Some(obj) = hook_output.as_object_mut()
    {
        obj.insert("permissionDecisionReason".to_string(), json!(r));
    }
    json!({ "hookSpecificOutput": hook_output })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision_word(out: &Value) -> &str {
        out["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .expect("permissionDecision present")
    }

    #[test]
    fn allow_maps_to_allow_no_reason() {
        let out = decision_to_output("allow", None);
        assert_eq!(decision_word(&out), "allow");
        assert!(
            out["hookSpecificOutput"]
                .get("permissionDecisionReason")
                .is_none(),
            "a trivially-granted allow carries no reason"
        );
    }

    #[test]
    fn deny_maps_to_deny_with_reason() {
        let out = decision_to_output("deny", Some("tool Bash not in allowlist"));
        assert_eq!(decision_word(&out), "deny");
        assert_eq!(
            out["hookSpecificOutput"]["permissionDecisionReason"],
            json!("tool Bash not in allowlist")
        );
    }

    #[test]
    fn ask_maps_to_ask_never_coerced() {
        let out = decision_to_output("ask", Some("empty policy set"));
        assert_eq!(decision_word(&out), "ask");
    }

    #[test]
    fn unknown_decision_escalates_never_proceeds() {
        // I6: a word the PDP does not emit is conservatively `ask`, never allow.
        let out = decision_to_output("something-else", None);
        assert_ne!(decision_word(&out), "allow");
        assert_eq!(decision_word(&out), "ask");
    }

    // Unix-only: `extract_paths` is gated to the UDS `ask_daemon` path. The
    // portable never-coerce mapping tests above stay ungated for host coverage.
    #[cfg(unix)]
    #[test]
    fn extract_paths_reads_file_path() {
        let input = json!({"tool_name": "Edit", "tool_input": {"file_path": "src/main.rs"}});
        assert_eq!(extract_paths(&input), Some(json!(["src/main.rs"])));
    }

    #[cfg(unix)]
    #[test]
    fn extract_paths_absent_is_none() {
        let input = json!({"tool_name": "Bash", "tool_input": {"command": "echo hi"}});
        assert_eq!(extract_paths(&input), None);
    }

    /// A bulky `tool_input` (> INLINE_CONTEXT_MAX) is a `tool_input` whose
    /// serialized form exceeds 4 KiB. Used to force the CAS-pin (I2) branch.
    #[cfg(unix)]
    fn bulky_tool_input() -> Value {
        // A command string comfortably over the 4 KiB inline ceiling.
        let big = "x".repeat(INLINE_CONTEXT_MAX + 1024);
        json!({"tool_name": "Bash", "tool_input": {"command": big}})
    }

    /// I2 (pin branch, happy path): a bulky `tool_input` produces a
    /// `context_ref` CAS-ref string; the bulk never rides inline. This is the
    /// branch the live oracle never exercises (it only sends sub-4KiB inputs),
    /// where the swallowed-`Err` bug hid.
    #[cfg(unix)]
    #[test]
    fn bulky_tool_input_pins_a_context_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = bulky_tool_input();
        let cref = context_ref_for(&input, dir.path()).expect("pin succeeds on a writable CAS");
        let cref = cref.expect("a bulky tool_input carries a context_ref (I2)");
        assert!(
            cref.starts_with("cas:blake3:"),
            "the context_ref is a CAS ref string, not inline bytes (I2): {cref}"
        );
    }

    /// I2 (inline path): a small `tool_input` omits the `context_ref` — it rides
    /// inline in the descriptor, no CAS round-trip. Paired with the pin test this
    /// pins that the threshold actually gates the two paths.
    #[cfg(unix)]
    #[test]
    fn small_tool_input_omits_context_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = json!({"tool_name": "Bash", "tool_input": {"command": "echo hi"}});
        let cref = context_ref_for(&input, dir.path()).expect("small input never fails");
        assert_eq!(
            cref, None,
            "a small tool_input omits context_ref (rides inline)"
        );
    }

    /// I6 (the must-fix, directly pinned): when the CAS pin FAILS on a bulky
    /// `tool_input`, `context_ref_for` returns `Err` — it does NOT degrade to
    /// `None` and silently drop the bulk context. The failure is forced
    /// deterministically by pointing the CAS root at a path UNDER an existing
    /// regular file, so `Cas::open`'s `create_dir_all` fails (a file is not a
    /// directory). `build_request` propagates this `Err`, and `decide()` maps it
    /// to a fail-closed `ask` — never a proceed on an incomplete descriptor.
    #[cfg(unix)]
    #[test]
    fn cas_pin_failure_surfaces_err_never_drops_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A regular FILE where the CAS root would need a directory — create_dir_all
        // under it fails deterministically and portably.
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"blocker").expect("write blocker file");
        let unwritable_root = file.join("cas"); // needs `file` to be a dir — it is not

        let input = bulky_tool_input();
        // The unit that had the bug: the CAS-pin failure must SURFACE as `Err`,
        // never degrade to `Ok(None)` (which would silently drop the bulk
        // context and decide the PDP on an incomplete descriptor).
        let result = context_ref_for(&input, &unwritable_root);
        assert!(
            result.is_err(),
            "a CAS-pin failure SURFACES (Err) — never a silent context_ref: None (I6)"
        );

        // And `build_request` PROPAGATES that Err (the `?` the must-fix added),
        // so it never emits a request with the bulk context silently dropped.
        // `REZIDNT_RUN` is read first in build_request; set it so the flow
        // reaches the pin and the CAS Err is the failure under test.
        // SAFETY: this test owns REZIDNT_RUN; no other test sets/reads it.
        unsafe {
            std::env::set_var("REZIDNT_RUN", "01SP2CASFAILRUN00000000R001");
        }
        let build = build_request(&input, &unwritable_root);
        unsafe {
            std::env::remove_var("REZIDNT_RUN");
        }
        assert!(
            build.is_err(),
            "build_request propagates the CAS-pin Err (does not degrade to None) — \
             the Err funnels through decide() → fail-closed ask (I6)"
        );
    }

    /// The fail-closed funnel decide() applies to ANY ask_daemon/build Err: it
    /// maps to a fail-closed `ask` hook output, never a proceed. Pinned on the
    /// pure mapping so the CAS-pin-Err (above) and the socket-unreachable Err
    /// (the integration board) both resolve to `ask` (I6).
    #[test]
    fn any_error_funnels_to_fail_closed_ask() {
        let out = decision_to_output("ask", Some("permit PDP unreachable: pin failed"));
        assert_ne!(
            decision_word(&out),
            "allow",
            "an Err is NEVER a silent proceed (I6)"
        );
        assert_eq!(
            decision_word(&out),
            "ask",
            "an Err funnels to fail-closed ask"
        );
    }
}
