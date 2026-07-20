//! SP4c-wire oracle — the DAEMON-side authority boundary (DR-020 §Decision 1,
//! ACCEPTED 2026-07-20). This is the load-bearing decision the MCP-core
//! `with_layered_permit_config` tests CANNOT reach: they inject three layers
//! DIRECTLY as `Vec<PermitVerifierSpec>`, so they prove `compose_layers` +
//! stricter-wins + layer-on-the-wire, but NEVER exercise the daemon's
//! `permit_config_for` SOURCING. DR-020 ratifies that the admin layer is sourced
//! from a host-level surface OUTSIDE the workspace spec — so a dev physically
//! cannot override (or forge) an admin deny. Without a test here, an implementer
//! could green all four core tests while wiring the daemon to read all three
//! layers from `workspace.spec.applied`, silently violating the DR's central
//! decision and making `deciding_layer: "admin"` a label a dev could forge
//! rather than an audit-backed authority claim (the whole I6 argument in the DR).
//!
//! WHAT THIS PINS (end-to-end through the REAL daemon socket + PDP):
//!   - the workspace's DEV `[gates.permit]` (from `workspace.spec.applied`)
//!     GRANTS `Edit`, AND
//!   - a HOST-LEVEL admin permit source (wired via `REZIDNT_ADMIN_PERMIT`,
//!     outside the workspace spec) DENIES `Edit`,
//!
//! so the live socket decision is **deny** — the dev-layer allow does NOT
//! override the admin deny sourced from the host surface (DR-020 §Decision 1,
//! stricter-wins END-TO-END through `permit_config_for`), AND the emitted
//! `permit.denied` fact's CAS-pinned `policy_ref` blob carries `"layer":
//! "admin"` (DR-020 §Decision 4) — the deciding AUTHORITY is surfaced,
//! disambiguating two identically-named `tool-allowlist` verifiers (I6). This is
//! the honest claim the audit trail must back.
//!
//! HOST-ADMIN SURFACE (the minimal shape defined for the implementer; DR-020
//! §"What this does NOT decide" leaves the FORMAT to /oracle+impl, ratifies the
//! BOUNDARY): env `REZIDNT_ADMIN_PERMIT` → a host TOML file with a top-level
//! `[gates.permit]` block (same `verifiers = [{ native, params }]` shape a
//! workspace uses), parsed into `Vec<PermitVerifierSpec>` STAMPED
//! `PermitLayer::Admin`, merged FIRST by `permit_config_for` via
//! `compose_layers(admin, dev, session)`. Full contract on
//! `common::start_daemon_with_admin_permit`.
//!
//! RED MODE: **assert-red** (behavior). The harness compiles today (env var +
//! TOML file are plain strings — no new symbol needed to build it), but the
//! daemon does not READ `REZIDNT_ADMIN_PERMIT` and `permit_config_for` sources
//! ONE layer (the workspace spec) — so the dev-layer allow decides `allow` and
//! no `"layer"` key is pinned. The `deny` + `layer == "admin"` assertions fail
//! until the implementer lands the three-source resolver, the host-admin source,
//! and the `deciding_layer` blob pin. Standard oracle-first RED (DR-020 ratifies
//! the seam): the implementer turns this green in THIS slice before `/vet`.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{
    connect, read_reply_line, read_until, send_line, start_daemon, start_daemon_with_admin_permit,
    stub_harness, try_start_daemon_with_admin_permit_path,
};
use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;
use serde_json::json;

const REPLY_DEADLINE: Duration = Duration::from_secs(10);
const TAIL_DEADLINE: Duration = Duration::from_secs(20);

