//! REGISTRY-CONVERGENCE ORACLE — the double-claim guard, made STRUCTURAL and
//! reachable through `alloc_worktree` (DR-046 §Decision 9; criteria C6 and the
//! once-forever half of C7).
//!
//! HOST-RUNNABLE, no `#[cfg(unix)]` gate. This board exists because DR-046
//! §Decision 9 owes a DISTINCT worktree-conflict refusal code, and a refusal
//! code is only as honest as the discriminator behind it. Today the sole
//! allocator's double-claim guard returns
//! `GitError::Registry(format!("path already registered: {canonical_str}"))`
//! (`crates/rezidnt-adapters/git/src/lib.rs`), which is a *string*. A daemon
//! that mapped that to a conflict code would be substring-matching an error
//! message to decide what to tell a caller — the exact failure mode DR-046
//! §"bound the source guard's window structurally" was written about.
//!
//! ## RED MODE (stated plainly)
//!
//! COMPILE-RED on `GitError::Conflict` (no such variant today) and on the
//! `WorktreeReq` seam fields this board shares with
//! `allocation_principal_and_sink.rs`. Once those compile, the first test is
//! still ASSERT-RED: today's guard returns the `Registry` variant, so the
//! `matches!(.., GitError::Conflict { .. })` assertion fires. The restart leg is
//! ASSERT-RED for a different reason — it drives the double-claim through
//! `alloc_worktree`, a path no existing suite reaches (`worktree_conflict.rs`
//! injects collisions through `observe`, the DISCOVERY seam, and
//! `restart_and_discovery.rs` proves durability through the reconciliation
//! scan; neither exercises the allocation-time guard after a restart).
//!
//! ## API this board PINS
//!
//! ```ignore
//! pub enum GitError {
//!     // ... existing variants unchanged ...
//!     /// The sole-allocator double-claim (DR-001): the canonicalized path is
//!     /// already registered to `holder`. STRUCTURAL, so a caller decides
//!     /// "contended, retry with the same keys" by MATCHING, never by
//!     /// substring-searching a message.
//!     #[error("worktree {path} is already claimed by {holder}")]
//!     Conflict { path: String, holder: String },
//! }
//! ```
//!
//! ## How the collision is made deterministic
//!
//! `alloc_worktree` runs `git worktree add` BEFORE it consults the registry, so
//! a naive second call with the same name fails inside git and never reaches
//! the guard. The reachable route is the real out-of-band one the sole-
//! allocator model exists for: allocate, then remove the tree OUT OF BAND with
//! the git CLI (the registry entry survives — rezidnt is the only writer of the
//! registry, and a human `git worktree remove` does not touch it), then
//! allocate again. `git worktree add` now succeeds, the canonicalized path
//! matches a live registry entry, and the guard fires. Requests are DETACHED
//! throughout so no branch name is re-created.

mod util;

use std::sync::{Arc, Mutex};

use rezidnt_adapter_git::{FactSink, GitAdapter, GitError, RepoSubstrate, WorktreeReq};
use rezidnt_types::{Event, WorkspaceId};
use ulid::Ulid;

#[derive(Default)]
struct RecordingSink {
    facts: Mutex<Vec<Event>>,
}

impl RecordingSink {
    fn count(&self, subject: &str) -> usize {
        self.facts
            .lock()
            .expect("sink lock")
            .iter()
            .filter(|e| e.subject.as_str() == subject)
            .count()
    }

    fn first(&self, subject: &str) -> Option<Event> {
        self.facts
            .lock()
            .expect("sink lock")
            .iter()
            .find(|e| e.subject.as_str() == subject)
            .cloned()
    }
}

impl FactSink for RecordingSink {
    fn emit(&self, event: &Event) -> Result<(), GitError> {
        self.facts.lock().expect("sink lock").push(event.clone());
        Ok(())
    }
}

fn detached_req(name: &str, workspace: WorkspaceId) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        detach: true,
        workspace: Some(workspace),
        correlation: Some(Ulid::new()),
        ..WorktreeReq::default()
    }
}

