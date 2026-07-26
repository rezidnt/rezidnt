//! DR-055 ORACLE — the `open_trial` door and the V x N width cap (DR-055
//! §Decision 2 and §Decision 4; trials slice B). Runs against a bare `McpCore`
//! with a RECORDING substrate, so it is deterministic and HOST-LINTABLE — the
//! `fan_out_door_and_width_cap.rs` board shape exactly.
//!
//! The ruled boundary, restated as falsifiable assertions:
//!
//! - `open_trial` is OPERATOR-badged only, through `check_operator_badge`
//!   reused VERBATIM — the `kill_run`/`resolve_permit` condition (a trial has
//!   no lead and no edge to key). NO verb parameter, NO new refusal code: a
//!   presented agent MACAROON — even one genuinely valid under this core's
//!   root key — simply fails the operator-token check and falls through to the
//!   EXISTING `BADGE_INVALID`, the same conflation DR-045 §Decision 4 declined
//!   to fix on `kill_run`'s door and DR-055 §Decision 2 declines again here.
//! - The V x N cap refuses the WHOLE call, BEFORE any effect — the
//!   `FAN_OUT_TOO_WIDE` whole-call-refusal precedent: the substrate is never
//!   reached, no `trial.opened`, no `worktree.allocated`, no `agent.spawned`
//!   lands. Never partially spawned.
//! - Door order: the badge door runs BEFORE the cap (an unauthenticated caller
//!   never learns the cap), and zero effect precedes every refusal.
//!
//! ## What DR-055 left open, disclosed rather than guessed
//!
//! - The exact CAP VALUE and its config knob are DEFERRED (§Consequences,
//!   "Deferred, named"). So this board pins no constant: the refused leg uses
//!   a 64 x 64 matrix (4096 — beyond any cap the ontology's own scale note
//!   contemplates, "single-digit KiB even at a cap an order of magnitude wider
//!   than fan-out's" `MAX_FAN_OUT_DEFAULT = 8`), and the admitted leg uses
//!   2 x 2 (4 — under fan-out's own DEFAULT, the floor any usable trial cap
//!   must clear, since V >= 1 and N >= 1 makes 2 x 2 the minimal interesting
//!   matrix).
//! - The exact REFUSAL CODE for an over-wide matrix is a SPEC GAP this board
//!   discloses: DR-055's Amends banner mints NO refusal code, and §Decision 4
//!   names only "the `FAN_OUT_TOO_WIDE` precedent" — mechanism, not code. The
//!   over-wide test therefore pins the MECHANISM exactly (whole-call refusal,
//!   machine-readable code, zero effect, substrate unreached) and pins the
//!   code only negatively (not a badge code — the door passed). When the code
//!   is ruled, ONE assertion below tightens; nothing else moves.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//!
//! A tool named `open_trial` dispatched by `tools_call`, arguments
//! `{badge, workspace, idempotency_key, variants: [{agent, harness, model?}],
//! samples}` — the DR-055-fixed constraints (one trial-level key, the whole
//! matrix server-side, operator badge) and nothing more. Types:
//! `rezidnt_types::mcp::OpenTrialArgs` carrying
//! `rezidnt_types::mcp::TrialVariant {agent: String, harness: String, model:
//! Option<String>}` (§9 no-drift, the `FanOutArgs`/`FanOutTask` pattern), and
//! in `rezidnt-mcp`:
//!
//! ```ignore
//! /// The daemon's ack: the minted (or retry-resolved) trial id, plus one
//! /// outcome per SAMPLE in matrix order — a refused sample (e.g.
//! /// worktree-conflicted) is reported REFUSED on this response and NEVER as
//! /// a run (the ratified FanOutTask refusal shape; the delta projection is
//! /// how the log side sees the deficit).
//! pub struct OpenTrialAck {
//!     pub trial: String,
//!     pub outcomes: Vec<FanOutOutcome>,
//! }
//!
//! trait McpSubstrate {
//!     /// DEFAULTED (SUBSTRATE_UNAVAILABLE), so every existing impl compiles
//!     /// untouched — the fan_out seam precedent.
//!     fn open_trial(
//!         &self,
//!         workspace: String,
//!         idempotency_key: String,
//!         variants: Vec<rezidnt_types::mcp::TrialVariant>,
//!         samples: u64,
//!     ) -> BoxFuture<Result<OpenTrialAck, ToolRefusal>>;
//! }
//! ```
//!
//! No operator badge id rides the seam: the `trial.opened` v1 payload carries
//! no operator field (the ratified baseline is `{trial, idempotency_key,
//! variants, samples}`), so the substrate needs none — the `kill_run` seam
//! precedent.
//!
//! ## RED MODE (against the tree at cut time — session 33, post-`bcd0db9`)
//!
//! COMPILE-RED on the seam (`OpenTrialAck`, `McpSubstrate::open_trial`,
//! `rezidnt_types::mcp::TrialVariant` — none exist, verified by grep this
//! session) and ASSERT-RED on dispatch (`open_trial` is an unknown tool, so
//! `tools_call` answers a JSON-RPC error and `util::tool_call`'s "expected a
//! result" panic fires). Both are red for the right reason: the tool and its
//! seam do not exist.

