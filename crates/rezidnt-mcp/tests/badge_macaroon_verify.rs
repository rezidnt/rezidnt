//! SP4b ORACLE — the §12 badge-door flips from token-EQUALITY to macaroon
//! VERIFY + caveat-eval on state-mutating MCP calls (DR-017 §Decision, ontology
//! badge-door delta). FAILING-FIRST: the macaroon `check_badge` path, the
//! daemon `RootKey` seam on `McpCore`, and the request-context caveat eval DO
//! NOT EXIST YET, so this file fails to compile (unresolved `RootKey` /
//! `with_root_key` / `Macaroon` verify path) until the implementer lands SP4b.
//! That is the correct red state.
//!
//! ## What flips (DR-017 §12 badge-door delta)
//! `check_badge` on a mutating call changes from `BadgeBook::id_for`
//! (token-equality) to: verify the presented macaroon against the daemon root
//! key + evaluate its caveats against the request context (this workspace, this
//! verb, this passed-in timestamp, this role). A macaroon whose caveats the
//! request VIOLATES → `badge.invalid`, no side effect. A valid one → the
//! loggable `badge_id` (`blake3(identifier)[..8]`).
//!
//! ## What must NOT change (DR-017 §Decision 4 — the operator-badge boundary)
//! The operator badge stays the DR-005 opaque daemon-lifetime class: an
//! admitted opaque `Badge` (token-equality via `BadgeBook`) still passes the
//! door unchanged. `crates/rezidnt-mcp/tests/badge_enforcement.rs` MUST stay
//! green — this board does not touch it. The door tries the opaque operator
//! badge first (unchanged), then macaroon-verify for an agent badge.
//!
//! ## API surface this board PINS (implementer builds to exactly this)
//! - `McpCore::with_root_key(self, root: rezidnt_run::badge::RootKey) -> Self`
//!   — the daemon's process-lifetime root key seam (builder-style, like
//!   `with_substrate`). A core with NO root key wired verifies no macaroon (an
//!   agent macaroon presented to a keyless core → `badge.invalid`).
//! - A mutating call carries, alongside `badge` (the serialized macaroon under
//!   the same arg), a request context: `workspace` (already required by
//!   `spawn_agent`/`open_project`), a `verb` DERIVED from the tool (spawn_agent
//!   → "spawn", open_project → "open"), and a caller-supplied `now` (RFC3339)
//!   the verifier evaluates expiry against — NEVER an ambient clock (I6). The
//!   `now` arg name is `now` on the tool arguments.
//!
//! Refusal codes reuse the existing `codes::BADGE_INVALID` (an agent macaroon
//! that fails verify or whose caveats are violated is `badge.invalid`, the same
//! machine-readable class the opaque unknown-token path already uses).

mod util;

use rezidnt_run::badge::{Caveat, Macaroon, RootKey};
use serde_json::json;

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const T_MID: &str = "2026-07-19T12:00:00Z";
const T_LATE: &str = "2026-07-20T00:00:00Z";

fn root() -> RootKey {
    RootKey::from_bytes([7u8; 32])
}

/// Mint an agent macaroon scoped to workspace WS, the `spawn` verb, and an
/// expiry after T_MID — i.e. VALID for a spawn_agent call at T_MID in WS.
fn agent_badge(root: &RootKey) -> Macaroon {
    Macaroon::mint(
        root,
        "run-01SP4BMCPVERIFY000000000",
        vec![
            Caveat::Workspace {
                workspace: WS.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into(), "open".into()],
            },
            Caveat::Expiry {
                not_after: T_LATE.into(),
            },
        ],
    )
}

/// A core whose daemon root key is `root()` — the SP4b seam. (Test-double: no
/// substrate wired, so the call refuses at `substrate.unavailable` AFTER the
/// badge door passes — proving the door let a valid macaroon through.)
fn core_with_root() -> (tempfile::TempDir, std::sync::Arc<rezidnt_mcp::McpCore>) {
    // A bare core (no substrate, no admitted opaque badge) with the daemon root
    // key wired — the SP4b seam under test. `with_root_key` is builder-style
    // (consumes + returns), mirroring `with_substrate`.
    let dir = tempfile::tempdir().expect("tempdir");
    let log = rezidnt_fabric::EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = rezidnt_fabric::Fabric::new(log, 1024);
    let core =
        rezidnt_mcp::McpCore::new(fabric, rezidnt_mcp::BadgeBook::new()).with_root_key(root());
    (dir, std::sync::Arc::new(core))
}

/// A valid macaroon (caveats satisfied by the request context) passes the door.
/// With no substrate wired, the call then refuses at `substrate.unavailable` —
/// which PROVES the badge door admitted it (a `badge.invalid` would short-circuit
/// BEFORE substrate dispatch, §12 door ordering).
#[tokio::test]
async fn valid_macaroon_passes_the_door() {
    let (_dir, core) = core_with_root();
    let m = agent_badge(&root());
    let result = util::tool_call(
        &core,
        1,
        "spawn_agent",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "agent": "impl",
            "idempotency_key": "k-1",
            "now": T_MID
        }),
    )
    .await;
    // The macaroon verified + caveats satisfied → the door passed; the bare core
    // then refuses at the substrate seam, NOT at the badge.
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::SUBSTRATE_UNAVAILABLE);
}