/// Remove the tree at `path` from git WITHOUT touching the rezidnt registry —
/// the out-of-band act the sole-allocator model guards against.
fn remove_out_of_band(repo: &std::path::Path, path: &std::path::Path) {
    util::git(
        repo,
        &[
            "worktree",
            "remove",
            "--force",
            path.to_str().expect("utf-8 worktree path"),
        ],
    );
}

/// CRITERION C6 — a double claim through `alloc_worktree` returns a STRUCTURAL
/// conflict error naming the contested path and its holder, and emits exactly
/// one `worktree.conflict`.
///
/// The structural half is the whole point: DR-046 §Decision 9 owes a refusal
/// code that lets a caller distinguish "the tree was contended, retry with the
/// same keys" from "this spawn is broken". A daemon can only mint that code
/// honestly if the adapter hands it a discriminator it can MATCH on. A string
/// message is not one — it is a literal that renames itself the next time
/// somebody improves the wording, and it would put the daemon's refusal
/// semantics at the mercy of a format string.
///
/// NON-VACUITY: the same adapter, a request that fails for an UNRELATED reason
/// (contradictory branch-and-detach), must NOT come back as `Conflict`. Without
/// that leg, an implementation that returned `Conflict` for every failure would
/// satisfy the first assertion and would make the new refusal code a lie.
#[tokio::test]
async fn a_double_claim_returns_a_structural_conflict_and_emits_exactly_one_fact() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);
    let workspace = WorkspaceId::new(Ulid::new());

    let sink = Arc::new(RecordingSink::default());
    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .expect("open adapter")
        .with_sink(Arc::clone(&sink) as Arc<dyn FactSink>);

    let wt = adapter
        .alloc_worktree(detached_req("contested", workspace))
        .await
        .expect("first allocation succeeds");
    let claimed = util::canon(&wt.path);

    // Out-of-band: the tree is gone from git, the registry claim is not.
    remove_out_of_band(&repo, &wt.path);

    let second = adapter
        .alloc_worktree(detached_req("contested", workspace))
        .await;

    let error = second.expect_err(
        "a second claim on a registered canonicalized path is REFUSED — the sole allocator \
         never takes over (DR-001, DR-044 §Decision 3)",
    );
    assert!(
        matches!(&error, GitError::Conflict { .. }),
        "the double-claim guard returns a STRUCTURAL conflict variant, not a generic registry \
         error carrying a message. DR-046 §Decision 9 owes a refusal code that distinguishes \
         \"contended, retry with the same keys\" from \"this spawn is broken\"; the daemon can \
         only mint that honestly by MATCHING a variant, never by substring-searching a format \
         string. Got: {error:?}"
    );
    if let GitError::Conflict { path, holder } = &error {
        assert_eq!(
            util::canon(std::path::Path::new(path)),
            claimed,
            "the conflict names the CONTESTED canonicalized path — the registry key (DR-001 \
             BINDING rule)"
        );
        assert!(
            !holder.is_empty(),
            "the conflict names the HOLDER (the principal already registered against that \
             path), so \"who has it\" is answerable without reading the registry file"
        );
    }

    assert_eq!(
        sink.count("worktree.conflict"),
        1,
        "EXACTLY ONE `worktree.conflict` per collision (S2 exit criterion, DR-001) — and it \
         reaches the SINK, i.e. the log, not only the broadcast (I3)"
    );
    let conflict = sink.first("worktree.conflict").expect("conflict fact");
    assert_eq!(
        conflict.workspace,
        Some(workspace),
        "the conflict fact carries the requesting workspace in its envelope, exactly as the \
         allocation fact does — a workspace-less refusal folds into no workspace's graph"
    );
    assert_eq!(
        sink.count("worktree.allocated"),
        1,
        "the REFUSED claim allocates nothing: one successful allocation, one allocated fact. \
         A conflict that also emitted an allocation would put a tree on the log that the \
         registry does not hold"
    );

    // NON-VACUITY: an unrelated failure is not dressed up as a conflict.
    let contradictory = adapter
        .alloc_worktree(WorktreeReq {
            name: "contradictory".to_string(),
            branch: Some("feat/x".to_string()),
            detach: true,
            ..WorktreeReq::default()
        })
        .await
        .expect_err("a request that is both branched and detached is rejected");
    assert!(
        !matches!(contradictory, GitError::Conflict { .. }),
        "an ordinary bad request is NOT a conflict — if every failure came back `Conflict`, \
         the new refusal code would tell callers to retry a request that can never succeed. \
         Got: {contradictory:?}"
    );
}

