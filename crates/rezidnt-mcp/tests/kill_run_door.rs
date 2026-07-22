//! DR-032 slice-1 (operator-kill-run) ORACLE — the `kill_run` MCP tool: the
//! operator-badge door (CRITERION 2), the emitted-fact attribution through the
//! single writer (CRITERION 3), and interrogability (CRITERION 4).
//!
//! ## What DR-032 §Decision 1 mandates (the door)
//! `kill_run { run: <ulid> }` sits behind the §12 badge door, but with a
//! NARROWED admission: the OPERATOR badge is REQUIRED and the AGENT-MACAROON
//! path is REJECTED — terminating a run is an operator action, not an agent
//! self-action. So unlike `spawn_agent`/`open_project` (which admit BOTH the
//! opaque operator badge and a valid agent macaroon), `kill_run` admits ONLY the
//! opaque operator badge. A macaroon presented to `kill_run` → refused, no kill.
//!
//! ## Side-effect discipline (I3, the badge_enforcement.rs precedent)
//! A REFUSED kill (no badge / macaroon / unknown token) emits NO `agent.signaled`
//! fact — if it isn't on the log, it didn't happen. An ADMITTED kill emits
//! EXACTLY ONE `agent.signaled` fact carrying the verified operator badge id +
//! the supplied reason, through the daemon single writer (I3 — the client never
//! writes the log directly).
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!   - A new tool name `kill_run` dispatched by `tools_call` (today: unknown
//!     tool → `-32602`, which is why the admit/emit tests are ASSERT-RED now, not
//!     merely compile-red).
//!   - `McpSubstrate::kill_run(&self, run: String) -> BoxFuture<Result<KillAck,
//!     ToolRefusal>>` — the seam that drives the EXISTING
//!     `reaper::stop_with_escalation` and reports the run's pid/exit description.
//!     A recorded/fake substrate stands in here so the FACT SHAPE is judged
//!     deterministically, NOT a live process kill (testing-oracles: build where
//!     a deterministic judge exists; the reaper's real SIGTERM→grace→SIGKILL is
//!     process-timing-dependent and is NOT exercised here).
//!   - The core, on an admitted kill, emits `agent.signaled` with
//!     `operator_badge_id` = the `check_badge`-verified operator id and `reason`
//!     = the call's `reason` arg (omitted when the caller gave none). The
//!     `run`/`signal`/`escalation` fields follow the reaper's existing emission.
//!   - `codes::BADGE_REQUIRED` for a missing badge; `codes::BADGE_INVALID` for a
//!     rejected macaroon / unknown token (reuse the existing machine-readable
//!     classes — a kill refusal is the same refusal vocabulary).
//!
//! RED MODE: ASSERT-RED (tool absent → `-32602` unknown-tool JSON-RPC error, so
//! `tool_call`'s "expected a result, got a JSON-RPC error" panic fires) for the
//! admit/emit/interrogability tests; COMPILE-RED for the substrate-seam type
//! (`KillAck` / `McpSubstrate::kill_run`) the emit test's fake implements. Both
//! are red for the RIGHT reason: the tool and its seam do not exist yet.

mod util;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{
    BadgeBook, BoxFuture, KillAck, McpCore, McpSubstrate, OpenAck, PermitConfig, ToolRefusal,
};
use rezidnt_run::badge::{Badge, Caveat, Macaroon, RootKey};
use serde_json::json;

const RUN: &str = "01DR032RVNK111D00R00000000";
const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const T_MID: &str = "2026-07-22T12:00:00Z";
const T_LATE: &str = "2026-07-23T00:00:00Z";

fn root() -> RootKey {
    RootKey::from_bytes([7u8; 32])
}

/// An agent macaroon valid for `spawn`/`kill` at T_MID — used to prove the kill
/// door REJECTS the macaroon path EVEN when the macaroon itself is otherwise
/// well-formed and verifiable (DR-032 §Decision 1: kill is operator-only).
fn agent_macaroon(root: &RootKey) -> Macaroon {
    Macaroon::mint(
        root,
        "run-01DR032KILLDOORMACAROON00",
        vec![
            Caveat::Workspace {
                workspace: WS.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into(), "kill".into(), "open".into()],
            },
            Caveat::Expiry {
                not_after: T_LATE.into(),
            },
        ],
    )
}

/// A fake substrate that RECORDS kill calls and reports success WITHOUT a real
/// process — the deterministic seam standing in for the reaper. It never touches
/// a pid; the fact SHAPE is what this board judges. `open_project`/`spawn_agent`/
/// `permit_config_for` are unused here (a kill-only board) and are stubbed.
#[derive(Default)]
struct RecordingKillSubstrate {
    kills: AtomicUsize,
}

