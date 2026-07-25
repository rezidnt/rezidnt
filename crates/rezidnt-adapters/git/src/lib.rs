//! rezidnt git adapter (doc §7): gix reads, git-CLI mutations, notify
//! watcher, sole-allocator worktree registry (DR-001).
//!
//! ## Contract (pinned by the S2 oracle tests; payloads ratified in
//! `spec/ontology.md`, S2 set 2026-07-17)
//!
//! - **Sole allocator (DR-001, BINDING):** every worktree is registered under
//!   its canonicalized path in the [`REGISTRY_PATH`] file with an `allocator`
//!   field. A second claim on an already-registered canonicalized path emits
//!   the `worktree.conflict` fact AT LEAST ONCE — exactly once on the
//!   crash-free path (the dedup mark persists after the emit), but the emit
//!   precedes the mark-persist, so a crash between them can re-emit on restart.
//!   Never silent double-tracking, never a duplicate registry entry.
//! - **Registry format (DEFAULT):** JSON Lines at
//!   `<repo>/.rezidnt/registry.jsonl` ([`REGISTRY_PATH`], moved off a collision
//!   with the daemon's worktree directory — see the constant),
//!   one live entry per line: `{"path": <canonicalized>, "allocator":
//!   "rezidnt"|"run:<ULID>"|"human", "branch"?: <string>, "id"?: <ULID>,
//!   "allocated_event"?: <ULID>, "conflicted"?: <bool>}`. The last three are
//!   the S2-remediation additions that make allocation identity and the
//!   exactly-once marks durable across restarts. The format evolves
//!   additively; pre-remediation lines parse with these migration defaults:
//!   missing `id`/`allocated_event` → the allocation is not releasable by id
//!   (ids were process-local before the fields existed, so nothing is lost)
//!   and later facts carry no causation; missing `conflicted` → `false` (a
//!   legacy collision surfaced before the upgrade may re-surface at most
//!   once). The observed mark needs no field, because
//!   [`GitAdapter::observe`] emits BEFORE it persists: a `"human"` entry is
//!   written only after `worktree.observed` was accepted, so the entry IS the
//!   mark. (Corrected 2026-07-24: this read "a `"human"` entry exists only
//!   because `worktree.observed` was emitted" while the code persisted first —
//!   so a refused fact left a durable mark for an entry the log never
//!   received, and both the re-observation arm and the reconciliation scan
//!   then treated the tree as already observed forever. The claim is now true
//!   because the ordering was fixed, not because the sentence was rewritten.)
//!   `release_worktree` closes (removes) the entry — though nothing in
//!   production calls it yet; see [`RepoSubstrate::release_worktree`].
//! - **On-open reconciliation scan (S2 remediation):** [`GitAdapter::open`]
//!   compares the reloaded registry against `git worktree list --porcelain`.
//!   Intact rezidnt allocations (the private-gitdir identity marker carries
//!   the registered [`WorktreeId`] — branch is NOT identity, S2-T3) are
//!   rebuilt live — releasable under their persisted id, re-watched; a tree
//!   without (or with a mismatched) marker on a rezidnt-registered path is a
//!   takeover, surfaced as `worktree.conflict` at least once (exactly once on
//!   the crash-free path — the mark persists after the emit); unregistered
//!   linked
//!   trees are discovered through the same dedup path as
//!   [`GitAdapter::observe`]. Scan facts ride the broadcast and are pinned
//!   via [`GitAdapter::startup_facts`].
//! - **Watcher (DEFAULT debounce fixed by ontology):** `alloc_worktree`
//!   starts the notify watch on the new tree; filesystem writes are debounced
//!   250 ms ([`DEBOUNCE_MS`], trailing-edge: emission happens once the tree
//!   has been quiet that long) and surface as `diff.ready` carrying the diff
//!   summary as a CAS ref (I2 — never inline diff bytes). S2 exit criterion:
//!   `diff.ready` lands within 1 s of the write, post-debounce.
//! - **`diff.ready` has TWO emitters, and they are not the same fact
//!   (disclosed 2026-07-24, registry-convergence remediation).** This adapter's
//!   watcher is one; `run_pre_merge` in `bins/rezidentd/src/runs.rs` is the
//!   other. Before the repoint the watcher was unreachable from the daemon, so
//!   only one ran in production; the repoint put both on the golden path and
//!   said so nowhere, which is why it is said here. The split is deliberate and
//!   the two are distinguished on the wire by `source`:
//!   - **watcher, `source` = [`SOURCE_ID`] (`"git-adapter"`)** — the CONTINUOUS
//!     observation: N facts over an allocation's life, one per quiet period, a
//!     consecutive identical summary suppressed, `causation` = the allocation
//!     fact. Asynchronous and best-effort by construction: nothing waits on it
//!     and no caller can be failed by it.
//!   - **daemon, `source` = `"rezidnt-adapter-git"`** — the GATE-TIME pin:
//!     exactly one per `pre_merge`, minted synchronously at the gate so the
//!     verified diff is pinned deterministically (I6) rather than depending on
//!     a debounced task having happened to fire, `causation` = the run's
//!     completion fact. `bins/rezidentd/tests/golden_path.rs` pins its ordering
//!     before `gate.entered(pre_merge)`.
//!
//!   This is NOT the double-emit that `worktree.allocated` was ruled on: that
//!   was two records of ONE occurrence, which folds one allocation twice. These
//!   are records of two different observations at two different instants;
//!   `WorktreeState.last_diff` takes the most recent and nothing counts them.
//!   Ownership is pinned by `bins/rezidentd/tests/registry_convergence_e2e.rs`
//!   (counts on a replayed daemon log) with a host-side backstop in
//!   `bins/rezidentd/tests/registry_convergence_structure.rs`. The daemon's
//!   `source` spelling is a legacy wart — it names the adapter for a fact the
//!   adapter did not mint — kept because the value is wire-visible and pinned
//!   by golden fixtures; renaming it is a `/subject` question, not a comment's.
//!   `spec/ontology.md`'s `diff.ready` emitter cell names the watcher only and
//!   is owed the second-emitter clause; that file is warden-only.
//! - **[`GitAdapter::observe`]** is the ingest point for a worktree discovered
//!   out-of-band (human `git worktree add`) — NO production caller today; the
//!   reconciliation scan's pass 2 is what actually surfaces such trees, running
//!   the same dedup rule at startup. Unregistered path → `worktree.observed`
//!   (allocator `"human"`, registered so re-observation stays silent);
//!   already-registered path → `worktree.conflict`, deduplicated per
//!   canonicalized path so repeated observation of the same collision emits
//!   nothing further once the mark is persisted: the dedup marks persist in the
//!   registry, so on the crash-free path restart does not resurface a fact.
//!   Because the emit precedes the mark-persist in BOTH arms, the fact is
//!   at-least-once — a crash in that window can resurface it on restart.
//! - **Facts** ride the envelope with `source` = [`SOURCE_ID`], `v = 1`, and
//!   payloads per `spec/ontology.md`. `workspace`, `correlation` and
//!   `causation` ride the ALLOCATION REQUEST ([`WorktreeReq`]) and are carried
//!   forward onto that worktree's later facts; a request naming no correlation
//!   falls back to a per-instance ULID minted at [`GitAdapter::open`]
//!   (DEFAULT). `diff.ready` and `worktree.released` carry the allocation
//!   fact's id as `causation`.
//! - **Fact delivery (DR-046 §Decision 8, I3):** facts always ride the
//!   broadcast. When the daemon injects a [`FactSink`] via
//!   [`GitAdapter::with_sink`], each fact ALSO goes through it first — a
//!   broadcast is a fan-out to live subscribers, not an append, and only an
//!   append is a commit point. What a refusal does then depends on whether
//!   there is an operation left to fail, and the honest split is:
//!   - `worktree.allocated` — the refusal FAILS the allocation, and removal of
//!     the tree just created is attempted BEST-EFFORT: `git worktree remove
//!     --force` runs, and a failure is logged at `warn` leaving the tree on
//!     disk (there is no second mechanism behind it). The append is the commit
//!     point, so nothing is registered and nothing is tracked live either way.
//!     (Narrowed 2026-07-24: this said the tree "is taken back off disk",
//!     unconditional prose over best-effort code.) The refusal, the removal on
//!     the path a test can reach, and the empty registry are asserted by
//!     `tests/allocation_principal_and_sink.rs`; the branch where `git worktree
//!     remove` itself fails is stated here and guarded nowhere — provoking it
//!     needs a git failure a tempdir will not produce.
//!   - `worktree.conflict` on the allocation path — the refusal does NOT
//!     convert the conflict into an append error: a double claim is a fact
//!     about the REGISTRY, true whether or not the log accepted the news, so
//!     [`GitError::Conflict`] is returned either way (I6 — a real double claim
//!     must never degrade into a generic spawn failure). The dedup mark is then
//!     left UNSET, so the collision is re-announced on the next claim rather
//!     than silently swallowed forever.
//!   - `diff.ready` — there is NO operation to fail: [`debounce_loop`] is a
//!     detached task with no caller. A refusal is logged at `warn` and the
//!     suppression hash is NOT advanced, so the same summary is re-emitted at
//!     the next filesystem event instead of being suppressed as a duplicate.
//!     Stated plainly: if no further write ever comes, that summary is absent
//!     from the log. Nothing on the golden path depends on it — the gate-time
//!     `diff.ready` above is minted by the daemon and its append failure does
//!     fail the merge.
//!   - every other fact (`worktree.observed`, `worktree.released`, and
//!     `worktree.conflict` raised through [`GitAdapter::observe`]) propagates
//!     the refusal to its caller unchanged — each of those operations IS
//!     "record this", so failing to record it is failing the operation.
//!
//!   A REGISTRY-WRITE failure after a successful emit is a separate question
//!   from a refused emit, and the two were answered inconsistently across
//!   `observe` and `alloc_worktree` until 2026-07-24. The rule ("mark, not
//!   claim") is stated in full on [`GitAdapter::observe`] and marked at the one
//!   site that deliberately differs.
//!
//!   The on-open reconciliation scan runs inside
//!   [`GitAdapter::open`], before any sink can be injected, so its facts reach
//!   the sink only via [`GitAdapter::startup_facts`] — the seam that already
//!   exists for exactly that reason.

