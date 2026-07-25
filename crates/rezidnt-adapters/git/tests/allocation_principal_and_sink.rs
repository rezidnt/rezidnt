//! REGISTRY-CONVERGENCE ORACLE — the allocation PRINCIPAL and the fact SINK
//! (DR-046 §Decision 8; criteria C1, C2, C4, C5 of the slice brief).
//!
//! HOST-RUNNABLE. No `#[cfg(unix)]` gate, no daemon, no socket: a real git repo
//! in a tempdir and the adapter's own seam. This is deliberate. DR-046
//! §Consequences (4) calls the repoint the highest-blast-radius change in the
//! arc, and criticises a guard whose only emitter-side proof is `#[cfg(unix)]`
//! and therefore outside host `/vet`. Everything on this board that CAN be
//! judged without a daemon is judged here.
//!
//! ## RED MODE (stated plainly)
//!
//! COMPILE-RED on the seam types this board pins into existence:
//! `Allocator`, `WorktreeReq::principal` / `workspace` / `correlation` /
//! `causation`, `WorktreeReq: Default`, `FactSink`, `GitAdapter::with_sink`.
//! None of them exist today (`crates/rezidnt-adapters/git/src/lib.rs`:
//! `WorktreeReq` is `name`/`branch`/`detach` only; `Inner::emit` builds a
//! `None`-workspace envelope and broadcasts).
//!
//! Once they compile, every test here is ASSERT-RED against today's behavior:
//! `alloc_worktree` hardcodes `"rezidnt"` into both the payload and the
//! `RegistryEntry`, so the delegating-principal test fails on the value; and
//! nothing routes a fact anywhere but the broadcast channel, so every sink
//! assertion fails on an empty recorder. Neither is a type-error-only red.
//!
//! ## API this board PINS (the implementer builds to EXACTLY this)
//!
//! ```ignore
//! /// The allocating PRINCIPAL — ontology `worktree.allocated.allocator` v1.
//! /// An ENUM, not a `String`: the ratified vocabulary is CLOSED and TAGGED
//! /// (`"rezidnt" | "run:<ULID>"`; a bare ULID is explicitly NOT legal, and
//! /// `"human"` is reserved and never emitted by rezidnt on this subject).
//! /// Making the illegal spelling unconstructible is the structural bound —
//! /// the same discipline DR-046 applied to the source guard.
//! #[derive(Debug, Clone, PartialEq, Eq, Default)]
//! pub enum Allocator {
//!     #[default]
//!     Rezidnt,
//!     Run(ulid::Ulid),
//! }
//!
//! impl Allocator {
//!     /// The verbatim value rendered onto the fact AND the registry entry.
//!     pub fn as_value(&self) -> String;
//! }
//!
//! /// Where an adapter fact goes when the daemon wants it on the FABRIC.
//! /// Injected, NOT called directly: `rezidnt-adapter-git` depends on
//! /// `rezidnt-types` only and must not grow a `rezidnt-fabric` dependency
//! /// (I4, substrates behind traits).
//! pub trait FactSink: Send + Sync {
//!     fn emit(&self, event: &rezidnt_types::Event) -> Result<(), GitError>;
//! }
//!
//! impl GitAdapter {
//!     pub fn with_sink(self, sink: std::sync::Arc<dyn FactSink>) -> Self;
//! }
//!
//! #[derive(Debug, Clone, Default)]
//! pub struct WorktreeReq {
//!     pub name: String,
//!     pub branch: Option<String>,
//!     pub detach: bool,
//!     pub principal: Allocator,
//!     pub workspace: Option<rezidnt_types::WorkspaceId>,
//!     pub correlation: Option<ulid::Ulid>,
//!     pub causation: Option<ulid::Ulid>,
//! }
//! ```
//!
//! ## Why the envelope fields ride the REQUEST, not the adapter
//!
//! Criterion C3 requires the daemon's adapter cache to be keyed on the
//! canonicalized REPO ROOT, so ONE adapter instance can serve TWO workspaces.
//! An adapter-level `workspace` would therefore be wrong by construction. The
//! envelope facts are per-allocation, so they ride the per-allocation request,
//! and the adapter carries them forward onto that worktree's later facts.
//!
//! ## Honest scope
//!
//! This board judges the ADAPTER. It cannot judge the double-emit hazard
//! (DR-046 §Decision 8: the daemon publishes its own `worktree.allocated` at
//! `runs.rs`, so a repoint without silencing one side emits two), because that
//! needs both emitters in one process. C4's whole-system leg is in
//! `bins/rezidentd/tests/registry_convergence_e2e.rs` (`#[cfg(unix)]`, so WSL
//! only) with a host-side structural backstop in
//! `bins/rezidentd/tests/registry_convergence_structure.rs` — a `tests/`
//! integration board that reads the manifest and `runs.rs` as TEXT, which is
//! why it runs on the host at all. (Corrected: this paragraph pointed at
//! `bins/rezidentd/src/registry_convergence_tests.rs`, which does not exist.
//! The similarly named `bins/rezidentd/src/runs/registry_convergence_tests.rs`
//! is the WSL-ONLY in-crate board — the daemon's `mod runs` is `#[cfg(unix)]` —
//! so the old pointer inverted the exact host/WSL distinction this paragraph
//! draws.) Said here so nobody reads the one-fact assertion below as covering
//! the whole-system leg.

