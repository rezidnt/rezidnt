//! DR-049 ORACLE (lane 1, part 2) — the daemon e2e judges for criteria (a)
//! and (c): released-at-merge on the golden path, and the failed tree that
//! survives until an explicit release closes it with its outcome preserved.
//!
//! `#[cfg(unix)]` + `*_e2e.rs`, per the house convention: this board runs on
//! WSL and NOT in host `/vet`. Everything liftable host-side has been lifted —
//! `tests/dr049_release_lifecycle_structure.rs` carries the structural legs
//! (production `release_worktree` call site, `WorktreeId` threading, the MCP
//! release tool's dispatch arm) as text guards that run everywhere. What is
//! left here genuinely needs a daemon: real allocations, a real merge, a real
//! failed gate, and a persisted log to replay (I3: the log is the judge).
//!
//! ## PLATFORM HONESTY (DR-049 risk (3))
//!
//! The watcher-drop assertion splits in two, exactly as the criterion does:
//! the STRUCTURAL leg is host-side (structure board); the BEHAVIORAL leg —
//! no `diff.ready` lands for a tree after `worktree.released` — is meaningful
//! ONLY where the notify backend reports the filesystem activity that would
//! wake a leaked watcher. That is inotify (WSL). `ReadDirectoryChangesW` emits
//! no open/read events, so a Windows run of the same scenario measured 0 where
//! WSL measured 1 (registry-convergence remediation). This whole file is
//! `#[cfg(unix)]`; the quiescence test additionally pins the watcher was LIVE
//! on the log before the merge, so its silence afterwards is evidence and not
//! vacuum.
//!
//! ## RED MODE (assert-red on today's tree, per test)
//!
//! Every test compiles against today's tree and fails on an ASSERTION:
//!
//! - the two golden-path tests fail at "a `worktree.released` fact is on the
//!   log": `release_worktree` has no production caller (the `runs.rs` OWED
//!   comment marks the spot), so no release fact ever lands;
//! - the failed-run survival test's disk/registry legs are GREEN today — the
//!   leak "provides" survival — and are disclosed as guards that become
//!   load-bearing the moment the golden path starts releasing at merge (they
//!   pin that the implementer must NOT blanket-release failed trees,
//!   §Decision 3). Its fold legs are RED: the derived entry has no
//!   `lifecycle`/`outcome` fields yet (the split is lane 1's
//!   `crates/rezidnt-state/tests/dr049_lifecycle_outcome_split.rs` board);
//! - the explicit-release test fails at the MCP call: `tools_call` answers
//!   `unknown tool: release_worktree`.
//!
//! ## SPEC GAPS, FLAGGED rather than vibes-tested
//!
//! 1. **`outcome = failed` has no attributing fact.** Criterion (c) demands
//!    the fold show `outcome = failed`, but `gate.failed` v1 carries
//!    `{run, gate, verifier, evidence, inputs}` — NO worktree — and no other
//!    minted fact ties a failure to a tree path. The reducer must either join
//!    run → tree through the shared `correlation` envelope field (pure, but a
//!    join the fold has never done) or a `/subject` session must add an
//!    additive optional `worktree` field to `gate.failed` (DR-049 §Decision 6
//!    says further discovered subject changes route through `/subject`).
//!    These tests judge the CONSEQUENCE (the folded entry) and deliberately do
//!    not pin the mechanism — but the implementer must settle it, and the
//!    settlement is warden-adjacent.
//! 2. **The MCP release tool's name/args are an oracle PIN, not a ratified
//!    shape**: `release_worktree` `{badge, path}` — rationale in the structure
//!    board's third test. A different landed name moves both boards together.
//! 3. `abandoned` stays unreachable from the taxonomy — flagged in the lane-1
//!    fold board's header; not re-litigated here.
//!
//! Scenarios are serialized (`SERIAL`) — core contention is the documented
//! flake vector for spawn-heavy daemon suites.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::{
    DaemonGuard, connect, gated_stub_harness, make_gated_project, mcp_post, open_request,
    read_until, rpc, send_line, start_daemon, start_daemon_with_mcp, wait_for_lockfile,
};
use rezidnt_fabric::EventLog;
use rezidnt_state::fold;
use rezidnt_types::Event;
use serde_json::{Value, json};