/// A temp project whose DEV `[gates.permit]` (in `workspace.spec.applied`) GRANTS
/// the given tools via a `tool-allowlist` native. This is the dev-editable
/// surface — the exact place a dev COULD write an "admin" rule if the boundary
/// were not enforced. The admin deny must come from ELSEWHERE (the host source).
fn make_dev_grants_project(gap_ms: u64, allow: &[&str]) -> (tempfile::TempDir, String) {
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
name = "sp4c-dev-grants"
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

/// A host-level admin `[gates.permit]` block (the SOURCE outside the workspace
/// spec) whose `tool-allowlist` DENIES the tools NOT in `allow`. Written to the
/// file `REZIDNT_ADMIN_PERMIT` points at by the harness.
fn admin_permit_toml(allow: &[&str]) -> String {
    let allow_list = allow
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"[gates.permit]
verifiers = [
  {{ native = "tool-allowlist", params = {{ allow = [{allow_list}] }} }},
]
"#
    )
}

/// Open the spec, tail until `agent.spawned`, return the spawned run's ulid.
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

/// Send one `request_permission` line and read the single reply frame.
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

/// Collect tail lines up to and including the last permit fact for `run`.
fn tail_permit_facts(socket: &Path, run: &str, until_subject: &str) -> Vec<serde_json::Value> {
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == until_subject && v["payload"]["run"] == json!(run)
    })
}

// ---------------------------------------------------------------------------
// DR-020 §Decision 1 (HEADLINE) — the admin deny sourced OUTSIDE the workspace
// spec is NON-OVERRIDABLE by the dev-layer allow inside it, END-TO-END.
// ---------------------------------------------------------------------------