mod util;

use std::sync::{Arc, Mutex};

use rezidnt_adapter_git::{Allocator, FactSink, GitAdapter, GitError, RepoSubstrate, WorktreeReq};
use rezidnt_types::{Event, WorkspaceId};
use ulid::Ulid;

/// A sink that RECORDS every fact handed to it. The daemon's real sink appends
/// to the fabric; this one answers "did the fact reach the sink at all, and
/// with which envelope".
#[derive(Default)]
struct RecordingSink {
    facts: Mutex<Vec<Event>>,
}

impl RecordingSink {
    fn facts(&self) -> Vec<Event> {
        self.facts.lock().expect("sink lock").clone()
    }

    fn of_subject(&self, subject: &str) -> Vec<Event> {
        self.facts()
            .into_iter()
            .filter(|e| e.subject.as_str() == subject)
            .collect()
    }
}

impl FactSink for RecordingSink {
    fn emit(&self, event: &Event) -> Result<(), GitError> {
        self.facts.lock().expect("sink lock").push(event.clone());
        Ok(())
    }
}

/// A sink that always REFUSES — standing in for a fabric append that failed.
#[derive(Default)]
struct FailingSink;

impl FactSink for FailingSink {
    fn emit(&self, _event: &Event) -> Result<(), GitError> {
        Err(GitError::Registry("sink refused the append".into()))
    }
}

/// A detached request: no branch, so the same `name` can be re-requested
/// without tripping git's "branch already exists".
fn detached_req(name: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        detach: true,
        ..WorktreeReq::default()
    }
}

/// Read the `allocator` value the registry JSONL persisted for `path`.
fn registry_allocator(repo: &std::path::Path, path: &std::path::Path) -> String {
    let entries = util::registry_entries_for(repo, path);
    assert_eq!(
        entries.len(),
        1,
        "exactly one registry entry for {}",
        path.display()
    );
    entries[0]["allocator"]
        .as_str()
        .unwrap_or_else(|| panic!("registry entry carries an `allocator`: {:#}", entries[0]))
        .to_string()
}

/// CRITERION C1 + C5 (the `"rezidnt"` half) — a request that names NO principal
/// allocates as `"rezidnt"` VERBATIM, on the FACT and in the REGISTRY entry.
///
/// This is the additive-default half of DR-046 §Decision 8: threading a
/// principal must not change what an ordinary allocation records. The ontology
/// is explicit that retrofitting a delegating value onto an ordinary allocation
/// would make "the daemon allocated this on its own initiative" unexpressible
/// (`spec/ontology.md` `worktree.allocated.allocator` v1), and
/// `bins/rezidentd/tests/open_flow.rs` pins the same value at the daemon edge.
/// That pin must survive the repoint; this is its host-visible counterpart.
#[tokio::test]
async fn an_unprincipled_request_allocates_as_rezidnt_verbatim() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let sink = Arc::new(RecordingSink::default());
    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .expect("open adapter")
        .with_sink(Arc::clone(&sink) as Arc<dyn FactSink>);

    let wt = adapter
        .alloc_worktree(detached_req("ordinary"))
        .await
        .expect("allocate");

    let allocated = sink.of_subject("worktree.allocated");
    assert_eq!(
        allocated.len(),
        1,
        "one allocation, one `worktree.allocated` fact on the sink: {:#?}",
        sink.facts()
    );
    assert_eq!(
        allocated[0].payload()["allocator"],
        serde_json::json!("rezidnt"),
        "a request naming no principal records `rezidnt` VERBATIM — the daemon on its own \
         initiative, the value every ordinary (non-fan-out) allocation keeps unchanged \
         (ontology `worktree.allocated.allocator` v1; DR-046 §Decision 8's additive-default \
         requirement): {:#}",
        allocated[0].payload()
    );
    assert_eq!(
        registry_allocator(&repo, &wt.path),
        "rezidnt",
        "and the REGISTRY entry records the same principal — the fact and the registry must \
         not disagree about who allocated the tree (both are hardcoded `\"rezidnt\"` today, \
         `git/src/lib.rs` payload and `RegistryEntry`, and both move together)"
    );
}