/// Far-end-of-chain deadline (spawn → complete → pre_merge → merge/fail), the
/// `golden_path.rs` tolerance.
const CHAIN_DEADLINE: Duration = Duration::from_secs(45);
const LOCK_DEADLINE: Duration = Duration::from_secs(10);

/// Stub-harness inter-message gap for the watcher-liveness scenario: 700 ms,
/// for the reason `registry_convergence_e2e.rs` documents at length — at 50 ms
/// the run is over before the adapter watcher's 250 ms trailing debounce
/// elapses, so the watcher's fact never exists and a liveness precondition
/// would be vacuous.
const WATCHER_GAP_MS: u64 = 700;

/// How long the daemon outlives the observed terminal fact before the log is
/// cold-read: room for the release (same task, moments later), the watcher's
/// debounce window several times over, and the capture chunking.
const SETTLE: Duration = Duration::from_millis(2000);

/// Serialize the scenarios (the `operator_liveops_e2e.rs` precedent): four
/// real daemons + stub harnesses under the default multi-threaded runner is
/// the documented flake vector. Poisoning ignored — a panicking test still
/// releases the lock.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Kill the daemon and re-open its PERSISTED log — the judge (I3).
fn cold_read(daemon: &mut DaemonGuard) -> Vec<Event> {
    let _ = daemon.child.kill();
    let _ = daemon.child.wait();
    let log = EventLog::open(&daemon.db).expect("re-open the daemon's persisted log cold");
    log.read_from(1)
        .expect("read the persisted log from seq 1")
        .into_iter()
        .map(|row| row.event)
        .collect()
}

fn subjects(log: &[Event]) -> Vec<&str> {
    log.iter().map(|e| e.subject.as_str()).collect()
}

fn position(log: &[Event], subject: &str) -> Option<usize> {
    log.iter().position(|e| e.subject.as_str() == subject)
}

/// The single allocated path on this log (every scenario here allocates one).
fn allocated_path(log: &[Event]) -> String {
    let facts: Vec<&Event> = log
        .iter()
        .filter(|e| e.subject.as_str() == "worktree.allocated")
        .collect();
    assert_eq!(
        facts.len(),
        1,
        "precondition: one agent, one allocation (DR-046 C4 pins exactly-once); saw {:?}",
        subjects(log)
    );
    facts[0].payload()["path"]
        .as_str()
        .expect("`path` is a REQUIRED worktree.allocated v1 field")
        .to_string()
}

/// Registry lines at the adapter's sole-allocator file. A missing file reads
/// as empty: after the last release the persisted registry may hold zero
/// lines, and "no claim for this path" is the question every caller asks.
fn registry_entries(repo: &Path) -> Vec<Value> {
    let file = repo.join(rezidnt_adapter_git::REGISTRY_PATH);
    std::fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("registry line is JSON"))
        .collect()
}

/// The registry claim for `path`, if any. Compared on the raw string: the
/// released fact's `path` is byte-identical to the spelling the allocation
/// minted, which is the canonicalized string the registry stores — and a
/// released tree no longer exists on disk to canonicalize.
fn registry_claim<'a>(entries: &'a [Value], path: &str) -> Option<&'a Value> {
    entries.iter().find(|e| e["path"].as_str() == Some(path))
}

/// The folded worktree entry for `path`, SERIALIZED — the shape every
/// consumer reads (same discipline as the lane-1 fold board: no typed-field
/// access, so this file compiles against today's single-`status` shape and
/// fails on assertions, never on a build error).
fn folded_entry(log: &[Event], path: &str) -> Value {
    let graph = fold(log.iter());
    let wt = graph
        .worktrees
        .get(path)
        .unwrap_or_else(|| panic!("the fold holds a worktree entry for {path}"));
    serde_json::to_value(wt).expect("worktree entry serializes")
}

/// Drive one gated `open` until `stop` matches on the tail, then let the
/// daemon outlive the observed fact by [`SETTLE`] before returning. The
/// caller's fixture `TempDir` stays alive with the caller — the assertions
/// read trees and the registry off disk. Returns the still-running guard and
/// every tail line seen.
fn open_and_watch(spec: &str, stop: impl FnMut(&Value) -> bool) -> (DaemonGuard, Vec<Value>) {
    let daemon = start_daemon();
    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(spec));

    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let seen = read_until(&mut tail, CHAIN_DEADLINE, stop);
    std::thread::sleep(SETTLE);
    (daemon, seen)
}

