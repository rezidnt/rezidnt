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
//!
//! ## The two GUARDS added after Stage B (ordering disclosed, not hidden)
//!
//! The three boards above were written before the code they judge. The two at
//! the bottom of this file were NOT: they were written AFTER Stage B landed,
//! which inverts the house rule and is stated here rather than left for a
//! reader to discover.
//!
//! 1. `the_daemon_appends_an_on_open_reconciliation_discovery_to_the_log` and
//!    `a_restarting_daemon_appends_its_startup_collision_to_the_log` — Stage B
//!    drains `GitAdapter::startup_facts()` onto the fabric because
//!    `reconcile_on_open` runs INSIDE `open`, before a `FactSink` can be
//!    attached. `startup_facts` itself is well covered at the adapter level
//!    (`restart_and_discovery.rs`, `worktree_identity.rs`); what nothing judged
//!    is that the DAEMON appends them, which is the whole I3 claim. Written late
//!    because the alternative is shipping an I3 mechanism with no judge. One
//!    test per reconciliation pass, so neither leg's red can mask the other's.
//! 2. `a_conflicted_fan_out_task_is_refused_alone_and_its_siblings_still_spawn`
//!    — C7's fourth leg, DR-046 §Consequences (e). The earlier work order
//!    encoded three legs and refused to fake this one, because a registry
//!    double-claim inside a LIVE fan-out was unreachable. Stage B's seam
//!    (`with_repo_substrate` over all three `DynRepoSubstrate` methods) makes it
//!    reachable, so the refusal no longer stands.
//!
//! Both were verified RED before being reported green — see each test's own
//! RED-MODE note for the exact provocation and what it broke.

/// The `bounded_reason` boundary oracle (see the child file's header for why
/// it rides under THIS module: `runs.rs` is frozen under review, and this is
/// the one already-declared descendant of `runs` that can see its private
/// items). Its subject is unrelated to registry convergence; only the seam is
/// shared.
mod bounded_reason_tests;

use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use rezidnt_adapter_git::{
    DynRepoSubstrate, GitAdapter, GitError, Worktree, WorktreeId, WorktreeReq,
};
use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_types::refs::CasRef;
use rezidnt_types::{Event, SourceId, Subject, WorkspaceId};
use ulid::Ulid;

use crate::mcp::allocation_refusal_code;
use crate::runs::{Daemon, FabricSink, OpenedWorkspace, RunRegistry};

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
// ---------------------------------------------------------------------------
// GUARD 1 — the daemon APPENDS the adapter's on-open reconciliation facts (I3)
// ---------------------------------------------------------------------------

/// Run `git -C <dir> <args>`, asserting success. A free function rather than a
/// reuse of `init_repo`'s private closure, so no existing board changes shape.
fn run_git(dir: &Path, args: &[&str]) {
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
}

/// Everything the daemon's log holds, read back through the real replay path.
fn log_of(daemon: &Daemon) -> Vec<Event> {
    daemon
        .fabric
        .replay_since(None)
        .expect("replay the daemon's own log")
}

fn count_subject(events: &[Event], subject: &str) -> usize {
    events
        .iter()
        .filter(|e| e.subject.as_str() == subject)
        .count()
}

fn canon(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
        .to_string_lossy()
        .into_owned()
}

