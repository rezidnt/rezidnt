//! DR-044 + DR-045 ORACLE (the `fan_out` door, the width cap, I2 response
//! cleanliness, honest partial failure) — DR-044 §Consequences guard (d) and the
//! response half of (e)/I2, plus the single guard DR-045 §Consequences owes.
//! Runs against a bare `McpCore` with a RECORDING substrate, so it is
//! deterministic and HOST-LINTABLE: no daemon, no process, no worktree.
//!
//! ## DR-045 re-cut (2026-07-24)
//!
//! This board was first cut while DR-044 left the door's badge-KIND semantics
//! undefined — the oracle flagged that gap as unanswerable and routed it to
//! `/dr`. DR-045 answered it: `fan_out` is LEAD-ONLY. It admits ONLY a verified
//! agent macaroon; an admitted DR-005 operator token is refused on POLICY with
//! `FAN_OUT_LEAD_ONLY`, deliberately distinct from `BADGE_INVALID` because an
//! operator badge is *valid*, just the wrong kind, and saying otherwise would be
//! an honesty regression (I6).
//!
//! Every admitted path here therefore presents a real lead MACAROON over a
//! ROOT-KEYED core — the badge kind the daemon actually injects. The original
//! cut presented an operator token on those paths and wired no root key, which
//! asserted the behavior DR-045 now forbids.
//!
//! Door order pinned by this board (DR-045 §Decision 3): `BADGE_REQUIRED` →
//! `FAN_OUT_LEAD_ONLY` → `BADGE_INVALID` → width cap → substrate, with zero
//! effect before any refusal.
//!
//! ## RED MODE
//!
//! COMPILE-RED on the seam types (`rezidnt_mcp::MAX_FAN_OUT_DEFAULT`,
//! `FanOutOutcome`, `McpSubstrate::fan_out`, `codes::FAN_OUT_TOO_WIDE`,
//! `codes::FAN_OUT_LEAD_ONLY`) and ASSERT-RED on dispatch (`fan_out` is an
//! unknown tool, so `tools_call` returns a `-32602` JSON-RPC error and
//! `util::tool_call`'s "expected a result" panic fires). Both are red for the
//! right reason: the tool and its seam do not exist.
//!
//! ## API surface this board PINS (implementer builds to EXACTLY this)
//!
//! A tool name `fan_out` dispatched by `tools_call`, taking
//! `rezidnt_types::mcp::FanOutArgs` (schema pinned by
//! `rezidnt-types/tests/fanout_schema_no_drift.rs`).
//!
//! `pub const MAX_FAN_OUT_DEFAULT: usize = 8;` in `rezidnt-mcp` — the DEFAULT
//! width cap of DR-044 §Decision 4, overridable per project via
//! `[orchestrator] max_fan_out` (a DEFAULT, revisable without a DR). Exposed as
//! a const so the cap is one number in one place and this test reads it rather
//! than hardcoding `8` twice.
//!
//! `pub const codes::FAN_OUT_TOO_WIDE: &str = "fan_out.too_wide";` — the
//! machine-readable whole-call refusal.
//!
//! `pub const codes::FAN_OUT_LEAD_ONLY: &str = "fan_out.lead_only";` — DR-045
//! §Decision 2, the badge-KIND policy refusal. Structurally a third door beside
//! `check_badge` and `check_operator_badge`, in the DR-032 shape.
//!
//! ```ignore
//! /// One task's outcome. Exactly one of `run` (spawned/deduped) or `code`
//! /// (refused) is present. Mirrors `KillAck`/`OpenAck`: a substrate ack type
//! /// owned by rezidnt-mcp, serialized into the tool result.
//! pub struct FanOutOutcome {
//!     pub agent: String,
//!     pub idempotency_key: String,
//!     pub run: Option<String>,
//!     pub code: Option<String>,
//!     pub message: Option<String>,
//! }
//!
//! trait McpSubstrate {
//!     /// DEFAULTED so every existing impl (the daemon bridge, the kill-only
//!     /// fake in kill_run_door.rs) compiles UNTOUCHED. Adding this method must
//!     /// not require editing a single existing test.
//!     fn fan_out(
//!         &self,
//!         workspace: String,
//!         lead_badge_id: String,
//!         tasks: Vec<rezidnt_types::mcp::FanOutTask>,
//!     ) -> BoxFuture<Result<Vec<FanOutOutcome>, ToolRefusal>> {
//!         Box::pin(async {
//!             Err(ToolRefusal::new(codes::SUBSTRATE_UNAVAILABLE, "no substrate"))
//!         })
//!     }
//! }
//! ```
//!
//! `lead_badge_id` is the id `check_badge` verified, NOT a caller-declared field
//! (DR-044 §Decision 1/2b: the lead is the identity the door established). The
//! substrate resolves the lead's RUN from it by folding
//! `agent.spawned.badge_id == lead_badge_id` — log-derived, no session object (I3).
//!
//! The tool result payload is `{"outcomes": [ ...one per task, in call order... ]}`.
//!
//! ## What this board does and does not prove
//!
//! It proves the CORE's obligations: the cap refuses the whole call before the
//! substrate is ever reached, the badge door runs first, and a partial failure is
//! passed through as a vector rather than collapsed into one error or silently
//! rolled back. It does NOT prove that a real worktree conflict produces a
//! refused task; that is the daemon's job and lives in
//! `bins/rezidentd/tests/fan_out_live_e2e.rs`. Said plainly so nobody reads the
//! fake substrate's scripted outcomes as evidence about the registry.
//!
//! ## Ontology posture
//!
//! This file emits NO `worktree.allocated` and asserts only on that subject's
//! ABSENCE. It has ZERO dependence on the `worktree.allocated.allocator` value
//! vocabulary the parallel warden `/subject` session is widening (DR-044
//! §Decision 6).

