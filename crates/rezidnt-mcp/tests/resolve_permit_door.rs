//! DR-033 slice-2 (operator-resolve-escalation) ORACLE — CRITERION 3 (the
//! operator-only `resolve_permit` door + DAEMON-DERIVE emit) and CRITERION 4
//! (tools_list advertisement). Mirrors the DR-032 slice-1 `kill_run_door.rs`
//! pattern for the door legs: the operator badge is required, an agent macaroon
//! is refused, the badge is checked BEFORE any side effect, and a refused
//! resolve emits NO fact.
//!
//! ## REVISION (2026-07-22, /debrief FAIL close) — conform to the ratified DR
//! DR-033 §Design ratified the tool as `{ run, request_id, decision, reason? }`
//! — NO `action`, NO `target` operator inputs. The DAEMON DERIVES `(action,
//! target)` from the log by `request_id` and stamps them on the fact. The
//! shipped CLI hardcoded `action="tool.invoke"` + `target={}`, so the emitted
//! `permit.resolved` carried an EMPTY `target.tool` and the PDP action-identity
//! match could NEVER fire — the end-to-end path was broken. This board REPLACES
//! the action/target-as-input expectations with the DERIVE contract: the tool
//! looks the escalation up by `request_id` and supplies the REAL, matchable
//! `(action, target)` the CLI could not. This is NOT a weakening — it PINS a
//! stronger contract (a real target must land, sourced from the log).
//!
//! ## What DR-033 §Design mandates (the door)
//! `resolve_permit { badge, run, request_id, decision: "allow"|"deny", reason? }`
//! sits behind the operator-only door (reuse `check_operator_badge`): the
//! OPERATOR badge is REQUIRED and the AGENT-MACAROON path is REJECTED —
//! resolving an escalation is an operator action, not an agent self-action
//! (exact mirror of `kill_run`, DR-032 §1). A macaroon presented to
//! `resolve_permit` → refused, no fact.
//!
//! ## Side-effect discipline (I3, the kill_run precedent)
//! A REFUSED resolve (no badge / macaroon / unknown token) emits NO
//! `permit.resolved` fact — if it isn't on the log, it didn't happen. An ADMITTED
//! resolve for a KNOWN escalation emits EXACTLY ONE `permit.resolved` fact
//! carrying `operator_badge_id` = the verified operator id (NEVER the token,
//! §12/I2) + the decision/reason + the DAEMON-DERIVED action/target, through the
//! daemon single writer (I3).
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!   - a new tool name `resolve_permit` dispatched by `tools_call` (today:
//!     unknown tool → `-32602`, which is why the admit/emit tests are ASSERT-RED
//!     now, not merely compile-red).
//!   - a NEW `McpSubstrate::resolve_permit` is NOT required: the door + a log
//!     fold (to derive action/target by request_id) + the single writer. So the
//!     fact SHAPE is judged with the DEFAULT core (no substrate).
//!   - args are `{ badge, run, request_id, decision, reason? }` — the operator
//!     supplies NO action and NO target (the DEFECT was making them inputs).
//!   - the core, on an admitted resolve for a KNOWN escalation, folds the run,
//!     looks the escalation up by `request_id`, and emits `permit.resolved` with
//!     the DERIVED `action` + `target` (from the folded `permit.requested`),
//!     `operator_badge_id` = the `check_operator_badge`-verified id, `decision` =
//!     the call's `decision` (the human input verb `allow`/`deny`, never
//!     coerced), `request_id` = the escalation it answers. `reason` rides when
//!     supplied.
//!   - an admitted resolve for an UNKNOWN `request_id` (no prior
//!     requested/escalated folded) is REFUSED — the daemon cannot derive an
//!     action/target, so it emits NO bogus fact (honesty, I3/I6).
//!   - `codes::BADGE_REQUIRED` for a missing badge; `codes::BADGE_INVALID` for a
//!     rejected macaroon / unknown token (the same refusal vocabulary as kill_run).
//!   - `tools/list` advertises `resolve_permit` with a real object inputSchema
//!     from `schema_for!(rezidnt_types::mcp::ResolvePermitArgs)` (§9 no-drift) —
//!     the TRIMMED shape (badge, run, request_id, decision, reason?), NO
//!     action/target property.
//!
//! RED MODE: ASSERT-RED (tool absent → `-32602`, OR present-but-expects-
//! action/target) for the admit/emit/tools_list tests; COMPILE-RED is NOT
//! expected — `ResolvePermitArgs` exists. The derive test and the unknown-req
//! refusal are ASSERT-RED against the CURRENT impl, which takes action/target as
//! args and emits whatever the operator supplied (here: nothing / defaults) with
//! no request_id lookup and no unknown-request refusal.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{BadgeBook, McpCore};
use rezidnt_run::badge::{Badge, Caveat, Macaroon, RootKey};
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::{Value, json};
use ulid::Ulid;

const RUN: &str = "01DR033RES0LVED00R0000000R1";
const ESCALATED_REQ: &str = "01DR033RES0LVEDESCREQ0000R1";
const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const T_LATE: &str = "2026-07-23T00:00:00Z";

fn root() -> RootKey {
    RootKey::from_bytes([9u8; 32])
}

fn ev(subject: &str, payload: Value) -> Event {
    Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new(subject),
        Ulid::new(),
        None,
        1,
        payload,
    )
    .expect("test event under 32KiB")
}