/// CRITERION C7 (the once-forever half) — the persisted `conflicted` mark makes
/// "exactly one `worktree.conflict`" survive a restart, on the ALLOCATION path.
///
/// DR-044 §Decision 3 fixes at most one conflict fact per collision. The
/// adapter already persists a `conflicted` flag on the registry entry for
/// exactly this reason, and `restart_and_discovery.rs` proves it across the
/// on-open RECONCILIATION scan. What no suite proves is the allocation-time
/// guard after a restart, which is the path the converged daemon will actually
/// take: a fan-out task re-claiming a contested tree in a fresh daemon process
/// must be refused SILENTLY (refused, but no second fact), or a restarting
/// daemon re-announces every historical collision it re-encounters.
///
/// Both legs are asserted, because either alone is satisfiable by a wrong
/// implementation: an adapter that forgot the flag emits a second fact (leg 1
/// fails), and an adapter that swallowed the refusal to stay quiet would let
/// the second claim through (leg 2 fails).
#[tokio::test]
async fn the_conflict_mark_survives_restart_on_the_allocation_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    let cas = tmp.path().join("cas");
    util::init_committed_repo(&repo);
    let workspace = WorkspaceId::new(Ulid::new());

    // --- process 1: allocate, then collide once -----------------------------
    let first_sink = Arc::new(RecordingSink::default());
    let first = GitAdapter::open(&repo, &cas)
        .await
        .expect("open adapter")
        .with_sink(Arc::clone(&first_sink) as Arc<dyn FactSink>);
    let wt = first
        .alloc_worktree(detached_req("durable", workspace))
        .await
        .expect("first allocation succeeds");
    remove_out_of_band(&repo, &wt.path);
    let _ = first
        .alloc_worktree(detached_req("durable", workspace))
        .await
        .expect_err("second claim refused");
    assert_eq!(
        first_sink.count("worktree.conflict"),
        1,
        "precondition: the collision was announced exactly once in the first process"
    );
    drop(first);

    // --- process 2: a fresh adapter over the same repo ----------------------
    // The tree the refused claim left behind is removed out of band again, so
    // the retry reaches the registry guard rather than failing inside git.
    remove_out_of_band(&repo, &wt.path);
    let second_sink = Arc::new(RecordingSink::default());
    let second = GitAdapter::open(&repo, &cas)
        .await
        .expect("reopen adapter")
        .with_sink(Arc::clone(&second_sink) as Arc<dyn FactSink>);

    let retry = second
        .alloc_worktree(detached_req("durable", workspace))
        .await;

    // Leg 2 first: the guard still REFUSES. Silence must come from the dedup
    // mark, never from the guard giving up.
    let error = retry.expect_err(
        "after a restart the contested path is STILL claimed — the registry is the truth and \
         it reloaded from disk, so the sole-allocator guard must still refuse (DR-001)",
    );
    assert!(
        matches!(&error, GitError::Conflict { .. }),
        "and it refuses with the same STRUCTURAL conflict variant, so a restarted daemon maps \
         it to the same refusal code (DR-046 §Decision 9). Got: {error:?}"
    );

    // Leg 1: one collision, one fact, FOREVER — restart notwithstanding.
    let announced = second_sink.count("worktree.conflict")
        + second
            .startup_facts()
            .iter()
            .filter(|e| e.subject.as_str() == "worktree.conflict")
            .count();
    assert_eq!(
        announced, 0,
        "no SECOND `worktree.conflict` for a collision already announced — not from the on-open \
         reconciliation scan and not from the re-claim. The persisted `conflicted` flag is what \
         makes \"exactly one\" mean forever rather than once per process (DR-044 §Decision 3, \
         S2 remediation)"
    );
}