mod util;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{
    BadgeBook, BoxFuture, FanOutOutcome, KillAck, McpCore, McpSubstrate, OpenAck, PermitConfig,
    ToolRefusal, codes,
};
use rezidnt_run::badge::{Badge, Caveat, Macaroon, RootKey};
use rezidnt_types::mcp::FanOutTask;
use serde_json::{Value, json};

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// The run the lead macaroon is minted over — its identifier, the value the
/// daemon uses when it mints a run's base badge (`bins/rezidentd/src/runs.rs:752`).
const LEAD_RUN: &str = "01DR045LEADRVN000000000001";

/// Deterministic run ULIDs the fake substrate hands back, one per admitted task.
const RUN_TEMPLATE: [&str; 8] = [
    "01DR044FANOVT0000000000RA1",
    "01DR044FANOVT0000000000RB1",
    "01DR044FANOVT0000000000RC1",
    "01DR044FANOVT0000000000RD1",
    "01DR044FANOVT0000000000RE1",
    "01DR044FANOVT0000000000RF1",
    "01DR044FANOVT0000000000RG1",
    "01DR044FANOVT0000000000RH1",
];

/// A fake substrate that RECORDS every `fan_out` call and returns one scripted
/// outcome per task WITHOUT allocating a worktree or spawning a process. Its
/// only job on this board is to answer "was the substrate reached at all?" — the
/// width cap's whole claim is that it is NOT, on an over-wide call.
///
/// `conflict_at` scripts one task index as a worktree-conflict refusal so the
/// partial-failure pass-through can be judged; every other task gets a run.
#[derive(Default)]
struct RecordingFanOutSubstrate {
    calls: AtomicUsize,
    tasks_seen: AtomicUsize,
    conflict_at: Option<usize>,
}

impl RecordingFanOutSubstrate {
    fn with_conflict_at(index: usize) -> Self {
        Self {
            conflict_at: Some(index),
            ..Self::default()
        }
    }
}