/// Seed the escalation history the daemon DERIVES `(action, target)` from: the
/// request carrying the real `action`/`target`, then the escalation routed to a
/// human. `resolve_permit { request_id: ESCALATED_REQ }` must fold this and
/// stamp the DERIVED action/target on the emitted `permit.resolved`. This is the
/// golden inline fixture — the whole derive input is visible.
fn seed_escalation(core: &McpCore) {
    core.fabric()
        .publish(ev(
            "agent.spawned",
            json!({"run": RUN, "agent": "impl", "harness": "claude-code"}),
        ))
        .expect("publish spawned");
    core.fabric()
        .publish(ev(
            "permit.requested",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "action": "tool.invoke", "target": {"tool": "Bash"}
            }),
        ))
        .expect("publish requested");
    core.fabric()
        .publish(ev(
            "permit.escalated",
            json!({
                "run": RUN, "request_id": ESCALATED_REQ,
                "reason": "no policy configured — routed to a human"
            }),
        ))
        .expect("publish escalated");
}

/// A well-formed agent macaroon — used to prove the resolve door REJECTS the
/// macaroon path EVEN when the macaroon is otherwise verifiable (DR-033 §Design:
/// resolving is operator-only, mirrors DR-032 §1 for kill).
fn agent_macaroon(root: &RootKey) -> Macaroon {
    Macaroon::mint(
        root,
        "run-01DR033RESOLVEMACAROON000",
        vec![
            Caveat::Workspace {
                workspace: WS.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into(), "resolve".into()],
            },
            Caveat::Expiry {
                not_after: T_LATE.into(),
            },
        ],
    )
}

/// A core with the operator badge admitted and the daemon root key wired (so a
/// macaroon COULD verify — proving the door rejects it on POLICY, not because
/// the core is keyless). No substrate: a resolve is a door + a fold + a fact emit.
fn operator_core(operator: &Badge) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(operator);
    let core = McpCore::new(fabric, book).with_root_key(root());
    (dir, Arc::new(core))
}

/// CRITERION 3 (admit) — `resolve_permit` with a valid OPERATOR badge, for a
/// KNOWN escalation, is admitted (the door passes, the fact emits). The operator
/// supplies NO action/target (the daemon derives them). ASSERT-RED now.
#[tokio::test]
async fn resolve_permit_with_operator_badge_is_admitted() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);
    let result = util::tool_call(
        &core,
        1,
        "resolve_permit",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "request_id": ESCALATED_REQ,
            "decision": "allow",
            "reason": "operator approved"
        }),
    )
    .await;
    assert_eq!(
        result["isError"],
        json!(false),
        "an operator-badged resolve_permit for a known escalation is admitted (not a \
         refusal) — got {result:#}"
    );
}