mod util;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{
    BadgeBook, BoxFuture, KillAck, McpCore, McpSubstrate, OpenAck, OpenTrialAck, PermitConfig,
    ToolRefusal, codes,
};
use rezidnt_run::badge::{Badge, Caveat, Macaroon, RootKey};
use rezidnt_types::mcp::TrialVariant;
use serde_json::{Value, json};
use std::sync::Mutex;

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const TRIAL: &str = "01DR055D00RTR1A00000000001";
const TRIAL_KEY: &str = "dr055-door-trial-key";

/// A fake substrate that RECORDS every `open_trial` call and returns a
/// scripted ack WITHOUT allocating a worktree or spawning anything. Its job is
/// answering "was the substrate reached at all?" — the width cap's and the
/// door's whole claim is that on a refused call it is NOT.
#[derive(Default)]
struct RecordingTrialSubstrate {
    calls: AtomicUsize,
    samples_seen: AtomicU64,
    variants_seen: Mutex<Vec<Vec<TrialVariant>>>,
}

impl McpSubstrate for RecordingTrialSubstrate {
    fn open_project(&self, _spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                codes::SUBSTRATE_UNAVAILABLE,
                "trial-only test substrate",
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
                "trial-only test substrate",
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
                "trial-only test substrate",
            ))
        })
    }

    /// The DR-055 seam. Reaching this AT ALL is the observable the door and
    /// cap tests assert the absence of.
    fn open_trial(
        &self,
        _workspace: String,
        _idempotency_key: String,
        variants: Vec<TrialVariant>,
        samples: u64,
    ) -> BoxFuture<Result<OpenTrialAck, ToolRefusal>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.samples_seen.store(samples, Ordering::SeqCst);
        self.variants_seen
            .lock()
            .expect("variants log")
            .push(variants);
        Box::pin(async move {
            Ok(OpenTrialAck {
                trial: TRIAL.to_string(),
                outcomes: Vec::new(),
            })
        })
    }
}

/// Fixed root key so the board is deterministic (the fan_out board's `root`).
fn root() -> RootKey {
    RootKey::from_bytes([55u8; 32])
}

/// An agent MACAROON genuinely valid under this core's root key — the badge
/// kind `open_trial` REFUSES on the operator door. Minting it against the
/// wired root key is what makes the refusal a KIND conflation (the disclosed
/// `kill_run` shape), not a verification artifact.
fn agent_macaroon_wire() -> String {
    Macaroon::mint(
        &root(),
        "01DR055D00RRVN000000000001",
        vec![Caveat::Workspace {
            workspace: WS.into(),
        }],
    )
    .to_wire()
}