impl McpSubstrate for RecordingFanOutSubstrate {
    fn open_project(&self, _spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                codes::SUBSTRATE_UNAVAILABLE,
                "fan-out-only test substrate",
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
                codes::SUBSTRATE_UNAVAILABLE,
                "fan-out-only test substrate",
            ))
        })
    }

    fn permit_config_for(&self, _run: String) -> BoxFuture<Option<PermitConfig>> {
        Box::pin(async { None })
    }

    fn kill_run(&self, _run: String) -> BoxFuture<Result<KillAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                codes::SUBSTRATE_UNAVAILABLE,
                "fan-out-only test substrate",
            ))
        })
    }

    /// The DR-044 seam. Reaching this AT ALL is the observable the width-cap
    /// tests assert the absence of.
    fn fan_out(
        &self,
        _workspace: String,
        _lead_badge_id: String,
        tasks: Vec<FanOutTask>,
    ) -> BoxFuture<Result<Vec<FanOutOutcome>, ToolRefusal>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.tasks_seen.fetch_add(tasks.len(), Ordering::SeqCst);
        let conflict_at = self.conflict_at;
        Box::pin(async move {
            Ok(tasks
                .into_iter()
                .enumerate()
                .map(|(i, task)| {
                    if conflict_at == Some(i) {
                        FanOutOutcome {
                            agent: task.agent,
                            idempotency_key: task.idempotency_key,
                            run: None,
                            code: Some("worktree.conflict".to_string()),
                            message: Some("another allocator holds this worktree".to_string()),
                        }
                    } else {
                        FanOutOutcome {
                            agent: task.agent,
                            idempotency_key: task.idempotency_key,
                            run: Some(RUN_TEMPLATE[i % RUN_TEMPLATE.len()].to_string()),
                            code: None,
                            message: None,
                        }
                    }
                })
                .collect())
        })
    }
}

/// The daemon root key this board's macaroons are minted and verified against.
/// Fixed bytes so the board is deterministic (mirrors `kill_run_door::root`).
fn root() -> RootKey {
    RootKey::from_bytes([45u8; 32])
}

/// The LEAD's own badge — an agent MACAROON, the badge kind the daemon injects
/// under `REZIDNT_BADGE` and the ONLY kind `fan_out` admits (DR-045 §Decision 1).
/// Narrowed by `Verb{spawn}`, the verb DR-044 §Decision 1 derives for this tool,
/// so the caveat is genuinely evaluated rather than absent.
///
/// Deliberately carries NO `Expiry` caveat. The door falls back to wall-clock
/// `now` when the caller sends none (`crates/rezidnt-mcp/src/lib.rs`), so a
/// dated expiry would turn this board into a time bomb that goes red on a
/// calendar date rather than on a behavior change. Expiry evaluation is already
/// pinned by `badge_macaroon_verify.rs`; it is not this board's subject.
fn lead_macaroon() -> Macaroon {
    Macaroon::mint(
        &root(),
        LEAD_RUN,
        vec![
            Caveat::Workspace {
                workspace: WS.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into()],
            },
        ],
    )
}

/// The wire form a lead presents on a `fan_out` call.
fn lead_badge() -> String {
    lead_macaroon().to_wire()
}

/// A core carrying BOTH badge kinds so the DR-045 door is judged honestly:
/// the DR-005 operator token is ADMITTED in the `BadgeBook` (so its refusal is a
/// POLICY refusal, not a verification failure) AND the daemon root key is wired
/// (so a lead macaroon genuinely verifies on this same core). Over a fresh temp
/// log, so side effects and their ABSENCE are readable.
fn core_with(
    operator: &Badge,
    substrate: Arc<RecordingFanOutSubstrate>,
) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(operator);
    let core = McpCore::new(fabric, book)
        .with_root_key(root())
        .with_substrate(substrate);
    (dir, Arc::new(core))
}

fn tasks(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(|i| json!({"agent": format!("sub-{i}"), "idempotency_key": format!("dr044-key-{i}")}))
            .collect(),
    )
}

/// The two subjects a fan-out would produce if it took ANY effect. Asserting
/// their absence — not just the refusal string — is the whole point of guard
/// (d): an implementation that allocated first and refused second would pass a
/// code-only check.
fn assert_no_fan_out_effect(core: &McpCore, context: &str) {
    let log = util::log_events(core);
    for subject in ["worktree.allocated", "agent.spawned"] {
        assert!(
            log.iter().all(|e| e.subject.as_str() != subject),
            "{context}: a refused fan_out must emit NO `{subject}` — the cap refuses the WHOLE \
             call BEFORE any allocation or spawn (DR-044 §Decision 4). A refusal code alone \
             would not catch an implementation that allocated first and refused second. \
             Log subjects: {:?}",
            log.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>()
        );
    }
}