impl McpSubstrate for RecordingKillSubstrate {
    fn open_project(&self, _spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                "substrate.unavailable",
                "kill-only test substrate",
            ))
        })
    }

    fn spawn_agent(
        &self,
        _workspace: String,
        _agent: String,
        _idempotency_key: String,
    ) -> BoxFuture<Result<String, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                "substrate.unavailable",
                "kill-only test substrate",
            ))
        })
    }

    fn permit_config_for(&self, _run: String) -> BoxFuture<Option<PermitConfig>> {
        Box::pin(async { None })
    }

    /// The DR-032 seam: drive the reaper (faked here) and report the stop.
    fn kill_run(&self, _run: String) -> BoxFuture<Result<KillAck, ToolRefusal>> {
        self.kills.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(KillAck {
                // Mirrors reaper::stop_with_escalation's returned description; the
                // FACT the core emits does not depend on these exact bytes, only
                // on the operator attribution + reason the door verified.
                signal: "SIGTERM".to_string(),
                escalation: Some("term".to_string()),
            })
        })
    }
}

/// A core with the operator badge admitted, the daemon root key wired (so a
/// macaroon COULD verify — proving the kill door rejects it on POLICY, not
/// because the core is keyless), and the recording kill substrate.
fn operator_core(operator: &Badge) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(operator);
    let core = McpCore::new(fabric, book)
        .with_root_key(root())
        .with_substrate(Arc::new(RecordingKillSubstrate::default()));
    (dir, Arc::new(core))
}

/// CRITERION 2 (admit) — `kill_run` with a valid OPERATOR badge is admitted (the
/// door passes, the kill runs). ASSERT-RED now: `kill_run` is an unknown tool.
#[tokio::test]
async fn kill_run_with_operator_badge_is_admitted() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    let result = util::tool_call(
        &core,
        1,
        "kill_run",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "reason": "runaway spend"
        }),
    )
    .await;
    assert_eq!(
        result["isError"],
        json!(false),
        "an operator-badged kill_run is admitted (not a refusal) — got {result:#}"
    );
}

/// CRITERION 2 (no badge) — `kill_run` with NO badge → `BADGE_REQUIRED`, and NO
/// `agent.signaled` fact lands (refuse before effect, I3). ASSERT-RED now.
#[tokio::test]
async fn kill_run_without_badge_is_refused_no_side_effect() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    let result = util::tool_call(&core, 2, "kill_run", json!({ "run": RUN })).await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_REQUIRED);
    assert!(
        util::log_events(&core)
            .iter()
            .all(|e| e.subject.as_str() != "agent.signaled"),
        "a badge-less kill emits no agent.signaled fact (refuse before effect, I3)"
    );
}

/// CRITERION 2 (macaroon rejected — the DR-032 §1 heart) — `kill_run` with a
/// VALID AGENT MACAROON is REFUSED: kill is an operator action, not agent
/// self-action. The macaroon here is well-formed and would verify for a spawn on
/// this same core (root key wired), so the refusal is a POLICY refusal, not a
/// verify failure. NO `agent.signaled` fact lands. ASSERT-RED now.
#[tokio::test]
async fn kill_run_with_agent_macaroon_is_refused_operator_only() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    let m = agent_macaroon(&root());
    let result = util::tool_call(
        &core,
        3,
        "kill_run",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "run": RUN,
            "now": T_MID
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(
        util::log_events(&core)
            .iter()
            .all(|e| e.subject.as_str() != "agent.signaled"),
        "an agent macaroon cannot kill (DR-032 §1); the log stays free of a kill fact"
    );
}

/// CRITERION 3 (emit + attribution through the single writer) — an admitted
/// `kill_run` emits EXACTLY ONE `agent.signaled` fact carrying `operator_badge_id`
/// = the verified operator badge id and `reason` = the supplied reason. The
/// client never writes the log; the daemon single writer does (I3). ASSERT-RED
/// now (and COMPILE-RED on `KillAck`/`kill_run` seam).
#[tokio::test]
async fn admitted_kill_emits_one_attributed_agent_signaled_fact() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    let _ = util::tool_call(
        &core,
        4,
        "kill_run",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "reason": "runaway spend"
        }),
    )
    .await;

    let signaled: Vec<_> = util::log_events(&core)
        .into_iter()
        .filter(|e| e.subject.as_str() == "agent.signaled")
        .collect();
    assert_eq!(
        signaled.len(),
        1,
        "an admitted kill emits EXACTLY ONE agent.signaled fact (single writer, I3)"
    );
    let fact = &signaled[0];
    assert_eq!(
        fact.payload()["run"],
        json!(RUN),
        "the fact keys on the killed run"
    );
    // The operator badge id is the loggable id the BadgeBook holds for the
    // admitted operator token — the SAME id `check_badge` returns (never the
    // token, §12/I2).
    let expected_id = operator.id().to_string();
    assert_eq!(
        fact.payload()["operator_badge_id"],
        json!(expected_id),
        "the emitted fact carries operator_badge_id = the verified operator id, \
         NOT the token (DR-032 §Decision 5; §12/I2)"
    );
    assert_eq!(
        fact.payload()["reason"],
        json!("runaway spend"),
        "the operator-supplied reason rides the fact (I6 interrogability)"
    );
}