mod summary;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rezidnt_cas::Cas;
use rezidnt_types::{Event, SourceId, Subject, WorkspaceId, refs::CasRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::Instrument;
use ulid::Ulid;

/// Filesystem-event debounce, milliseconds. Fixed by the ontology
/// (`diff.ready` emitter note: "debounced 250 ms"); DEFAULT per doc §7.
pub const DEBOUNCE_MS: u64 = 250;

/// `source` field on every fact this adapter emits. The ontology names the
/// git adapter (RepoSubstrate) as the owning emitter of `worktree.allocated`.
pub const SOURCE_ID: &str = "git-adapter";

/// Sole-allocator worktree registry file, relative to the repo root (DR-001).
///
/// **Moved 2026-07-24 (registry-convergence slice; DEFAULT, note in lieu of a
/// `/dr`).** This was `.rezidnt/worktrees`, which collided head-on with the
/// daemon's own worktree base directory of the identical spelling
/// (`bins/rezidentd/src/runs.rs`, `repo.join(".rezidnt").join("worktrees")` +
/// `create_dir_all`). One is a JSONL file, the other a directory; they cannot
/// coexist, and [`GitAdapter::open`] would have tried to `read_to_string` a
/// directory the moment the daemon and the adapter met. The registry moves off
/// the collision rather than the daemon's shipped v0.0.1 layout moving.
///
/// **No migration code exists, deliberately.** No crate depends on
/// `rezidnt-adapter-git` today, so no production registry file has ever been
/// written at the old path. Migrating a file that cannot exist would be
/// ceremony asserting a history the tree does not have.
pub const REGISTRY_PATH: &str = ".rezidnt/registry.jsonl";

/// Directory hosting allocated worktrees, relative to the repo root (DEFAULT).
/// See [`GitAdapter::derive_worktree_path`] for why the layout is in-repo.
///
/// `pub` so a test can locate an allocated tree from the constant rather than
/// re-spelling the layout — the same reason [`REGISTRY_PATH`] is public, and
/// the same failure it prevents (a literal that silently stops naming the file
/// the adapter uses; that has happened once in this arc already).
pub const WORKTREE_BASE: &str = ".rezidnt/worktrees";

/// Identity-marker filename inside a worktree's PRIVATE gitdir
/// (`<repo>/.git/worktrees/<name>/`), carrying the persisted [`WorktreeId`]
/// (pre-S4 remediation, S2-T3). The location is the honest discriminator:
/// it survives every in-tree operation (checkout/switch/commit), never rides
/// the working tree or its diffs, and is destroyed by `git worktree remove` —
/// so a foreign re-add at the same path/branch has no marker and is detected,
/// while an occupant branch switch keeps it intact. DEFAULT mechanism.
const IDENTITY_MARKER: &str = "rezidnt-worktree-id";

/// Fan-out capacity of the adapter's fact stream (DEFAULT). Fabric delivery
/// rules apply: a lagged subscriber resyncs from the log, never pretends
/// continuity — so the bound protects the adapter, not the subscriber.
const BROADCAST_CAPACITY: usize = 1024;

/// Bound of the notify→debouncer mpsc (rust-conventions: bound every mpsc).
/// The notify callback thread uses `blocking_send`, so a full channel briefly
/// parks the watcher thread rather than dropping events; the debounce loop
/// drains in batches, keeping occupancy near zero.
const WATCH_CHANNEL_BOUND: usize = 256;

/// Worktree identity. Newtyped per rust-conventions.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct WorktreeId(Ulid);

impl WorktreeId {
    pub fn new(id: Ulid) -> Self {
        Self(id)
    }

    pub fn ulid(&self) -> Ulid {
        self.0
    }
}

/// The allocating PRINCIPAL — the ratified `worktree.allocated.allocator` v1
/// vocabulary (`spec/ontology.md`), which is CLOSED and SCHEME-TAGGED:
/// `"rezidnt"` (the daemon on its own initiative) or `"run:<ULID>"` (a
/// delegating lead run, DR-044 §Decision 3). A bare ULID is explicitly not
/// legal on that field, and `"human"` is RESERVED for out-of-band observation
/// (`worktree.observed`) and is never emitted by rezidnt on an allocation.
///
/// An enum rather than a `String` so the illegal spellings are
/// UNCONSTRUCTIBLE, not merely untested — the same structural discipline
/// DR-046 applied to the source guard. This matters beyond tidiness: the
/// reconciliation scan reads `allocator == "human"` as "already observed,
/// never news", so an allocation able to claim the sentinel would hide its own
/// tree from the scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Allocator {
    /// The daemon allocated this on its own initiative. The DEFAULT, so
    /// threading a principal changes nothing about an ordinary allocation.
    #[default]
    Rezidnt,
    /// A lead run delegated this allocation (DR-044 §Decision 3).
    Run(Ulid),
}

impl Allocator {
    /// The verbatim value rendered onto the fact AND the registry entry. The
    /// two must never disagree about who allocated a tree.
    pub fn as_value(&self) -> String {
        match self {
            Self::Rezidnt => "rezidnt".to_string(),
            Self::Run(run) => format!("run:{run}"),
        }
    }
}

/// Allocation request (doc §7 `WorktreeReq`; fields DEFAULT).
///
/// The principal and envelope fields ride the REQUEST, never the adapter
/// instance (DR-046 §Decision 8): the registry is per-repo, so one adapter
/// serves every workspace over that repo, and an adapter-level `workspace`
/// would be wrong by construction. Construct with `..WorktreeReq::default()`.
#[derive(Debug, Clone, Default)]
pub struct WorktreeReq {
    /// Human-stable name; the adapter derives the on-disk location.
    pub name: String,
    /// Branch to create/check out; `None` with `detach` for detached HEAD.
    pub branch: Option<String>,
    /// `git worktree add --detach`: check out the current HEAD, no branch.
    pub detach: bool,
    /// `git worktree add --orphan -b <branch>`: create the branch with NO
    /// parent commit. Requires `branch`; incompatible with `detach`.
    ///
    /// **Added 2026-07-24 (registry-convergence Stage B; DEFAULT `false`, so
    /// every existing caller is unchanged).** This is not a convenience: a repo
    /// that has been `git init`-ed but never committed has no HEAD, so
    /// `worktree add --detach` and `worktree add -b` both fail, and an orphan
    /// checkout (git ≥ 2.42) is the only way to allocate in one. The daemon's
    /// retired private allocator had exactly this fallback
    /// (`bins/rezidentd/src/runs.rs`, `rezidnt/<agent>-<run>`) and the daemon's
    /// own `make_project` fixture — the base of `golden_path`, `open_flow`,
    /// `fan_out_live_e2e`, `run_persistence` and five more suites — builds an
    /// empty repo, so the repoint would have lost a live, load-bearing
    /// capability without it. DR-046 §Decision 8's brief does not mention it.
    pub orphan: bool,
    /// Who is allocating. Defaults to [`Allocator::Rezidnt`].
    pub principal: Allocator,
    /// Workspace this allocation belongs to. `None` folds into no workspace's
    /// graph, so the daemon always supplies it.
    pub workspace: Option<WorkspaceId>,
    /// The caller's causal chain. `None` falls back to the adapter's own
    /// per-instance correlation (the pre-DR-046 behavior).
    pub correlation: Option<Ulid>,
    /// The direct trigger — e.g. the vet verdict id, so "this tree was
    /// allocated BECAUSE vet passed" is answerable from the log alone.
    pub causation: Option<Ulid>,
}

/// A live allocated worktree.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub id: WorktreeId,
    /// On-disk location (canonicalizes to the registry key).
    pub path: PathBuf,
    pub branch: Option<String>,
    /// The id of the `worktree.allocated` fact this allocation minted.
    ///
    /// **Added 2026-07-24 (registry-convergence Stage B).** Returned because
    /// the allocation fact is now the adapter's to emit, and the caller still
    /// has to chain to it: the daemon sets its `agent.spawned` causation to the
    /// allocation fact's id, so a repoint that returned only the tree would
    /// have severed the allocated → spawned causal edge (I3). The adapter
    /// already had this value — it persists it as `RegistryEntry.allocated_event`
    /// — so this exposes an existing fact rather than minting a new one.
    pub allocated_event: Ulid,
}

