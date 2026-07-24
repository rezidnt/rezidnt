//! REGISTRY-CONVERGENCE ORACLE — the daemon-side seams that DR-046 §Decision 8
//! and §Decision 9 require, judged in-process (criteria C3 and C6).
//!
//! ## THIS FILE IS WSL-ONLY, AND THAT IS NOT A CHOICE
//!
//! `bins/rezidentd/src/main.rs` declares `mod runs`, `mod mcp` and `mod gates`
//! under `#[cfg(unix)]`. The whole daemon implementation is therefore absent
//! from a Windows host build, so NO daemon unit test can be seen by host
//! `/vet`. Everything on this board that could be lifted out of the daemon has
//! been: the manifest and single-emitter guards live in
//! `bins/rezidentd/tests/registry_convergence_structure.rs` (host-runnable,
//! text-only), and the principal, sink and conflict-variant guards live in the
//! git adapter's own suites (host-runnable, real repos). What is left here
//! genuinely needs `Daemon`, and is stated as WSL-only rather than quietly
//! landed as if host `/vet` covered it.
//!
//! ## RED MODE
//!
//! COMPILE-RED on both seams, neither of which exists:
//! `Daemon::repo_adapter` (there is no adapter cache — no crate depends on
//! `rezidnt-adapter-git` at all) and `crate::mcp::allocation_refusal_code`
//! (every allocation refusal collapses to `SPAWN_FAILED` inline in the
//! `fan_out` bridge). Once they compile, both tests are ASSERT-RED against any
//! implementation that keys the cache on a workspace id or that decides the
//! refusal code by inspecting an error MESSAGE.
//!
//! ## API this board PINS
//!
//! ```ignore
//! // in rezidnt-adapter-git — the Send-bounded dyn wrapper the `RepoSubstrate`
//! // doc comment already names as implementer scope ("a Send-bounded dyn
//! // wrapper is implementer scope if the supervisor needs one"). Needed here
//! // because `RepoSubstrate` uses `async fn` in trait and is not dyn-safe, and
//! // because DR-046 §Decision 8 requires this slice to land the INJECTABLE
//! // ALLOCATION SEAM that makes a conflict reachable from a test.
//! pub trait DynRepoSubstrate: Send + Sync {
//!     fn alloc_worktree(
//!         &self,
//!         req: WorktreeReq,
//!     ) -> std::pin::Pin<Box<dyn Future<Output = Result<Worktree, GitError>> + Send + '_>>;
//! }
//!
//! impl Daemon {
//!     /// The repo substrate for `repo_root`, CACHED BY CANONICALIZED ROOT.
//!     /// One instance per repo, shared by every workspace over it — the
//!     /// registry is per-repo, and two instances over one repo would be two
//!     /// allocators (DR-001).
//!     pub async fn repo_adapter(&self, repo_root: &Path)
//!         -> anyhow::Result<std::sync::Arc<dyn DynRepoSubstrate>>;
//!
//!     /// Override the cache with an injected substrate. THE seam DR-046 Item
//!     /// 3(a) says the owed I6 conflict test needs; without it a double-claim
//!     /// is unreachable from any test, because worktree paths are ULID-derived
//!     /// and nothing can pre-claim the path a task will take.
//!     pub fn with_repo_substrate(self, substrate: std::sync::Arc<dyn DynRepoSubstrate>) -> Self;
//! }
//!
//! /// Map an allocation failure onto the per-task refusal code (DR-046
//! /// §Decision 9). Reads the error CHAIN, so `anyhow` context does not erase
//! /// the discriminator.
//! pub(crate) fn allocation_refusal_code(err: &anyhow::Error) -> &'static str;
//! ```

use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use rezidnt_adapter_git::{DynRepoSubstrate, GitError, Worktree, WorktreeId, WorktreeReq};
use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_types::refs::CasRef;

use crate::mcp::allocation_refusal_code;
use crate::runs::{Daemon, RunRegistry};