/// CRITERION C1 + C5 (the delegating half) — a request naming a LEAD RUN
/// allocates as the scheme-tagged `run:<ULID>`, never a bare ULID.
///
/// Whole-string equality, never prefix-only and never "contains the ULID": the
/// ontology fixes the `run:` scheme prefix precisely so a consumer never has to
/// guess whether a value is a sentinel or an id, and states that a bare ULID is
/// NOT legal on this field. `Allocator` being an enum is what makes the illegal
/// spelling unconstructible rather than merely untested.
///
/// NON-VACUITY: the same adapter, the same call, differing ONLY in the
/// principal, produces a DIFFERENT value from the test above. An
/// implementation that ignored the new field and kept hardcoding `"rezidnt"`
/// passes that test and fails this one; an implementation that repointed every
/// allocation at the delegating value passes this one and fails that one.
/// Neither single-value implementation can satisfy both.
#[tokio::test]
async fn a_delegating_request_allocates_as_the_scheme_tagged_lead_run() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let sink = Arc::new(RecordingSink::default());
    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .expect("open adapter")
        .with_sink(Arc::clone(&sink) as Arc<dyn FactSink>);

    let lead = Ulid::new();
    let wt = adapter
        .alloc_worktree(WorktreeReq {
            principal: Allocator::Run(lead),
            ..detached_req("delegated")
        })
        .await
        .expect("allocate");

    let expected = format!("run:{lead}");
    let allocated = sink.of_subject("worktree.allocated");
    assert_eq!(allocated.len(), 1, "one allocation, one fact");
    let value = allocated[0].payload()["allocator"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "`allocator` is a REQUIRED v1 field: {:#}",
                allocated[0].payload()
            )
        })
        .to_string();

    assert_eq!(
        value, expected,
        "a delegating request records the scheme-TAGGED `run:<lead ULID>`, so the log ALONE \
         answers \"which lead allocated this worktree\" (DR-044 §Decision 3, ontology \
         `worktree.allocated.allocator` v1)"
    );
    assert_ne!(
        value,
        lead.to_string(),
        "the delegating form is TAGGED, never a BARE ULID — the ontology's parse discipline \
         exists so a consumer never has to guess sentinel-vs-id"
    );
    assert_eq!(
        registry_allocator(&repo, &wt.path),
        expected,
        "the registry entry carries the delegating principal too: `RegistryEntry.allocator` \
         hardcodes `\"rezidnt\"` today and moves with the payload, or the sole-allocator \
         registry and the log disagree about the same allocation"
    );
}

/// CRITERION C1, the reserved sentinel — the principal vocabulary a REQUEST can
/// express is exactly `{rezidnt, run:<ULID>}`.
///
/// `"human"` is reserved for out-of-band OBSERVATION (`worktree.observed`) and
/// is never emitted by rezidnt on `worktree.allocated`. The adapter's discovery
/// branches test `allocator == "human"` to decide "already-observed, never
/// news" (`git/src/lib.rs`), so an allocation able to claim `"human"` would
/// make its own tree invisible to the reconciliation scan. Pinned as a NEGATIVE
/// over the rendered value so it holds however the enum is spelled.
#[tokio::test]
async fn no_request_can_render_the_reserved_human_sentinel() {
    for principal in [Allocator::Rezidnt, Allocator::Run(Ulid::new())] {
        let value = principal.as_value();
        assert_ne!(
            value, "human",
            "`human` is RESERVED for out-of-band observation and is never emitted by rezidnt on \
             `worktree.allocated`; the reconciliation scan reads it as \"already observed, never \
             news\", so an allocation claiming it would hide its own tree from the scan"
        );
        assert!(
            value == "rezidnt" || value.starts_with("run:"),
            "the rendered principal stays inside the ratified v1 vocabulary \
             {{\"rezidnt\", \"run:<ULID>\"}}: got {value:?}"
        );
    }
}