/// Errors for the git adapter (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git: {0}")]
    Git(String),
    #[error("worktree registry: {0}")]
    Registry(String),
    /// The sole-allocator double-claim (DR-001): the canonicalized `path` is
    /// already registered to `holder`. STRUCTURAL on purpose — a caller
    /// decides "contended, retry with the same keys" versus "this spawn is
    /// broken" by MATCHING this variant, never by substring-searching a
    /// message that renames itself the next time somebody improves the
    /// wording (DR-046 §Decision 9).
    #[error("worktree {path} is already claimed by {holder}")]
    Conflict { path: String, holder: String },
    #[error("unknown worktree {0:?}")]
    UnknownWorktree(WorktreeId),
    #[error("cas: {0}")]
    Cas(#[from] rezidnt_cas::CasError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("watch: {0}")]
    Watch(#[from] notify::Error),
    #[error("envelope: {0}")]
    Event(#[from] rezidnt_types::EventError),
}

/// Where an adapter fact goes when the daemon wants it on the FABRIC
/// (DR-046 §Decision 8, I3).
///
/// INJECTED, never called directly: `rezidnt-adapter-git` depends on
/// `rezidnt-types` only and must not grow a `rezidnt-fabric` dependency —
/// substrates stay behind traits (I4). The daemon's implementation appends to
/// the event log; the adapter only knows that the append either happened or
/// did not.
///
/// The error type is [`GitError`] rather than a sink-owned associated type or
/// a boxed dyn error. Justification: the sink's failure is observed on exactly
/// one path — inside [`RepoSubstrate::alloc_worktree`], whose signature already
/// returns `GitError` — so any other choice would be converted to `GitError` at
/// its single call site and buy nothing but a generic parameter on a trait that
/// must stay dyn-safe. `GitError::Registry` is the honest arm for "the durable
/// record refused this fact".
pub trait FactSink: Send + Sync {
    /// Append one fact. An `Err` FAILS the operation that minted it: the
    /// append is the commit point (I3), and an allocation whose fact never
    /// reached the log is a tree on disk the log does not know about.
    fn emit(&self, event: &Event) -> Result<(), GitError>;
}

/// The repo substrate seam (doc §7; shape BINDING, signatures DEFAULT).
///
/// `release_worktree` is the S2 addition: allocate → use → release is the
/// lifecycle the slice pins, and `worktree.released` closes the registry
/// entry. Native `async fn` in trait: the daemon consumes concrete adapters
/// through generics; a Send-bounded dyn wrapper is implementer scope if the
/// supervisor needs one.
#[allow(async_fn_in_trait)]
pub trait RepoSubstrate: Send + Sync {
    /// Allocate a worktree: git-CLI mutation, registry claim under the
    /// canonicalized path, `worktree.allocated` fact, watch started.
    async fn alloc_worktree(&self, req: WorktreeReq) -> Result<Worktree, GitError>;

    /// Diff summary for the worktree's current state, persisted to the CAS.
    /// Deterministic: the same tree state yields the same ref (I6-adjacent).
    async fn diff_summary(&self, wt: &WorktreeId) -> Result<CasRef, GitError>;

    /// Release: git-CLI worktree removal, registry entry closed,
    /// `worktree.released` fact (exactly one), watch stopped.
    async fn release_worktree(&self, wt: &WorktreeId) -> Result<(), GitError>;
}

/// A boxed, `Send`-bounded future — the dyn-safe rendering of one
/// [`RepoSubstrate`] method.
pub type RepoFuture<'a, T> =
    std::pin::Pin<Box<dyn Future<Output = Result<T, GitError>> + Send + 'a>>;

/// The OBJECT-SAFE face of [`RepoSubstrate`] — the "Send-bounded dyn wrapper"
/// that trait's own doc comment already puts in implementer scope.
///
/// [`RepoSubstrate`] uses native `async fn` in trait, whose desugared return
/// type is neither nameable nor `Send`-bounded, so `dyn RepoSubstrate` does not
/// exist. The daemon needs one anyway, for two reasons that are not about
/// convenience:
///
/// - **One adapter per repo, held in a cache.** The registry is per-repo, so a
///   second adapter over the same repo is a SECOND ALLOCATOR whose in-memory
///   mirror does not see the first's claims (DR-001). A cache of concrete
///   generic adapters cannot be a `HashMap`, so the cache needs a trait object.
/// - **The injectable allocation seam** (DR-046 Item 3(a)): a registry
///   double-claim is unreachable from a black-box test, because worktree paths
///   are ULID-derived and nothing can pre-claim the path a task will take. A
///   substitutable allocation seam is what makes the owed I6 conflict test
///   writable at all, and substitution needs a trait object.
///
/// ALL THREE methods are mirrored, deliberately. Mirroring only
/// `alloc_worktree` would leave the daemon reaching for the concrete adapter to
/// summarize or release — two handles to one adapter through two traits, which
/// is the split-path shape this slice exists to dissolve, one level up.
pub trait DynRepoSubstrate: Send + Sync {
    fn alloc_worktree(&self, req: WorktreeReq) -> RepoFuture<'_, Worktree>;
    fn diff_summary(&self, wt: &WorktreeId) -> RepoFuture<'_, CasRef>;
    fn release_worktree(&self, wt: &WorktreeId) -> RepoFuture<'_, ()>;
}

/// [`GitAdapter`] through the object-safe face. Written out rather than
/// blanket-implemented over `T: RepoSubstrate`: the blanket form cannot compile,
/// because `async fn` in trait yields an opaque future that is not provably
/// `Send` from the bound alone, and forcing it would mean re-spelling
/// [`RepoSubstrate`]'s signatures as `-> impl Future + Send` — a change to the
/// S2 seam that this slice has no mandate to make.
impl DynRepoSubstrate for GitAdapter {
    fn alloc_worktree(&self, req: WorktreeReq) -> RepoFuture<'_, Worktree> {
        Box::pin(RepoSubstrate::alloc_worktree(self, req))
    }

    fn diff_summary(&self, wt: &WorktreeId) -> RepoFuture<'_, CasRef> {
        let wt = *wt;
        Box::pin(async move { RepoSubstrate::diff_summary(self, &wt).await })
    }

    fn release_worktree(&self, wt: &WorktreeId) -> RepoFuture<'_, ()> {
        let wt = *wt;
        Box::pin(async move { RepoSubstrate::release_worktree(self, &wt).await })
    }
}

/// One live registry line (JSONL at [`REGISTRY_PATH`]). `path` is the
/// canonicalized spelling — the registry key (DR-001 BINDING rule). The
/// optional fields are the S2-remediation additions (additive evolution;
/// migration defaults documented in the module header).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryEntry {
    path: String,
    allocator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    /// Allocation identity for `"rezidnt"` entries — what makes a reloaded
    /// allocation releasable after restart. Never set on `"human"` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<WorktreeId>,
    /// `worktree.allocated` event id — causation for post-restart facts
    /// (`diff.ready`, `worktree.released`). DEFAULT chain, best-effort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocated_event: Option<Ulid>,
    /// `worktree.conflict` already emitted for this path — one collision,
    /// one fact, forever, restart notwithstanding.
    #[serde(default, skip_serializing_if = "is_false")]
    conflicted: bool,
}

/// A worktree this adapter instance allocated and still tracks.
struct LiveWorktree {
    /// Canonicalized on-disk location.
    path: PathBuf,
    /// The canonical spelling the allocated fact minted (registry key).
    path_str: String,
    branch: Option<String>,
    /// The requesting envelope, carried forward onto this worktree's later
    /// facts. Defaulted for an allocation reloaded by the on-open scan.
    ctx: FactCtx,
    /// `worktree.allocated` event id — causation for later facts. `None` for
    /// an allocation reloaded from a legacy registry line (migration default).
    allocated: Option<Ulid>,
    /// Held so the watch survives exactly as long as the allocation; dropping
    /// it stops notify delivery and (by closing the mpsc) ends the debouncer.
    _watcher: notify::RecommendedWatcher,
}

/// Mutable adapter state behind one async mutex. The exactly-once dedup
/// marks live ON the registry entries (a `"human"` entry is the observed
/// mark; `conflicted` is a persisted flag), so they survive restart with the
/// registry — the in-memory sets they replaced were the S2 debrief blocker.
#[derive(Default)]
struct State {
    /// In-memory mirror of the JSONL registry, keyed by canonical path.
    registry: BTreeMap<String, RegistryEntry>,
    live: BTreeMap<WorktreeId, LiveWorktree>,
}

/// The envelope context one allocation's facts ride (DR-046 §Decision 8).
/// Supplied per REQUEST — an adapter serves every workspace over its repo, so
/// these cannot be adapter-level state — and carried forward onto that
/// worktree's later facts for the lifetime of the allocation.
///
/// Restart degradation, stated rather than hidden: a reloaded allocation's
/// later facts (`diff.ready`, `worktree.released`) fall back to the default
/// context, exactly as `allocated_event` already degrades on a legacy line.
/// The registry persists no envelope fields; nothing in this slice pins that,
/// and inventing an unpinned format change is not this stage's work.
#[derive(Debug, Clone, Copy, Default)]
struct FactCtx {
    workspace: Option<WorkspaceId>,
    correlation: Option<Ulid>,
}

impl FactCtx {
    fn of(req: &WorktreeReq) -> Self {
        Self {
            workspace: req.workspace,
            correlation: req.correlation,
        }
    }
}