/// A bare `Daemon` over a temp log + CAS — the same shape as the SP3 resolver
/// fixture in `bins/rezidentd/src/mcp.rs`. No transports, no spawns.
fn test_daemon() -> (tempfile::TempDir, Arc<Daemon>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Arc::new(Fabric::new(log, 1024));
    let cas = Arc::new(Cas::open(&dir.path().join("cas")).expect("open cas"));
    let daemon = Arc::new(Daemon::new(fabric, cas, Arc::new(RunRegistry::default())));
    (dir, daemon)
}

/// A real git repo with one commit — the adapter's on-open reconciliation scan
/// shells out to `git worktree list`, so an empty directory is not enough.
fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mkdir repo");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git CLI must be runnable");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "oracle@rezidnt.test"]);
    git(&["config", "user.name", "rezidnt oracle"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("README.md"), "# convergence fixture\n").expect("seed file");
    git(&["add", "."]);
    git(&["commit", "-m", "initial commit"]);
}

/// CRITERION C3 — the daemon's repo-adapter cache is keyed on the CANONICALIZED
/// REPO ROOT, not on a workspace id.
///
/// This is the load-bearing half of the convergence. The `GitAdapter` registry
/// is a per-repo JSONL file under the repo root, and the sole-allocator guard
/// works by finding a canonicalized path already in ITS OWN in-memory mirror of
/// that file. Two adapter instances over one repo therefore have two mirrors:
/// each claims a path the other does not know about, both write the registry
/// file, and the double-claim guard never fires. That is not a cache-efficiency
/// question — it is the difference between having a sole-allocator guard and
/// only appearing to (DR-001).
///
/// Keying on workspace id is the natural mistake, because the daemon already
/// holds `workspaces: HashMap<Ulid, OpenedWorkspace>` and each entry carries
/// its own `root`. Two workspaces over one repo is the exact case that breaks,
/// and it is the case a fan-out racing an ordinary spawn lands in — the reason
/// DR-046 §Decision 8 insists both allocation paths converge together.
///
/// Both legs are pinned: SAME repo through two different SPELLINGS yields ONE
/// adapter, and DIFFERENT repos yield DIFFERENT adapters. Without the second
/// leg, an implementation returning a single global adapter would pass.
#[tokio::test]
async fn the_repo_adapter_cache_is_keyed_on_the_canonicalized_repo_root() {
    let (dir, daemon) = test_daemon();
    let repo = dir.path().join("repo");
    let other = dir.path().join("other-repo");
    init_repo(&repo);
    init_repo(&other);

    // Two spellings of the SAME repo. A daemon keyed on the raw path it was
    // handed would treat these as two repos; canonicalization is what makes
    // them one, and canonicalization is already the registry's own key rule.
    let respelled = repo.join("..").join("repo");
    assert_eq!(
        std::fs::canonicalize(&respelled).expect("canonicalize respelled"),
        std::fs::canonicalize(&repo).expect("canonicalize repo"),
        "test setup: the two spellings name the same directory"
    );

    let first = daemon
        .repo_adapter(&repo)
        .await
        .expect("adapter for the repo");
    let second = daemon
        .repo_adapter(&respelled)
        .await
        .expect("adapter for the respelled repo");

    assert!(
        Arc::ptr_eq(&first, &second),
        "two requests for ONE repo must return ONE adapter. The worktree registry is per-repo \
         and its sole-allocator guard only sees claims made through its OWN instance, so a \
         second adapter over the same repo is a SECOND ALLOCATOR: each claims paths the other \
         does not know about and the double-claim guard never fires (DR-001). Keying this cache \
         on workspace id — the map the daemon already has — is exactly the bug, because two \
         workspaces over one repo is the case a fan-out racing an ordinary spawn lands in \
         (DR-046 §Decision 8)."
    );

    let elsewhere = daemon
        .repo_adapter(&other)
        .await
        .expect("adapter for the other repo");
    assert!(
        !Arc::ptr_eq(&first, &elsewhere),
        "and a DIFFERENT repo gets a DIFFERENT adapter — each repo owns its own registry file, \
         so one shared adapter would key every repo's claims into one namespace. Without this \
         leg a single global adapter would satisfy the assertion above."
    );
}

