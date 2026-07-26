//! DR-055 ORACLE (live trial, end-to-end) — the daemon half of trials slice
//! B, driven over the REAL loopback-HTTP MCP transport against a REAL daemon:
//! `open_trial` server-side matrix expansion, the `trial.opened` intent fact,
//! sample membership on `agent.spawned.trial?`, daemon-derived distinct
//! per-sample keys, model attribution per variant cell, retry idempotency
//! (DR-055 §Decision 1/2/3), and the harness-axis HONEST REFUSAL (DR-055
//! §Consequences risk 2). House `#[cfg(unix)]` + `*_e2e.rs` convention —
//! WSL/unix-only, NOT part of the host clippy surface; run WSL-side.
//!
//! ## Why a live board and not only the pure folds
//!
//! `crates/rezidnt-state/tests/dr055_trial_fold.rs` judges the fold and the
//! delta over hand-authored events; `crates/rezidnt-mcp/tests/open_trial_door.rs`
//! judges the door and cap against a recording substrate. Neither proves the
//! DAEMON emits what those judges consume: that `trial.opened` actually lands
//! (with the envelope `workspace` its idempotency scope REQUIRES), that each
//! sample's `agent.spawned` really carries `trial` + `model` + a distinct
//! derived `idempotency_key`, and that a retry re-derives the SAME keys so the
//! EXISTING `spawn_keys` map dedupes every sample. The DR-044 precedent
//! (`fan_out_live_e2e.rs`) exists for exactly this reason.
//!
//! ## RED MODE (against the tree at cut time — session 33, post-`bcd0db9`)
//!
//! ASSERT-RED: `open_trial` is not dispatched (no handler, no `OpenTrialArgs`,
//! no emit site — verified by grep this session), so `tools/call` answers a
//! JSON-RPC error and `mcp_tool_call`'s "must not be a protocol error" assert
//! fires on the first call in each test. Red for the right reason: the tool
//! does not exist.
//!
//! ## Harness-axis honesty (the silent-wrong class, named)
//!
//! `SUPPORTED_HARNESSES = &["claude-code"]` (`bins/rezidentd/src/runs.rs`), so
//! a harness-varying matrix CANNOT be honored today. DR-055 §Consequences
//! risk 2 requires the implementer to REFUSE it honestly rather than silently
//! narrow it: a tool that accepted a claude-code x codex matrix and quietly
//! spawned only the claude-code half would report an experiment that was
//! never run — records asserting mechanisms the code lacks is the exact
//! defect class this arc keeps finding (fanout-silent-wrong). The refusal
//! test below is that clause's judge: WHOLE-call refusal, zero effect, no
//! narrowed success.
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::{
    make_project, mcp_post, mcp_tool_call, rpc, start_daemon_with_mcp, tool_payload,
    wait_for_lockfile,
};
use serde_json::{Value, json};

const LOCK_DEADLINE: Duration = Duration::from_secs(10);
const FACT_DEADLINE: Duration = Duration::from_secs(30);
/// Bounded settle for NEGATIVE assertions (no new facts after a retry/refusal).
const SETTLE: Duration = Duration::from_millis(600);

const TRIAL_KEY: &str = "dr055-live-trial-key";

fn initialize(url: &str) {
    let response = mcp_post(
        url,
        &rpc(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "dr055-oracle", "version": "0"}
            }),
        ),
    );
    assert!(
        response.get("error").is_none(),
        "initialize must succeed: {response:#}"
    );
}

