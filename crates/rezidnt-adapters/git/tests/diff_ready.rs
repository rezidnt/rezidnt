//! S2 oracle — exit criterion 1: `diff.ready` within 1 s of a write
//! (post-debounce; the ontology fixes the debounce at 250 ms, emitter = git
//! adapter notify watcher). Plus the I2 contract: the diff summary is a CAS
//! ref, never inline diff bytes.
//!
//! Timing discipline: the 1 s bound IS the slice criterion, so it is asserted
//! directly as wall time from the last write to fact receipt. The OUTER
//! tolerance (test hang guard) is generous; the criterion assertion is not.
//! With a 250 ms debounce the bound leaves 750 ms of real margin — a miss is
//! an adapter defect, not CI weather.
//!
//! Emitter note (disclosed 2026-07-24, registry-convergence remediation): this
//! board judges the WATCHER's `diff.ready` — `source` = `SOURCE_ID` — which is
//! one of TWO emitters of the subject. The daemon mints a second, deterministic
//! one at `pre_merge` (`bins/rezidentd/src/runs.rs`, `run_pre_merge`); the
//! split and its ownership guard are documented in the adapter's module header.
//! Nothing on this board covers that emitter.
//!
//! Payload-shape note (CORRECTED 2026-07-24; the same stale-caveat class the
//! warden ruled on for `worktree_conflict.rs`): `diff.ready` v1 IS ratified —
//! `{worktree: string, diff: CasRef}`, S2 set, 2026-07-17. What these tests pin
//! — the worktree it concerns (`worktree`) and the summary ref (`diff: CasRef`)
//! — is that baseline, not a guess at it; no /subject is owed for it.

mod util;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rezidnt_adapter_git::{FactSink, GitAdapter, RepoSubstrate, WorktreeReq};
use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;

const OUTER: Duration = Duration::from_secs(5);
const BOUND: Duration = Duration::from_secs(1);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
        ..WorktreeReq::default()
    }
}

fn payload_diff_ref(ev: &rezidnt_types::Event) -> CasRef {
    serde_json::from_value(ev.payload()["diff"].clone())
        .expect("diff.ready payload carries the summary as `diff: CasRef` (I2)")
}

/// THE criterion: write → `diff.ready` in ≤ 1 s, carrying a resolvable CAS
/// ref whose blob is a diff summary naming the changed file.
#[tokio::test]
async fn diff_ready_within_one_second_of_write() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-diff", "feat/diff"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    std::fs::write(wt.path.join("oracle_change.txt"), "the write under test\n").unwrap();
    let written_at = Instant::now();

    let ev = util::recv_subject(&mut rx, "diff.ready", OUTER).await;
    let elapsed = written_at.elapsed();
    assert!(
        elapsed <= BOUND,
        "S2 exit criterion: diff.ready within 1 s of write (250 ms debounce leaves 750 ms margin) — took {elapsed:?}"
    );

    assert_eq!(ev.v, 1, "taxonomy v0 mints diff.ready at v = 1");
    assert_eq!(ev.source.as_str(), rezidnt_adapter_git::SOURCE_ID);
    let for_wt = PathBuf::from(
        ev.payload()["worktree"]
            .as_str()
            .expect("diff.ready payload names the `worktree` it concerns"),
    );
    assert_eq!(util::canon(&for_wt), util::canon(&wt.path));

    // I2: the summary is a ref, and the ref resolves.
    let r = payload_diff_ref(&ev);
    assert_eq!(r.hash.len(), 64, "blake3 hex, 64 lowercase chars");
    assert!(
        r.hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert!(r.bytes > 0, "an actual change produces a non-empty summary");
    let blob = Cas::open(&cas_root).unwrap().get(&r).unwrap();
    let text = String::from_utf8_lossy(&blob);
    assert!(
        text.contains("oracle_change.txt"),
        "the diff summary names the changed file — got:\n{text}"
    );
}

/// Post-debounce semantics: a burst of writes inside one 250 ms debounce
/// window coalesces into EXACTLY ONE `diff.ready`, followed by quiescence.
#[tokio::test]
async fn write_burst_coalesces_into_one_diff_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-burst", "feat/burst"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    // Five writes spanning ~80 ms — all inside a single 250 ms debounce window.
    for i in 0..5u8 {
        std::fs::write(
            wt.path.join(format!("burst_{i}.txt")),
            format!("write {i}\n"),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The whole criterion window from the LAST write: one coalesced fact.
    let events = util::drain_for(&mut rx, BOUND).await;
    assert_eq!(
        util::count_subject(&events, "diff.ready"),
        1,
        "a debounced burst yields exactly one diff.ready within the 1 s bound — got {events:#?}"
    );

    // Quiescence: nothing trailing once the tree is quiet.
    let more = util::drain_for(&mut rx, Duration::from_millis(600)).await;
    assert_eq!(
        util::count_subject(&more, "diff.ready"),
        0,
        "no trailing diff.ready after the coalesced emission"
    );
}

/// The RepoSubstrate read path (doc §7): `diff_summary` returns a CAS ref and
/// is deterministic over an unchanged tree — same state, same hash (I6-adjacent;
/// this ref is future gate-verifier input, so it must be content-stable).
#[tokio::test]
async fn diff_summary_is_deterministic_cas_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let wt = adapter
        .alloc_worktree(branch_req("feat-sum", "feat/sum"))
        .await
        .unwrap();
    std::fs::write(wt.path.join("summed.txt"), "content under summary\n").unwrap();

    let first = adapter.diff_summary(&wt.id).await.unwrap();
    let second = adapter.diff_summary(&wt.id).await.unwrap();
    assert_eq!(
        first.hash, second.hash,
        "unchanged tree state must summarize to the identical CAS ref (deterministic reads)"
    );
    let blob = Cas::open(&cas_root).unwrap().get(&first).unwrap();
    assert!(
        String::from_utf8_lossy(&blob).contains("summed.txt"),
        "the summary names the changed file"
    );
}