/// A substrate that refuses every allocation as a registry double-claim. The
/// injected stand-in for a contended tree — which cannot otherwise be produced,
/// because worktree paths are ULID-derived and no test can pre-claim the path a
/// task will take (DR-046 Item 3(a)).
///
/// `diff_summary` / `release_worktree` are implemented because
/// `DynRepoSubstrate` mirrors all three `RepoSubstrate` methods (a wrapper
/// covering only allocation would leave the daemon holding two handles to one
/// adapter through two traits). Neither is reachable from this board — the
/// substrate never allocates, so no `WorktreeId` it could be asked about
/// exists — and each says exactly that with the trait's own honest error rather
/// than a `todo!()` that would turn an unexpected call into a panic instead of
/// a diagnosable refusal.
struct ConflictingSubstrate;

impl DynRepoSubstrate for ConflictingSubstrate {
    fn alloc_worktree(
        &self,
        _req: WorktreeReq,
    ) -> Pin<Box<dyn Future<Output = Result<Worktree, GitError>> + Send + '_>> {
        Box::pin(async {
            Err(GitError::Conflict {
                path: "/repo/injected-contended-tree".to_string(),
                holder: "rezidnt".to_string(),
            })
        })
    }

    fn diff_summary(
        &self,
        wt: &WorktreeId,
    ) -> Pin<Box<dyn Future<Output = Result<CasRef, GitError>> + Send + '_>> {
        let wt = *wt;
        Box::pin(async move { Err(GitError::UnknownWorktree(wt)) })
    }

    fn release_worktree(
        &self,
        wt: &WorktreeId,
    ) -> Pin<Box<dyn Future<Output = Result<(), GitError>> + Send + '_>> {
        let wt = *wt;
        Box::pin(async move { Err(GitError::UnknownWorktree(wt)) })
    }
}

/// CRITERION C6 — the INJECTABLE ALLOCATION SEAM exists, and a worktree
/// conflict is reachable through it from a test.
///
/// DR-046 Item 3(a) is the reason this test exists and the reason the record
/// deferred the wiring: the owed I6 conflict test "cannot be written black-box
/// even with the registry wired", because fan-out worktree paths are
/// ULID-derived, so no test can pre-claim the path a task will take and two
/// fan-out tasks can never collide with each other. The record's own conclusion
/// is that making it testable needs an injectable allocation seam — a daemon
/// change — and §Decision 8 puts that seam in this slice.
///
/// So this test judges the seam itself: an injected substrate REPLACES the
/// cached one for every repo, and its refusal arrives as the structural
/// conflict variant that `allocation_refusal_code` maps to the new code. Those
/// two together are what make a conflicted fan-out task reachable at all.
///
/// NON-VACUITY: the override must actually override. The repo used here is a
/// real one whose genuine adapter would SUCCEED, so a `with_repo_substrate`
/// that quietly lost the injection would return `Ok` and fail this test rather
/// than passing it vacuously.
#[tokio::test]
async fn an_injected_substrate_makes_a_worktree_conflict_reachable() {
    use rezidnt_mcp::codes;

    let (dir, daemon) = test_daemon();
    let repo = dir.path().join("repo");
    init_repo(&repo);

    let daemon = Arc::new(
        Arc::try_unwrap(daemon)
            .unwrap_or_else(|_| panic!("sole owner"))
            .with_repo_substrate(Arc::new(ConflictingSubstrate)),
    );

    let substrate = daemon
        .repo_adapter(&repo)
        .await
        .expect("the injected substrate is returned for any repo");
    let outcome = substrate
        .alloc_worktree(WorktreeReq {
            name: "contended".to_string(),
            detach: true,
            ..WorktreeReq::default()
        })
        .await;

    let error = outcome.err().unwrap_or_else(|| {
        panic!(
            "the INJECTED substrate must replace the cached one: this repo is real and its \
             genuine adapter would have allocated successfully, so an `Ok` here means the \
             injection was silently dropped and the seam does not exist"
        )
    });
    assert!(
        matches!(error, GitError::Conflict { .. }),
        "the seam carries the structural conflict variant through unchanged: {error:?}"
    );
    assert_eq!(
        allocation_refusal_code(&anyhow::Error::new(error)),
        codes::WORKTREE_CONFLICT,
        "and the daemon maps it to the conflict refusal code — the two halves that together \
         make DR-044 §Decision 3's refused-sub rule REACHED rather than described (DR-046 \
         §Decision 8/9)"
    );
}