/// Poll `tail_events` until `pred` matches some envelope; return the snapshot.
fn tail_until(url: &str, deadline: Duration, mut pred: impl FnMut(&Value) -> bool) -> Vec<Value> {
    let until = Instant::now() + deadline;
    loop {
        let result = mcp_tool_call(url, 40, "tail_events", json!({}));
        let events = tool_payload(&result)["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if events.iter().any(&mut pred) {
            return events;
        }
        assert!(
            Instant::now() < until,
            "deadline: tail_events never showed the expected event; last saw {} envelopes",
            events.len()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// One `tail_events` snapshot, no waiting.
fn tail_now(url: &str) -> Vec<Value> {
    let result = mcp_tool_call(url, 41, "tail_events", json!({}));
    tool_payload(&result)["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn count_by(events: &[Value], pred: impl Fn(&Value) -> bool) -> usize {
    events.iter().filter(|e| pred(e)).count()
}

/// Stand up daemon + MCP, open the stub project, and wait for the ordinary
/// (non-trial) agent's spawn — the baseline every trial assertion compares
/// against. Returns (url, operator badge, workspace).
fn open_baseline(lockfile: &std::path::Path, spec: &str) -> (String, String, String) {
    let lock = wait_for_lockfile(lockfile, LOCK_DEADLINE);
    let url = lock["url"].as_str().expect("lockfile url").to_string();
    let operator = lock["badge"]
        .as_str()
        .expect("lockfile carries the operator badge token")
        .to_string();
    initialize(&url);

    let opened = mcp_tool_call(
        &url,
        2,
        "open_project",
        json!({"badge": operator, "spec_toml": spec}),
    );
    assert_ne!(
        opened["isError"],
        json!(true),
        "open_project must succeed: {opened:#}"
    );
    let workspace = tool_payload(&opened)["workspace"]
        .as_str()
        .expect("open ack names the workspace ulid")
        .to_string();

    // The ordinary agent's spawn is on the log before any trial runs, so the
    // "no new spawns" negatives below have a stable baseline.
    tail_until(&url, FACT_DEADLINE, |e| e["subject"] == "agent.spawned");
    (url, operator, workspace)
}

/// CRITERION (f) — a HARNESS-VARYING matrix is refused HONESTLY, whole-call:
/// `SUPPORTED_HARNESSES` is `["claude-code"]`, so a matrix with a `codex`
/// variant cannot be honored — and it must NOT be silently narrowed to the
/// claude-code cells. Zero effect: no `trial.opened`, no new `agent.spawned`,
/// no new `worktree.allocated`.
#[test]
fn a_harness_varying_matrix_is_refused_honestly_never_narrowed() {
    let (daemon, lockfile) = start_daemon_with_mcp(None);
    let _ = &daemon;
    let (_project, spec) = make_project(50);
    let (url, operator, workspace) = open_baseline(&lockfile, &spec);

    let before = tail_now(&url);
    let spawned_before = count_by(&before, |e| e["subject"] == "agent.spawned");
    let trees_before = count_by(&before, |e| e["subject"] == "worktree.allocated");

    let refused = mcp_tool_call(
        &url,
        3,
        "open_trial",
        json!({
            "badge": operator,
            "workspace": workspace,
            "idempotency_key": TRIAL_KEY,
            "variants": [
                {"agent": "impl", "harness": "claude-code"},
                {"agent": "impl", "harness": "codex"},
            ],
            "samples": 2,
        }),
    );

    assert_eq!(
        refused["isError"],
        json!(true),
        "a matrix this daemon cannot honor on its harness axis is REFUSED as a \
         whole call — accepting it and spawning only the claude-code half \
         would report an experiment that never ran (DR-055 §Consequences risk \
         2: refuse honestly, never silently narrow): {refused:#}"
    );
    let payload = tool_payload(&refused);
    assert!(
        payload["code"].as_str().is_some_and(|c| !c.is_empty()),
        "the refusal carries a machine-readable code (I6): {payload:#}"
    );
    assert!(
        payload.get("trial").is_none() || payload["trial"].is_null(),
        "a refused call mints NO trial — a trial id on a refusal would be the \
         narrowed-success shape in disguise: {payload:#}"
    );

    std::thread::sleep(SETTLE);
    let after = tail_now(&url);
    assert_eq!(
        count_by(&after, |e| e["subject"] == "trial.opened"),
        0,
        "no trial.opened lands for a refused matrix — the intent record is \
         minted only for matrices the daemon can honor"
    );
    assert_eq!(
        count_by(&after, |e| e["subject"] == "agent.spawned"),
        spawned_before,
        "zero samples spawned — not even the claude-code cells (whole-call \
         refusal, never partial)"
    );
    assert_eq!(
        count_by(&after, |e| e["subject"] == "worktree.allocated"),
        trees_before,
        "zero worktrees allocated before the refusal"
    );
}

/// CRITERIA (c)+(d)+(e), live — the admitted 2x2 matrix, end to end:
///
/// 1. `trial.opened` lands with the ratified v1 payload VERBATIM and the
///    envelope `workspace` set (the fact's own idempotency-scope obligation).
/// 2. Exactly V x N = 4 sample spawns land, each carrying `trial` (membership
///    on the payload field, never the envelope), the variant's `model`
///    verbatim (2 per cell), and NO `lead_run` (a trial has no lead — DR-055
///    §Context 2).
/// 3. The 4 daemon-derived `idempotency_key`s are DISTINCT (N samples = N
///    runs — the DR-055 §Context 1 fatality, dissolved).
/// 4. RETRY: the identical call re-derives identical keys, so every sample
///    hits the existing `spawn_keys` map — the ack resolves to the SAME trial
///    id, NO new run spawns, and no second `trial.opened` lands (at-most-once
///    by idempotent construction).
#[test]
fn an_admitted_matrix_opens_spawns_attributes_and_retries_idempotently() {
    let (daemon, lockfile) = start_daemon_with_mcp(None);
    let _ = &daemon;
    let (_project, spec) = make_project(50);
    let (url, operator, workspace) = open_baseline(&lockfile, &spec);

    let open_trial_args = json!({
        "badge": operator,
        "workspace": workspace,
        "idempotency_key": TRIAL_KEY,
        "variants": [
            {"agent": "impl", "harness": "claude-code", "model": "model-live-a"},
            {"agent": "impl", "harness": "claude-code", "model": "model-live-b"},
        ],
        "samples": 2,
    });

    let ack = mcp_tool_call(&url, 3, "open_trial", open_trial_args.clone());
    assert_ne!(
        ack["isError"],
        json!(true),
        "a 2x2 all-claude-code matrix is admissible today: {ack:#}"
    );
    let trial = tool_payload(&ack)["trial"]
        .as_str()
        .expect("the ack names the minted trial id")
        .to_string();

    // --- 1. the intent fact -------------------------------------------------
    let events = tail_until(&url, FACT_DEADLINE, |e| e["subject"] == "trial.opened");
    let opened: Vec<&Value> = events
        .iter()
        .filter(|e| e["subject"] == "trial.opened")
        .collect();
    assert_eq!(
        opened.len(),
        1,
        "exactly one trial.opened for one open_trial call"
    );
    let fact = opened[0];
    assert_eq!(
        fact["payload"]["trial"].as_str(),
        Some(trial.as_str()),
        "the payload carries the trial's own id — the explicit fold key: {fact:#}"
    );
    assert_eq!(
        fact["payload"]["idempotency_key"], TRIAL_KEY,
        "the ONE trial-level key rides the fact, so the key->trial map is \
         log-derivable (I3): {fact:#}"
    );
    assert_eq!(
        fact["payload"]["samples"], 2,
        "sample count VERBATIM: {fact:#}"
    );
    assert_eq!(
        fact["payload"]["variants"],
        json!([
            {"agent": "impl", "harness": "claude-code", "model": "model-live-a"},
            {"agent": "impl", "harness": "claude-code", "model": "model-live-b"},
        ]),
        "the requested variant list rides the fact VERBATIM, ordered as \
         requested (the delta's requested end): {fact:#}"
    );
    assert_eq!(
        fact["workspace"].as_str(),
        Some(workspace.as_str()),
        "emitter obligation: a keyed trial.opened MUST set the envelope \
         workspace, or the rebuilt key->trial map has no dedup scope: {fact:#}"
    );

    // --- 2 + 3. the four samples, attributed and distinct --------------------
    let sample_count = |events: &[Value]| {
        count_by(events, |e| {
            e["subject"] == "agent.spawned" && e["payload"]["trial"].as_str() == Some(&trial)
        })
    };
    let events = wait_for_sample_count(&url, &trial, 4, FACT_DEADLINE);
    let samples: Vec<&Value> = events
        .iter()
        .filter(|e| {
            e["subject"] == "agent.spawned" && e["payload"]["trial"].as_str() == Some(&trial)
        })
        .collect();
    assert_eq!(
        samples.len(),
        4,
        "V x N = 2 x 2 = 4 sample runs spawned, every one carrying its trial \
         membership on the payload (never the envelope — DR-049): {events:#?}"
    );

    let mut keys = std::collections::BTreeSet::new();
    let mut models = std::collections::BTreeMap::<String, usize>::new();
    for sample in &samples {
        let payload = &sample["payload"];
        assert_eq!(payload["agent"], "impl", "axis 1 on the sample fact");
        assert_eq!(
            payload["harness"], "claude-code",
            "axis 2 on the sample fact"
        );
        let model = payload["model"]
            .as_str()
            .unwrap_or_else(|| panic!("axis 3: each sample names its variant's model: {sample:#}"));
        *models.entry(model.to_string()).or_insert(0) += 1;
        let key = payload["idempotency_key"]
            .as_str()
            .unwrap_or_else(|| panic!("each sample records its DERIVED key: {sample:#}"));
        keys.insert(key.to_string());
        assert!(
            payload.get("lead_run").is_none() || payload["lead_run"].is_null(),
            "a trial has NO lead (DR-055 §Context 2) — lead_run must be absent \
             on a sample spawn: {sample:#}"
        );
    }
    assert_eq!(
        keys.len(),
        4,
        "4 samples, 4 DISTINCT daemon-derived spawn keys — a collision is a \
         requested sample silently swallowed by dedup (DR-055 §Decision 1)"
    );
    assert_eq!(
        models,
        [
            ("model-live-a".to_string(), 2),
            ("model-live-b".to_string(), 2)
        ]
        .into_iter()
        .collect(),
        "2 samples per variant cell, attributed by the model axis — the \
         leaderboard's fold ground (DR-055 §Context 3)"
    );

    // --- 4. retry dedupes everything ----------------------------------------
    let retry = mcp_tool_call(&url, 4, "open_trial", open_trial_args);
    assert_ne!(
        retry["isError"],
        json!(true),
        "a retry of the same trial key is not an error — it is the honest \
         retry (DR-055 §Decision 1): {retry:#}"
    );
    assert_eq!(
        tool_payload(&retry)["trial"].as_str(),
        Some(trial.as_str()),
        "the retry resolves to the SAME trial — the key->trial map, \
         log-derived: {retry:#}"
    );

    std::thread::sleep(SETTLE);
    let after = tail_now(&url);
    assert_eq!(
        sample_count(&after),
        4,
        "the retry re-derived the SAME 4 keys, every sample hit the existing \
         spawn_keys map, and NOTHING new spawned — the dedup rule is \
         untouched, only the key space extended (DR-055 §Decision 1, DR-048's \
         binding idempotency clause discharged)"
    );
    assert_eq!(
        count_by(&after, |e| e["subject"] == "trial.opened"),
        1,
        "no second trial.opened on a retry — the emitter is at-most-once by \
         idempotent construction (DR-055 set fold-semantics bullet)"
    );
}

/// Poll `tail_events` until the log shows at least `want` spawns tagged with
/// `trial` (`tail_until`'s predicate sees one envelope at a time; this
/// condition is a COUNT over the snapshot). Returns the snapshot.
fn wait_for_sample_count(url: &str, trial: &str, want: usize, deadline: Duration) -> Vec<Value> {
    let until = Instant::now() + deadline;
    loop {
        let events = tail_now(url);
        let seen = events
            .iter()
            .filter(|e| {
                e["subject"] == "agent.spawned" && e["payload"]["trial"].as_str() == Some(trial)
            })
            .count();
        if seen >= want {
            return events;
        }
        assert!(
            Instant::now() < until,
            "deadline: only {seen}/{want} trial-tagged agent.spawned facts landed for {trial}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