struct Inner {
    /// Canonicalized repo root.
    repo_root: PathBuf,
    registry_file: PathBuf,
    cas: Arc<Cas>,
    tx: broadcast::Sender<Event>,
    /// The durable append seam, when the daemon injected one
    /// ([`GitAdapter::with_sink`]). Absent → broadcast only, the pre-DR-046
    /// standalone behavior every existing suite exercises.
    sink: OnceLock<Arc<dyn FactSink>>,
    /// One correlation per adapter instance (DEFAULT): the fallback for a
    /// request that names none, so every fact still belongs to some chain.
    correlation: Ulid,
    /// Facts minted by the on-open reconciliation scan, set exactly once at
    /// the end of [`GitAdapter::open`] (the scan predates every subscriber,
    /// so they are pinned here for deterministic retrieval — see
    /// [`GitAdapter::startup_facts`]).
    startup: OnceLock<Vec<Event>>,
    state: Mutex<State>,
}

impl Inner {
    /// Mint and publish one fact (`v = 1`, `source` = [`SOURCE_ID`]) under the
    /// supplied envelope context. Returns the fact so callers can causally
    /// chain later facts (`.id`) or pin it (the startup scan collects its facts
    /// for [`GitAdapter::startup_facts`]).
    ///
    /// The INJECTED SINK GOES FIRST and its error propagates: the append is
    /// the commit point (I3), so a fact that could not be appended never
    /// happened and must not be broadcast as though it had. The broadcast that
    /// follows is a live-subscriber convenience, and its `send` failure stays
    /// tolerated — "no live subscribers" is not a failure for a fan-out, but it
    /// is one for an append.
    fn emit(
        &self,
        subject: &str,
        ctx: &FactCtx,
        causation: Option<Ulid>,
        payload: Value,
    ) -> Result<Event, GitError> {
        let event = Event::new(
            SourceId::new(SOURCE_ID),
            ctx.workspace,
            Subject::new(subject),
            ctx.correlation.unwrap_or(self.correlation),
            causation,
            1,
            payload,
        )?;
        if let Some(sink) = self.sink.get() {
            sink.emit(&event)?;
        }
        let fact = event.clone();
        if self.tx.send(event).is_err() {
            // No live subscribers: not a failure for a broadcast fan-out.
            tracing::debug!(subject, "adapter fact emitted with no live subscribers");
        }
        Ok(fact)
    }

    /// Serialize the registry back to its JSONL file. Callers hold the state
    /// lock, so writes are serialized.
    async fn persist_registry(&self, state: &State) -> Result<(), GitError> {
        let mut content = String::new();
        for entry in state.registry.values() {
            let line = serde_json::to_string(entry)
                .map_err(|e| GitError::Registry(format!("encode entry: {e}")))?;
            content.push_str(&line);
            content.push('\n');
        }
        tokio::fs::write(&self.registry_file, content).await?;
        Ok(())
    }
}

/// The git adapter: owns the worktree registry, the notify watcher, and the
/// CAS handle for diff summaries.
pub struct GitAdapter {
    inner: Arc<Inner>,
}

impl GitAdapter {
    /// Open the adapter over a repo root. Loads (or creates) the
    /// [`REGISTRY_PATH`] registry and opens its OWN CAS at `cas_root`.
    ///
    /// A caller that already holds a CAS handle must use
    /// [`GitAdapter::open_with_cas`] instead — see there for why two roots is a
    /// defect, not a preference.
    pub async fn open(repo_root: &Path, cas_root: &Path) -> Result<Self, GitError> {
        let cas_root = cas_root.to_path_buf();
        let cas = tokio::task::spawn_blocking(move || Cas::open(&cas_root))
            .await
            .map_err(join_err)??;
        Self::open_with_cas(repo_root, Arc::new(cas)).await
    }

