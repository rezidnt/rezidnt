//! DR-049 ORACLE (lane 1, part 2) — the HOST-SIDE structural legs of criteria
//! (a) and (c): the release lifecycle's production call paths exist as code,
//! not as comments.
//!
//! HOST-RUNNABLE, and deliberately so (the `registry_convergence_structure.rs`
//! precedent): `bins/rezidentd/src/main.rs` declares `mod runs`, `mod mcp`,
//! `mod gates` under `#[cfg(unix)]`, so every daemon behavior test is invisible
//! to host `/vet`. These guards read the sources as text and run everywhere.
//! The behavioral judges live in `tests/dr049_release_lifecycle_e2e.rs`
//! (`#[cfg(unix)]`, WSL-side) — this file narrows that gap, it does not close
//! it.
//!
//! ## RED MODE (assert-red on today's tree, stated per test)
//!
//! All three tests fail TODAY on assertions, because the state DR-049 opens
//! against is exactly what it says it is (every premise checked against the
//! tree, §Context):
//!
//! - `release_worktree` has NO production caller — the daemon sources mention
//!   it in comments only, so the comment-stripped scan counts zero call sites;
//! - `RunTaskContext` carries the worktree as a `PathBuf` only
//!   (`bins/rezidentd/src/runs.rs:1615`) — no `WorktreeId` appears in the
//!   file's code, so the `WorktreeId`-keyed release verb is uncallable from the
//!   run task;
//! - no MCP tool dispatches an explicit release — `tools_call` in
//!   `crates/rezidnt-mcp/src/lib.rs` has no `release_worktree` arm.
//!
//! ## What these guards are, and are not (disclosure, house style)
//!
//! SOURCE-TEXT guards with a naive `//`-strip (a `//` inside a string literal
//! truncates that line — acceptable for counting these identifiers, none of
//! which plausibly rides a string). A call site assembled through an alias or
//! a macro would slip past them. They are BACKSTOPS so host `/vet` goes red on
//! this slice at all; the judges are the WSL e2e board's replayed logs. Say
//! "the identifier appears in the file's code", never "the lifecycle works".
//!
//! ## Why the watcher-drop leg is (structurally) THIS file's first test
//!
//! DR-049 criterion (a) wants the watcher and debounce task "demonstrably
//! dropped — asserted structurally host-side". The drop MECHANISM is already
//! built and unit-tested in the adapter: `release_worktree` drops `_watcher`,
//! which closes the debounce mpsc and ends the loop
//! (`crates/rezidnt-adapters/git/src/lib.rs`, DR-049 §Decision 4 verified it).
//! What production lacks is any CALL reaching that mechanism. So the honest
//! structural assertion host-side is "the production call site exists"; the
//! behavioral assertion (no `diff.ready` after release) is WSL-only, per
//! DR-049 risk (3).

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The file with `//` line comments stripped (naive: everything from the first
/// `//` on a line is dropped, string literals included — disclosed in the
/// module header). Good enough to distinguish "mentioned in the OWED comment"
/// from "called in code", which is the entire question this board asks.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// CRITERION (a), structural leg — the run task's sources call
/// `release_worktree` in CODE, so a merged run's watcher/debounce/tree/registry
/// teardown is reachable from production at all.
///
/// DR-049 §Decision 1: after `diff.merged`, the run task calls
/// `release_worktree`. §Decision 4: the release drops the notify watcher and
/// debounce task — the mechanism is already implemented and tested in the
/// adapter; the slice owes the CALL. Today `bins/rezidentd/src/runs.rs` holds
/// the "OWED: the allocation is never RELEASED" comment where the call would
/// go, and `bins/rezidentd/src/gates.rs` mentions the verb in a doc comment —
/// comments are exactly what the strip removes, so this scan counts ZERO and
/// the test is RED.
///
/// Both `runs.rs` and `gates.rs` are scanned so the guard does not over-pin
/// WHERE in the post-merge chain the implementer places the call (the run task
/// body or the merge helper it drives) — either satisfies §Decision 1.
#[test]
fn the_daemon_calls_release_worktree_in_production_code() {
    let sources = ["runs.rs", "gates.rs"];
    let hits: usize = sources
        .iter()
        .map(|name| {
            code_only(&read(&crate_root().join("src").join(name)))
                .matches("release_worktree")
                .count()
        })
        .sum();

    assert!(
        hits >= 1,
        "DR-049 §Decision 1: a merged worktree is RELEASED at merge — the run task must call \
         `release_worktree` after `diff.merged`. The comment-stripped sources of {sources:?} \
         contain {hits} occurrences, i.e. the verb is still mentioned only in comments (the \
         `runs.rs` OWED note names this exact spot). Until a production call exists, every \
         allocation leaks a notify watcher plus its debounce task for the daemon's lifetime, \
         the tree stays on disk, and the sole-allocator registry only ever grows \
         (DR-047 risk-register ADD 1). The drop mechanism itself is already built and tested \
         in the adapter (§Decision 4) — what is owed is this call."
    );
}

/// DR-049 §Decision 5, structural leg — `WorktreeId` is threaded through the
/// run task. The release verb is `WorktreeId`-keyed
/// (`RepoSubstrate::release_worktree(&self, wt: &WorktreeId)`), and
/// `RunTaskContext` carries the worktree as a `PathBuf` only, so the daemon
/// cannot call the verb it owns without threading the id. RED today: the
/// comment-stripped `runs.rs` contains no `WorktreeId` at all (the one mention
/// is inside the OWED comment).
#[test]
fn the_run_task_threads_the_worktree_id() {
    let runs = crate_root().join("src").join("runs.rs");
    let hits = code_only(&read(&runs)).matches("WorktreeId").count();

    assert!(
        hits >= 1,
        "DR-049 §Decision 5: `WorktreeId` is threaded through `RunTaskContext` — the context \
         carries only the worktree `PathBuf` today, and `release_worktree` is keyed by \
         `WorktreeId`, so the run task cannot release what it allocated. The comment-stripped \
         {} contains {hits} occurrences of `WorktreeId`. Thread the id from `alloc_worktree`'s \
         return through the context to the release call.",
        runs.display()
    );
}

/// CRITERION (c), structural leg — the operator surface dispatches an explicit
/// release tool, so a failed run's retained tree is closeable at all.
///
/// DR-049 §Decision 3: a failed run's tree survives until an EXPLICIT release
/// closes it — "operator or lead calls the release verb, exposed MCP-first
/// (I5) on the write-capable operator surface, never the read-only board
/// (DR-031)". The DR names no tool; this board PINS the oracle's choice so the
/// e2e judge has a callable surface, and disclosed here so the implementer
/// knows it is a pin, not a ratified name:
///
/// - tool name `release_worktree` — snake_case verb, matching the trait verb
///   and the `resolve_permit`/`kill_run` mutating-tool naming;
/// - arguments `{badge, path}` — `badge` because every mutating MCP call
///   passes the §12 door first (the `resolve_permit`/`kill_run` precedent),
///   `path` because the canonicalized path is the identity every consumer
///   already keys on (the fold's `worktrees` map, the registry line, the
///   `worktree.released` v1 payload).
///
/// If the implementer lands a different name, this test and the e2e move
/// TOGETHER with a note — never by weakening either.
///
/// RED today: `tools_call` in `crates/rezidnt-mcp/src/lib.rs` answers
/// `unknown tool` for anything but its eleven dispatch arms, and none of them
/// is a release. The scan counts the quoted literal in code (the dispatch arm
/// and the `tools/list` entry are both quoted strings — the naive `//`-strip
/// keeps quoted strings on comment-free lines, which these are).
#[test]
fn the_mcp_write_surface_dispatches_release_worktree() {
    let mcp = crate_root().join("../../crates/rezidnt-mcp/src/lib.rs");
    let hits = code_only(&read(&mcp))
        .matches("\"release_worktree\"")
        .count();

    assert!(
        hits >= 1,
        "DR-049 §Decision 3: the explicit release of a failed run's tree is exposed MCP-FIRST \
         (I5) on the write-capable operator surface. {} dispatches no `release_worktree` tool \
         (found {hits} quoted occurrences in code), so the retained-for-triage tree of \
         §Decision 3 has no closing door and failed trees accumulate with no operator recourse \
         — the risk register accepts accumulation until an operator ACTS, not accumulation \
         forever. Tool name and args are the oracle's disclosed pin (see this test's doc); the \
         behavioral judge is `dr049_release_lifecycle_e2e.rs`.",
        mcp.display()
    );
}