/// A core with the operator badge ADMITTED and the root key WIRED, over a
/// fresh temp log, so side effects and their ABSENCE are readable.
fn core_with(
    operator: &Badge,
    substrate: Arc<RecordingTrialSubstrate>,
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

/// A V-variant matrix argument (all claude-code; models distinct per variant
/// so the list is genuinely varied, not V copies of one cell).
fn variants(v: usize) -> Value {
    Value::Array(
        (0..v)
            .map(|i| {
                json!({"agent": "impl", "harness": "claude-code", "model": format!("model-{i}")})
            })
            .collect(),
    )
}

fn trial_args(badge: Option<&str>, v: usize, samples: u64) -> Value {
    let mut args = json!({
        "workspace": WS,
        "idempotency_key": TRIAL_KEY,
        "variants": variants(v),
        "samples": samples,
    });
    if let Some(badge) = badge {
        args["badge"] = json!(badge);
    }
    args
}

/// The subjects a trial would put on the log if the call took ANY effect.
/// Asserting their absence — not just the refusal — is the point: an
/// implementation that spawned first and refused second would pass a
/// code-only check (the fan_out board's guard, plus `trial.opened` itself).
fn assert_no_trial_effect(core: &McpCore, context: &str) {
    let log = util::log_events(core);
    for subject in ["trial.opened", "worktree.allocated", "agent.spawned"] {
        assert!(
            log.iter().all(|e| e.subject.as_str() != subject),
            "{context}: a refused open_trial must emit NO `{subject}` — the whole \
             call is refused BEFORE any effect (DR-055 §Decision 4, the \
             FAN_OUT_TOO_WIDE whole-call precedent; never partially spawned). \
             Log subjects: {:?}",
            log.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>()
        );
    }
}

/// DOOR — a badge-less call is `BADGE_REQUIRED`, zero effect, substrate never
/// reached. Sent OVER-WIDE on purpose: the door runs BEFORE the cap, so an
/// unauthenticated caller does not learn the cap exists (the fan_out board's
/// ordering pin, mirrored onto the operator door).
#[tokio::test]
async fn the_badge_door_runs_before_the_width_cap() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingTrialSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let result = util::tool_call(&core, 1, "open_trial", trial_args(None, 64, 64)).await;

    util::assert_tool_refusal(&result, codes::BADGE_REQUIRED);
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "a badge-less call never reaches the substrate"
    );
    assert_no_trial_effect(&core, "badge-less over-wide call");
}

/// DOOR — `check_operator_badge` VERBATIM, no new refusal code (DR-055
/// §Decision 2): an agent MACAROON that is GENUINELY VALID under this core's
/// wired root key falls through to the EXISTING `BADGE_INVALID` — the same
/// conflation DR-045 §Decision 4 disclosed on `kill_run` and DR-055 declines
/// to fix here. Pinned in both directions: the code IS `BADGE_INVALID` and is
/// NOT `FAN_OUT_LEAD_ONLY` (no third-door code appears on this tool — a
/// distinct trial-door code would be the mint the record forbids).
///
/// NON-VACUITY: the SAME core, SAME substrate, SAME args, presented with the
/// admitted DR-005 operator token, IS admitted and reaches the substrate — so
/// the refusal above is about badge KIND, not a closed door.
#[tokio::test]
async fn a_macaroon_falls_through_to_badge_invalid_and_the_operator_is_admitted() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingTrialSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    // --- refusal leg: a valid macaroon, wrong KIND -------------------------
    let refused = util::tool_call(
        &core,
        2,
        "open_trial",
        trial_args(Some(&agent_macaroon_wire()), 2, 2),
    )
    .await;

    util::assert_tool_refusal(&refused, codes::BADGE_INVALID);
    assert_ne!(
        util::tool_payload(&refused)["code"],
        json!(codes::FAN_OUT_LEAD_ONLY),
        "open_trial mints NO refusal code and borrows none: a macaroon on the \
         operator door is the existing BADGE_INVALID (DR-055 §Decision 2), \
         never fan_out's lead-only policy code"
    );
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "a macaroon-badged open_trial never reaches the substrate — an agent \
         cannot open a trial (a trial is opened by an OPERATOR; the DR-045 \
         mirror, the other way)"
    );
    assert_no_trial_effect(&core, "macaroon-badged call");

    // --- non-vacuity leg: the operator token is admitted -------------------
    let admitted = util::tool_call(
        &core,
        3,
        "open_trial",
        trial_args(Some(&operator.token_hex()), 2, 2),
    )
    .await;

    assert_ne!(
        admitted["isError"],
        json!(true),
        "the SAME core admits the DR-005 operator token — the door is \
         discriminating, not closed: {admitted:#}"
    );
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        1,
        "the operator-badged call reached the substrate exactly once — one \
         call, the whole matrix (server-side expansion, DR-055 §Decision 2)"
    );
}