/// CRITERION 3 (no badge) — `resolve_permit` with NO badge → `BADGE_REQUIRED`,
/// and NO `permit.resolved` fact lands (refuse before effect, I3). ASSERT-RED.
#[tokio::test]
async fn resolve_permit_without_badge_is_refused_no_side_effect() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);
    let result = util::tool_call(
        &core,
        2,
        "resolve_permit",
        json!({
            "run": RUN, "request_id": ESCALATED_REQ, "decision": "allow"
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_REQUIRED);
    assert!(
        util::log_events(&core)
            .iter()
            .all(|e| e.subject.as_str() != "permit.resolved"),
        "a badge-less resolve emits no permit.resolved fact (refuse before effect, I3)"
    );
}

/// CRITERION 3 (macaroon rejected — the operator-only heart) — `resolve_permit`
/// with a VALID AGENT MACAROON is REFUSED: resolving is an operator action, not
/// agent self-action. The macaroon is well-formed and would verify on this core
/// (root key wired), so the refusal is a POLICY refusal, not a verify failure.
/// NO `permit.resolved` fact lands. ASSERT-RED.
#[tokio::test]
async fn resolve_permit_with_agent_macaroon_is_refused_operator_only() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);
    let m = agent_macaroon(&root());
    let result = util::tool_call(
        &core,
        3,
        "resolve_permit",
        json!({
            "badge": m.to_wire(),
            "run": RUN, "request_id": ESCALATED_REQ, "decision": "allow"
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(
        util::log_events(&core)
            .iter()
            .all(|e| e.subject.as_str() != "permit.resolved"),
        "an agent macaroon cannot resolve (DR-033 §Design); the log stays free of a \
         resolution fact"
    );
}

/// CRITERION 3 (DAEMON-DERIVE emit — THE key revised assertion, closes the
/// /debrief FAIL) — an admitted `resolve_permit` for a KNOWN escalation emits
/// EXACTLY ONE `permit.resolved` fact whose `action` + `target` are DERIVED from
/// the seeded `permit.requested` (looked up by `request_id`), NOT supplied by
/// the operator. This proves the daemon stamps a REAL, matchable target — the
/// exact thing the CLI's hardcoded `target={}` could not.
///
/// The operator call carries NO action and NO target. The emitted fact must
/// still carry `action = "tool.invoke"` and `target = {"tool":"Bash"}` — proof
/// the source moved from operator-arg to daemon-derive. ASSERT-RED now: the
/// current impl reads action/target from the args (absent here → empty/default),
/// with no request_id lookup, so the derived-target assertion fails.
#[tokio::test]
async fn admitted_resolve_derives_action_target_and_emits_one_attributed_fact() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    seed_escalation(&core);
    let _ = util::tool_call(
        &core,
        4,
        "resolve_permit",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "request_id": ESCALATED_REQ,
            "decision": "allow",
            "reason": "operator approved after review"
        }),
    )
    .await;

    let resolved: Vec<_> = util::log_events(&core)
        .into_iter()
        .filter(|e| e.subject.as_str() == "permit.resolved")
        .collect();
    assert_eq!(
        resolved.len(),
        1,
        "an admitted resolve emits EXACTLY ONE permit.resolved fact (single writer, I3)"
    );
    let fact = &resolved[0];
    assert_eq!(
        fact.payload()["run"],
        json!(RUN),
        "the fact keys on the run"
    );
    assert_eq!(
        fact.payload()["request_id"],
        json!(ESCALATED_REQ),
        "the escalated request_id it answers rides the fact (the audit correlation)"
    );
    assert_eq!(
        fact.payload()["action"],
        json!("tool.invoke"),
        "the action is DERIVED from the escalation looked up by request_id (NOT an \
         operator input) — half the (run, action, target) match key"
    );
    assert_eq!(
        fact.payload()["target"],
        json!({"tool": "Bash"}),
        "the target is DERIVED from the escalation's permit.requested.target — a REAL, \
         matchable descriptor, NOT the empty `{{}}` the CLI hardcoded (this is the \
         /debrief FAIL being closed: the PDP action-identity match can now fire)"
    );
    assert_eq!(
        fact.payload()["decision"],
        json!("allow"),
        "the human decision rides as the input verb `allow`, NEVER coerced to \
         `granted` (I6, DR-033 §Decision — the coercion is the PDP's, on the next ask)"
    );
    let expected_id = operator.id().to_string();
    assert_eq!(
        fact.payload()["operator_badge_id"],
        json!(expected_id),
        "the fact carries operator_badge_id = the verified operator id, NOT the \
         token (§12/I2; the exact leak-discipline as agent.signaled.operator_badge_id)"
    );
    assert_eq!(
        fact.payload()["reason"],
        json!("operator approved after review"),
        "the operator-supplied reason rides the fact (I6 interrogability)"
    );
}