/// GUARD (I3) — the on-open reconciliation facts reach the LOG, because the
/// daemon appends them.
///
/// ## The hole this closes
///
/// `reconcile_on_open` runs INSIDE `GitAdapter::open`, which is strictly before
/// `with_sink` can attach a durable append seam. Every fact that scan mints
/// therefore reaches `startup_facts()` and the adapter's broadcast and NOTHING
/// else. A broadcast is not an append (I3): left undrained, a restarting daemon
/// reconciles its registry against out-of-band reality and puts nothing on the
/// log — unfoldable, unreplayable, invisible to `debrief`. Stage B drains them;
/// this is the only test that judges the drain.
///
/// The adapter half is already covered (`restart_and_discovery.rs` pins that
/// `startup_facts()` CONTAINS these facts, in both directions). This board never
/// calls `startup_facts()` — it reads the DAEMON'S LOG, which is the only place
/// the claim "the daemon appended them" can be falsified.
///
/// ## Two tests, not one, because each pass must be falsifiable alone
///
/// `worktree.observed` comes from reconcile pass 2 (reality the registry does
/// not hold); `worktree.conflict` comes from pass 1 (a registry claim reality no
/// longer honours). They are separate tests so that neither leg's red is hidden
/// behind the other's assertion firing first — the sibling test below is
/// `a_restarting_daemon_appends_its_startup_collision_to_the_log`.
///
/// ## RED MODE (observed, not asserted)
///
/// Provoked by commenting out the `for fact in adapter.startup_facts()` drain in
/// `Daemon::repo_adapter`: `worktree.observed` count 0, expected 1, over an
/// empty log — i.e. the daemon reconciled and told the log nothing, which is the
/// defect verbatim.
///
/// ## Non-vacuity
///
/// The count is exact and positive, so a scan that mints nothing fails rather
/// than passes. The second `repo_adapter` call pins the other direction: the
/// per-repo cache means one adapter, one reconciliation, so a drain that re-ran
/// or re-appended per call would double the count and fail here.
#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_appends_an_on_open_reconciliation_discovery_to_the_log() {
    let repo_home = tempfile::tempdir().expect("repo home");
    let repo = repo_home.path().join("repo");
    init_repo(&repo);

    // An out-of-band human tree, created while NO daemon is alive.
    let human = repo_home.path().join("human-wt");
    run_git(
        &repo,
        &["worktree", "add", "--detach", human.to_str().expect("utf8")],
    );

    let (_first_dir, first) = test_daemon();
    let substrate = first
        .repo_adapter(&repo)
        .await
        .expect("the daemon opens an adapter over the repo");

    let log = log_of(&first);
    assert_eq!(
        count_subject(&log, "worktree.observed"),
        1,
        "the on-open reconciliation scan discovered an out-of-band tree, and the DAEMON APPENDED \
         that fact to its log. The scan runs inside `GitAdapter::open`, before any `FactSink` can \
         be attached, so the adapter cannot append it — only the daemon's `startup_facts()` drain \
         can. Zero here means the daemon reconciled its registry against reality and recorded \
         nothing: a broadcast is not an append (I3). Log: {log:#?}"
    );
    let observed = log
        .iter()
        .find(|e| e.subject.as_str() == "worktree.observed")
        .expect("the observed fact just counted");
    assert_eq!(
        canon(Path::new(
            observed.payload()["path"]
                .as_str()
                .expect("v1 payload requires `path`")
        )),
        canon(&human),
        "and it is the RECONCILIATION fact for the tree actually discovered — not some other \
         `worktree.observed` that happened to be on the log"
    );

    // The cache means one adapter per repo, so one reconciliation: a second
    // request must not re-append. (A drain that re-ran per call would double.)
    let again = first
        .repo_adapter(&repo)
        .await
        .expect("the cached adapter for the same repo");
    assert!(
        Arc::ptr_eq(&substrate, &again),
        "test setup: the second request is the CACHED adapter, so nothing re-reconciled"
    );
    assert_eq!(
        count_subject(&log_of(&first), "worktree.observed"),
        1,
        "one reconciliation, one appended fact: a second `repo_adapter` call for the same repo \
         appends nothing further"
    );
    drop(again);
}