/// CRITERION (d), DR-044 §Consequences — an OVER-WIDE `fan_out` is refused as a
/// WHOLE CALL with `FAN_OUT_TOO_WIDE`, and takes ZERO effect: the substrate is
/// never reached, and no `worktree.allocated` / `agent.spawned` lands on the log.
///
/// The cap is DR-044 §Decision 4's SINGLE door: the plan's `rezidnt-supervise`
/// backoff/circuit-breaker does not exist in this tree, so this is the only
/// backpressure the slice has. That is why the absence-of-effect leg is
/// load-bearing rather than belt-and-braces.
#[tokio::test]
async fn over_wide_fan_out_is_refused_whole_call_with_no_effect() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let over = rezidnt_mcp::MAX_FAN_OUT_DEFAULT + 1;
    let result = util::tool_call(
        &core,
        1,
        "fan_out",
        json!({
            "badge": lead_badge(),
            "workspace": WS,
            "tasks": tasks(over),
        }),
    )
    .await;

    util::assert_tool_refusal(&result, codes::FAN_OUT_TOO_WIDE);

    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "the substrate is NEVER reached on an over-wide call — the cap refuses before any \
         allocation or spawn (DR-044 §Decision 4)"
    );
    assert_no_fan_out_effect(&core, "over-wide call");
}

/// CRITERION (d), boundary non-vacuity — a call at EXACTLY the cap is NOT
/// refused for width. Without this, an implementation that refused every
/// `fan_out` unconditionally would satisfy the test above; the cap has to be a
/// strict `>`, not a `>=` and not a blanket refusal.
#[tokio::test]
async fn a_fan_out_at_exactly_the_cap_is_admitted() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let at_cap = rezidnt_mcp::MAX_FAN_OUT_DEFAULT;
    let result = util::tool_call(
        &core,
        2,
        "fan_out",
        json!({
            "badge": lead_badge(),
            "workspace": WS,
            "tasks": tasks(at_cap),
        }),
    )
    .await;

    assert_ne!(
        result["isError"],
        json!(true),
        "a call at exactly `max_fan_out` is ADMITTED — the cap is a strict `>` (DR-044 \
         §Decision 4 DEFAULT 8 means 8 is allowed): {result:#}"
    );
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        1,
        "the admitted call reaches the substrate exactly once — one call, N tasks"
    );
    assert_eq!(
        substrate.tasks_seen.load(Ordering::SeqCst),
        at_cap,
        "all {at_cap} tasks ride the ONE substrate call (DR-044 §Decision 1 rejects N separate \
         delegates)"
    );
}

/// CRITERION (d), ORDERING — the cap is checked AFTER the badge door. An
/// over-wide call with NO badge is refused `BADGE_REQUIRED`, not
/// `FAN_OUT_TOO_WIDE`: an unauthenticated caller must not learn the cap, and the
/// door is still the first thing that runs (doc §12, DR-044 §Decision 4 "after
/// the badge door and before any effect"). Still zero effect.
#[tokio::test]
async fn the_badge_door_runs_before_the_width_cap() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let result = util::tool_call(
        &core,
        3,
        "fan_out",
        json!({
            "workspace": WS,
            "tasks": tasks(rezidnt_mcp::MAX_FAN_OUT_DEFAULT + 1),
        }),
    )
    .await;

    util::assert_tool_refusal(&result, codes::BADGE_REQUIRED);
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "a badge-less call never reaches the substrate"
    );
    assert_no_fan_out_effect(&core, "badge-less over-wide call");
}

/// Extract the per-task outcome vector from an admitted `fan_out` result.
fn outcomes(result: &Value) -> Vec<Value> {
    let payload = util::tool_payload(result);
    payload["outcomes"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("fan_out returns a per-task outcome VECTOR under `outcomes`: {payload:#}")
        })
        .clone()
}