/// A macaroon whose WORKSPACE caveat the request violates → `badge.invalid`,
/// NO side effect (the door refuses before any substrate dispatch).
#[tokio::test]
async fn macaroon_with_violated_workspace_caveat_is_refused() {
    let (_dir, core) = core_with_root();
    let m = agent_badge(&root());
    let result = util::tool_call(
        &core,
        2,
        "spawn_agent",
        json!({
            "badge": m.to_wire(),
            "workspace": "01BX5ZZKBKACTAV9WEVGEMMVRZ", // a DIFFERENT workspace
            "agent": "impl",
            "idempotency_key": "k-2",
            "now": T_MID
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(
        util::log_events(&core).is_empty(),
        "a caveat-violating macaroon leaves the log untouched (§12 refuse-before-effect)"
    );
}

/// A macaroon whose EXPIRY caveat the request's passed-in timestamp violates →
/// `badge.invalid`, driven entirely by the request `now` (no ambient clock, I6).
#[tokio::test]
async fn macaroon_past_expiry_against_passed_in_now_is_refused() {
    let (_dir, core) = core_with_root();
    // Mint one that expires AT T_MID; the request presents now = T_LATE.
    let m = Macaroon::mint(
        &root(),
        "run-01SP4BMCPEXPIRY0000000000",
        vec![
            Caveat::Workspace {
                workspace: WS.into(),
            },
            Caveat::Verb {
                verbs: vec!["spawn".into()],
            },
            Caveat::Expiry {
                not_after: T_MID.into(),
            },
        ],
    );
    let result = util::tool_call(
        &core,
        3,
        "spawn_agent",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "agent": "impl",
            "idempotency_key": "k-3",
            "now": T_LATE // after not_after → expired
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(util::log_events(&core).is_empty());
}

/// A forged / tampered macaroon (broken MAC chain) → `badge.invalid`, no side
/// effect. Here: a macaroon minted under a FOREIGN root key presented to this
/// daemon.
#[tokio::test]
async fn foreign_root_macaroon_is_refused() {
    let (_dir, core) = core_with_root();
    let foreign = RootKey::from_bytes([9u8; 32]);
    let m = agent_badge(&foreign); // minted under a key this daemon does not hold
    let result = util::tool_call(
        &core,
        4,
        "spawn_agent",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "agent": "impl",
            "idempotency_key": "k-4",
            "now": T_MID
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(util::log_events(&core).is_empty());
}

/// DR-017 §Decision 4 — the OPERATOR badge is UNCHANGED: an admitted opaque
/// `Badge` (token-equality, DR-005) still passes the door on a mutating call.
/// This pins that the macaroon flip did NOT break the opaque operator path (the
/// door tries the opaque badge first). With no substrate, it lands at
/// `substrate.unavailable` — proving the opaque badge was ACCEPTED.
#[tokio::test]
async fn opaque_operator_badge_still_passes_unchanged() {
    let operator = rezidnt_run::badge::Badge::mint().expect("mint operator badge");
    // A core with the operator badge admitted AND a root key wired (the daemon
    // holds both: opaque operator + agent-macaroon verification).
    let dir = tempfile::tempdir().expect("tempdir");
    let log = rezidnt_fabric::EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = rezidnt_fabric::Fabric::new(log, 1024);
    let mut book = rezidnt_mcp::BadgeBook::new();
    book.admit(&operator);
    let core = std::sync::Arc::new(rezidnt_mcp::McpCore::new(fabric, book).with_root_key(root()));
    let result = util::tool_call(
        &core,
        5,
        "spawn_agent",
        json!({
            "badge": operator.token_hex(), // the opaque operator token, NOT a macaroon
            "workspace": WS,
            "agent": "impl",
            "idempotency_key": "k-5",
            "now": T_MID
        }),
    )
    .await;
    // The opaque operator badge is honored unchanged → door passes → bare core
    // refuses at the substrate seam, NOT at the badge (I: operator boundary held).
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::SUBSTRATE_UNAVAILABLE);
}

/// An agent macaroon presented to a core with NO root key wired → `badge.invalid`
/// (a keyless core cannot verify any macaroon; it is not a token in the opaque
/// book either).
#[tokio::test]
async fn agent_macaroon_on_keyless_core_is_refused() {
    let (_dir, core) = util::core(); // no root key, no admitted badges
    let m = agent_badge(&root());
    let result = util::tool_call(
        &core,
        6,
        "spawn_agent",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "agent": "impl",
            "idempotency_key": "k-6",
            "now": T_MID
        }),
    )
    .await;
    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(util::log_events(&core).is_empty());
}