/// CAP (DR-055 §Decision 4) — an over-wide matrix is refused as a WHOLE CALL
/// with ZERO effect: the substrate is never reached, and no `trial.opened`,
/// `worktree.allocated`, or `agent.spawned` lands. 64 x 64 = 4096 requested
/// samples is beyond any cap this record contemplates (see the module header
/// for why no constant is read: the exact cap value is DEFERRED).
///
/// The refusal CODE is pinned only negatively — machine-readable and not a
/// badge code, because the badge door passed — per the disclosed spec gap in
/// the module header. Tighten to an exact code the day one is ruled.
#[tokio::test]
async fn an_over_wide_matrix_is_refused_whole_call_with_no_effect() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingTrialSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let result = util::tool_call(
        &core,
        4,
        "open_trial",
        trial_args(Some(&operator.token_hex()), 64, 64),
    )
    .await;

    assert_eq!(
        result["isError"],
        json!(true),
        "a 64x64 matrix is refused as a WHOLE CALL — never admitted, never \
         partially spawned (DR-055 §Decision 4): {result:#}"
    );
    let payload = util::tool_payload(&result);
    let code = payload["code"]
        .as_str()
        .unwrap_or_else(|| panic!("the refusal carries a machine-readable code: {payload:#}"));
    assert!(!code.is_empty(), "the refusal code is non-empty");
    assert!(
        code != codes::BADGE_REQUIRED && code != codes::BADGE_INVALID,
        "the operator badge PASSED the door — an over-wide refusal must name \
         the width, not the badge (I6: a refusal never misstates why); got {code:?}"
    );
    assert_eq!(
        substrate.calls.load(Ordering::SeqCst),
        0,
        "the substrate is NEVER reached on an over-wide call — the cap refuses \
         before any allocation or spawn (the FAN_OUT_TOO_WIDE precedent)"
    );
    assert_no_trial_effect(&core, "over-wide call");
}

/// CAP non-vacuity + verbatim pass-through — a 2 x 2 matrix (V x N = 4, under
/// even fan-out's own DEFAULT 8, the floor any usable trial cap clears) is
/// ADMITTED, reaches the substrate exactly once, and arrives VERBATIM: the
/// variant list in requested order and the sample count untouched. Server-side
/// expansion means the daemon sees the WHOLE matrix — a core that reordered,
/// deduped, or truncated variants would break key derivation silently
/// (variant order is semantic: per-sample keys derive from (variant,
/// sample-index)). The ack's trial id passes through to the caller.
#[tokio::test]
async fn an_in_cap_matrix_is_admitted_and_arrives_verbatim() {
    let operator = Badge::mint().expect("mint operator badge");
    let substrate = Arc::new(RecordingTrialSubstrate::default());
    let (_dir, core) = core_with(&operator, Arc::clone(&substrate));

    let result = util::tool_call(
        &core,
        5,
        "open_trial",
        trial_args(Some(&operator.token_hex()), 2, 2),
    )
    .await;

    assert_ne!(
        result["isError"],
        json!(true),
        "a 2x2 matrix is admitted — the cap is backpressure, not a blanket \
         refusal: {result:#}"
    );
    assert_eq!(substrate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        substrate.samples_seen.load(Ordering::SeqCst),
        2,
        "the sample count reaches the substrate untouched"
    );
    let seen = substrate.variants_seen.lock().expect("variants log");
    assert_eq!(
        seen[0],
        vec![
            TrialVariant {
                agent: "impl".to_string(),
                harness: "claude-code".to_string(),
                model: Some("model-0".to_string()),
            },
            TrialVariant {
                agent: "impl".to_string(),
                harness: "claude-code".to_string(),
                model: Some("model-1".to_string()),
            },
        ],
        "the variant list reaches the substrate VERBATIM, in requested order"
    );

    let payload = util::tool_payload(&result);
    assert_eq!(
        payload["trial"],
        json!(TRIAL),
        "the ack's trial id rides the tool response — the caller learns which \
         trial to watch: {payload:#}"
    );
}
