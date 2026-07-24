//! REGISTRY-CONVERGENCE ORACLE — the distinct worktree-conflict refusal code
//! DR-046 §Decision 9 owes to this slice (criterion C6, the code half).
//!
//! HOST-LINTABLE: a bare `McpCore` with a scripted substrate, no daemon, no
//! process, no worktree — the same board shape as
//! `fan_out_door_and_width_cap.rs`, whose
//! `lead_only_is_distinguishable_from_an_invalid_badge` this file mirrors.
//!
//! ## RED MODE
//!
//! COMPILE-RED on `codes::WORKTREE_CONFLICT`, which does not exist: the `codes`
//! module runs `badge.required` … `fan_out.lead_only`
//! (`crates/rezidnt-mcp/src/lib.rs`) and every allocation refusal collapses to
//! `SPAWN_FAILED`. DR-046 §Decision 9 declined to mint it early because it
//! would have been "a code no path can emit"; Decision 8's wiring is what makes
//! it emittable, so it is minted here.
//!
//! ## API this board PINS
//!
//! ```ignore
//! /// DR-046 §Decision 9 — a fan-out task whose worktree could not be claimed
//! /// because the sole-allocator registry already holds that canonicalized
//! /// path (DR-001). Deliberately NOT `SPAWN_FAILED`: a caller must be able to
//! /// distinguish "the tree was contended, retry with the same keys" from
//! /// "this spawn is broken", and collapsing them tells the caller something
//! /// false about what to do next (I6). Additive code — older peers tolerate
//! /// an unknown refusal code (the `scope.requires_ttl` precedent, I5).
//! pub const WORKTREE_CONFLICT: &str = "worktree.conflict";
//! ```
//!
//! ## What this board proves, and what it CANNOT prove (read this)
//!
//! The scripted substrate CHOOSES the code it returns, so nothing here is
//! evidence that a REAL registry double-claim produces `WORKTREE_CONFLICT`.
//! Saying that plainly is the point: DR-046's own erratum flags guard (c) as
//! "discharged in letter only … a fixture tautology that a restored emitter
//! would leave green", and repeating that mistake here would be worse than
//! writing nothing.
//!
//! What IS load-bearing on this board:
//!
//! - the code EXISTS and is a distinct string from `SPAWN_FAILED`, in both
//!   directions, with neither an alias nor a prefix of the other;
//! - the core PASSES A PER-TASK CODE THROUGH VERBATIM. An implementation that
//!   normalized outcome codes, or that mapped anything unfamiliar onto
//!   `SPAWN_FAILED`, or that promoted a code-bearing outcome into a whole-call
//!   error, fails here. That is a real obligation of the core, and it is the
//!   one this board is the right place for.
//!
//! The mapping itself — a `GitError::Conflict` becoming `WORKTREE_CONFLICT`
//! while every other allocation failure stays `SPAWN_FAILED` — is judged in
//! `bins/rezidentd/src/runs/registry_convergence_tests.rs` over the daemon's
//! pure mapping function, and end-to-end in
//! `bins/rezidentd/tests/registry_convergence_e2e.rs`.

mod util;

use std::sync::Arc;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{
    BadgeBook, BoxFuture, FanOutOutcome, KillAck, McpCore, McpSubstrate, OpenAck, PermitConfig,
    ToolRefusal, codes,
};
use rezidnt_run::badge::{Badge, Caveat, Macaroon, RootKey};
use rezidnt_types::mcp::FanOutTask;
use serde_json::{Value, json};

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const LEAD_RUN: &str = "01DR046LEADRVN000000000001";
const SPAWNED_RUN: &str = "01DR046SUBRVN0000000000001";

/// A substrate that scripts task 0 as a WORKTREE CONFLICT, task 1 as an
/// ordinary spawn failure, and task 2 as a success — the three outcome classes
/// that must stay distinguishable in one response.
#[derive(Default)]
struct ScriptedSubstrate;

impl McpSubstrate for ScriptedSubstrate {
    fn open_project(&self, _spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                codes::SUBSTRATE_UNAVAILABLE,
                "conflict-code test substrate",
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
                "conflict-code test substrate",
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
                "conflict-code test substrate",
            ))
        })
    }

    fn fan_out(
        &self,
        _workspace: String,
        _lead_badge_id: String,
        tasks: Vec<FanOutTask>,
    ) -> BoxFuture<Result<Vec<FanOutOutcome>, ToolRefusal>> {
        Box::pin(async move {
            Ok(tasks
                .into_iter()
                .enumerate()
                .map(|(i, task)| {
                    let (run, code, message) = match i {
                        0 => (
                            None,
                            Some(codes::WORKTREE_CONFLICT.to_string()),
                            Some("worktree already claimed".to_string()),
                        ),
                        1 => (
                            None,
                            Some(codes::SPAWN_FAILED.to_string()),
                            Some("harness binary missing".to_string()),
                        ),
                        _ => (Some(SPAWNED_RUN.to_string()), None, None),
                    };
                    FanOutOutcome {
                        agent: task.agent,
                        idempotency_key: task.idempotency_key,
                        run,
                        code,
                        message,
                    }
                })
                .collect())
        })
    }
}

