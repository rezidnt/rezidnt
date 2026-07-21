//! rezidnt DEV-ONLY test-support (DR-023 §Decision (C)): daemon fixture
//! builders + socket-driving helpers relocated from
//! `bins/rezidentd/tests/common/mod.rs`.
//!
//! Consumed ONLY as a `[dev-dependency]` — by `bins/rezidentd`'s integration
//! tests (via a thin `mod common` re-export shim so the 15 test files stay
//! unchanged) and by `bench/harness/tests/real_driver.rs`. It NEVER enters a
//! shipped crate's production `[dependencies]` (the `testkit_dev_only.rs` guard
//! pins that). Unix-only: it drives the daemon over a `UnixStream`.
//!
//! # Binary location
//! Unlike the original `common/mod.rs`, this crate cannot use
//! `env!("CARGO_BIN_EXE_rezidentd")` — cargo only defines that macro for a
//! package's OWN integration tests, not for a separate crate. Instead
//! [`daemon_bin`] / [`cli_bin`] resolve `rezidentd` / `rezidnt` at runtime as
//! siblings of the running test binary (the cargo profile dir, one level above
//! `deps/`). Callers must have built the workspace bins first (`cargo test
//! --workspace` or `cargo build -p rezidentd -p rezidnt`), exactly as the
//! `cli_bin` precedent already required.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// The cargo profile directory the current test binary lives under (the parent
/// of `deps/`), where the workspace's built binaries (`rezidentd`, `rezidnt`)
/// sit. Resolved from `current_exe()` so it works from any consuming crate's
/// integration tests without `env!("CARGO_BIN_EXE_…")` (unavailable across
/// crates).
fn target_bin_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    // .../target/<profile>/deps/<test-bin> -> up to <profile>.
    let deps = exe.parent().expect("test binary has a parent dir");
    if deps.file_name().map(|n| n == "deps").unwrap_or(false) {
        deps.parent()
            .expect("deps has a parent (profile dir)")
            .to_path_buf()
    } else {
        // Some runners place the test binary directly in the profile dir.
        deps.to_path_buf()
    }
}

/// Absolute path to a workspace binary (`rezidentd` / `rezidnt`) built into the
/// profile dir. Panics with a build hint if it is missing.
fn workspace_bin(name: &str) -> PathBuf {
    let path = target_bin_dir().join(name);
    assert!(
        path.exists(),
        "{name} binary not found at {} — build every workspace bin first \
         (`cargo test --workspace` or `cargo build -p rezidentd -p rezidnt`)",
        path.display()
    );
    path
}

/// The `rezidentd` daemon binary (built into the profile dir).
pub fn daemon_bin() -> PathBuf {
    workspace_bin("rezidentd")
}

/// Kills the daemon on drop so a failing test never leaks a process.
pub struct DaemonGuard {
    pub child: Child,
    pub socket: PathBuf,
    pub db: PathBuf,
    _dir: tempfile::TempDir,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start rezidentd with a temp socket + db; wait for the socket to appear.
pub fn start_daemon() -> DaemonGuard {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    DaemonGuard {
        child,
        socket,
        db,
        _dir: dir,
    }
}

/// Connect, consume the hello line, leave the stream positioned at frame 2.
pub fn connect(socket: &Path) -> BufReader<UnixStream> {
    let stream = UnixStream::connect(socket).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");
    let mut reader = BufReader::new(stream);
    let mut hello = String::new();
    reader.read_line(&mut hello).expect("hello line");
    assert!(
        hello.contains("\"proto\""),
        "first frame must be the hello, got {hello:?}"
    );
    reader
}

pub fn send_line(reader: &mut BufReader<UnixStream>, line: &str) {
    let stream = reader.get_mut();
    stream.write_all(line.as_bytes()).expect("write request");
    stream.write_all(b"\n").expect("write newline");
}

/// Read envelope lines until `stop` returns true or the deadline passes;
/// returns every parsed line. Panics on deadline — the caller's assertion
/// message is the failure.
pub fn read_until(
    reader: &mut BufReader<UnixStream>,
    deadline: Duration,
    mut stop: impl FnMut(&serde_json::Value) -> bool,
) -> Vec<serde_json::Value> {
    let until = Instant::now() + deadline;
    let mut seen = Vec::new();
    let mut line = String::new();
    while Instant::now() < until {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // daemon closed the stream
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    let done = stop(&v);
                    seen.push(v);
                    if done {
                        return seen;
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => panic!("tail read failed: {e}"),
        }
    }
    panic!(
        "deadline: stop condition never met; saw {} lines: {seen:?}",
        seen.len()
    );
}

/// Write the stub harness script into `dir` and return its path. The script
/// emits fake stream-json with `gap_ms` between lines, so tests can hold a
/// run open long enough to kill clients (or daemons) around it.
pub fn stub_harness(dir: &Path, gap_ms: u64) -> PathBuf {
    let gap_s = gap_ms as f64 / 1000.0;
    let script = dir.join("harness.sh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo '{{"type":"system","subtype":"init","session_id":"fixture-session","claude_code_version":"2.1.191","tools":[]}}'
sleep {gap_s}
echo '{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"working"}}]}}}}'
sleep {gap_s}
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"duration_ms":5,"total_cost_usd":0.001,"usage":{{"input_tokens":1,"output_tokens":1}},"session_id":"fixture-session"}}'
"#
        ),
    )
    .expect("write harness stub");
    let mut perms = std::fs::metadata(&script).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");
    script
}