/// CRITERION C2 — the allocation fact reaches the INJECTED SINK carrying the
/// envelope the daemon supplied: `Some(workspace)`, the caller's correlation,
/// and the vet verdict id as causation.
///
/// This is the I3 half of DR-046 §Decision 8. The adapter's `Inner::emit`
/// today builds `Event::new(.., None /* workspace */, .., self.correlation, ..)`
/// and hands it to a `broadcast::Sender`. A broadcast is not an append: a fact
/// that only reaches live subscribers is not on the log, cannot be replayed,
/// and cannot be folded — so a naive repoint DROPS the allocation fact off the
/// log. The sink is the seam that fixes it without the adapter growing a
/// `rezidnt-fabric` dependency (I4).
///
/// The envelope is asserted field-by-field because each field has a distinct
/// consumer: `workspace` scopes every reducer that folds on it,
/// `correlation` keeps the allocation inside the open/spawn causal chain, and
/// `causation` is what makes "this tree was allocated BECAUSE vet passed"
/// answerable from the log. `bins/rezidentd/src/runs.rs` supplies exactly these
/// three today; the repoint must not lose them.
#[tokio::test]
async fn the_allocation_fact_reaches_the_sink_with_its_workspace_and_causation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let sink = Arc::new(RecordingSink::default());
    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .expect("open adapter")
        .with_sink(Arc::clone(&sink) as Arc<dyn FactSink>);

    let workspace = WorkspaceId::new(Ulid::new());
    let correlation = Ulid::new();
    let vet_verdict = Ulid::new();

    adapter
        .alloc_worktree(WorktreeReq {
            workspace: Some(workspace),
            correlation: Some(correlation),
            causation: Some(vet_verdict),
            ..detached_req("enveloped")
        })
        .await
        .expect("allocate");

    let allocated = sink.of_subject("worktree.allocated");
    assert_eq!(
        allocated.len(),
        1,
        "the allocation fact reaches the SINK, not just the broadcast — a fact that only hits \
         a broadcast channel is not on the log and is not replayable (I3, DR-046 §Decision 8). \
         Sink saw: {:#?}",
        sink.facts()
    );
    let fact = &allocated[0];
    assert_eq!(
        fact.workspace,
        Some(workspace),
        "the envelope carries `Some(workspace)` — the adapter's own `emit` passes `None` \
         today, and a workspace-less allocation fact folds into no workspace's graph"
    );
    assert_eq!(
        fact.correlation, correlation,
        "the envelope carries the CALLER's correlation, not the adapter's per-instance one — \
         the allocation belongs to the open/spawn causal chain that requested it"
    );
    assert_eq!(
        fact.causation,
        Some(vet_verdict),
        "the envelope carries the vet verdict id as causation, so \"this tree was allocated \
         BECAUSE vet passed\" is answerable from the log alone (I3/I6)"
    );
}

/// CRITERION C2, the commit-point leg — if the sink REFUSES the fact, the
/// allocation FAILS.
///
/// I3 says the append is the commit point. An allocation that succeeded while
/// its fact never reached the log would leave a tree on disk and in the
/// registry that the log does not know about — derived state that cannot be
/// rebuilt, which is the exact misdesign the invariant names. So a sink error
/// must propagate, not be swallowed the way the current broadcast send is
/// (`self.tx.send(event).is_err()` is deliberately tolerated today, correctly,
/// because "no live subscribers" is not a failure for a fan-out — but it is a
/// failure for an append).
///
/// Without this leg, an implementation that logged-and-continued on a failed
/// append would pass every other test on this board.
///
/// ## The rollback legs (added by remediation, 2026-07-24)
///
/// `outcome.is_err()` alone was the whole test, and it was the weaker half. The
/// module header's four-case fact-delivery split makes its STRONGEST claim about
/// this path — the refusal fails the allocation *and the tree just created is
/// taken back off disk* — and nothing asserted the second clause, so an
/// implementation that returned `Err` while leaving an unregistered worktree
/// behind passed. The disk and registry legs below are that clause.
///
/// Removal is BEST-EFFORT by construction (a `worktree remove` failure is
/// warned and the tree stays), so this asserts what the code does on the path
/// it can actually reach: in a healthy tempdir the removal succeeds, and the
/// registry — which is written only after a successful emit — was never touched
/// at all.
#[tokio::test]
async fn an_allocation_whose_fact_cannot_be_appended_fails() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .expect("open adapter")
        .with_sink(Arc::new(FailingSink) as Arc<dyn FactSink>);

    let outcome = adapter.alloc_worktree(detached_req("unappendable")).await;

    assert!(
        outcome.is_err(),
        "a sink refusal FAILS the allocation: the append is the commit point (I3), and an \
         allocation whose fact never reached the log is a tree on disk that the log does not \
         know about — underivable state. Got: {outcome:?}"
    );

    // DISK: the tree `git worktree add` created is gone. Located from the
    // adapter's own layout constant rather than a re-derived guess, so a layout
    // move cannot turn this leg into a check of a path nothing ever used.
    let tree = repo
        .join(rezidnt_adapter_git::WORKTREE_BASE)
        .join("unappendable");
    assert!(
        !tree.exists(),
        "the tree created for a refused allocation is taken back off disk. Left behind, it is a \
         worktree the log never heard of: `git worktree list` reports it, the next reconciliation \
         scan discovers it as a HUMAN tree, and the allocation that failed reappears as somebody \
         else's. Still at {}",
        tree.display()
    );

    // REGISTRY: nothing was claimed. The entry is written after the emit, so a
    // refusal must leave no claim — a claimed path with no log fact is the
    // sole-allocator registry asserting an allocation that never happened.
    let entries = util::registry_entries_if_any(&repo);
    assert!(
        entries.is_empty(),
        "a refused allocation claims nothing in the sole-allocator registry (DR-001): a claim \
         with no `worktree.allocated` on the log would block the path forever against an \
         allocation the log cannot show. Registry holds: {entries:#?}"
    );
}