/// CRITERION C6 — the daemon maps a registry double-claim onto the NEW conflict
/// refusal code, and every other allocation failure onto `SPAWN_FAILED`.
///
/// This is the mapping the scripted-substrate board in
/// `crates/rezidnt-mcp/tests/worktree_conflict_code.rs` explicitly cannot
/// judge, and it is the only place the claim "a conflicted task carries the
/// conflict code" becomes a fact about the daemon rather than about a fake.
///
/// The CONTEXT leg is the one that matters. `launch_agent` returns
/// `anyhow::Result`, so by the time an allocation failure reaches the `fan_out`
/// bridge it has been wrapped in whatever context the path added. A mapping
/// that matched only the top-level error would silently degrade every real
/// conflict to `SPAWN_FAILED` while passing a naive unwrapped test — the
/// failure mode is invisible precisely where it costs most. Reading the chain
/// (a `downcast_ref` walk) is the requirement, and asserting it over a
/// CONTEXT-WRAPPED error is what pins it.
///
/// Both directions are pinned, mirroring
/// `lead_only_is_distinguishable_from_an_invalid_badge`: a conflict is never
/// `SPAWN_FAILED`, and an ordinary failure is never `WORKTREE_CONFLICT`. A
/// mapping that answered one code for everything satisfies neither pair.
#[test]
fn a_registry_double_claim_maps_to_the_conflict_code_and_nothing_else_does() {
    use rezidnt_mcp::codes;

    let conflict = || GitError::Conflict {
        path: "/repo/.rezidnt/worktrees/impl-01J".to_string(),
        holder: "rezidnt".to_string(),
    };

    assert_eq!(
        allocation_refusal_code(&anyhow::Error::new(conflict())),
        codes::WORKTREE_CONFLICT,
        "a sole-allocator double-claim is reported as a CONFLICT: the tree was contended and \
         re-issuing the call with the same keys is the honest retry (DR-044 §Decision 3, \
         DR-046 §Decision 9)"
    );
    assert_ne!(
        allocation_refusal_code(&anyhow::Error::new(conflict())),
        codes::SPAWN_FAILED,
        "and never as a broken spawn — collapsing them is the defect DR-046 §Decision 9 names"
    );

    // THE LOAD-BEARING LEG: anyhow context must not erase the discriminator.
    let wrapped = anyhow::Error::new(conflict())
        .context("allocate worktree for agent impl")
        .context("launch_agent");
    assert_eq!(
        allocation_refusal_code(&wrapped),
        codes::WORKTREE_CONFLICT,
        "the mapping reads the error CHAIN, not just its head. `launch_agent` returns \
         `anyhow::Result` and the allocation failure arrives at the `fan_out` bridge already \
         wrapped in context; a top-level-only match would degrade every REAL conflict to \
         `spawn.failed` while still passing an unwrapped test"
    );

    for (error, why) in [
        (
            anyhow::Error::new(GitError::Git(
                "git worktree add failed: no space left".into(),
            )),
            "a git-CLI failure is a broken spawn, not a contended tree — telling the caller to \
             retry with the same keys would loop forever",
        ),
        (
            anyhow::Error::new(GitError::Registry("bad registry line".into())),
            "a corrupt registry line is NOT a double-claim: the new code must not swallow every \
             registry-flavoured error, or it is `SPAWN_FAILED` under a new name",
        ),
        (
            anyhow::anyhow!("harness binary is not executable"),
            "and a failure that is not a GitError at all stays `spawn.failed`",
        ),
    ] {
        assert_eq!(
            allocation_refusal_code(&error),
            codes::SPAWN_FAILED,
            "{why}: {error:#}"
        );
        assert_ne!(
            allocation_refusal_code(&error),
            codes::WORKTREE_CONFLICT,
            "reverse direction, pinned explicitly: {why}"
        );
    }
}