    /// Open the adapter over a repo root, SHARING the caller's CAS.
    ///
    /// **Added 2026-07-24 (registry-convergence Stage B).** The daemon holds an
    /// `Arc<Cas>` and [`GitAdapter::open`] opened a second one at whatever root
    /// it was handed. Two CAS roots split content addressing: a `diff.ready`
    /// carrying a [`CasRef`] the adapter minted would be UNRESOLVABLE from the
    /// daemon's CAS, so the fact would name content no reader of the log could
    /// fetch (I2 — the ref is the whole point of keeping bytes off the fabric).
    /// Sharing the handle is what makes the ref mean the same thing on both
    /// sides. DR-046 §Decision 8's brief does not mention this.
    pub async fn open_with_cas(repo_root: &Path, cas: Arc<Cas>) -> Result<Self, GitError> {
        let span = tracing::info_span!("adapter", kind = "git", op = "open");
        async move {
            let repo_root = tokio::fs::canonicalize(repo_root).await?;

            let registry_file = repo_root.join(REGISTRY_PATH);
            if let Some(parent) = registry_file.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut registry = BTreeMap::new();
            match tokio::fs::read_to_string(&registry_file).await {
                Ok(content) => {
                    for line in content.lines().filter(|l| !l.trim().is_empty()) {
                        let entry: RegistryEntry = serde_json::from_str(line).map_err(|e| {
                            GitError::Registry(format!("bad registry line ({e}): {line}"))
                        })?;
                        registry.insert(entry.path.clone(), entry);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }

            let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
            let adapter = Self {
                inner: Arc::new(Inner {
                    repo_root,
                    registry_file,
                    cas,
                    tx,
                    sink: OnceLock::new(),
                    correlation: Ulid::new(),
                    startup: OnceLock::new(),
                    state: Mutex::new(State {
                        registry,
                        ..State::default()
                    }),
                }),
            };
            // On-open reconciliation scan (S2 remediation): registry against
            // reality, before any subscriber can exist.
            let facts = adapter.reconcile_on_open().await?;
            // set() cannot fail here — open is the only writer and runs once
            // per instance — but there is no invariant worth panicking over.
            let _ = adapter.inner.startup.set(facts);
            Ok(adapter)
        }
        .instrument(span)
        .await
    }

    /// Inject the durable append seam (DR-046 §Decision 8, I3/I4). Every fact
    /// this adapter mints from here on goes through `sink` BEFORE it is
    /// broadcast, and a sink refusal fails the operation that minted it.
    ///
    /// Injected once, at construction. A second injection is refused and said
    /// so out loud rather than silently discarded — two sinks would mean two
    /// answers to "was this appended".
    pub fn with_sink(self, sink: Arc<dyn FactSink>) -> Self {
        if self.inner.sink.set(sink).is_err() {
            tracing::warn!("fact sink already injected; the first sink stands");
        }
        self
    }

    /// Subscribe to the adapter's fact stream (fabric delivery rules apply:
    /// a lagged subscriber resyncs from the log, never pretends continuity).
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.tx.subscribe()
    }

    /// Facts minted by the on-open reconciliation scan (S2 remediation; the
    /// auditor fail verdict of 2026-07-17 is the work order). [`GitAdapter::open`]
    /// reconciles the registry against out-of-band reality (e.g. `git worktree
    /// list`) and routes discoveries through the same dedup path as
    /// [`GitAdapter::observe`]. The scan runs before any subscriber can exist,
    /// so its facts are exposed here for deterministic retrieval; they ride
    /// the broadcast stream as well for any subscriber wired before later
    /// scans. Contract pinned by `tests/restart_and_discovery.rs`; signature
    /// DEFAULT.
    pub fn startup_facts(&self) -> Vec<Event> {
        self.inner.startup.get().cloned().unwrap_or_default()
    }

    /// The on-open reconciliation scan (S2 remediation; the auditor fail
    /// verdict of 2026-07-17 is the work order). Two passes under one state
    /// lock:
    ///
    /// 1. **Registry → reality.** Every reloaded `"rezidnt"` entry is checked
    ///    against the tree actually at its path
    ///    ([`GitAdapter::list_linked_worktrees`]) via the private-gitdir
    ///    identity marker (S2-T3: branch is not identity). Marker carries the
    ///    registered [`WorktreeId`] → rezidnt's own intact tree: the
    ///    allocation is rebuilt live (releasable under its persisted id,
    ///    re-watched) and is not news. Marker missing or mismatched → a
    ///    foreign tree occupies the registered path: exactly one
    ///    `worktree.conflict`, with the persisted `conflicted` flag making
    ///    "once" mean forever.
    ///    Missing from git entirely → logged, entry retained (unpinned).
    ///    `"human"` entries are already-observed by definition — never news.
    /// 2. **Reality → registry.** Linked worktrees git reports that the
    ///    registry does not hold are out-of-band discoveries, routed through
    ///    the same dedup rule as [`GitAdapter::observe`]: registered plus
    ///    exactly one `worktree.observed` (allocator `"human"`).
    ///
    /// Returns the minted facts; they also ride the broadcast (a subscriber
    /// wired later resyncs from the log per fabric delivery rules).
    async fn reconcile_on_open(&self) -> Result<Vec<Event>, GitError> {
        let span = tracing::info_span!("adapter", kind = "git", op = "reconcile");
        async move {
            let actual = self.list_linked_worktrees().await?;
            let mut facts = Vec::new();
            let mut dirty = false;
            let mut state = self.inner.state.lock().await;

            // Pass 1: registry → reality.
            let keys: Vec<String> = state.registry.keys().cloned().collect();
            for key in keys {
                let Some(entry) = state.registry.get(&key).cloned() else {
                    continue;
                };
                if entry.allocator == "human" {
                    continue; // the entry IS the observed mark — never news
                }
                let Some(tree) = actual.get(&key) else {
                    tracing::warn!(
                        path = %key,
                        "registered worktree missing from git; entry retained"
                    );
                    continue;
                };
                // Identity probe (S2-T3): the discriminator is the persisted
                // WorktreeId marker in the tree's private gitdir, never the
                // branch — an occupant switching HEAD keeps the marker (not a
                // takeover); a foreign re-add at the same path/branch lacks
                // it (a takeover branch equality would hide).
                let intact = if entry.id.is_some() {
                    match self.read_identity_marker(&tree.path).await {
                        Ok(marker) => marker == entry.id,
                        Err(e) => {
                            // Uninterrogable tree: cannot verify either way —
                            // retained without a fact (mirrors missing-from-
                            // git handling; unpinned).
                            tracing::warn!(
                                path = %key,
                                error = %e,
                                "worktree identity unverifiable; entry retained"
                            );
                            continue;
                        }
                    }
                } else {
                    // Legacy line without an id (migration default): no
                    // marker was ever written, so branch comparison remains
                    // the only available discriminator.
                    tree.branch == entry.branch
                };
                if intact {
                    // rezidnt's own intact tree: rebuild the live allocation
                    // under its persisted identity.
                    let Some(id) = entry.id else {
                        // Legacy line without an id (migration default): the
                        // id was process-local and died with its process.
                        tracing::warn!(
                            path = %key,
                            "legacy registry entry without a worktree id; not releasable"
                        );
                        continue;
                    };
                    // A reloaded allocation has no request to inherit an
                    // envelope from (see [`FactCtx`] on restart degradation).
                    let watcher = self.spawn_watcher(
                        tree.path.clone(),
                        key.clone(),
                        FactCtx::default(),
                        entry.allocated_event,
                    )?;
                    state.live.insert(
                        id,
                        LiveWorktree {
                            path: tree.path.clone(),
                            path_str: key.clone(),
                            branch: entry.branch.clone(),
                            ctx: FactCtx::default(),
                            allocated: entry.allocated_event,
                            _watcher: watcher,
                        },
                    );
                } else if !entry.conflicted {
                    // The checkout is not what rezidnt registered: a human
                    // tree occupies the path. One collision, one fact.
                    let fact = self.inner.emit(
                        "worktree.conflict",
                        &FactCtx::default(),
                        None,
                        serde_json::json!({ "path": entry.path, "holder": entry.allocator }),
                    )?;
                    facts.push(fact);
                    if let Some(entry) = state.registry.get_mut(&key) {
                        entry.conflicted = true;
                    }
                    dirty = true;
                }
            }

            // Pass 2: reality → registry (out-of-band discoveries).
            for (key, tree) in &actual {
                if state.registry.contains_key(key) {
                    continue;
                }
                let mut payload = serde_json::Map::new();
                payload.insert("path".into(), Value::String(key.clone()));
                payload.insert("allocator".into(), Value::String("human".into()));
                if let Some(branch) = &tree.branch {
                    payload.insert("branch".into(), Value::String(branch.clone()));
                }
                state.registry.insert(
                    key.clone(),
                    RegistryEntry {
                        path: key.clone(),
                        allocator: "human".into(),
                        branch: tree.branch.clone(),
                        id: None,
                        allocated_event: None,
                        conflicted: false,
                    },
                );
                let fact = self.inner.emit(
                    "worktree.observed",
                    &FactCtx::default(),
                    None,
                    Value::Object(payload),
                )?;
                facts.push(fact);
                dirty = true;
            }

            if dirty {
                self.inner.persist_registry(&state).await?;
            }
            Ok(facts)
        }
        .instrument(span)
        .await
    }

    /// Enumerate the repo's LINKED worktrees via `git worktree list
    /// --porcelain` (scan mechanism DEFAULT; the primary working tree is
    /// excluded — it is not an allocation), keyed by canonical path.
    async fn list_linked_worktrees(&self) -> Result<BTreeMap<String, ActualTree>, GitError> {
        let out = self.run_git(&["worktree", "list", "--porcelain"]).await?;
        let mut map = BTreeMap::new();
        for block in parse_worktree_porcelain(&out) {
            let canonical = match tokio::fs::canonicalize(&block.path).await {
                Ok(c) => c,
                Err(e) => {
                    // Listed but unresolvable (prunable leftovers): git's
                    // bookkeeping, not a discovery — skip, never fail open.
                    tracing::warn!(
                        path = %block.path.display(),
                        error = %e,
                        "skipping unresolvable listed worktree"
                    );
                    continue;
                }
            };
            if canonical == self.inner.repo_root {
                continue;
            }
            let key = utf8_path(&canonical)?;
            map.insert(
                key,
                ActualTree {
                    path: canonical,
                    branch: block.branch,
                },
            );
        }
        Ok(map)
    }

    /// Watcher ingest for an out-of-band worktree discovery (see module
    /// docs). Idempotent per canonicalized path: re-observation of a known
    /// tree or an already-emitted collision emits nothing further.
    ///
    /// **No production caller today.** Out-of-band trees reach the log through
    /// [`GitAdapter::reconcile_on_open`]'s pass 2, which runs the same dedup
    /// rule at startup; `observe` is the per-event door for a watcher that does
    /// not yet call it. Reachable from tests only — which bounds the blast
    /// radius of anything wrong here, and is not a licence for the code to be
    /// wrong.
    ///
    /// ## MARK, NOT CLAIM — what a failed registry persist does here
    ///
    /// Both arms below emit first and persist second, and a persist failure is
    /// WARNED rather than returned. The rule, stated once and applied at every
    /// site that writes the registry after a fact has been APPENDED:
    ///
    /// - a registry write that is a DEDUP MARK for a fact already on the log is
    ///   not part of the operation. The operation was "record this"; the record
    ///   landed. Losing the mark costs a possible re-announcement after a
    ///   restart — the at-least-once window the module header documents — and
    ///   failing the call instead would tell the caller nothing happened when
    ///   something did. This is the rule `alloc_worktree`'s conflict arm
    ///   already followed; `observe` returned `Err` in the identical situation
    ///   until this was reconciled (2026-07-24), which is why it is written
    ///   down rather than left to be inferred from two sites that agreed by
    ///   accident.
    /// - a registry write that constitutes the sole-allocator CLAIM is part of
    ///   the operation and fails it. That is `alloc_worktree`'s success path
    ///   only, where the entry is what makes the tree rezidnt's under DR-001.
    ///   The asymmetry is deliberate and marked at that site.
    ///
    /// Two registry writes are outside the rule because neither follows an
    /// append, and they are named so the rule is not read as covering them:
    /// [`GitAdapter::reconcile_on_open`]'s batched persist (its facts have not
    /// been appended yet — they reach the log only later, via
    /// [`GitAdapter::startup_facts`] — so aborting `open` there discards
    /// nothing the log holds), and [`RepoSubstrate::release_worktree`], which
    /// persists BEFORE it emits.
    pub async fn observe(&self, path: &Path) -> Result<(), GitError> {
        let span = tracing::info_span!("adapter", kind = "git", op = "observe");
        async move {
            let claimed = path.to_path_buf();
            let canonical = tokio::fs::canonicalize(path).await?;
            let canonical_str = utf8_path(&canonical)?;

            let mut state = self.inner.state.lock().await;
            if let Some(entry) = state.registry.get_mut(&canonical_str) {
                // Registered path. A "human" entry means worktree.observed
                // was already emitted (the entry is the mark — durable, so
                // restart never resurfaces it). Anything else is an
                // out-of-band second claim: conflict is emitted INSTEAD of
                // double-tracking (DR-001), exactly once, forever — the
                // conflicted flag persists with the entry.
                if entry.allocator == "human" || entry.conflicted {
                    return Ok(());
                }
                entry.conflicted = true;
                let holder = entry.allocator.clone();
                let registered = entry.path.clone();
                let mut payload = serde_json::Map::new();
                payload.insert("path".into(), Value::String(registered));
                let claimed_str = claimed.to_string_lossy().into_owned();
                if claimed_str != canonical_str {
                    // The colliding spelling, pre-canonicalization — triage
                    // evidence, present only when it differs (ontology v1).
                    payload.insert("claimed_path".into(), Value::String(claimed_str));
                }
                payload.insert("holder".into(), Value::String(holder));
                if let Err(e) = self.inner.emit(
                    "worktree.conflict",
                    &FactCtx::default(),
                    None,
                    Value::Object(payload),
                ) {
                    // The fact never reached the log, so the mark it dedups
                    // against must not stand: an in-memory `conflicted` the
                    // registry file does not carry would silence the NEXT
                    // observation of a collision the log never heard about.
                    // Reverted, and the error propagates — `observe`'s whole
                    // contract is "record this observation".
                    unmark_conflicted(&mut state, &canonical_str);
                    return Err(e);
                }
                // The fact IS on the log; only its dedup mark failed to
                // persist. Warned, not failed — see [`GitAdapter::observe`]'s
                // "mark, not claim" rule. A restart may re-announce this
                // collision, which is the at-least-once window the module
                // header already documents.
                if let Err(e) = self.inner.persist_registry(&state).await {
                    tracing::warn!(
                        path = %canonical_str,
                        error = %e,
                        "worktree.conflict was recorded but its dedup mark could not be \
                         persisted; a restart may re-announce this collision"
                    );
                }
                return Ok(());
            }

            // Fresh out-of-band tree: observed (allocator "human", fixed in
            // v1) and registered so it holds its key from now on.
            let branch = {
                let tree = canonical.clone();
                tokio::task::spawn_blocking(move || summary::read_branch(&tree))
                    .await
                    .map_err(join_err)?
            };
            let mut payload = serde_json::Map::new();
            payload.insert("path".into(), Value::String(canonical_str.clone()));
            payload.insert("allocator".into(), Value::String("human".into()));
            if let Some(branch) = &branch {
                payload.insert("branch".into(), Value::String(branch.clone()));
            }
            // EMIT BEFORE PERSIST — the ordering, and the reason for it
            // (remediation, 2026-07-24). This ran the other way round: the
            // `"human"` entry was written and persisted FIRST, so a refused
            // `worktree.observed` returned `Err` while leaving a durable entry
            // behind. Because that entry IS the dedup mark, the discovery was
            // then unrecoverable: re-observation returns `Ok(())` silently
            // (the `allocator == "human"` arm above), and the on-open
            // reconciliation scan skips `"human"` entries as already-observed.
            // The registry ended up asserting a fact the log never received —
            // derived state claiming a log entry that does not exist (I3), the
            // same shape as the `last_hash` bug in [`debounce_loop`] with the
            // ordering inverted. Emitting first leaves NOTHING to unwind on a
            // refusal: no entry, no mark, and the next observation of this tree
            // is news again.
            //
            // This is also what reconciliation pass 2 does (see
            // [`GitAdapter::reconcile_on_open`]), which aborts `open` on a
            // refusal before any persist runs. `observe` was the outlier.
            self.inner.emit(
                "worktree.observed",
                &FactCtx::default(),
                None,
                Value::Object(payload),
            )?;
            state.registry.insert(
                canonical_str.clone(),
                RegistryEntry {
                    path: canonical_str.clone(),
                    allocator: "human".into(),
                    branch,
                    id: None,
                    allocated_event: None,
                    conflicted: false,
                },
            );
            // Mark, not claim: the observation is recorded, and a mark that
            // fails to persist only means the next restart re-announces it.
            if let Err(e) = self.inner.persist_registry(&state).await {
                tracing::warn!(
                    path = %canonical_str,
                    error = %e,
                    "worktree.observed was recorded but its registry entry could not be \
                     persisted; a restart may re-announce this discovery"
                );
            }
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Derive the on-disk location for a named allocation (DEFAULT):
    /// `<repo>/.rezidnt/worktrees/<name>`.
    ///
    /// **Moved 2026-07-24 (registry-convergence slice; DEFAULT, note in lieu of
    /// a `/dr`).** This was the sibling layout `<repo-parent>/<repo>-wt-<name>`,
    /// designed so allocated trees never polluted the primary working tree.
    ///
    /// **The pollution claim, narrowed (remediation, same day).** This comment
    /// argued the concern was answered because "the repo `.gitignore`
    /// designates `.rezidnt/` as the home for worktrees" — true of THIS repo and
    /// of nothing else. rezidnt writes no ignore rule into an operator's repo,
    /// so in an arbitrary repo an allocated tree under `.rezidnt/worktrees/` is
    /// UNTRACKED content: invisible to `git status --porcelain` only if the
    /// operator ignores it themselves, and removable by `git clean -fdx`. What
    /// actually justifies the layout is narrower and does not depend on any
    /// repo's ignore file: the daemon has shipped this exact on-disk layout
    /// since v0.0.1, so converging on it preserves shipped
    /// v0.0.1 on-disk behavior and the two test levers built against it
    /// (`bins/rezidentd/tests/spec_init_open_e2e.rs`'s tempdir-confinement
    /// guard, and `fan_out_live_e2e.rs`'s `block_allocations`), which a move in
    /// the other direction would have broken.
    fn derive_worktree_path(&self, name: &str) -> Result<PathBuf, GitError> {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        Ok(self.inner.repo_root.join(WORKTREE_BASE).join(safe))
    }

    /// Run `git -C <repo_root> <args>` via tokio::process; nonzero exit maps
    /// to [`GitError::Git`] carrying stderr.
    async fn run_git(&self, args: &[&str]) -> Result<String, GitError> {
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(cli_path(&self.inner.repo_root))
            .args(args)
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Git(format!(
                "git {args:?} failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Resolve a worktree's PRIVATE gitdir (`<repo>/.git/worktrees/<name>/`
    /// for a linked tree) from inside the tree. This is where the identity
    /// marker lives — never in the working tree itself.
    async fn worktree_gitdir(&self, tree: &Path) -> Result<PathBuf, GitError> {
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(cli_path(tree))
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .await?;
        if !output.status.success() {
            return Err(GitError::Git(format!(
                "git rev-parse --absolute-git-dir in {} failed ({}): {}",
                tree.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim(),
        ))
    }

    /// Write the identity marker for a freshly allocated worktree.
    async fn write_identity_marker(&self, tree: &Path, id: WorktreeId) -> Result<(), GitError> {
        let gitdir = self.worktree_gitdir(tree).await?;
        tokio::fs::write(gitdir.join(IDENTITY_MARKER), id.ulid().to_string()).await?;
        Ok(())
    }

    /// Read the identity marker of the tree currently at `tree`, if any.
    /// `Ok(None)` means no marker (or an unparsable one) — a tree rezidnt did
    /// not allocate. `Err` means the tree could not be interrogated at all.
    async fn read_identity_marker(&self, tree: &Path) -> Result<Option<WorktreeId>, GitError> {
        let gitdir = self.worktree_gitdir(tree).await?;
        match tokio::fs::read_to_string(gitdir.join(IDENTITY_MARKER)).await {
            Ok(text) => Ok(Ulid::from_string(text.trim()).ok().map(WorktreeId::new)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Start the notify watch on an allocated tree and spawn its debounce
    /// loop. The returned watcher must be kept alive with the allocation.
    fn spawn_watcher(
        &self,
        path: PathBuf,
        path_str: String,
        ctx: FactCtx,
        causation: Option<Ulid>,
    ) -> Result<notify::RecommendedWatcher, GitError> {
        use notify::Watcher as _;

        let (tx, rx) = mpsc::channel::<()>(WATCH_CHANNEL_BOUND);
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                match res {
                    // Reads are not changes ([`is_change_event`]) and must not
                    // wake the debounce loop.
                    Ok(event) if !is_change_event(&event.kind) => {}
                    // Send failure means the receiver (debounce loop) is
                    // gone, which only happens on release — nothing to do.
                    Ok(_event) => drop(tx.blocking_send(())),
                    Err(e) => tracing::warn!(error = %e, "notify watcher error"),
                }
            })?;
        watcher.watch(&path, notify::RecursiveMode::Recursive)?;

        let inner = Arc::clone(&self.inner);
        let span = tracing::info_span!("adapter", kind = "git-watch", worktree = %path_str);
        tokio::spawn(debounce_loop(inner, path, path_str, ctx, causation, rx).instrument(span));
        Ok(watcher)
    }
}

impl RepoSubstrate for GitAdapter {
    async fn alloc_worktree(&self, req: WorktreeReq) -> Result<Worktree, GitError> {
        let span =
            tracing::info_span!("adapter", kind = "git", op = "alloc_worktree", name = %req.name);
        async move {
            let ctx = FactCtx::of(&req);
            let principal = req.principal.as_value();
            let target = self.derive_worktree_path(&req.name)?;
            // `git worktree add` creates the leaf; the in-repo base above it
            // is the adapter's to provide.
            if let Some(base) = target.parent() {
                tokio::fs::create_dir_all(base).await?;
            }
            let target_cli = cli_path(&target);
            match (&req.branch, req.detach, req.orphan) {
                (Some(branch), false, false) => {
                    self.run_git(&["worktree", "add", "-b", branch, &target_cli])
                        .await?
                }
                // No HEAD to branch from (a repo `git init`-ed but never
                // committed): the branch is created parentless. git ≥ 2.42.
                (Some(branch), false, true) => {
                    self.run_git(&["worktree", "add", "--orphan", "-b", branch, &target_cli])
                        .await?
                }
                (None, true, false) => {
                    self.run_git(&["worktree", "add", "--detach", &target_cli])
                        .await?
                }
                (None, false, false) => self.run_git(&["worktree", "add", &target_cli]).await?,
                (Some(_), true, _) => {
                    return Err(GitError::Git(
                        "contradictory request: both a branch and detach".into(),
                    ));
                }
                (None, _, true) => {
                    return Err(GitError::Git(
                        "contradictory request: an orphan checkout needs a branch to create".into(),
                    ));
                }
            };
            let canonical = tokio::fs::canonicalize(&target).await?;
            let canonical_str = utf8_path(&canonical)?;

            let mut state = self.inner.state.lock().await;
            if let Some(entry) = state.registry.get_mut(&canonical_str) {
                // Sole-allocator guard: a second claim emits exactly one
                // conflict instead of silently double-tracking (DR-001); the
                // flag persists so "once" survives restart. (Single lookup —
                // no panic-capable indexing; auditor tracked item 5.)
                let emit_conflict = !entry.conflicted;
                entry.conflicted = true;
                let holder = entry.allocator.clone();
                if emit_conflict {
                    // I6: the REFUSAL is not contingent on the log. A double
                    // claim is a fact about the registry — the path is held,
                    // and it is held whether or not the fabric accepted the
                    // news — so a failed append must never turn a contended
                    // tree into a generic append error, which the daemon would
                    // map to `codes::SPAWN_FAILED` and a caller would read as
                    // "this spawn is broken" instead of "retry with the same
                    // keys" (DR-046 §Decision 9). Both failure arms therefore
                    // fall through to the `Conflict` return below; what differs
                    // is what happens to the dedup mark.
                    match self.inner.emit(
                        "worktree.conflict",
                        &ctx,
                        None,
                        serde_json::json!({ "path": canonical_str, "holder": holder }),
                    ) {
                        Ok(_) => {
                            if let Err(e) = self.inner.persist_registry(&state).await {
                                // MARK, NOT CLAIM (the rule is stated in full
                                // on [`GitAdapter::observe`], which now follows
                                // it too). The fact IS on the log; only its
                                // mark failed to persist. In-memory stays
                                // marked (this process emits once), disk does
                                // not — which is exactly the at-least-once
                                // window the module header already documents: a
                                // restart may re-announce this collision.
                                tracing::warn!(
                                    path = %canonical_str,
                                    error = %e,
                                    "worktree.conflict was recorded but its dedup mark could \
                                     not be persisted; a restart may re-announce this collision"
                                );
                            }
                        }
                        Err(e) => {
                            // The fact never reached the log. Leaving the mark
                            // set would diverge memory from disk AND silence
                            // the next claim on a collision the log never heard
                            // about — a silent double claim, the one outcome
                            // DR-001 exists to prevent. Unmark: the collision is
                            // re-announced on the next claim (at-least-once).
                            unmark_conflicted(&mut state, &canonical_str);
                            tracing::warn!(
                                path = %canonical_str,
                                error = %e,
                                "worktree.conflict could not be appended; the collision stays \
                                 unmarked and will be re-announced on the next claim"
                            );
                        }
                    }
                }
                // STRUCTURAL, not a message: the daemon maps this variant to a
                // distinct refusal code, and must never have to parse prose to
                // do it (DR-046 §Decision 9).
                return Err(GitError::Conflict {
                    path: canonical_str,
                    holder,
                });
            }

            // Mint the identity and stamp it into the tree's private gitdir
            // BEFORE the fact is emitted: a tree without a marker was never
            // a rezidnt allocation (S2-T3 identity discriminator).
            let id = WorktreeId::new(Ulid::new());
            self.write_identity_marker(&canonical, id).await?;

            let mut payload = serde_json::Map::new();
            payload.insert("path".into(), Value::String(canonical_str.clone()));
            if let Some(branch) = &req.branch {
                payload.insert("branch".into(), Value::String(branch.clone()));
            }
            payload.insert("allocator".into(), Value::String(principal.clone()));
            // The append is the COMMIT POINT (I3). If the fact cannot be
            // recorded, the allocation did not happen: nothing is registered,
            // nothing is tracked live, and the tree just created is taken back
            // out so the disk does not hold a worktree the log never heard of.
            let allocated = match self.inner.emit(
                "worktree.allocated",
                &ctx,
                req.causation,
                Value::Object(payload),
            ) {
                Ok(fact) => fact.id,
                Err(e) => {
                    if let Err(cleanup) = self
                        .run_git(&["worktree", "remove", "--force", &cli_path(&canonical)])
                        .await
                    {
                        tracing::warn!(
                            path = %canonical_str,
                            error = %cleanup,
                            "allocation fact could not be recorded and the tree could not be \
                             removed; an unregistered worktree remains on disk"
                        );
                    }
                    return Err(e);
                }
            };

            // The registry entry carries the allocation identity and the
            // allocated event id (S2 remediation) — minted just above so one
            // persist suffices — making the allocation releasable and its
            // causal chain recoverable across restart.
            state.registry.insert(
                canonical_str.clone(),
                RegistryEntry {
                    path: canonical_str.clone(),
                    // The same principal the fact recorded — the registry and
                    // the log must not disagree about who allocated a tree.
                    allocator: principal,
                    branch: req.branch.clone(),
                    id: Some(id),
                    allocated_event: Some(allocated),
                    conflicted: false,
                },
            );
            // CLAIM, not mark — the one site where a persist failure FAILS the
            // call, and the deliberate other side of the rule on
            // [`GitAdapter::observe`]. This entry is what makes the tree
            // rezidnt's under DR-001: without it on disk, a restart sees an
            // unclaimed path and the sole-allocator guard has nothing to guard
            // with. Stated exactly: on failure the call returns `Err` while the
            // tree, its identity marker and its `worktree.allocated` fact all
            // remain — this process's in-memory registry holds the claim, a
            // restarted one does not, and the reconciliation scan meets the
            // tree as an unregistered linked worktree (a `"human"` discovery).
            self.inner.persist_registry(&state).await?;

            // Watch starts after the allocated fact is minted so its id can
            // causally chain the diff.ready stream; callers only write after
            // alloc returns, so no event can precede the watch.
            let watcher = self.spawn_watcher(
                canonical.clone(),
                canonical_str.clone(),
                ctx,
                Some(allocated),
            )?;
            state.live.insert(
                id,
                LiveWorktree {
                    path: canonical.clone(),
                    path_str: canonical_str,
                    branch: req.branch.clone(),
                    ctx,
                    allocated: Some(allocated),
                    _watcher: watcher,
                },
            );
            Ok(Worktree {
                id,
                path: canonical,
                branch: req.branch,
                allocated_event: allocated,
            })
        }
        .instrument(span)
        .await
    }

    async fn diff_summary(&self, wt: &WorktreeId) -> Result<CasRef, GitError> {
        let span = tracing::info_span!("adapter", kind = "git", op = "diff_summary");
        async move {
            let path = {
                let state = self.inner.state.lock().await;
                state
                    .live
                    .get(wt)
                    .map(|live| live.path.clone())
                    .ok_or(GitError::UnknownWorktree(*wt))?
            };
            summarize_to_cas(&self.inner.cas, &path).await
        }
        .instrument(span)
        .await
    }

    async fn release_worktree(&self, wt: &WorktreeId) -> Result<(), GitError> {
        let span = tracing::info_span!("adapter", kind = "git", op = "release_worktree");
        async move {
            let mut state = self.inner.state.lock().await;
            let live = state
                .live
                .remove(wt)
                .ok_or(GitError::UnknownWorktree(*wt))?;
            let LiveWorktree {
                path,
                path_str,
                branch,
                ctx,
                allocated,
                _watcher,
            } = live;
            // Stop the watch (and thereby the debounce loop) before the tree
            // is mutated, so removal churn never surfaces as diff.ready.
            drop(_watcher);

            self.run_git(&["worktree", "remove", "--force", &cli_path(&path)])
                .await?;
            state.registry.remove(&path_str);
            self.inner.persist_registry(&state).await?;

            let mut payload = serde_json::Map::new();
            // Byte-identical to the spelling the allocation minted (v1).
            payload.insert("path".into(), Value::String(path_str));
            if let Some(branch) = branch {
                payload.insert("branch".into(), Value::String(branch));
            }
            self.inner
                .emit("worktree.released", &ctx, allocated, Value::Object(payload))?;
            Ok(())
        }
        .instrument(span)
        .await
    }
}

/// Does this notify event report a CHANGE to the tree, or merely a read of it?
///
/// `diff.ready` is about content, so only content events may wake the debounce
/// loop: `Create`, `Modify`, `Remove`, and the two catch-alls (`Any`/`Other`,
/// kept as changes because an unclassified event is not evidence of a read).
/// Every other `Access` variant is treated as a read and ignored — including
/// the ambiguous `Access(Any)`, on the ground that an event a backend could
/// classify no further than "access" is not evidence that anything changed.
/// `Close(Write)` is the one retained, because it marks a completed write. No
/// backend in use reports a write as a bare `Access(Any)`; if one ever does,
/// this is the line that will need to know.
///
/// **This is a platform-asymmetric defect fix, measured rather than reasoned
/// (registry-convergence remediation, 2026-07-24).** The inotify backend arms
/// `IN_OPEN`, so *opening a file for reading* inside a watched tree surfaces as
/// `EventKind::Access(Open(Any))`. The closure previously treated every `Ok`
/// event as a change, which had two measured consequences on Linux — the
/// daemon's platform — and neither on Windows, whose `ReadDirectoryChangesW`
/// backend has no open/read notification at all (so host `/vet` could not see
/// either):
///
/// 1. **The golden-path clobber.** `gates::merge_worktree` runs `git add -A`
///    plus `git commit` INSIDE the still-watched tree. (At the time nothing
///    released the watch — [`RepoSubstrate::release_worktree`] had no
///    production caller. DR-049 §Decision 1 gave it one: the run task releases
///    at merge, so the watch no longer outlives the run. The filter below stays
///    the narrower fix and is still load-bearing — it holds for every OTHER
///    reader of a watched tree, and for the whole pre-merge window before the
///    release.) Those commands modify nothing INSIDE the worktree: a linked
///    tree's index and refs live in its private gitdir and its objects in the
///    shared repo, so neither `add` nor `commit` writes a byte under the
///    watched path (measured — nothing in the tree is newer afterwards). They
///    only READ the tracked files, and those reads woke the loop. 250 ms later
///    the now-clean tree summarized to the header-only string and appended a
///    fresh `diff.ready`, overwriting `WorktreeState.last_diff` on a worktree
///    `diff.merged` had just folded to merged with the merged diff (one
///    collapsed `status` field at the time; DR-049 §Decision 2 split it into
///    `lifecycle` + `outcome`). Derived state then disagreed with the log about
///    what was merged
///    (I3). Measured: 1 post-merge `diff.ready` of 26 bytes (the bare header)
///    under WSL, 0 under Windows.
/// 2. **A self-feeding treadmill.** [`summarize_to_cas`] reads the tree to hash
///    it, which fired `Access(Open)` on the tree it was summarizing, which
///    re-armed the debounce, which re-summarized. The `last_hash` dedup kept
///    the extra facts off the log, so the cost was invisible: a gix status walk
///    plus a blake3 of the changed files every 250 ms, forever, per allocated
///    worktree. Measured on one allocation with a single write and then no test
///    activity at all: 40 notify events in a 2 s window before the fix, 0
///    after (`tests/diff_ready.rs`,
///    `an_allocated_worktree_goes_quiet_when_nothing_touches_it`).
///
/// Filtering here rather than dropping the watch at merge is the narrower fix
/// and the one the evidence names: the merge burst is entirely reads, so no
/// change event is lost by ignoring it, and the fix holds for every other
/// reader of a watched tree (a verifier, a grep, the operator's editor opening
/// a file) rather than for the one caller that was caught.
fn is_change_event(kind: &notify::EventKind) -> bool {
    use notify::event::{AccessKind, AccessMode};

    match kind {
        notify::EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        notify::EventKind::Access(_) => false,
        _ => true,
    }
}

/// Trailing-edge debounce loop for one worktree's notify stream: after any
/// event, wait for [`DEBOUNCE_MS`] of quiet, then summarize the tree into the
/// CAS and emit one `diff.ready`. Consecutive identical summaries are not
/// re-emitted (an unchanged tree carries no new information). Ends when the
/// watcher (the channel's only sender) is dropped on release.
async fn debounce_loop(
    inner: Arc<Inner>,
    path: PathBuf,
    path_str: String,
    ctx: FactCtx,
    causation: Option<Ulid>,
    mut rx: mpsc::Receiver<()>,
) {
    let mut last_hash: Option<String> = None;
    while rx.recv().await.is_some() {
        loop {
            match tokio::time::timeout(Duration::from_millis(DEBOUNCE_MS), rx.recv()).await {
                Ok(Some(())) => continue, // burst still going: keep absorbing
                Ok(None) => return,       // released mid-burst: emit nothing
                Err(_elapsed) => break,   // quiet for DEBOUNCE_MS: fire
            }
        }
        match summarize_to_cas(&inner.cas, &path).await {
            Ok(r) => {
                if last_hash.as_deref() == Some(r.hash.as_str()) {
                    continue;
                }
                let hash = r.hash.clone();
                let payload = serde_json::json!({ "worktree": path_str, "diff": r });
                match inner.emit("diff.ready", &ctx, causation, payload) {
                    // The suppression hash advances ONLY on a fact that
                    // actually landed. Advancing it first (as this loop did
                    // until the registry-convergence remediation) made a
                    // refused append lose that summary permanently: the next
                    // identical summary matched the hash of a fact the log
                    // never received and was suppressed as a duplicate of
                    // nothing. There is no caller here to fail — see the
                    // module header's fact-delivery split — so re-emission at
                    // the next filesystem event is the whole recovery, and it
                    // is stated rather than implied.
                    Ok(_) => last_hash = Some(hash),
                    Err(e) => tracing::warn!(
                        error = %e,
                        worktree = %path_str,
                        "diff.ready could not be appended; the summary is NOT on the log and \
                         will be re-emitted at the next change to this tree"
                    ),
                }
            }
            Err(e) => tracing::warn!(error = %e, "diff summary failed; skipping emission"),
        }
    }
}

/// Render the worktree's diff summary (gix, in `spawn_blocking`) and persist
/// it to the CAS as `text/x-diff` (DEFAULT mime, ontology v1).
async fn summarize_to_cas(cas: &Arc<Cas>, worktree: &Path) -> Result<CasRef, GitError> {
    let cas = Arc::clone(cas);
    let worktree = worktree.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let text = summary::diff_summary_text(&worktree)?;
        Ok(cas.put(text.as_bytes(), "text/x-diff")?)
    })
    .await
    .map_err(join_err)?
}

/// One linked worktree as reality reports it: canonical path plus the
/// checked-out branch (`None` when detached) — the reconciliation scan's
/// comparison unit against a registry entry.
struct ActualTree {
    path: PathBuf,
    branch: Option<String>,
}

/// One raw block of `git worktree list --porcelain` output, pre-resolution.
struct PorcelainBlock {
    path: PathBuf,
    branch: Option<String>,
}

/// Parse `git worktree list --porcelain`: blocks led by `worktree <path>`,
/// carrying `branch refs/heads/<name>` for attached checkouts. `detached`,
/// `HEAD <oid>`, `locked`, `prunable`, and blank lines add nothing the scan
/// compares on — branch stays `None` for detached trees.
fn parse_worktree_porcelain(out: &str) -> Vec<PorcelainBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<PorcelainBlock> = None;
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(done) = current.take() {
                blocks.push(done);
            }
            current = Some(PorcelainBlock {
                path: PathBuf::from(path),
                branch: None,
            });
        } else if let Some(reference) = line.strip_prefix("branch ")
            && let Some(block) = current.as_mut()
        {
            block.branch = Some(
                reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or(reference)
                    .to_string(),
            );
        }
    }
    if let Some(done) = current.take() {
        blocks.push(done);
    }
    blocks
}

/// Roll the in-memory `conflicted` dedup mark back to `false` for one key.
///
/// Called on exactly one path: the conflict fact was minted, the sink REFUSED
/// it, and `persist_registry` therefore never ran. Leaving the optimistic mark
/// standing would put memory and disk into disagreement about whether a
/// collision has been announced, and memory is the side that decides — so the
/// next claim on that path would be refused SILENTLY, with nothing on the log
/// to say why. Missing key is a no-op: the caller has already established the
/// entry exists, and re-establishing it here would be ceremony.
fn unmark_conflicted(state: &mut State, key: &str) {
    if let Some(entry) = state.registry.get_mut(key) {
        entry.conflicted = false;
    }
}

/// serde helper: keep the registry JSONL lean — `conflicted` is written only
/// once a conflict has actually been surfaced.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}

/// UTF-8 rendering of a canonical path — the registry key. Non-UTF-8 paths
/// cannot ride JSON payloads and are refused rather than lossily mangled.
fn utf8_path(p: &Path) -> Result<String, GitError> {
    p.to_str()
        .map(str::to_owned)
        .ok_or_else(|| GitError::Registry(format!("non-UTF-8 path: {}", p.display())))
}

/// Path spelling for the `git` CLI: strip the Windows extended-length prefix
/// (`\\?\`, and `\\?\UNC\` back to `\\`), which canonicalization introduces
/// and the CLI does not reliably accept. Elsewhere the canonical spelling is
/// kept verbatim.
fn cli_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    }
}

fn join_err(e: tokio::task::JoinError) -> GitError {
    GitError::Git(format!("background task join: {e}"))
}

#[cfg(test)]
mod tests {
    use notify::EventKind;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind};

    use super::is_change_event;

    /// The read/change split, asserted on the event VARIANTS rather than on a
    /// live filesystem.
    ///
    /// The behavioral guard for this
    /// (`crates/rezidnt-adapters/git/tests/diff_ready.rs`,
    /// `a_git_commit_inside_the_watched_tree_is_not_a_change`) can only be
    /// meaningful where the backend reports reads at all, which is inotify and
    /// not `ReadDirectoryChangesW` — so on Windows it passes vacuously. This
    /// test does not: `Access(Open)` is a value on every platform, so the rule
    /// that the defect turned on is judged in host `/vet` too.
    ///
    /// NARROWED, NOT CLOSED — say it that way (the arc's own precedent is the
    /// DR-046 source guard). What host `/vet` judges is the PREDICATE, never
    /// the closure's USE of it: delete the guard clause that calls this from
    /// the watch loop and this test stays green, and only the WSL-only
    /// behavioral boards go red. The wiring remains outside host `/vet`.
    #[test]
    fn reads_are_not_changes_and_writes_are() {
        // The exact variant the inotify backend produced during `git add -A`
        // and `git commit` inside a watched worktree (measured, WSL).
        assert!(
            !is_change_event(&EventKind::Access(AccessKind::Open(AccessMode::Any))),
            "opening a file for READING is not a change to the tree: git reads every tracked \
             file during `add`/`commit`, and treating those opens as changes is what appended a \
             post-merge `diff.ready` over the merged one"
        );
        assert!(!is_change_event(&EventKind::Access(AccessKind::Read)));
        assert!(!is_change_event(&EventKind::Access(AccessKind::Any)));
        assert!(!is_change_event(&EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));

        // A completed WRITE reported through the same enum is a change.
        assert!(
            is_change_event(&EventKind::Access(AccessKind::Close(AccessMode::Write))),
            "`Close(Write)` is the one Access variant that reports a finished write; ignoring it \
             would risk dropping a real change on a backend that reports writes that way"
        );
        assert!(is_change_event(&EventKind::Create(CreateKind::File)));
        assert!(is_change_event(&EventKind::Modify(ModifyKind::Data(
            DataChange::Any
        ))));
        assert!(is_change_event(&EventKind::Remove(RemoveKind::File)));
        // Unclassified events stay changes: not knowing what happened is not
        // evidence that nothing did.
        assert!(is_change_event(&EventKind::Any));
        assert!(is_change_event(&EventKind::Other));
    }
}
