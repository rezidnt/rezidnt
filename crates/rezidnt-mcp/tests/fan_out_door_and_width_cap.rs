//! DR-044 ORACLE (width-cap refusal + I2 response cleanliness + honest partial
//! failure) — guards (d) and the response half of (e)/I2 from DR-044
//! §Consequences. Runs against a bare `McpCore` with a RECORDING substrate, so
//! it is deterministic and HOST-LINTABLE: no daemon, no process, no worktree.
//!
//! ## RED MODE
//!
//! COMPILE-RED on the seam types (`rezidnt_mcp::MAX_FAN_OUT_DEFAULT`,
//! `FanOutOutcome`, `McpSubstrate::fan_out`, `codes::FAN_OUT_TOO_WIDE`) and
//! ASSERT-RED on dispatch (`fan_out` is an unknown tool, so `tools_call` returns
//! a `-32602` JSON-RPC error and `util::tool_call`'s "expected a result" panic
//! fires). Both are red for the right reason: the tool and its seam do not exist.
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
use rezidnt_run::badge::Badge;
use rezidnt_types::mcp::FanOutTask;
use serde_json::{Value, json};

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

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

/// A core with the operator badge admitted and the recording fan-out substrate
/// wired, over a fresh temp log (so side effects and their ABSENCE are readable).
fn core_with(
    operator: &Badge,
    substrate: Arc<RecordingFanOutSubstrate>,
) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(operator);
    let core = McpCore::new(fabric, book).with_substrate(substrate);
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
            "badge": operator.token_hex(),
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
            "badge": operator.token_hex(),
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
            "badge": operator.token_hex(),
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
            "badge": operator.token_hex(),
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