/// The load-bearing test: the workspace DEV spec GRANTS `Edit` (a dev-editable
/// `tool-allowlist` allowing it), while the HOST admin source DENIES `Edit` (an
/// admin `tool-allowlist` excluding it). Driving the live socket decision must
/// yield **deny** — the dev allow does NOT override the admin deny, because
/// `permit_config_for` sources admin FIRST from the host surface and the
/// aggregate has no allow-override primitive (DR-020 §Decision 1). This is the
/// authority boundary: admin is sourced from beyond the dev's edit reach.
///
/// RED today: the daemon does not read `REZIDNT_ADMIN_PERMIT`, so only the
/// dev-layer allow is sourced → `allow`. The `deny` assertion fails until the
/// three-source resolver lands.
#[test]
fn admin_source_outside_workspace_spec_denies_over_dev_allow() {
    // dev spec grants Edit; host admin allows only Read → admin DENIES Edit.
    let (daemon, _cas_root) = start_daemon_with_admin_permit(&admin_permit_toml(&["Read"]));
    let (_project, spec) = make_dev_grants_project(600, &["Read", "Edit", "Bash"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Edit", "01SP4CADMINDENYREQ00000001");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_ne!(
        reply["decision"],
        json!("allow"),
        "the DEV-layer allow in the workspace spec must NOT override the ADMIN deny \
         sourced from the host surface — a dev cannot grant past admin (DR-020 \
         §Decision 1, the authority boundary): {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("deny"),
        "an admin deny sourced OUTSIDE the workspace spec is NON-OVERRIDABLE by the \
         dev-layer allow, LIVE through permit_config_for (DR-020 §Decision 1): {reply:#}"
    );
}

/// The interrogability leg (DR-020 §Decision 4): the emitted `permit.denied`
/// fact's CAS-pinned `policy_ref` blob carries `"layer": "admin"` — the deciding
/// AUTHORITY is surfaced on the wire, disambiguating the two identically-named
/// `tool-allowlist` verifiers (dev's grant vs admin's deny). This is what makes
/// the admin deny an audit-backed claim, not a forgeable label (I6). Read the
/// blob back from the daemon's own CAS (pinned to `cas_root` by the harness).
///
/// RED today: no admin layer is sourced (so no admin verifier decides) AND the
/// emit path does not pin `"layer"` — both must land for this to pass.
#[test]
fn admin_deny_fact_pins_deciding_layer_admin_in_policy_blob() {
    let (daemon, cas_root) = start_daemon_with_admin_permit(&admin_permit_toml(&["Read"]));
    // BOTH layers carry a `tool-allowlist` — the NAME is ambiguous; only the
    // LAYER distinguishes which authority denied. Dev grants Edit; admin denies.
    let (_project, spec) = make_dev_grants_project(600, &["Read", "Edit", "Bash"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Edit", "01SP4CADMINLAYERREQ0000001");
    // Crisp red FIRST: assert the deny frame here so a failure names the absent
    // feature, not a 20s tail timeout.
    assert_eq!(
        reply["decision"],
        json!("deny"),
        "precondition — the admin source denies Edit over the dev allow (DR-020 \
         §Decision 1): {reply:#}"
    );

    let lines = tail_permit_facts(&daemon.socket, &run, "permit.denied");
    let denied = lines
        .iter()
        .find(|v| v["subject"] == "permit.denied" && v["payload"]["run"] == json!(run))
        .unwrap_or_else(|| panic!("permit.denied must land on the log (I3); saw {lines:#?}"));

    // The `policy_ref` is an opaque CasRef {hash, bytes, mime}; read the pinned
    // policy blob back from the daemon's own CAS and assert the deciding LAYER.
    let policy_ref: CasRef = serde_json::from_value(denied["payload"]["policy_ref"].clone())
        .unwrap_or_else(|e| panic!("policy_ref is an opaque CasRef ({e}): {denied:#}"));
    let cas = Cas::open(&cas_root).expect("open the daemon's CAS root");
    let blob = cas
        .get(&policy_ref)
        .expect("read the pinned policy blob from the daemon's CAS");
    let policy: serde_json::Value =
        serde_json::from_slice(&blob).expect("policy blob is the pinned policy JSON");

    assert_eq!(
        policy["verifier"],
        json!("tool-allowlist"),
        "the deciding verifier NAME is pinned (unchanged) — but AMBIGUOUS: both the \
         dev and admin layers hold a `tool-allowlist`. policy={policy:#}"
    );
    assert_eq!(
        policy["layer"],
        json!("admin"),
        "the pinned policy blob carries the DECIDING LAYER `admin` — the audit trail \
         BACKS the claim that an admin (not a dev) denied, disambiguating the two \
         identically-named verifiers by AUTHORITY (I6, DR-020 §Decision 4 makes the \
         §Decision-1 boundary an audit-backed claim). policy={policy:#}"
    );
}

/// The ABSENT-env regression control (DR-020 §Decision 1 / `main.rs:131-133`): an
/// UNSET `REZIDNT_ADMIN_PERMIT` preserves the pre-SP4c single-source behavior —
/// the empty admin layer contributes zero verifiers, so a dev-layer allow decides
/// `allow`, unchanged. This uses the PLAIN daemon-start helper, which never sets
/// `REZIDNT_ADMIN_PERMIT`, so it genuinely exercises the absent-env path (the
/// prior version of this test wired a PERMISSIVE admin source and only claimed to
/// test the absent path — this one actually does).
///
/// Passes both before and after the resolver: no admin env ⇒ empty admin layer ⇒
/// no regression to the existing dev-only path.
#[test]
fn unset_admin_env_preserves_single_source_dev_allow() {
    // No REZIDNT_ADMIN_PERMIT in the daemon's env: the admin layer is empty.
    let daemon = start_daemon();
    let (_project, spec) = make_dev_grants_project(600, &["Read", "Edit", "Bash"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Edit", "01SP4CNOADMINENVREQ0000001");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("allow"),
        "an UNSET REZIDNT_ADMIN_PERMIT ⇒ empty admin layer ⇒ the dev-layer allow \
         decides `allow`, unchanged from the pre-SP4c single-source path (DR-020 \
         absent-env guarantee, no regression): {reply:#}"
    );
}

/// The boundary-not-theater control: a PERMISSIVE admin source (allowing the tool
/// too) does NOT deny — so the deny in `admin_source_outside_workspace_spec_...`
/// is caused by the admin DENY specifically, not by the mere presence of three
/// layers. Paired with the headline deny, this pins that swapping the admin rule
/// from allow to deny is what flips the decision (the admin layer is load-bearing,
/// not incidental).
#[test]
fn permissive_admin_source_does_not_deny() {
    // Admin source ALLOWS Edit too; the dev spec grants it. With both layers
    // permissive, the decision is allow.
    let (daemon, _cas_root) =
        start_daemon_with_admin_permit(&admin_permit_toml(&["Read", "Edit", "Bash"]));
    let (_project, spec) = make_dev_grants_project(600, &["Read", "Edit", "Bash"]);
    let run = open_and_get_run(&daemon.socket, &spec);

    let reply = ask_permission(&daemon.socket, &run, "Edit", "01SP4CADMINALLOWREQ0000001");
    assert_eq!(
        reply["reply"],
        json!("permit_decision"),
        "the socket answers a permit_decision frame: {reply:#}"
    );
    assert_eq!(
        reply["decision"],
        json!("allow"),
        "with a PERMISSIVE admin layer AND a permissive dev layer, Edit is allowed — \
         the deny in the sibling test is caused by the admin DENY specifically, not \
         by the mere presence of three layers (the boundary is real, not theater): {reply:#}"
    );
}

// ---------------------------------------------------------------------------
// DR-020 §Decision 1 (security-adjacent) — a set-but-unreadable admin surface is
// an HONEST startup error, NEVER a silently-empty admin layer that drops the
// boundary (`main.rs:135` read + `:141` parse propagate via `?`).
// ---------------------------------------------------------------------------

/// A set-but-MALFORMED `REZIDNT_ADMIN_PERMIT` (unparseable TOML) must ABORT
/// daemon startup — the daemon must NOT come up serving with an empty admin
/// layer, which would silently drop the authority boundary (an I6/security
/// defect). Asserts the daemon never becomes READY (its socket never binds).
#[test]
fn malformed_admin_permit_aborts_startup_never_silently_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let admin = dir.path().join("admin-permit.toml");
    // Not valid TOML — the host admin permit parser must reject this and the
    // daemon must propagate the error (never fall through to an empty layer).
    std::fs::write(&admin, "this is not = valid [ toml ][[[\n").expect("write malformed toml");

    let became_ready = try_start_daemon_with_admin_permit_path(&admin, Duration::from_secs(3));
    assert!(
        !became_ready,
        "a set-but-MALFORMED REZIDNT_ADMIN_PERMIT must ABORT startup — the daemon \
         must NOT bind its socket and serve with a silently-empty admin layer \
         (that would drop the authority boundary, an I6/security defect; DR-020 \
         §Decision 1, main.rs read+parse propagate via `?`)"
    );
}

/// A set-but-MISSING `REZIDNT_ADMIN_PERMIT` (path that does not exist) must
/// likewise ABORT startup — a configured-but-unreadable admin surface is an
/// honest error, never a silently-dropped boundary (`main.rs:135` read_to_string
/// propagates via `?`). Distinct from the UNSET-env path, which is the honest
/// empty-layer default asserted by `unset_admin_env_preserves_single_source_dev_allow`.
#[test]
fn missing_admin_permit_file_aborts_startup() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A path we deliberately never create: set-but-unreadable.
    let admin = dir.path().join("does-not-exist-admin-permit.toml");
    assert!(
        !admin.exists(),
        "the admin permit path must be absent for this test"
    );

    let became_ready = try_start_daemon_with_admin_permit_path(&admin, Duration::from_secs(3));
    assert!(
        !became_ready,
        "a set-but-MISSING REZIDNT_ADMIN_PERMIT (configured, unreadable) must ABORT \
         startup, never come up with a silently-empty admin layer (DR-020 §Decision \
         1; distinct from the honest UNSET-env empty-layer default)"
    );
}