/// CRITERION 3 (honesty — unknown request_id is REFUSED on the LOOKUP) — an
/// admitted (operator-badged) `resolve_permit` for a `request_id` with NO prior
/// requested/escalated in the run's folded state cannot derive an
/// `(action, target)`. The daemon MUST refuse and emit NO `permit.resolved`
/// fact — it never fabricates a bogus action/target for an unknown escalation
/// (I3/I6: if the daemon can't derive it, it doesn't happen).
///
/// HONEST-RED: this call supplies `action`/`target` so the CURRENT impl (which
/// reads them as args and does NO request_id lookup) ADMITS it and emits a fact
/// — the exact wrong behavior. Under the target derive impl those fields are
/// ignored (the trimmed args carry no action/target) and the daemon refuses on
/// the missing-escalation LOOKUP. Two asserts make this red-for-the-right-reason
/// now AND correct under the target impl: (a) it must refuse and emit no fact;
/// (b) the refusal must NOT be `ARGS_INVALID` — a trimmed-args parse failure is
/// not the honest "unknown escalation" refusal. Against the current impl the
/// admit-and-emit fails (a), so this is ASSERT-RED.
#[tokio::test]
async fn resolve_permit_for_unknown_request_is_refused_no_fact() {
    let operator = Badge::mint().expect("mint operator badge");
    let (_dir, core) = operator_core(&operator);
    // NO seed_escalation — the run has no requested/escalated for this request_id.
    // action/target are supplied only to defeat the CURRENT impl's args.invalid
    // shortcut so the red is the ADMIT-AND-EMIT (the real defect), not a parse
    // refusal; the target derive impl ignores them and refuses on the lookup.
    let result = util::tool_call(
        &core,
        5,
        "resolve_permit",
        json!({
            "badge": operator.token_hex(),
            "run": RUN,
            "request_id": ESCALATED_REQ,
            "action": "tool.invoke",
            "target": {"tool": "Bash"},
            "decision": "allow"
        }),
    )
    .await;
    assert_eq!(
        result["isError"],
        json!(true),
        "an operator resolve for an UNKNOWN request_id is REFUSED — the daemon cannot \
         derive an action/target and must not fabricate one (I3/I6); got {result:#}"
    );
    if result["isError"] == json!(true) {
        let payload = util::tool_payload(&result);
        assert_ne!(
            payload["code"],
            json!(rezidnt_mcp::codes::ARGS_INVALID),
            "the refusal must be the honest UNKNOWN-ESCALATION refusal (the request_id has \
             no folded requested/escalated to derive from), NOT an ARGS_INVALID parse \
             failure — got {payload:#}"
        );
    }
    assert!(
        util::log_events(&core)
            .iter()
            .all(|e| e.subject.as_str() != "permit.resolved"),
        "a resolve for an unknown escalation emits NO permit.resolved fact — no bogus \
         action/target lands on the log (I3)"
    );
}

/// CRITERION 4 (I5 served surface) — `resolve_permit` is advertised by
/// `tools/list` with a REAL object inputSchema (properties, not `{}`), generated
/// from `rezidnt_types::mcp::ResolvePermitArgs` (§9 BINDING no-drift). Slice 1
/// MISSED this for `kill_run` at first (an I5 fail); pin it NOW.
///
/// ASSERT-RED now: `tools_list()` does not list `resolve_permit`, so `find_tool`
/// panics on the missing entry.
#[tokio::test]
async fn tools_list_advertises_resolve_permit_with_a_real_schema() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let tool = util::find_tool(&tools, "resolve_permit");

    let schema = &tool["inputSchema"];
    assert!(
        schema.is_object(),
        "resolve_permit's inputSchema must be an object schema (§9 no-drift), got {schema:#}"
    );
    let props = &schema["properties"];
    assert!(
        props.is_object() && !props.as_object().unwrap().is_empty(),
        "resolve_permit's inputSchema must be a REAL schema with properties (badge, \
         run, request_id, decision, ...), not an empty `{{}}` stub — §9 BINDING no-drift; \
         got {schema:#}"
    );
    // The DERIVE contract (DR-033 §Design): the operator does NOT supply
    // action/target — the daemon derives them. So the served schema must NOT
    // advertise `action`/`target` properties. This pins the trim, not just
    // non-emptiness. A drift here means the operator-input defect regressed.
    let props_obj = props.as_object().unwrap();
    assert!(
        !props_obj.contains_key("action") && !props_obj.contains_key("target"),
        "resolve_permit's inputSchema must NOT carry `action`/`target` properties — \
         the daemon DERIVES them from the log by request_id (DR-033 §Design); operator \
         action/target inputs were the /debrief FAIL. got properties: {props:#}"
    );
    // The S3+ surface is unchanged — resolve_permit is ADDITIVE (mirrors the
    // request_permission additivity check).
    for name in [
        "open_project",
        "spawn_agent",
        "kill_run",
        "request_permission",
        "gate_explain",
        "tail_events",
    ] {
        util::find_tool(&tools, name);
    }
}

/// CRITERION 4 (no-drift) — the served `inputSchema` EQUALS `schema_for!` of the
/// `ResolvePermitArgs` shape, so surface and published type can never drift
/// (doc §9 BINDING). Once `ResolvePermitArgs` is trimmed to
/// `{ badge, run, request_id, decision, reason? }`, this asserts the served
/// schema equals the trimmed type's schema. ASSERT-RED until it is served.
#[tokio::test]
async fn resolve_permit_schema_is_generated_from_rezidnt_types() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let tool = util::find_tool(&tools, "resolve_permit");
    let expected =
        serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::ResolvePermitArgs)).unwrap();
    assert_eq!(
        tool["inputSchema"], expected,
        "served resolve_permit inputSchema must EQUAL schemars::schema_for! of the \
         rezidnt-types shape (no drift, doc §9)"
    );
}