/// A temp project: git repo + stub harness script + §13 spec pointing at both.
pub fn make_project(gap_ms: u64) -> (tempfile::TempDir, String) {
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
name = "s1-exit"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
bin_override = "{script}"
"#,
        repo = repo.display(),
        script = script.display(),
    );
    (dir, spec)
}

/// JSON-escape a spec into an `open` request line.
pub fn open_request(spec_toml: &str) -> String {
    serde_json::to_string(&serde_json::json!({"op": "open", "spec_toml": spec_toml}))
        .expect("open request json")
}

// ---------------------------------------------------------------------------
// S3 additions: MCP loopback-HTTP transport (lockfile-announced) and log
// pre-seeding for the stubbed-verdict gate_explain board.
// ---------------------------------------------------------------------------

/// The workspace `spec/fixtures` dir. Resolved off this crate's manifest
/// (`crates/rezidnt-testkit/../../spec/fixtures` = the workspace root's
/// fixtures), identical to the daemon-test and bench-harness manifests.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// Start rezidentd with the MCP HTTP transport requested via
/// `REZIDNT_MCP_LOCKFILE` (S3 board pin: env-overridable lockfile path,
/// mirroring `REZIDNT_SOCKET`). Optionally pre-seed the event db from a
/// committed golden fixture BEFORE the daemon starts — the log is truth
/// (I3), so a stub gate verdict is seeded there and nowhere else.
///
/// Returns the guard plus the lockfile path (the file itself appears when
/// the transport is up — waiting for it is the caller's assertion).
pub fn start_daemon_with_mcp(seed_fixture: Option<&str>) -> (DaemonGuard, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let lockfile = dir.path().join("mcp.lock");

    if let Some(name) = seed_fixture {
        let text = std::fs::read_to_string(fixtures_dir().join(name)).expect("fixture exists");
        let mut log = rezidnt_fabric::EventLog::open(&db).expect("open seed db");
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let event = rezidnt_types::Event::from_json_line(line).expect("fixture line parses");
            log.append(&event).expect("seed append");
        }
    }

    let child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .env("REZIDNT_MCP_LOCKFILE", &lockfile)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    (
        DaemonGuard {
            child,
            socket,
            db,
            _dir: dir,
        },
        lockfile,
    )
}