// ---------------------------------------------------------------------------
// Criterion (a) — the golden path releases at merge.
// ---------------------------------------------------------------------------

/// CRITERION (a), legs 1–3 on one replayed golden run: `worktree.released`
/// lands on the log AFTER `diff.merged`; the tree is gone from disk; the
/// sole-allocator registry entry is closed.
///
/// "Closed" is REMOVAL: the adapter's release removes the claim line and
/// persists (the registry is the adapter's live-claims working copy; history
/// lives on the log — I3). So the assertion is "no claim for this path
/// remains", not a status flag.
///
/// RED today at the first assertion: no production caller of
/// `release_worktree` exists, so no release fact ever lands, the tree stays on
/// disk, and the claim stays open — DR-047 risk-register ADD 1, verbatim.
#[test]
fn a_merged_runs_worktree_is_released_gone_from_disk_and_closed_in_the_registry() {
    let _serial = lock();
    let (project, spec) = make_gated_project(50);
    let repo = project.path().join("repo");
    let (mut daemon, _seen) = open_and_watch(&spec, |v| v["subject"] == "diff.merged");
    let log = cold_read(&mut daemon);

    let merged_at = position(&log, "diff.merged").expect("the fixture waited for diff.merged");
    let path = allocated_path(&log);

    // Leg 1 — the release FACT, ordered after the merge it completes.
    let released_at = log.iter().position(|e| {
        e.subject.as_str() == "worktree.released" && e.payload()["path"].as_str() == Some(&path)
    });
    let released_at = released_at.unwrap_or_else(|| {
        panic!(
            "DR-049 §Decision 1: a golden-path run ends with `worktree.released` on the log — \
             after `diff.merged`, the run task calls `release_worktree` and the adapter emits \
             the fact. No release for {path} is on this log: the run completed, merged, and \
             LEAKED its allocation (watcher + debounce task live for the daemon's lifetime, \
             tree on disk, registry claim open). Subjects seen: {:?}",
            subjects(&log)
        )
    });
    assert!(
        released_at > merged_at,
        "the release completes the lifecycle the merge closed — `worktree.released` (index \
         {released_at}) must land AFTER `diff.merged` (index {merged_at}); a release that \
         precedes the merge would have removed the tree the merge reads"
    );

    // Leg 2 — the tree is gone from disk.
    assert!(
        !Path::new(&path).exists(),
        "DR-049 criterion (a): the merged tree is GONE from disk — the merged diff is pinned \
         in CAS and the log, so a tree the log can rebuild is not a record (I3, the §Context \
         counterargument ruling). {path} still exists"
    );

    // Leg 3 — the sole-allocator registry claim is closed (removed).
    let entries = registry_entries(&repo);
    assert!(
        registry_claim(&entries, &path).is_none(),
        "DR-049 criterion (a): the registry entry for a released tree is CLOSED — the adapter \
         removes the claim line and persists. An open claim for {path} means the registry's \
         live claims only ever grow (DR-047 risk-register ADD 1). Registry holds: {entries:#?}"
    );
}