/// CRITERION 4 (interrogability, I6) — after a `kill_run` fact folds, `gate_explain`
/// surfaces the run as human-killed with the operator attribution, DISTINCT from a
/// daemon-timeout stop. Here the run's LATEST verdict-bearing surface is the
/// operator kill: `gate_explain` (or `debrief`) must be able to name the operator
/// id + reason. ASSERT-RED now — `kill_run` does not exist, so nothing folds.
///
/// NOTE FOR THE IMPLEMENTER: this pins that the operator attribution is
/// INTERROGABLE after the kill — the exact accessor may be `gate_explain`
/// surfacing a `killed_by`/`kill_reason`, or a dossier read
/// (`rezidnt://run/<ulid>/dossier`) exposing the folded fields (CRITERION 1's
/// `AgentRunState.killed_by`/`.kill_reason`). Do NOT weaken the "distinct from a
/// daemon stop" requirement; if `gate_explain`'s current shape has no home for
/// kill attribution, the dossier resource is the honest surface — assert against
/// whichever the implementer wires, but it MUST expose the operator id + reason.
#[tokio::test]
async fn killed_run_is_interrogable_as_operator_attributed() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    let _ = util::tool_call(
        &core,
        5,
        "kill_run",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "reason": "runaway spend"
        }),
    )
    .await;

    // Read the run's folded dossier (the I3 derived state, resources/read). The
    // operator attribution must be present and distinct from an unattributed
    // (daemon) stop — a reader can tell a human kill from a timeout.
    let dossier = util::call_ok(
        &core,
        6,
        "resources/read",
        json!({ "uri": format!("rezidnt://run/{RUN}/dossier") }),
    )
    .await;
    let text = dossier["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("dossier read must carry contents[0].text: {dossier:#}"));
    let state: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("dossier text must be JSON ({e})"));
    let expected_id = operator.id().to_string();
    assert_eq!(
        state["killed_by"],
        json!(expected_id),
        "the folded dossier surfaces the operator badge id — a human-killed run is \
         interrogable and DISTINCT from a daemon-timeout stop (I6, DR-032 §Decision 5)"
    );
    assert_eq!(
        state["kill_reason"],
        json!("runaway spend"),
        "the operator reason is interrogable on the dossier (I6)"
    );
}

/// I5 SERVED-SURFACE (the /debrief gap) — `kill_run` is dispatched by `tools_call`
/// but must ALSO be advertised by `tools/list`, or an MCP client cannot DISCOVER
/// it (I5: every capability is an MCP tool BEFORE a keybinding; a tool the client
/// can call but not list is half-served). This pins that `tools/list` CONTAINS a
/// `kill_run` entry whose `inputSchema` is a REAL object schema, not `{}` — the
/// §9 BINDING no-drift rule (the schema is generated from a `rezidnt_types::mcp`
/// shape, exactly like `open_project`/`spawn_agent`, never hand-stubbed).
///
/// ASSERT-RED now: `tools_list()` lists open_project/spawn_agent/
/// request_permission/gate_explain/tail_events but NOT kill_run, so `find_tool`
/// panics on the missing entry. This is a served-surface gap, not a dispatch gap.
#[tokio::test]
async fn tools_list_advertises_kill_run_with_a_real_schema() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let tool = util::find_tool(&tools, "kill_run");

    let schema = &tool["inputSchema"];
    assert!(
        schema.is_object(),
        "kill_run's inputSchema must be an object schema (§9 no-drift: generated \
         from a rezidnt_types::mcp shape), got {schema:#}"
    );
    let props = &schema["properties"];
    assert!(
        props.is_object() && !props.as_object().unwrap().is_empty(),
        "kill_run's inputSchema must be a REAL schema with properties (the `run` \
         it takes), not an empty `{{}}` stub — §9 BINDING no-drift; got {schema:#}"
    );
}