/// A sink that refuses the FIRST `diff.ready` and accepts everything after,
/// recording every fact it was offered. Stands in for a fabric append that
/// failed once — the transient case, the one where recovery is meaningful.
#[derive(Default)]
struct FlakySink {
    offered: Mutex<Vec<rezidnt_types::Event>>,
}

impl FlakySink {
    /// Hashes of the `diff.ready` facts offered, in order, paired with whether
    /// this sink accepted them.
    fn diff_ready(&self) -> Vec<(String, bool)> {
        self.offered
            .lock()
            .expect("sink lock")
            .iter()
            .filter(|e| e.subject.as_str() == "diff.ready")
            .enumerate()
            .map(|(i, e)| (payload_diff_ref(e).hash, i > 0))
            .collect()
    }
}

impl FactSink for FlakySink {
    fn emit(&self, event: &rezidnt_types::Event) -> Result<(), rezidnt_adapter_git::GitError> {
        let mut offered = self.offered.lock().expect("sink lock");
        let refuse = event.subject.as_str() == "diff.ready"
            && !offered.iter().any(|e| e.subject.as_str() == "diff.ready");
        offered.push(event.clone());
        if refuse {
            return Err(rezidnt_adapter_git::GitError::Registry(
                "sink refused the first diff.ready".into(),
            ));
        }
        Ok(())
    }
}

/// A `diff.ready` whose append is REFUSED is re-emitted at the next change —
/// it is not suppressed as a duplicate of a fact the log never received.
///
/// The regression this pins is an ordering one. `debounce_loop` advanced its
/// suppression hash BEFORE the emit, so a refused append lost that summary
/// permanently: the next identical summary matched the remembered hash and was
/// dropped as "an unchanged tree carries no new information" — true of the tree,
/// false of the log, which had never heard of it. Unlike an allocation there is
/// no caller to fail here (the debounce loop is detached), so re-emission at the
/// next change is the ENTIRE recovery, and the module header says so rather than
/// letting the header's "a sink refusal fails the operation" stand as a claim
/// covering a path where no operation exists.
///
/// The second write is byte-IDENTICAL to the first, deliberately: that is the
/// only shape in which the two summaries collide and the suppression path is
/// reached at all. A differing second write would pass under the defect too.
#[tokio::test]
async fn a_refused_diff_ready_is_retried_not_suppressed_as_a_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    util::init_committed_repo(&repo);

    let sink = Arc::new(FlakySink::default());
    let adapter = GitAdapter::open(&repo, &tmp.path().join("cas"))
        .await
        .unwrap()
        .with_sink(Arc::clone(&sink) as Arc<dyn FactSink>);
    let wt = adapter
        .alloc_worktree(branch_req("feat-refused", "feat/refused"))
        .await
        .unwrap();

    let file = wt.path.join("refused_change.txt");
    const CONTENT: &str = "one summary, offered twice\n";
    std::fs::write(&file, CONTENT).unwrap();

    // The refused attempt. Polled rather than slept on: the debounce is 250 ms
    // and the outer tolerance is the hang guard, not a timing criterion.
    let refused = await_diff_ready_count(&sink, 1, OUTER).await;
    assert_eq!(
        refused.len(),
        1,
        "the watcher offered the summary to the sink once and the sink refused it"
    );
    assert!(!refused[0].1, "precondition: that first offer was REFUSED");

    // The same bytes again: a filesystem event whose summary is IDENTICAL to
    // the one the log never received.
    std::fs::write(&file, CONTENT).unwrap();

    let offers = await_diff_ready_count(&sink, 2, OUTER).await;
    assert_eq!(
        offers.len(),
        2,
        "the summary the log refused is offered AGAIN at the next change to the tree. \
         Suppressing it — because the loop remembered a hash it never got onto the log — \
         loses that summary permanently and silently (I3: the log is truth, and a fact that \
         reached no append never happened). Offers: {offers:?}"
    );
    assert_eq!(
        offers[0].0, offers[1].0,
        "and it is the SAME summary, not a different one that happened to arrive: the \
         suppression path is only reached when the hashes collide"
    );
    assert!(offers[1].1, "the retry was accepted, so the log now has it");
}

/// Poll `sink` until it has been offered `want` `diff.ready` facts, or panic at
/// `deadline`. Polling (not sleeping) keeps the wait proportional to the 250 ms
/// debounce without turning a hang guard into a timing assertion.
async fn await_diff_ready_count(
    sink: &FlakySink,
    want: usize,
    deadline: Duration,
) -> Vec<(String, bool)> {
    let started = Instant::now();
    loop {
        let seen = sink.diff_ready();
        if seen.len() >= want {
            return seen;
        }
        assert!(
            started.elapsed() < deadline,
            "waited {deadline:?} for {want} `diff.ready` offer(s); saw {seen:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
