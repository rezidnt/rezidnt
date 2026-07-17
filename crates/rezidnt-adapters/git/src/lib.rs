//! rezidnt git adapter (doc §7): gix reads, git-CLI mutations, notify
//! watcher, sole-allocator worktree registry (DR-001).
//!
//! ## Contract (pinned by the S2 oracle tests; payloads ratified in
//! `spec/ontology.md`, S2 set 2026-07-17)
//!
//! - **Sole allocator (DR-001, BINDING):** every worktree is registered under
//!   its canonicalized path in the [`REGISTRY_PATH`] file with an `allocator`
//!   field. A second claim on an already-registered canonicalized path emits
//!   exactly one `worktree.conflict` — never silent double-tracking, never a
//!   duplicate registry entry.
//! - **Registry format (DEFAULT):** JSON Lines at `<repo>/.rezidnt/worktrees`,
//!   one live entry per line: `{"path": <canonicalized>, "allocator":
//!   "rezidnt"|"human", "branch"?: <string>, "id"?: <ULID>,
//!   "allocated_event"?: <ULID>, "conflicted"?: <bool>}`. The last three are
//!   the S2-remediation additions that make allocation identity and the
//!   exactly-once marks durable across restarts. The format evolves
//!   additively; pre-remediation lines parse with these migration defaults:
//!   missing `id`/`allocated_event` → the allocation is not releasable by id
//!   (ids were process-local before the fields existed, so nothing is lost)
//!   and later facts carry no causation; missing `conflicted` → `false` (a
//!   legacy collision surfaced before the upgrade may re-surface at most
//!   once). The observed mark needs no field: a `"human"` entry exists only
//!   because `worktree.observed` was emitted, so the entry IS the mark.
//!   `release_worktree` closes (removes) the entry.
//! - **On-open reconciliation scan (S2 remediation):** [`GitAdapter::open`]
//!   compares the reloaded registry against `git worktree list --porcelain`.
//!   Intact rezidnt allocations (the private-gitdir identity marker carries
//!   the registered [`WorktreeId`] — branch is NOT identity, S2-T3) are
//!   rebuilt live — releasable under their persisted id, re-watched; a tree
//!   without (or with a mismatched) marker on a rezidnt-registered path is a
//!   takeover, surfaced as exactly one `worktree.conflict` forever;
//!   unregistered linked
//!   trees are discovered through the same dedup path as
//!   [`GitAdapter::observe`]. Scan facts ride the broadcast and are pinned
//!   via [`GitAdapter::startup_facts`].
//! - **Watcher (DEFAULT debounce fixed by ontology):** `alloc_worktree`
//!   starts the notify watch on the new tree; filesystem writes are debounced
//!   250 ms ([`DEBOUNCE_MS`], trailing-edge: emission happens once the tree
//!   has been quiet that long) and surface as `diff.ready` carrying the diff
//!   summary as a CAS ref (I2 — never inline diff bytes). S2 exit criterion:
//!   `diff.ready` lands within 1 s of the write, post-debounce.
//! - **[`GitAdapter::observe`]** is the watcher's ingest point for a worktree
//!   discovered out-of-band (human `git worktree add`): unregistered path →
//!   `worktree.observed` (allocator `"human"`, registered so re-observation
//!   stays silent); already-registered path → `worktree.conflict`,
//!   deduplicated per canonicalized path so repeated observation of the same
//!   collision emits nothing further — forever: the dedup marks persist in
//!   the registry, so restart never resurfaces a fact.
//! - **Facts** ride the envelope with `source` = [`SOURCE_ID`], `v = 1`, and
//!   payloads per `spec/ontology.md`. All facts of one adapter instance share
//!   a correlation ULID minted at [`GitAdapter::open`] (DEFAULT); `diff.ready`
//!   and `worktree.released` carry the allocation fact's id as `causation`.

mod summary;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rezidnt_cas::Cas;
use rezidnt_types::{Event, SourceId, Subject, refs::CasRef};
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
pub const REGISTRY_PATH: &str = ".rezidnt/worktrees";

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