/// CRITERION — PARTIAL FAILURE IS HONEST (DR-044 §Decision 1: "there is no
/// all-or-nothing rollback — spawns are not transactional and pretending
/// otherwise would be a lie"). A mid-fan-out task failure does NOT collapse the
/// call into a single error and does NOT erase the runs that already spawned.
/// The response is a per-task vector, in call order, one entry per task.
///
/// Pinned so nobody later "fixes" this into a fake transaction. This asserts the
/// CORE's pass-through obligation; the fake substrate scripts the conflict, so
/// it is not evidence about the real registry (see the module header).
#[tokio::test]
async fn a_partial_failure_reports_per_task_and_does_not_roll_back() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::with_conflict_at(1));
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let result = util::tool_call(
        &core,
        4,
        "fan_out",
        json!({
            "badge": lead_badge(),
            "workspace": WS,
            "tasks": tasks(3),
        }),
    )
    .await;

    assert_ne!(
        result["isError"],
        json!(true),
        "a PARTIAL failure is not a call failure — the surviving subs stand and are reported \
         (DR-044 §Decision 1/3): {result:#}"
    );

    let outcomes = outcomes(&result);
    assert_eq!(
        outcomes.len(),
        3,
        "one outcome per task, in call order — the vector IS the report: {outcomes:#?}"
    );
    assert_eq!(
        outcomes[0]["idempotency_key"],
        json!("dr044-key-0"),
        "outcomes are correlated to their tasks by idempotency key, in call order: {outcomes:#?}"
    );

    // Task 1 conflicted: REFUSED, carrying a code — and NO run, because no run
    // was minted (DR-044 §Decision 3: nothing folds as `fail`, there is nothing
    // to fail).
    assert_eq!(
        outcomes[1]["code"],
        json!("worktree.conflict"),
        "the conflicted task's outcome carries the machine-readable conflict code: {outcomes:#?}"
    );
    assert!(
        outcomes[1].get("run").is_none() || outcomes[1]["run"].is_null(),
        "a conflicted task mints NO run — it is REFUSED, never a failed sub (I6, DR-044 \
         §Decision 3): {outcomes:#?}"
    );

    // The other two stand — no rollback.
    for i in [0usize, 2] {
        assert!(
            outcomes[i]["run"].as_str().is_some(),
            "task {i} spawned and its run is still reported: a mid-fan-out failure does NOT \
             roll back already-spawned subs (DR-044 §Decision 1): {outcomes:#?}"
        );
        assert!(
            outcomes[i].get("code").is_none() || outcomes[i]["code"].is_null(),
            "a spawned task carries no refusal code: {outcomes:#?}"
        );
    }
}

/// CRITERION — I2 FOLD-BACK CLEANLINESS (DR-044 §Decision 5, stated there as "a
/// criterion, not a guideline"). The `fan_out` response carries run ULIDs and
/// refusal codes ONLY. No sub diff, no dossier, no transcript bytes ride the
/// tool response; sub work folds back through the existing per-run CAS-ref paths
/// and the lead learns outcomes by READING `orchestration_graph`.
#[tokio::test]
async fn the_fan_out_response_carries_no_sub_content() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::with_conflict_at(1));
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let result = util::tool_call(
        &core,
        5,
        "fan_out",
        json!({
            "badge": lead_badge(),
            "workspace": WS,
            "tasks": tasks(2),
        }),
    )
    .await;

    // Each outcome's keys are a CLOSED set. A new key is how sub content would
    // first appear, so the set is pinned rather than spot-checked.
    for outcome in outcomes(&result) {
        let object = outcome
            .as_object()
            .unwrap_or_else(|| panic!("each outcome is an object: {outcome:#}"));
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys.retain(|k| !matches!(*k, "agent" | "idempotency_key" | "run" | "code" | "message"));
        assert!(
            keys.is_empty(),
            "a fan_out outcome carries ONLY {{agent, idempotency_key, run?, code?, message?}} — \
             no diff, dossier, or transcript bytes ride the control plane (I2, DR-044 \
             §Decision 5). Unexpected keys: {keys:?} in {outcome:#}"
        );
    }

    // Belt-and-braces over the WHOLE serialized response, so content smuggled
    // outside `outcomes` is caught too.
    let serialized = result.to_string();
    for banned in ["diff", "dossier", "transcript", "patch", "stdout"] {
        assert!(
            !serialized.contains(banned),
            "the fan_out response must carry no sub content — found {banned:?} in {serialized}"
        );
    }
}

// --- DR-045: fan_out is lead-only -------------------------------------------

