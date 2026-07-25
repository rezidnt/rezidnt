//! DR-049 §Decision 3 — the MACAROON LEG of the `release_worktree` door.
//!
//! ## Why this board exists
//!
//! `release_worktree` deliberately widens admission relative to `kill_run`: it
//! sits behind `check_badge`'s DUAL path under verb `"merge"`, so an OPERATOR
//! badge OR a verified agent macaroon may call it, where `kill_run` is
//! operator-only (DR-032 §1). That widening is a policy call the slice made,
//! and it is on a REPO-MUTATING door — the call removes a worktree from disk and
//! closes its sole-allocator registry claim.
//!
//! The DR-049 e2e board (`bins/rezidentd/tests/dr049_release_lifecycle_e2e.rs`)
//! drives the operator leg against a live daemon. The macaroon leg had no test
//! at all: the rustdoc on `call_release_worktree` admitted as much ("the lead
//! leg is admitted by the same door every other run-scoped mutation uses"),
//! which is an argument, not a judge. A widened authorization path with no test
//! is exactly the shape that ships wrong quietly — nothing fails when the wider
//! door is wrong, it simply admits more than anyone intended.
//!
//! ## Scope, stated
//!
//! This is a DOOR board, not a lifecycle board. The substrate is a recording
//! fake, so nothing here removes a real tree — the e2e board owns the on-disk
//! and folded consequences. What is judged here is exactly the authorization
//! decision and whether the admitted call REACHED the substrate, which is the
//! only thing a door can be right or wrong about.
//!
//! Both legs are asserted for one reason: an "admitted" assertion alone cannot
//! distinguish a door that verifies the macaroon from a door that waves
//! everything through. The narrowed-verb refusal is the control that makes the
//! admission mean something.

mod util;

use std::sync::Arc;
use std::sync::Mutex;

use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_mcp::{
    BadgeBook, BoxFuture, KillAck, McpCore, McpSubstrate, OpenAck, PermitConfig, ToolRefusal,
};
use rezidnt_run::badge::{Caveat, Macaroon, RootKey};
use serde_json::json;

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const T_MID: &str = "2026-07-22T12:00:00Z";
const T_LATE: &str = "2026-07-23T00:00:00Z";
const PATH: &str = "/tmp/rezidnt/wt/01DR049RELEASEMACAROON0000";

fn root() -> RootKey {
    RootKey::from_bytes([9u8; 32])
}

/// A lead's agent macaroon carrying the `merge` verb — the badge shape the
/// daemon injects into a governed run (`REZIDNT_BADGE`), narrowed to the verbs
/// a lead legitimately exercises.
fn lead_macaroon(verbs: &[&str]) -> Macaroon {
    Macaroon::mint(
        &root(),
        "run-01DR049RELEASEMACAROONDOOR",
        vec![
            Caveat::Workspace {
                workspace: WS.into(),
            },
            Caveat::Verb {
                verbs: verbs.iter().map(|v| (*v).to_string()).collect(),
            },
            Caveat::Expiry {
                not_after: T_LATE.into(),
            },
        ],
    )
}

/// Records the paths `release_worktree` was driven with. Removes nothing: this
/// board judges the DOOR, and the substrate call is the observable that says
/// the door admitted rather than silently no-opped.
#[derive(Default)]
struct RecordingReleaseSubstrate {
    released: Mutex<Vec<String>>,
}

impl RecordingReleaseSubstrate {
    fn released(&self) -> Vec<String> {
        self.released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl McpSubstrate for RecordingReleaseSubstrate {
    fn open_project(&self, _spec_toml: String) -> BoxFuture<Result<OpenAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                "substrate.unavailable",
                "release-only test substrate",
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
                "release-only test substrate",
            ))
        })
    }

    fn permit_config_for(&self, _run: String) -> BoxFuture<Option<PermitConfig>> {
        Box::pin(async { None })
    }

    fn kill_run(&self, _run: String) -> BoxFuture<Result<KillAck, ToolRefusal>> {
        Box::pin(async {
            Err(ToolRefusal::new(
                "substrate.unavailable",
                "release-only test substrate",
            ))
        })
    }

    fn release_worktree(&self, path: String) -> BoxFuture<Result<(), ToolRefusal>> {
        self.released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(path);
        Box::pin(async { Ok(()) })
    }
}