/// Restart the daemon over the SAME db/socket/lockfile paths — a real
/// restart: process memory is gone (SIGKILL, the S0 precedent), the log
/// survives. The old socket and lockfile are removed BEFORE the respawn so a
/// caller waiting on them observes the new process, never a stale artifact.
pub fn restart_daemon_with_mcp(guard: &mut DaemonGuard, lockfile: &Path) {
    let _ = guard.child.kill();
    let _ = guard.child.wait();
    let _ = std::fs::remove_file(&guard.socket);
    let _ = std::fs::remove_file(lockfile);
    guard.child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &guard.socket)
        .env("REZIDNT_DB", &guard.db)
        .env("REZIDNT_MCP_LOCKFILE", lockfile)
        .spawn()
        .expect("respawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !guard.socket.exists() {
        assert!(
            Instant::now() < deadline,
            "restarted daemon socket never appeared"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Wait for the MCP lockfile to appear and parse (S3: port 0 is announced
/// HERE, never fixed). Panicking on the deadline IS the red assertion.
pub fn wait_for_lockfile(path: &Path, deadline: Duration) -> serde_json::Value {
    let until = Instant::now() + deadline;
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
        {
            return v;
        }
        assert!(
            Instant::now() < until,
            "MCP lockfile never appeared/parsed at {} — the HTTP transport was not announced",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Minimal HTTP/1.1 POST of one JSON-RPC message to the announced endpoint.
/// Tolerates plain JSON and SSE (`data:` line) response bodies.
pub fn mcp_post(url: &str, body: &str) -> serde_json::Value {
    use std::io::Read;
    let rest = url
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("loopback http url expected, got {url}"));
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = std::net::TcpStream::connect(host).expect("connect announced endpoint");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let request = format!(
        "POST /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("send request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let text = String::from_utf8_lossy(&raw);
    let (head, payload) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no header/body split in: {text}"));
    assert!(
        head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"),
        "endpoint must answer 200, got: {head}"
    );
    let json_text = if payload.contains("data:") {
        payload
            .lines()
            .find_map(|l| l.strip_prefix("data:"))
            .expect("an SSE body carries a data: line")
            .trim()
            .to_string()
    } else {
        let start = payload.find('{').expect("a JSON body");
        let end = payload.rfind('}').expect("a JSON body");
        payload[start..=end].to_string()
    };
    serde_json::from_str(&json_text)
        .unwrap_or_else(|e| panic!("response body must be JSON-RPC ({e}): {json_text}"))
}

/// JSON-RPC request builder + tools/call sugar over [`mcp_post`].
pub fn rpc(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// tools/call over HTTP; returns the MCP tool result object.
pub fn mcp_tool_call(url: &str, id: u64, tool: &str, args: serde_json::Value) -> serde_json::Value {
    let response = mcp_post(
        url,
        &rpc(
            id,
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        ),
    );
    assert!(
        response.get("error").is_none(),
        "tools/call must not be a protocol error: {response:#}"
    );
    response["result"].clone()
}

/// `content[0].text` parsed as JSON — the machine-readable tool payload.
pub fn tool_payload(result: &serde_json::Value) -> serde_json::Value {
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool result must carry content[0].text: {result:#}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("content[0].text must be JSON ({e}): {text}"))
}

// ---------------------------------------------------------------------------
// S4 additions: gated projects (vet + pre_merge on the golden path), the CLI
// binary locator (shared by the S4 verb boards; `cli_verbs.rs` predates this
// and keeps its private copy), prepared-daemon startup for seeded log + CAS,
// and MCP polling sugar (mcp_workspace_recovery.rs's local copies can migrate
// here when touched).
// ---------------------------------------------------------------------------

/// Locate the `rezidnt` CLI as a sibling of the daemon binary (the
/// documented seam in `cli_verbs.rs`: run `cargo test --workspace` or build
/// the CLI first).
pub fn cli_bin() -> PathBuf {
    workspace_bin("rezidnt")
}

/// Run a CLI verb against the test daemon's socket/db env; capture output.
pub fn run_cli(daemon: &DaemonGuard, args: &[&str]) -> std::process::Output {
    Command::new(cli_bin())
        .args(args)
        .env("REZIDNT_SOCKET", &daemon.socket)
        .env("REZIDNT_DB", &daemon.db)
        .output()
        .expect("run rezidnt CLI")
}

/// Start rezidentd after `prepare(dir)` has seeded the temp dir — the log at
/// `dir/events.db` (via `rezidnt_fabric::EventLog`) and the CAS at
/// `dir/cas` (the daemon's REZIDNT_DB-relative default) both count as
/// pre-daemon truth (I3: log + CAS are the whole persistent state).
pub fn start_daemon_prepared(prepare: impl FnOnce(&Path)) -> DaemonGuard {
    let dir = tempfile::tempdir().expect("tempdir");
    prepare(dir.path());
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    DaemonGuard {
        child,
        socket,
        db,
        _dir: dir,
    }
}

/// A daemon started with a HOST-LEVEL admin permit source wired OUTSIDE any
/// workspace spec (SP4c-wire, DR-020 §Decision 1). This is the authority-boundary
/// harness: `admin_permit_toml` is a host config file the daemon reads at startup,
/// physically unreachable from the `spec_toml` an `open` request carries — so a
/// dev cannot edit or reorder the admin layer. The CAS root is pinned to a
/// returned path so the caller can read the pinned policy blob back and assert the
/// emitted `deciding_layer` (DR-020 §Decision 4).
pub fn start_daemon_with_admin_permit(admin_permit_toml: &str) -> (DaemonGuard, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let cas = dir.path().join("cas");
    // The host admin permit file lives OUTSIDE any workspace repo/spec — a
    // sibling of the daemon's own state, set by whoever launches the daemon.
    let admin = dir.path().join("admin-permit.toml");
    std::fs::write(&admin, admin_permit_toml).expect("write host admin permit toml");

    let child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .env("REZIDNT_CAS", &cas)
        .env("REZIDNT_ADMIN_PERMIT", &admin)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    (
        DaemonGuard {
            child,
            socket,
            db,
            _dir: dir,
        },
        cas,
    )
}

/// A daemon started with a HOST-LEVEL egress-secrets source wired OUTSIDE any
/// workspace spec (DR-029 §Decision 3): `REZIDNT_EGRESS_SECRETS` points at a host
/// TOML (`secret_ref = "value"`) the daemon reads at fold time, physically
/// unreachable from the `spec_toml` an `open` carries — a dev cannot self-grant a
/// secret. The `REZIDNT_ADMIN_PERMIT` authority-boundary harness applied to
/// secrets. Returns a plain [`DaemonGuard`] (the test binds a single value and
/// uses `.socket`).
pub fn start_daemon_with_egress_secrets(secrets_toml: &str) -> DaemonGuard {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let cas = dir.path().join("cas");
    // The host secrets file lives OUTSIDE any workspace repo/spec — a sibling of
    // the daemon's own state, set by whoever launches the daemon (the boundary).
    let secrets = dir.path().join("egress-secrets.toml");
    std::fs::write(&secrets, secrets_toml).expect("write host egress secrets toml");

    let child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .env("REZIDNT_CAS", &cas)
        .env("REZIDNT_EGRESS_SECRETS", &secrets)
        .spawn()
        .expect("spawn rezidentd");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    DaemonGuard {
        child,
        socket,
        db,
        _dir: dir,
    }
}

/// A temp project carrying a non-empty `[egress]` block (DR-029): a git repo +
/// the stub harness + a §13 spec with `allowlist` + an `[egress.secrets]`
/// `host → secret_ref` LABEL map (repo-safe — labels, never values). `gap_ms`
/// sizes the harness's fake work; `allowlist` names the hosts; `secrets` is the
/// `(host, secret_ref)` mapping the daemon-side `SecretSource` resolves. Returns
/// `(tempdir, spec_toml)` — mirrors [`make_project`].
pub fn make_egress_project(
    gap_ms: u64,
    allowlist: &[&str],
    secrets: &[(&str, &str)],
) -> (tempfile::TempDir, String) {
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

    // The allowlist array + the [egress.secrets] LABEL map (a secret_ref, never a
    // value — the values resolve daemon-side from REZIDNT_EGRESS_SECRETS).
    let allow = allowlist
        .iter()
        .map(|h| format!("\"{h}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let secret_lines = secrets
        .iter()
        .map(|(host, secret_ref)| format!("\"{host}\" = \"{secret_ref}\""))
        .collect::<Vec<_>>()
        .join("\n");

    let spec = format!(
        r#"[project]
name = "c3-egress-fold"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
bin_override = "{script}"

[egress]
allowlist = [{allow}]

[egress.secrets]
{secret_lines}
"#,
        repo = repo.display(),
        script = script.display(),
    );
    (dir, spec)
}

/// Attempt to start `rezidentd` with `REZIDNT_ADMIN_PERMIT` pointed at
/// `admin_permit_path` (which may be missing or malformed) and report whether the
/// daemon BECOMES READY — i.e. whether its socket appears within `deadline`.
/// Returns `true` if the daemon came up (socket bound), `false` if it never did
/// (startup aborted). Unlike [`start_daemon_with_admin_permit`], this NEVER
/// panics on non-readiness — the whole point is to observe an honest startup
/// FAILURE (DR-020 §Decision 1: a set-but-unreadable admin surface aborts start,
/// never a silently-empty admin layer that drops the boundary).
///
/// The child is reaped before return (killed if somehow still alive), so a
/// failing start leaks no process. `admin_permit_path` is taken as-is so the
/// caller can point it at a nonexistent path (never written) or a file it wrote
/// with malformed content.
pub fn try_start_daemon_with_admin_permit_path(
    admin_permit_path: &Path,
    deadline: Duration,
) -> bool {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("rezidnt.sock");
    let db = dir.path().join("events.db");
    let cas = dir.path().join("cas");

    let mut child = Command::new(daemon_bin())
        .env("REZIDNT_SOCKET", &socket)
        .env("REZIDNT_DB", &db)
        .env("REZIDNT_CAS", &cas)
        .env("REZIDNT_ADMIN_PERMIT", admin_permit_path)
        .spawn()
        .expect("spawn rezidentd");

    let until = Instant::now() + deadline;
    let mut became_ready = false;
    loop {
        if socket.exists() {
            became_ready = true;
            break;
        }
        // The daemon aborting startup is exactly what we want to observe: if the
        // process has already exited it will never bind the socket.
        if let Ok(Some(_status)) = child.try_wait() {
            break;
        }
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Reap: kill if still running (a ready daemon, or a slow one), then wait so
    // no zombie/leak survives the test.
    let _ = child.kill();
    let _ = child.wait();
    became_ready
}

/// Append every line of a committed golden fixture to the event db at
/// `db` (chain-honest: goes through the real EventLog).
pub fn seed_db_from_fixture(db: &Path, fixture: &str) {
    let text = std::fs::read_to_string(fixtures_dir().join(fixture)).expect("fixture exists");
    let mut log = rezidnt_fabric::EventLog::open(db).expect("open seed db");
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let event = rezidnt_types::Event::from_json_line(line).expect("fixture line parses");
        log.append(&event).expect("seed append");
    }
}

/// The stub harness for GATED runs: emits the S1 stream-json lines AND
/// appends a marker line to `src/checkout/cart.rs` in its working directory
/// (the daemon runs a harness in its allocated worktree), so the S2 watcher
/// produces a real `diff.ready` for pre_merge to verify.
pub fn gated_stub_harness(dir: &Path, gap_ms: u64) -> PathBuf {
    let gap_s = gap_ms as f64 / 1000.0;
    let script = dir.join("gated-harness.sh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo '{{"type":"system","subtype":"init","session_id":"fixture-session","claude_code_version":"2.1.191","tools":[]}}'
printf 'oracle-change\n' >> src/checkout/cart.rs
sleep {gap_s}
echo '{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"working"}}]}}}}'
sleep {gap_s}
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"duration_ms":5,"total_cost_usd":0.001,"usage":{{"input_tokens":1,"output_tokens":1}},"session_id":"fixture-session"}}'
"#
        ),
    )
    .expect("write gated harness stub");
    let mut perms = std::fs::metadata(&script).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");
    script
}

/// An exec verifier stub speaking the §8 contract: consumes stdin, answers
/// `pass`. Stands in for the tests-pass runner on the golden path (the S4
/// oracle's stated scoping: diff-scope + forbidden-path are REAL natives,
/// the test-suite runner is exec-stubbed).
pub fn exec_pass_verifier(dir: &Path) -> PathBuf {
    let script = dir.join("verifier-pass.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\ncat > /dev/null\nprintf '{\"verdict\":\"pass\",\"evidence\":[],\"cost_ms\":7}\\n'\n",
    )
    .expect("write exec verifier stub");
    let mut perms = std::fs::metadata(&script).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");
    script
}

/// A temp GATED project: git repo with a committed `src/checkout/cart.rs`,
/// the diff-writing stub harness, an exec pass-verifier, and a §13 spec
/// wiring `gates = ["vet", "pre_merge"]` with the governed agent fields
/// (bare / harness_version / allowed_tools) and a `[gates.pre_merge]`
/// verifier set: two REAL natives + the exec stub named `tests-pass`.
pub fn make_gated_project(gap_ms: u64) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("src/checkout")).expect("mkdir repo/src/checkout");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "oracle@rezidnt.test"]);
    git(&["config", "user.name", "rezidnt oracle"]);
    std::fs::write(repo.join("src/checkout/cart.rs"), "// cart v0\n").expect("seed file");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "s4 oracle seed"]);

    let harness = gated_stub_harness(dir.path(), gap_ms);
    let verifier = exec_pass_verifier(dir.path());

    let spec = format!(
        r#"[project]
name = "s4-exit"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["vet", "pre_merge"]
bare = true
harness_version = "2.1.191"
allowed_tools = ["Read", "Edit"]
bin_override = "{harness}"

[gates.pre_merge]
verifiers = [
  {{ native = "diff-scope", params = {{ allow = ["src/checkout/**"] }} }},
  {{ native = "forbidden-path", params = {{ forbid = [".env", "secrets/**"] }} }},
  {{ exec = "{verifier}", name = "tests-pass" }},
]
"#,
        repo = repo.display(),
        harness = harness.display(),
        verifier = verifier.display(),
    );
    (dir, spec)
}

/// Read one reply line from a socket connection with a deadline; panics on
/// timeout (the caller's message is the failure).
pub fn read_reply_line(
    reader: &mut BufReader<UnixStream>,
    deadline: Duration,
) -> serde_json::Value {
    let until = Instant::now() + deadline;
    let mut line = String::new();
    while Instant::now() < until {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => panic!("connection closed before a reply frame arrived"),
            Ok(_) if line.trim().is_empty() => continue,
            Ok(_) => {
                return serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("reply frame must be JSON ({e}): {line}"));
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => panic!("reply read failed: {e}"),
        }
    }
    panic!("deadline: no reply frame arrived on the connection");
}