/// Allocation request (doc §7 `WorktreeReq`; fields DEFAULT).
#[derive(Debug, Clone)]
pub struct WorktreeReq {
    /// Human-stable name; the adapter derives the on-disk location.
    pub name: String,
    /// Branch to create/check out; `None` with `detach` for detached HEAD.
    pub branch: Option<String>,
    /// `git worktree add --detach`: check out the current HEAD, no branch.
    pub detach: bool,
}

/// A live allocated worktree.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub id: WorktreeId,
    /// On-disk location (canonicalizes to the registry key).
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// Errors for the git adapter (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git: {0}")]
    Git(String),
    #[error("worktree registry: {0}")]
    Registry(String),
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

struct Inner {
    /// Canonicalized repo root.
    repo_root: PathBuf,
    registry_file: PathBuf,
    cas: Arc<Cas>,
    tx: broadcast::Sender<Event>,
    /// One correlation per adapter instance (DEFAULT): every fact this
    /// adapter emits belongs to the same causal chain.
    correlation: Ulid,
    /// Facts minted by the on-open reconciliation scan, set exactly once at
    /// the end of [`GitAdapter::open`] (the scan predates every subscriber,
    /// so they are pinned here for deterministic retrieval — see
    /// [`GitAdapter::startup_facts`]).
    startup: OnceLock<Vec<Event>>,
    state: Mutex<State>,
}