/// A core with the daemon root key wired and NO operator badge admitted, so the
/// only way through the door is the macaroon leg. An empty `BadgeBook` is the
/// point: it forecloses the reading that path 1 quietly admitted the caller.
fn macaroon_only_core() -> (
    tempfile::TempDir,
    Arc<McpCore>,
    Arc<RecordingReleaseSubstrate>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let substrate = Arc::new(RecordingReleaseSubstrate::default());
    let core = McpCore::new(fabric, BadgeBook::new())
        .with_root_key(root())
        .with_substrate(substrate.clone());
    (dir, Arc::new(core), substrate)
}

/// THE GAP THIS BOARD CLOSES — a verified agent macaroon carrying the `merge`
/// verb is ADMITTED by `release_worktree`, and the call reaches the substrate.
///
/// DR-049 §Decision 3 says "operator **or** lead calls the release verb", and
/// `call_release_worktree` implements that as `check_badge(.., "merge")` — the
/// DUAL path — rather than `kill_run`'s operator-only `check_operator_badge`.
/// Until this test existed, nothing in the tree distinguished the shipped DUAL
/// door from an operator-only one: the e2e board presents an operator badge,
/// which both doors admit identically.
///
/// The substrate leg is the non-vacuity guard. A door that returned a bare
/// success without driving the release would satisfy an `isError == false`
/// assertion while releasing nothing.
#[tokio::test]
async fn release_worktree_admits_a_verified_lead_macaroon_under_the_merge_verb() {
    let (_dir, core, substrate) = macaroon_only_core();
    let m = lead_macaroon(&["spawn", "merge", "open"]);

    let result = util::tool_call(
        &core,
        1,
        "release_worktree",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "path": PATH,
            "now": T_MID
        }),
    )
    .await;

    assert_eq!(
        result["isError"],
        json!(false),
        "DR-049 §Decision 3 admits a LEAD, not only an operator: `call_release_worktree` goes \
         through `check_badge`'s DUAL path under verb \"merge\", so a verified agent macaroon \
         carrying that verb must be admitted. No operator badge is admitted on this core, so a \
         refusal here means the macaroon leg does not work — got {result:#}"
    );
    assert_eq!(
        substrate.released(),
        vec![PATH.to_string()],
        "and the admitted call REACHED the substrate with the requested path — an admission that \
         drives no release is a door reporting success for nothing"
    );
}

/// THE CONTROL — the same lead macaroon, narrowed to verbs that EXCLUDE
/// `merge`, is refused `badge.invalid`, and nothing is released.
///
/// Without this leg the test above cannot tell a verifying door from an open
/// one. It also pins the second settlement the slice made: release reuses the
/// EXISTING `merge` verb rather than minting a new one, so a badge narrowed
/// away from `merge` loses release along with it. A base badge carries no
/// `Verb` caveat and is unaffected either way — this only decides what a
/// NARROWED badge may still do.
#[tokio::test]
async fn a_macaroon_narrowed_away_from_merge_cannot_release() {
    let (_dir, core, substrate) = macaroon_only_core();
    let m = lead_macaroon(&["spawn", "open"]);

    let result = util::tool_call(
        &core,
        2,
        "release_worktree",
        json!({
            "badge": m.to_wire(),
            "workspace": WS,
            "path": PATH,
            "now": T_MID
        }),
    )
    .await;

    util::assert_tool_refusal(&result, rezidnt_mcp::codes::BADGE_INVALID);
    assert!(
        substrate.released().is_empty(),
        "the refusal lands BEFORE any effect (I3): a badge without the `merge` verb releases \
         nothing. Released: {:?}",
        substrate.released()
    );
}