/// The `observe` counterpart, and the ordering defect it pins (remediation,
/// 2026-07-24) — a REFUSED `worktree.observed` leaves NO durable registry
/// entry, and the discovery stays news.
///
/// ## The defect
///
/// `observe` persisted the `"human"` registry entry BEFORE emitting. On a
/// refusal it returned `Err` with that entry already on disk — and because the
/// entry IS the observed mark, the discovery became unrecoverable: the
/// re-observation arm returns `Ok(())` for any `"human"` entry, and the on-open
/// reconciliation scan skips `"human"` entries as already-observed. The
/// registry asserted a fact the log never received (I3), permanently and
/// silently. Same shape as the `debounce_loop` suppression-hash bug the same
/// remediation fixed, with the ordering inverted.
///
/// ## What passes and what fails
///
/// Three legs. The refusal fails the call; the registry FILE holds nothing
/// afterwards; and a RESTARTED adapter — which reloads that file and is the
/// only reader that can prove durability rather than in-memory state — still
/// reports the tree as a discovery. The third leg is the one the defect fails:
/// with a durable `"human"` entry present, pass 1 of the reconciliation scan
/// skips it as already-observed and pass 2 never sees it, so the restart is
/// silent and the fact is lost for good.
///
/// The human tree is created AFTER the adapter opens, deliberately: `open` runs
/// the reconciliation scan, so a tree that exists beforehand is discovered by
/// the scan and `observe` is never the emitter under test.
#[tokio::test]
async fn a_refused_observation_leaves_no_mark_and_stays_news() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let refusing = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .expect("open adapter")
        .with_sink(Arc::new(FailingSink) as Arc<dyn FactSink>);

    // A human tree, added out-of-band once the scan has already run.
    let human = tmp.path().join("human-tree");
    util::git(
        &util::plain_spelling(&repo),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &util::plain_spelling(&human).to_string_lossy(),
        ],
    );

    let outcome = refusing.observe(&human).await;
    assert!(
        outcome.is_err(),
        "a refused `worktree.observed` fails the observation — `observe`'s whole contract is \
         \"record this discovery\" (module header, fact-delivery split). Got: {outcome:?}"
    );
    let entries = util::registry_entries_if_any(&repo);
    assert!(
        entries.is_empty(),
        "and it leaves NO registry entry. A durable `\"human\"` entry here is the mark for a \
         fact the log never received: re-observation then returns `Ok(())` and the on-open scan \
         skips `\"human\"` entries, so the discovery is lost forever while the registry asserts \
         it happened (I3). Registry holds: {entries:#?}"
    );
    drop(refusing);

    // RESTART — the durability leg. A fresh adapter reloads the registry file
    // and reconciles against reality; the tree must come back as news.
    let restarted = GitAdapter::open(&repo, &tmp.path().join("cas2"))
        .await
        .expect("re-open adapter over the same repo");
    let facts = restarted.startup_facts();
    let observed: Vec<&Event> = facts
        .iter()
        .filter(|e| e.subject.as_str() == "worktree.observed")
        .filter(|e| {
            e.payload()["path"]
                .as_str()
                .and_then(|p| std::fs::canonicalize(p).ok())
                .is_some_and(|p| p == util::canon(&human))
        })
        .collect();
    assert_eq!(
        observed.len(),
        1,
        "a restart re-announces the discovery the log never received. Zero means the refused \
         attempt left its mark on disk after all — the entry the scan reads as \"already \
         observed, never news\" — and no reader will ever surface this tree again. Startup \
         facts: {facts:#?}"
    );
    assert_eq!(
        observed[0].payload()["allocator"],
        serde_json::json!("human"),
        "and it is recorded as a HUMAN tree (ontology `worktree.observed` v1): {:#}",
        observed[0].payload()
    );
}