impl Inner {
    /// Mint and publish one fact (`v = 1`, `source` = [`SOURCE_ID`]).
    /// Returns the fact so callers can causally chain later facts (`.id`) or
    /// pin it (the startup scan collects its facts for [`GitAdapter::startup_facts`]).
    fn emit(
        &self,
        subject: &str,
        causation: Option<Ulid>,
        payload: Value,
    ) -> Result<Event, GitError> {
        let event = Event::new(
            SourceId::new(SOURCE_ID),
            None,
            Subject::new(subject),
            self.correlation,
            causation,
            1,
            payload,
        )?;
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
    /// [`REGISTRY_PATH`] registry and opens the CAS at `cas_root`.
    pub async fn open(repo_root: &Path, cas_root: &Path) -> Result<Self, GitError> {
        let span = tracing::info_span!("adapter", kind = "git", op = "open");
        async move {
            let repo_root = tokio::fs::canonicalize(repo_root).await?;
            let cas_root = cas_root.to_path_buf();
            let cas = tokio::task::spawn_blocking(move || Cas::open(&cas_root))
                .await
                .map_err(join_err)??;

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
                    cas: Arc::new(cas),
                    tx,
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
                    let watcher =
                        self.spawn_watcher(tree.path.clone(), key.clone(), entry.allocated_event)?;
                    state.live.insert(
                        id,
                        LiveWorktree {
                            path: tree.path.clone(),
                            path_str: key.clone(),
                            branch: entry.branch.clone(),
                            allocated: entry.allocated_event,
                            _watcher: watcher,
                        },
                    );
                } else if !entry.conflicted {
                    // The checkout is not what rezidnt registered: a human
                    // tree occupies the path. One collision, one fact.
                    let fact = self.inner.emit(
                        "worktree.conflict",
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
                let fact = self
                    .inner
                    .emit("worktree.observed", None, Value::Object(payload))?;
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
                self.inner
                    .emit("worktree.conflict", None, Value::Object(payload))?;
                self.inner.persist_registry(&state).await?;
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
            self.inner.persist_registry(&state).await?;
            self.inner
                .emit("worktree.observed", None, Value::Object(payload))?;
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Derive the on-disk location for a named allocation (DEFAULT):
    /// `<repo-parent>/<repo-name>-wt-<name>` — a sibling of the repo root so
    /// allocated trees never pollute the primary working tree.
    fn derive_worktree_path(&self, name: &str) -> Result<PathBuf, GitError> {
        let parent = self.inner.repo_root.parent().ok_or_else(|| {
            GitError::Git("repo root has no parent directory to host worktrees".into())
        })?;
        let repo_name = self
            .inner
            .repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");
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
        Ok(parent.join(format!("{repo_name}-wt-{safe}")))
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
        causation: Option<Ulid>,
    ) -> Result<notify::RecommendedWatcher, GitError> {
        use notify::Watcher as _;

        let (tx, rx) = mpsc::channel::<()>(WATCH_CHANNEL_BOUND);
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                match res {
                    // Send failure means the receiver (debounce loop) is
                    // gone, which only happens on release — nothing to do.
                    Ok(_event) => drop(tx.blocking_send(())),
                    Err(e) => tracing::warn!(error = %e, "notify watcher error"),
                }
            })?;
        watcher.watch(&path, notify::RecursiveMode::Recursive)?;

        let inner = Arc::clone(&self.inner);
        let span = tracing::info_span!("adapter", kind = "git-watch", worktree = %path_str);
        tokio::spawn(debounce_loop(inner, path, path_str, causation, rx).instrument(span));
        Ok(watcher)
    }
}

impl RepoSubstrate for GitAdapter {
    async fn alloc_worktree(&self, req: WorktreeReq) -> Result<Worktree, GitError> {
        let span =
            tracing::info_span!("adapter", kind = "git", op = "alloc_worktree", name = %req.name);
        async move {
            let target = self.derive_worktree_path(&req.name)?;
            let target_cli = cli_path(&target);
            match (&req.branch, req.detach) {
                (Some(branch), false) => {
                    self.run_git(&["worktree", "add", "-b", branch, &target_cli])
                        .await?
                }
                (None, true) => {
                    self.run_git(&["worktree", "add", "--detach", &target_cli])
                        .await?
                }
                (None, false) => self.run_git(&["worktree", "add", &target_cli]).await?,
                (Some(_), true) => {
                    return Err(GitError::Git(
                        "contradictory request: both a branch and detach".into(),
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
                    self.inner.emit(
                        "worktree.conflict",
                        None,
                        serde_json::json!({ "path": canonical_str, "holder": holder }),
                    )?;
                    self.inner.persist_registry(&state).await?;
                }
                return Err(GitError::Registry(format!(
                    "path already registered: {canonical_str}"
                )));
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
            payload.insert("allocator".into(), Value::String("rezidnt".into()));
            let allocated = self
                .inner
                .emit("worktree.allocated", None, Value::Object(payload))?
                .id;

            // The registry entry carries the allocation identity and the
            // allocated event id (S2 remediation) — minted just above so one
            // persist suffices — making the allocation releasable and its
            // causal chain recoverable across restart.
            state.registry.insert(
                canonical_str.clone(),
                RegistryEntry {
                    path: canonical_str.clone(),
                    allocator: "rezidnt".into(),
                    branch: req.branch.clone(),
                    id: Some(id),
                    allocated_event: Some(allocated),
                    conflicted: false,
                },
            );
            self.inner.persist_registry(&state).await?;

            // Watch starts after the allocated fact is minted so its id can
            // causally chain the diff.ready stream; callers only write after
            // alloc returns, so no event can precede the watch.
            let watcher =
                self.spawn_watcher(canonical.clone(), canonical_str.clone(), Some(allocated))?;
            state.live.insert(
                id,
                LiveWorktree {
                    path: canonical.clone(),
                    path_str: canonical_str,
                    branch: req.branch.clone(),
                    allocated: Some(allocated),
                    _watcher: watcher,
                },
            );
            Ok(Worktree {
                id,
                path: canonical,
                branch: req.branch,
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
                .emit("worktree.released", allocated, Value::Object(payload))?;
            Ok(())
        }
        .instrument(span)
        .await
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
                last_hash = Some(r.hash.clone());
                let payload = serde_json::json!({ "worktree": path_str, "diff": r });
                if let Err(e) = inner.emit("diff.ready", causation, payload) {
                    tracing::warn!(error = %e, "diff.ready emission failed");
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