/// CRITERION (a), leg 4 — the BEHAVIORAL watcher-drop: after
/// `worktree.released`, NOTHING about that tree lands as `diff.ready`. WSL-ONLY
/// signal (module header): inotify reports the activity a leaked watcher would
/// wake on; `ReadDirectoryChangesW` does not, so a host run of this scenario
/// proves nothing — which is why this leg lives in the `#[cfg(unix)]` board
/// and the host-side leg is structural.
///
/// Two legs make a pass mean something (the
/// `the_merged_diff_is_not_clobbered_by_a_post_merge_watcher_fact` precedent):
///
/// 1. NON-VACUITY — the watcher was a LIVE emitter on this log before the
///    merge (at least one `diff.ready` from the adapter's source; the 700 ms
///    harness gap exists to give the 250 ms debounce room to fire);
/// 2. QUIESCENCE — zero `diff.ready` for the released path after the release
///    fact, with the daemon outliving the release by [`SETTLE`] (many debounce
///    windows), so absence is evidence and not an artifact of killing the
///    daemon early.
///
/// The release drops the watch BEFORE removing the tree precisely so removal
/// churn never surfaces as `diff.ready` (§Decision 4, adapter-side ordering) —
/// a fact here means either the drop regressed or a second watch exists.
///
/// RED today at the release-fact precondition: no release is ever emitted, so
/// the quiescence claim is unjudgeable — stated as the failure, not dressed as
/// a pass.
#[test]
fn release_quiesces_the_watcher_no_diff_ready_after_worktree_released() {
    let _serial = lock();
    let (_project, spec) = make_gated_project(WATCHER_GAP_MS);
    let (mut daemon, _seen) = open_and_watch(&spec, |v| v["subject"] == "diff.merged");
    let log = cold_read(&mut daemon);

    let merged_at = position(&log, "diff.merged").expect("the fixture waited for diff.merged");
    let path = allocated_path(&log);

    // Leg 1 — NON-VACUITY: the watcher observably emitted before the merge.
    let watcher_before = log[..merged_at]
        .iter()
        .filter(|e| {
            e.subject.as_str() == "diff.ready"
                && e.source.as_str() == rezidnt_adapter_git::SOURCE_ID
        })
        .count();
    assert!(
        watcher_before > 0,
        "precondition: the adapter's watcher appended at least one `diff.ready` BEFORE the \
         merge, so this board observes a live watcher being dropped rather than the absence \
         of one. Zero means the watch never started or the fixture stopped outliving the \
         250 ms debounce (see `WATCHER_GAP_MS`)"
    );

    // Leg 2 — the release fact exists (the gate for the quiescence claim).
    let released_at = log
        .iter()
        .position(|e| {
            e.subject.as_str() == "worktree.released" && e.payload()["path"].as_str() == Some(&path)
        })
        .unwrap_or_else(|| {
            panic!(
                "DR-049 criterion (a), behavioral leg: the watcher-drop is judged by silence \
                 AFTER `worktree.released` — and no release fact is on this log, because \
                 `release_worktree` has no production caller. Until the run task releases at \
                 merge (§Decision 1), the watcher demonstrably OUTLIVES the run it was started \
                 for. Subjects seen: {:?}",
                subjects(&log)
            )
        });

    // Leg 3 — QUIESCENCE: nothing about this tree lands after its release.
    let after: Vec<&Event> = log[released_at + 1..]
        .iter()
        .filter(|e| {
            e.subject.as_str() == "diff.ready" && e.payload()["worktree"].as_str() == Some(&path)
        })
        .collect();
    assert!(
        after.is_empty(),
        "a `diff.ready` for {path} landed AFTER `worktree.released`. The release drops the \
         notify watcher (closing the debounce mpsc) BEFORE removing the tree, so removal \
         churn must never surface as a watcher fact (DR-049 §Decision 4). A fact here means \
         the drop regressed or a second watch exists. Facts: {:#?}",
        after
            .iter()
            .map(|e| e.payload().clone())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Criterion (c) — a failed run's tree survives; an explicit release closes it
// with the outcome preserved.
// ---------------------------------------------------------------------------

/// An exec verifier speaking the §8 contract that always answers `fail` — the
/// deterministic failed-gate fixture (a REAL fail verdict, not a broken
/// verifier: `inconclusive` is a different verdict and a different test).
fn exec_fail_verifier(dir: &Path) -> std::path::PathBuf {
    let script = dir.join("verifier-fail.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\ncat > /dev/null\nprintf '{\"verdict\":\"fail\",\"evidence\":[],\"cost_ms\":7}\\n'\n",
    )
    .expect("write exec fail-verifier stub");
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");
    script
}

/// A gated project whose `pre_merge` ALWAYS fails: the `make_gated_project`
/// shape (committed seed, diff-writing stub harness, vet + pre_merge) with the
/// verifier set replaced by the fail stub. The run completes, the diff pins,
/// `gate.failed` lands, the merge is blocked — the §Decision 3 starting state.
fn make_failing_gated_project(gap_ms: u64) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("src/checkout")).expect("mkdir repo/src/checkout");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "oracle@rezidnt.test"]);
    git(&["config", "user.name", "rezidnt oracle"]);
    std::fs::write(repo.join("src/checkout/cart.rs"), "// cart v0\n").expect("seed file");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "dr049 failed-run seed"]);

    let harness = gated_stub_harness(dir.path(), gap_ms);
    let verifier = exec_fail_verifier(dir.path());

    let spec = format!(
        r#"[project]
name = "dr049-failed-run"
repo = "{repo}"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
gates = ["vet", "pre_merge"]
bare = true
harness_version = "2.1.191"
allowed_tools = ["Read", "Edit"]
bin_override = "{harness}"

[gates.pre_merge]
verifiers = [
  {{ exec = "{verifier}", name = "always-fail" }},
]
"#,
        repo = repo.display(),
        harness = harness.display(),
        verifier = verifier.display(),
    );
    (dir, spec)
}