/// CRITERION (DR-045 §Consequences, the one guard that record owes) — a
/// `fan_out` call carrying a VALID DR-005 operator badge is refused
/// `FAN_OUT_LEAD_ONLY` and emits NO `worktree.allocated` and NO `agent.spawned`.
///
/// The operator token here is genuinely admitted in this core's `BadgeBook`, so
/// the refusal is a POLICY refusal on badge KIND, not a verification failure —
/// the same shape as DR-032's operator-only `kill_run`, inverted. Fan-out is a
/// run-scoped capability: an operator badge maps to no run, so there would be no
/// lead to key `permit.delegated` on (DR-044 §Decision 2b) and the graph would
/// gain an unparented sub.
///
/// NON-VACUITY (named by DR-045 §Consequences): the SAME core, the SAME
/// substrate, the SAME task list, presented with a real lead MACAROON, is
/// ADMITTED and reaches the substrate. Without this leg a door that refused
/// everything would satisfy the assertion above.
#[tokio::test]
async fn an_operator_badge_is_refused_lead_only_and_a_lead_macaroon_is_admitted() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    // --- the refusal leg: a VALID operator badge, wrong KIND ---------------
    let refused = util::tool_call(
        &core,
        6,
        "fan_out",
        json!({
            "badge": operator.token_hex(),
            "workspace": WS,
            "tasks": tasks(2),
        }),
    )
    .await;

    util::assert_tool_refusal(&refused, codes::FAN_OUT_LEAD_ONLY);
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "an operator-badged fan_out never reaches the substrate — the door refuses before any \
         allocation or spawn (DR-045 §Decision 3)"
    );
    assert_no_fan_out_effect(&core, "operator-badged call");

    // --- the non-vacuity leg: the SAME core admits a real lead macaroon ----
    let admitted = util::tool_call(
        &core,
        7,
        "fan_out",
        json!({
            "badge": lead_badge(),
            "workspace": WS,
            "tasks": tasks(2),
        }),
    )
    .await;

    assert_ne!(
        admitted["isError"],
        json!(true),
        "the SAME core, root key wired, ADMITTS a real lead macaroon — so the refusal above is \
         about badge KIND, not about this core being unable to verify anything (DR-045 \
         §Consequences non-vacuity leg): {admitted:#}"
    );
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        1,
        "the lead-badged call DID reach the substrate — the door is discriminating, not closed"
    );
}

/// CRITERION (DR-045 §Decision 2/4) — the new code is DISTINGUISHABLE from a
/// failed badge, which is the entire reason for minting it rather than reusing
/// `BADGE_INVALID`. An operator token is *valid*; calling it invalid would be
/// false, and I6 does not permit a refusal that misstates why (DR-045
/// §Invariant posture).
///
/// So both directions are pinned:
/// an admitted operator token is `FAN_OUT_LEAD_ONLY` and NEVER `BADGE_INVALID`;
/// a genuinely unverifiable macaroon (unparseable, or well-formed under a
/// FOREIGN root key) is `BADGE_INVALID` and NEVER `FAN_OUT_LEAD_ONLY`.
/// Every one of them takes zero effect.
#[tokio::test]
async fn lead_only_is_distinguishable_from_an_invalid_badge() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingFanOutSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    // A well-formed macaroon minted under a DIFFERENT root key: it parses, it
    // is the right KIND, and it still cannot verify on this core.
    let foreign = Macaroon::mint(
        &RootKey::from_bytes([99u8; 32]),
        LEAD_RUN,
        vec![Caveat::Workspace {
            workspace: WS.into(),
        }],
    )
    .to_wire();

    for (id, badge, expected, why) in [
        (
            8u64,
            operator.token_hex(),
            codes::FAN_OUT_LEAD_ONLY,
            "an ADMITTED operator token is valid — refusing it BADGE_INVALID would tell the \
             caller its badge is bad, which is false (DR-045 §Invariant posture, I6)",
        ),
        (
            9,
            "not-a-macaroon".to_string(),
            codes::BADGE_INVALID,
            "an UNPARSEABLE value is a bad badge, and must NOT be dressed up as a lead-only \
             policy refusal (DR-045 §Decision 3 door order)",
        ),
        (
            10,
            foreign,
            codes::BADGE_INVALID,
            "a well-formed macaroon under a FOREIGN root key is the right KIND but does not \
             verify — that is BADGE_INVALID, not FAN_OUT_LEAD_ONLY",
        ),
    ] {
        let result = util::tool_call(
            &core,
            id,
            "fan_out",
            json!({"badge": badge, "workspace": WS, "tasks": tasks(2)}),
        )
        .await;

        util::assert_tool_refusal(&result, expected);
        assert_ne!(
            util::tool_payload(&result)["code"],
            json!(if expected == codes::FAN_OUT_LEAD_ONLY {
                codes::BADGE_INVALID
            } else {
                codes::FAN_OUT_LEAD_ONLY
            }),
            "{why}: {result:#}"
        );
    }

    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "no refused call of any kind reaches the substrate (DR-045 §Decision 3)"
    );
    assert_no_fan_out_effect(&core, "refused-badge calls");
}