/// GUARD (I3), the RESTART half — a collision that happened while the daemon was
/// DOWN is on the restarted daemon's log, not merely in its memory.
///
/// This is the leg the drain exists for. A daemon that dies holding an allocated
/// tree, and comes back to find a foreign tree at that path, learns about the
/// takeover in `reconcile_on_open` — which is inside `GitAdapter::open`, before
/// a `FactSink` exists. Without the drain the only record is a broadcast to
/// subscribers that do not exist yet: the fleet's most safety-relevant fact,
/// unfoldable and unreplayable.
///
/// Split from its sibling above deliberately: run as one test, whichever
/// assertion fires first would mask the other's red.
///
/// ## RED MODE (observed, not asserted)
///
/// Same provocation — the drain commented out in `Daemon::repo_adapter`:
/// `worktree.conflict` count 0, expected 1, over an empty log.
///
/// ## Non-vacuity
///
/// The takeover is REAL git activity against a REAL prior allocation (the
/// registry entry carries the identity marker the scan probes), so a scan that
/// silently found nothing to reconcile fails this test instead of passing it.
#[tokio::test(flavor = "multi_thread")]
async fn a_restarting_daemon_appends_its_startup_collision_to_the_log() {
    let repo_home = tempfile::tempdir().expect("repo home");
    let repo = repo_home.path().join("repo");
    init_repo(&repo);

    // Allocate a tree through the daemon's own substrate, so the per-repo
    // registry holds a `rezidnt` claim on it (with an identity marker).
    let (_first_dir, first) = test_daemon();
    let substrate = first
        .repo_adapter(&repo)
        .await
        .expect("the daemon opens an adapter over the repo");
    let allocated = substrate
        .alloc_worktree(WorktreeReq {
            name: "restart-leg".to_string(),
            detach: true,
            ..WorktreeReq::default()
        })
        .await
        .expect("allocate a tree the registry will hold across the restart");
    let claimed = allocated.path.clone();
    drop(substrate);
    drop(first);

    // The out-of-band takeover, with no daemon alive to see it: rezidnt's tree
    // is removed and a foreign (unmarked) tree is created at the same path.
    let claimed_str = claimed.to_str().expect("utf8 worktree path").to_string();
    run_git(&repo, &["worktree", "remove", "--force", &claimed_str]);
    run_git(&repo, &["worktree", "add", "--detach", &claimed_str]);

    // The restart: a NEW daemon, a FRESH log, the SAME repo.
    let (_second_dir, second) = test_daemon();
    let _restarted = second
        .repo_adapter(&repo)
        .await
        .expect("the restarted daemon opens an adapter over the same repo");

    let restart_log = log_of(&second);
    assert_eq!(
        count_subject(&restart_log, "worktree.conflict"),
        1,
        "a collision that happened while the daemon was DOWN is surfaced at startup — and lands \
         on the restarted daemon's LOG. This is the pass-1 half of the reconciliation scan, and \
         it is the case the drain exists for: without it the only durable record of a takeover \
         would be a broadcast to subscribers that did not exist yet (I3). Log: {restart_log:#?}"
    );
    let conflict = restart_log
        .iter()
        .find(|e| e.subject.as_str() == "worktree.conflict")
        .expect("the conflict fact just counted");
    assert_eq!(
        canon(Path::new(
            conflict.payload()["path"]
                .as_str()
                .expect("v1 payload requires `path`")
        )),
        canon(&claimed),
        "the appended conflict names the CONTESTED registry key"
    );
    assert_eq!(
        count_subject(&restart_log, "worktree.observed"),
        0,
        "and NO `worktree.observed` accompanies it: a conflict is emitted INSTEAD of \
         double-tracking a registered path (DR-001). The daemon appends what the scan minted, \
         exactly — it does not re-derive facts of its own"
    );
}

// ---------------------------------------------------------------------------
// GUARD 2 — C7's fourth leg: a conflicted fan-out task is refused ALONE
// ---------------------------------------------------------------------------

/// A REAL `GitAdapter`, wired to the daemon's REAL [`FabricSink`], wrapped so
/// that exactly ONE agent's allocation lands on a path the registry already
/// holds for someone else.
///
/// The wrapper's entire contribution is one line: it rewrites the requested
/// NAME for the designated agent. Everything the test then judges is production
/// — the sole-allocator guard, the `worktree.conflict` emit, the append through
/// the sink, the `GitError::Conflict` variant, the daemon's mapping of it.
/// Nothing is fabricated for the assertions to find.
///
/// The rewrite exists because worktree names are `<agent>-<run ULID>` and the
/// ULID is minted INSIDE `launch_agent`: no test can predict, and therefore
/// pre-claim, the path a given task will take. That is the exact obstacle
/// DR-046 Item 3(a) names as the reason the conflict leg was deferred, and
/// substituting the NAME (not the outcome) is the smallest thing that removes
/// it. A wrapper that returned a canned `GitError::Conflict` would be judging
/// itself; this one cannot, because it never decides anything.
struct ContendedForOneAgent {
    inner: GitAdapter,
    /// Worktree-name prefix of the ONE task whose allocation must contend.
    conflict_prefix: String,
    /// The name already claimed in the registry by another allocator.
    contended_name: String,
}