fn root() -> RootKey {
    RootKey::from_bytes([46u8; 32])
}

fn lead_badge() -> String {
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
    .to_wire()
}

fn core() -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(&Badge::mint().expect("mint operator badge"));
    let core = McpCore::new(fabric, book)
        .with_root_key(root())
        .with_substrate(Arc::new(ScriptedSubstrate));
    (dir, Arc::new(core))
}

fn tasks(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(|i| json!({"agent": format!("sub-{i}"), "idempotency_key": format!("dr046-key-{i}")}))
            .collect(),
    )
}

/// CRITERION C6 — the conflict code is a DISTINCT string from `SPAWN_FAILED`,
/// in both directions.
///
/// DR-046 §Decision 9: "a caller cannot distinguish 'the tree was contended,
/// retry with the same keys' from 'this spawn is broken'". Two codes that are
/// equal, or where one is a prefix or alias of the other, do not fix that — a
/// client matching on the string would still conflate them. Pinned as a pure
/// value assertion so it holds independently of any dispatch.
#[test]
fn the_conflict_code_is_distinct_from_spawn_failed_in_both_directions() {
    assert_ne!(
        codes::WORKTREE_CONFLICT,
        codes::SPAWN_FAILED,
        "DR-046 §Decision 9 mints the conflict code precisely so a contended tree is not \
         reported as a broken spawn. Reusing `spawn.failed` tells the caller to give up on a \
         request that a retry with the same keys would satisfy (I6)"
    );
    assert!(
        !codes::WORKTREE_CONFLICT.starts_with(codes::SPAWN_FAILED)
            && !codes::SPAWN_FAILED.starts_with(codes::WORKTREE_CONFLICT),
        "neither code is a PREFIX of the other — a client matching on prefixes must not be able \
         to conflate them: {:?} vs {:?}",
        codes::WORKTREE_CONFLICT,
        codes::SPAWN_FAILED
    );
    assert!(
        !codes::WORKTREE_CONFLICT.is_empty(),
        "the code is a real machine-readable string, not an empty placeholder"
    );
}

/// CRITERION C6 — the core passes a per-task refusal code through VERBATIM, so
/// a conflict, an ordinary spawn failure, and a success stay three
/// distinguishable outcomes inside ONE response.
///
/// This is the core's own obligation, not the substrate's: an implementation
/// that normalized codes, mapped unfamiliar ones onto `SPAWN_FAILED`, or
/// promoted a code-bearing outcome into a whole-call error would fail here. See
/// the module header for what this deliberately does NOT claim — the scripted
/// substrate chooses the codes, so this is evidence about pass-through only.
#[tokio::test]
async fn per_task_conflict_and_spawn_failure_codes_survive_the_response_unmerged() {
    let (_dir, core) = core();

    let result = util::tool_call(
        &core,
        1,
        "fan_out",
        json!({"badge": lead_badge(), "workspace": WS, "tasks": tasks(3)}),
    )
    .await;

    assert_ne!(
        result["isError"],
        json!(true),
        "a per-task refusal is NOT a whole-call error — the report is the outcome vector \
         (DR-044 §Decision 1): {result:#}"
    );

    let payload = util::tool_payload(&result);
    let outcomes = payload["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("fan_out returns a per-task outcome vector: {payload:#}"));
    assert_eq!(outcomes.len(), 3, "one outcome per task: {outcomes:#?}");

    assert_eq!(
        outcomes[0]["code"],
        json!(codes::WORKTREE_CONFLICT),
        "the CONTENDED task carries the conflict code verbatim — DR-044 §Decision 3's \
         \"the task's outcome carries the conflict code\", which DR-046 §Decision 9 finally \
         makes possible: {outcomes:#?}"
    );
    assert_ne!(
        outcomes[0]["code"],
        json!(codes::SPAWN_FAILED),
        "and it is NOT collapsed into `spawn.failed`, which is the whole reason the code was \
         minted: {outcomes:#?}"
    );
    assert!(
        outcomes[0].get("run").is_none() || outcomes[0]["run"].is_null(),
        "a conflicted task mints NO run — it is REFUSED, never a failed sub (I6, DR-044 \
         §Decision 3): {outcomes:#?}"
    );

    assert_eq!(
        outcomes[1]["code"],
        json!(codes::SPAWN_FAILED),
        "an ordinary spawn failure stays `spawn.failed` — the new code must not swallow every \
         refusal, or it is just `SPAWN_FAILED` under a new name: {outcomes:#?}"
    );
    assert_ne!(
        outcomes[1]["code"],
        json!(codes::WORKTREE_CONFLICT),
        "the reverse direction, pinned explicitly: a broken spawn must not be reported as a \
         contended tree, which would tell the caller to retry forever: {outcomes:#?}"
    );

    assert_eq!(
        outcomes[2]["run"],
        json!(SPAWNED_RUN),
        "and the SIBLING that succeeded still stands, carrying its run: a refused task does not \
         roll back or mask its siblings (DR-044 §Decision 3's refused-sub rule): {outcomes:#?}"
    );
    assert!(
        outcomes[2].get("code").is_none() || outcomes[2]["code"].is_null(),
        "a spawned task carries no refusal code: {outcomes:#?}"
    );
}