/// The `gate.failed`-at-`pre_merge` stop condition for the failing fixture.
fn pre_merge_failed(v: &Value) -> bool {
    v["subject"] == "gate.failed" && v["payload"]["gate"] == "pre_merge"
}

/// CRITERION (c), first half — a failed run's tree demonstrably SURVIVES: on
/// disk, registry claim open, and the fold reads `lifecycle = allocated`,
/// `outcome = failed`.
///
/// The disk/registry legs are GREEN today — the leak provides survival — and
/// are guards, not oracles (module header): they pin that once the golden
/// path releases at merge, the implementer does NOT blanket-release failed
/// trees (§Decision 3 retains them for triage, explicit-only, no TTL). The
/// no-release-fact leg is the same guard on the log. The two FOLD legs are
/// RED today: no `lifecycle`/`outcome` fields exist, and — spec gap 1,
/// module header — no minted fact even attributes a failure to a tree path
/// yet, so `outcome = failed` needs an implementer settlement (correlation
/// join or a `/subject`-routed additive `gate.failed` field) that this test
/// deliberately judges only by its folded consequence.
#[test]
fn a_failed_runs_tree_survives_on_disk_registry_open_folded_allocated_and_failed() {
    let _serial = lock();
    let (project, spec) = make_failing_gated_project(50);
    let repo = project.path().join("repo");
    let (mut daemon, _seen) = open_and_watch(&spec, pre_merge_failed);
    let log = cold_read(&mut daemon);

    let path = allocated_path(&log);

    // Guards (green today, load-bearing after the golden path releases at
    // merge): no merge happened, nothing released, tree and claim survive.
    assert!(
        position(&log, "diff.merged").is_none(),
        "precondition: the always-fail verifier blocked the merge; a `diff.merged` here means \
         the fixture stopped failing and this test is judging the wrong scenario"
    );
    assert!(
        log.iter()
            .all(|e| e.subject.as_str() != "worktree.released"),
        "DR-049 §Decision 3: a FAILED run's tree is retained until an EXPLICIT release — v1 is \
         explicit-only, no TTL, no auto-reap at run end. A `worktree.released` on this log \
         means the daemon released a failed tree on its own initiative, destroying the triage \
         evidence the retention exists for"
    );
    assert!(
        Path::new(&path).exists(),
        "DR-049 criterion (c): the failed run's tree SURVIVES on disk for triage. {path} is \
         gone — a failed tree was reaped"
    );
    let entries = registry_entries(&repo);
    assert!(
        registry_claim(&entries, &path).is_some(),
        "DR-049 criterion (c): the failed tree's registry claim stays OPEN (the tree is still \
         genuinely allocated). No claim for {path}; registry holds: {entries:#?}"
    );

    // The fold — RED today (split fields absent; failure unattributed).
    let entry = folded_entry(&log, &path);
    assert_eq!(
        entry["lifecycle"],
        json!("allocated"),
        "DR-049 criterion (c): the folded entry reads `lifecycle = allocated` — the tree was \
         never released, and lifecycle answers ONLY the allocate/release question \
         (§Decision 2, split pinned by the lane-1 fold board). Entry: {entry:#}"
    );
    assert_eq!(
        entry["outcome"],
        json!("failed"),
        "DR-049 criterion (c): the folded entry reads `outcome = failed` — the run's pre_merge \
         gate failed and the merge was blocked. NOTE the flagged spec gap (module header): \
         `gate.failed` v1 names no worktree, so the implementer must settle HOW the fold \
         attributes the failure to this path (correlation join, or a `/subject`-routed \
         additive payload field) — this board judges the consequence only. Entry: {entry:#}"
    );
}