impl DynRepoSubstrate for ContendedForOneAgent {
    fn alloc_worktree(
        &self,
        mut req: WorktreeReq,
    ) -> Pin<Box<dyn Future<Output = Result<Worktree, GitError>> + Send + '_>> {
        if req.name.starts_with(&self.conflict_prefix) {
            req.name = self.contended_name.clone();
        }
        <GitAdapter as DynRepoSubstrate>::alloc_worktree(&self.inner, req)
    }

    fn diff_summary(
        &self,
        wt: &WorktreeId,
    ) -> Pin<Box<dyn Future<Output = Result<CasRef, GitError>> + Send + '_>> {
        <GitAdapter as DynRepoSubstrate>::diff_summary(&self.inner, wt)
    }

    fn release_worktree(
        &self,
        wt: &WorktreeId,
    ) -> Pin<Box<dyn Future<Output = Result<(), GitError>> + Send + '_>> {
        <GitAdapter as DynRepoSubstrate>::release_worktree(&self.inner, wt)
    }
}

/// CRITERION C7, fourth leg / DR-044 §Decision 3 / DR-046 §Consequences (e) —
/// a fan-out task whose worktree is CONTENDED is refused ALONE: it carries
/// `worktree.conflict`, its siblings still spawn, and it is tallied nowhere.
///
/// ## What was uncovered before this
///
/// `fan_out_live_e2e::an_unallocatable_task_is_a_refused_sub_and_never_a_tallied_one`
/// pins the refusal SHAPE, but forces a permissions failure and asserts
/// `spawn.failed`, with every task refused. Its own honesty note states what it
/// cannot reach: "that a conflict raised INSIDE a live fan-out surfaces as this
/// refusal shape ... Making it testable needs an injectable allocation seam ...
/// a change to the daemon, not to this test." Stage B made that change. This
/// board is the leg that note left open, and it is MIXED — one task contends,
/// two do not — which is the only arrangement in which "its siblings still
/// proceed" is falsifiable at all.
///
/// ## RED MODE (observed, not asserted)
///
/// Two independent provocations, both in the `fan_out` bridge's `Err` arm:
///
/// - replacing `allocation_refusal_code(&e)` with `codes::SPAWN_FAILED` — the
///   code leg goes red (the collapse DR-046 §Decision 9 forbids);
/// - replacing the per-task `outcomes.push(refused(..))` with an early
///   `return Err(ToolRefusal::new(..))` — the whole call fails, so the sibling
///   AFTER the contended task never spawns. That is the rollback shape DR-044
///   §Decision 3 forbids, and it is why the contended task sits in the MIDDLE
///   of the call order rather than last.
///
/// ## Non-vacuity
///
/// The siblings' allocations are REAL (the wrapper delegates them untouched, to
/// a real adapter over a real repo), so a seam that leaked the injection or an
/// adapter that refused everything fails this test rather than passing it. The
/// conflict is counted EXACTLY once and named by path and holder, so a
/// fixture-minted or duplicated fact fails too.
#[tokio::test(flavor = "multi_thread")]
async fn a_conflicted_fan_out_task_is_refused_alone_and_its_siblings_still_spawn() {
    use rezidnt_mcp::{McpSubstrate, codes};
    use rezidnt_run::spec::ProjectSpec;
    use rezidnt_types::mcp::FanOutTask;

    use crate::mcp::McpBridge;

    const CONTENDED_NAME: &str = "contended-by-a-human";
    const LEAD_BADGE: &str = "0badc0de0badc0de";

    let repo_home = tempfile::tempdir().expect("repo home");
    let repo = repo_home.path().join("repo");
    init_repo(&repo);
    let harness = rezidnt_testkit::stub_harness(repo_home.path(), 0);

    let (_dir, daemon) = test_daemon();

    // The REAL adapter, with the daemon's REAL durable append seam — the same
    // construction `Daemon::repo_adapter` performs in production.
    let adapter = GitAdapter::open_with_cas(&repo, Arc::clone(&daemon.cas))
        .await
        .expect("open the git adapter over the repo")
        .with_sink(Arc::new(FabricSink {
            fabric: Arc::clone(&daemon.fabric),
        }));

    // The out-of-band claim. `observe` is the adapter's own public ingest for a
    // tree rezidnt did not allocate: it registers the path under allocator
    // "human". The directory is then removed, leaving the CLAIM without a tree
    // — which is what makes the later `git worktree add` succeed and the
    // SOLE-ALLOCATOR GUARD (not git) the thing that refuses.
    let contended = repo.join(".rezidnt").join("worktrees").join(CONTENDED_NAME);
    std::fs::create_dir_all(&contended).expect("create the contended path");
    adapter
        .observe(&contended)
        .await
        .expect("register the out-of-band claim");
    let contended_key = canon(&contended);
    std::fs::remove_dir_all(&contended).expect("leave the claim without a tree");

    let daemon = Arc::new(
        Arc::try_unwrap(daemon)
            .unwrap_or_else(|_| panic!("sole owner"))
            .with_repo_substrate(Arc::new(ContendedForOneAgent {
                inner: adapter,
                conflict_prefix: "sub-contended-".to_string(),
                contended_name: CONTENDED_NAME.to_string(),
            })),
    );

    // Three agents; the CONTENDED one is in the middle, so a bridge that
    // aborted the call on the first refusal would visibly lose `sub-b`.
    let spec_toml = format!(
        r#"[project]
name = "registry-convergence-fan-out"
repo = "{repo}"

[[agent]]
name = "sub-a"
harness = "claude-code"
worktree = "auto"
bin_override = "{harness}"

[[agent]]
name = "sub-contended"
harness = "claude-code"
worktree = "auto"
bin_override = "{harness}"

[[agent]]
name = "sub-b"
harness = "claude-code"
worktree = "auto"
bin_override = "{harness}"
"#,
        repo = repo.display(),
        harness = harness.display(),
    );
    let spec = ProjectSpec::from_toml_str(&spec_toml).expect("parse the fan-out spec");

    // The opened-workspace entry `begin_open` would insert, inserted directly:
    // `begin_open` spawns its declared agents from a DETACHED materialize task,
    // which would race this fan-out and put allocations of its own on the log,
    // making "exactly one worktree.conflict" a coin flip. The struct is the same
    // one the real open path builds; what is under test is `fan_out`.
    let ws = WorkspaceId::new(Ulid::new());
    daemon.workspaces.lock().await.insert(
        ws.ulid(),
        OpenedWorkspace {
            root: spec.repo.clone(),
            agents: spec.agents.clone(),
            gates: spec.gates.clone(),
            egress: spec.egress.clone(),
            spawn_keys: HashMap::new(),
        },
    );

    // The lead is LOG-DERIVED (`agent.spawned.badge_id == lead_badge_id`, I3),
    // so seeding the fact IS how a lead is presented to `fan_out`.
    let lead_run = Ulid::new();
    daemon
        .fabric
        .publish(
            Event::new(
                SourceId::new("rezidnt-run"),
                Some(ws),
                Subject::new("agent.spawned"),
                Ulid::new(),
                None,
                1,
                serde_json::json!({
                    "run": lead_run.to_string(),
                    "agent": "lead",
                    "harness": "claude-code",
                    "badge_id": LEAD_BADGE,
                }),
            )
            .expect("lead spawn envelope"),
        )
        .expect("publish the lead's spawn fact");

    let bridge = McpBridge {
        daemon: Arc::clone(&daemon),
    };
    let outcomes = bridge
        .fan_out(
            ws.ulid().to_string(),
            LEAD_BADGE.to_string(),
            vec![
                FanOutTask {
                    agent: "sub-a".to_string(),
                    idempotency_key: "conv-a".to_string(),
                },
                FanOutTask {
                    agent: "sub-contended".to_string(),
                    idempotency_key: "conv-contended".to_string(),
                },
                FanOutTask {
                    agent: "sub-b".to_string(),
                    idempotency_key: "conv-b".to_string(),
                },
            ],
        )
        .await
        .expect(
            "a contended TASK is not a failed CALL: the report is the per-task outcome vector \
             (DR-044 §Decision 1) — a whole-call refusal here is the rollback shape §Decision 3 \
             forbids",
        );

    assert_eq!(
        outcomes.len(),
        3,
        "one outcome per task, in call order (DR-044's honest-partial-failure shape): {outcomes:#?}"
    );
    let names: Vec<&str> = outcomes.iter().map(|o| o.agent.as_str()).collect();
    assert_eq!(
        names,
        vec!["sub-a", "sub-contended", "sub-b"],
        "outcomes are returned in CALL ORDER, refusals in place — a caller correlates by position \
         and key, so a refused task is never dropped from the vector: {outcomes:#?}"
    );

    // --- the refused task ----------------------------------------------------
    let refused = &outcomes[1];
    assert_eq!(
        refused.code.as_deref(),
        Some(codes::WORKTREE_CONFLICT),
        "a CONTENDED tree carries the conflict code, not `spawn.failed`. This is the first time \
         DR-044 §Decision 3's refused-sub rule is REACHED from a live fan-out rather than \
         described: the registry's sole-allocator guard refused the claim, and the daemon mapped \
         the structural `GitError::Conflict` through (DR-046 §Decision 9). Collapsing it into \
         `spawn.failed` would tell the caller to give up on a tree a retry could win: {refused:#?}"
    );
    assert!(
        refused.run.is_none(),
        "a refused task mints NO run — there is nothing to fail, so nothing can fold as a failed \
         sub (I6, DR-044 §Decision 3): {refused:#?}"
    );

    // --- the siblings still proceed -----------------------------------------
    for sibling in [&outcomes[0], &outcomes[2]] {
        assert_eq!(
            sibling.code, None,
            "a SIBLING of the contended task is not refused: one task's contended tree refuses \
             THAT task only. No rollback, no whole-call abort (DR-044 §Decision 3, DR-046 \
             §Consequences (e)): {sibling:#?}"
        );
        let run = sibling.run.as_deref().unwrap_or_else(|| {
            panic!("a sibling that proceeded returns its run: {sibling:#?}");
        });
        Ulid::from_string(run).unwrap_or_else(|_| {
            panic!("and that run is a real ULID the daemon minted: {sibling:#?}");
        });
    }
    assert_ne!(
        outcomes[0].run, outcomes[2].run,
        "the two siblings are DISTINCT runs — a single run echoed twice would satisfy the checks \
         above without two spawns having happened"
    );

    // --- the refused task is tallied NOWHERE --------------------------------
    let keys = daemon.workspaces.lock().await[&ws.ulid()]
        .spawn_keys
        .clone();
    assert!(
        keys.contains_key("conv-a") && keys.contains_key("conv-b"),
        "both siblings' idempotency keys entered the shared §9 map: {keys:#?}"
    );
    assert!(
        !keys.contains_key("conv-contended"),
        "the CONTENDED task's key did NOT: a refused task mints no run, so a key that resolved to \
         one would hand a later retry a run that never existed: {keys:#?}"
    );

    let log = log_of(&daemon);
    let lead_key = lead_run.to_string();
    let subs = log
        .iter()
        .filter(|e| {
            e.subject.as_str() == "agent.spawned"
                && e.payload()["lead_run"].as_str() == Some(lead_key.as_str())
        })
        .count();
    assert_eq!(
        subs, 2,
        "the LOG says two subs, not three: a refused task never inflates the lead's fan-out (I3 — \
         the log is the judge, not the return value)"
    );
    let view = rezidnt_state::orchestration_graph(&rezidnt_state::fold(log.iter()));
    let lead = view
        .leads
        .iter()
        .find(|l| l.lead_run == lead_key)
        .unwrap_or_else(|| panic!("the lead has subs, so it has a row: {view:#?}"));
    assert_eq!(
        lead.fan_out, 2,
        "and the PROJECTION agrees: fan_out counts the two subs that spawned (DR-044 §Decision 3)"
    );
    assert_eq!(
        lead.verdict_rollup.passed
            + lead.verdict_rollup.failed
            + lead.verdict_rollup.inconclusive
            + lead.verdict_rollup.pending,
        2,
        "the refused task is in NO VerdictRollup bucket — not passed, not failed, not \
         inconclusive, and not pending (I6, DR-044 §Consequences (e)): {lead:#?}"
    );

    // --- exactly ONE worktree.conflict, and it is the real one --------------
    assert_eq!(
        count_subject(&log, "worktree.conflict"),
        1,
        "exactly ONE `worktree.conflict` on the log: one collision, one fact (DR-001, S2's exit \
         criterion). The emitter is the adapter's own sole-allocator guard and the appender is \
         the daemon's `FabricSink` — neither is the test fixture, which only chose which task \
         would land on the contended name"
    );
    let conflict = log
        .iter()
        .find(|e| e.subject.as_str() == "worktree.conflict")
        .expect("the conflict fact just counted");
    assert_eq!(
        conflict.payload()["path"].as_str(),
        Some(contended_key.as_str()),
        "and it names the contended path"
    );
    assert_eq!(
        conflict.payload()["holder"].as_str(),
        Some("human"),
        "whose holder is the standing registry claim the fan-out lost to (v1)"
    );
    assert_eq!(
        count_subject(&log, "worktree.allocated"),
        2,
        "two allocations succeeded — the siblings' trees are REAL, allocated through the same \
         real adapter. A wrapper that refused everything, or a seam that lost the delegation, \
         would show 0 here and the sibling assertions above would be vacuous"
    );
}