/// CRITERION (c), second half — an EXPLICIT release then closes the failed
/// tree: driven MCP-first (I5) through the write-capable operator surface
/// (`release_worktree {badge, path}` — the oracle's disclosed pin, spec gap 2),
/// and afterwards the tree is gone, the claim is closed, and the fold reads
/// `lifecycle = released` with `outcome = failed` PRESERVED — the failed
/// sibling of the lane-1 board's `released_after_merged_keeps_both`, judged on
/// a real log: the release fact sets lifecycle ONLY and must not clobber the
/// outcome the failure earned (§Decision 2).
///
/// RED today at the tool call: `tools_call` answers
/// `unknown tool: release_worktree`.
#[test]
fn an_explicit_mcp_release_closes_a_failed_tree_with_outcome_failed_preserved() {
    let _serial = lock();
    let (project, spec) = make_failing_gated_project(50);
    let repo = project.path().join("repo");

    let (mut daemon, lockfile) = start_daemon_with_mcp(None);
    let lock_info = wait_for_lockfile(&lockfile, LOCK_DEADLINE);
    let url = lock_info["url"].as_str().expect("lockfile url").to_string();
    let badge = lock_info["badge"]
        .as_str()
        .expect("operator badge token")
        .to_string();

    let mut opener = connect(&daemon.socket);
    send_line(&mut opener, &open_request(&spec));
    let mut tail = connect(&daemon.socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let seen = read_until(&mut tail, CHAIN_DEADLINE, pre_merge_failed);

    let path = seen
        .iter()
        .find(|v| v["subject"] == "worktree.allocated")
        .and_then(|v| v["payload"]["path"].as_str())
        .expect("the tail carried the run's worktree.allocated")
        .to_string();
    assert!(
        Path::new(&path).exists(),
        "precondition: the failed tree is on disk when the operator acts"
    );

    // THE ACT — the operator's explicit release, MCP-first (§Decision 3).
    let response = mcp_post(
        &url,
        &rpc(
            1,
            "tools/call",
            json!({"name": "release_worktree", "arguments": {"badge": badge, "path": path}}),
        ),
    );
    assert!(
        response.get("error").is_none(),
        "DR-049 §Decision 3: the explicit release of a failed tree is exposed MCP-FIRST (I5) \
         on the write-capable operator surface — `release_worktree {{badge, path}}` (the \
         oracle's disclosed pin; see the structure board). The surface refused the call: \
         {response:#}"
    );
    assert_ne!(
        response["result"]["isError"],
        json!(true),
        "the release call must be honored, not answered with a tool-level error: {response:#}"
    );

    // The fact lands (live), then the cold log is the judge.
    let _ = read_until(&mut tail, Duration::from_secs(15), |v| {
        v["subject"] == "worktree.released" && v["payload"]["path"].as_str() == Some(&path)
    });
    std::thread::sleep(SETTLE);
    let log = cold_read(&mut daemon);

    let failed_at = log
        .iter()
        .position(|e| {
            e.subject.as_str() == "gate.failed" && e.payload()["gate"].as_str() == Some("pre_merge")
        })
        .expect("the pre_merge failure is on the persisted log");
    let released_at = log
        .iter()
        .position(|e| {
            e.subject.as_str() == "worktree.released" && e.payload()["path"].as_str() == Some(&path)
        })
        .unwrap_or_else(|| {
            panic!(
                "the operator's release must land as `worktree.released` on the log — the act \
                 is a fact or it did not happen (I3). Subjects: {:?}",
                subjects(&log)
            )
        });
    assert!(
        released_at > failed_at,
        "the explicit release (index {released_at}) follows the failure it triages (index \
         {failed_at})"
    );
    assert!(
        !Path::new(&path).exists(),
        "after the explicit release the tree is gone from disk: {path}"
    );
    let entries = registry_entries(&repo);
    assert!(
        registry_claim(&entries, &path).is_none(),
        "after the explicit release the registry claim is CLOSED; registry holds: {entries:#?}"
    );

    let entry = folded_entry(&log, &path);
    assert_eq!(
        entry["lifecycle"],
        json!("released"),
        "the explicit release folds `lifecycle = released`: {entry:#}"
    );
    assert_eq!(
        entry["outcome"],
        json!("failed"),
        "DR-049 criterion (c), the preservation leg: `worktree.released` sets lifecycle ONLY \
         (§Decision 2) — the `failed` outcome the run earned must SURVIVE its tree's release, \
         or triage history is clobbered the way merges were about to be. Entry: {entry:#}"
    );
}
