//! DR-059 ORACLE — the WATCHER emitter's half of the `patch?: CasRef` sibling
//! ref (`spec/ontology.md` `diff.ready` v1, `patch?` bullet; DR-059 §Decision
//! 1/4/5), judged behaviorally against the real notify watcher.
//!
//! The finding this board serves: a real end-to-end run proved `cas_read`
//! returns a file-status list, not a diff — the Review panel renders
//! filenames and cannot answer "is this change correct". The ontology now
//! rules a SECOND CAS blob of real `git diff` unified-format bytes, emitted
//! as `patch` on `diff.ready`, mime `text/x-diff` — while the existing
//! summary ref keeps its bytes and finally gets an honest mime,
//! `text/x-diff-summary`.
//!
//! EMITTER SYMMETRY (criterion 1): the ontology rules BOTH emitters gain the
//! field TOGETHER. This board is the watcher's judge; the gate-time pin's
//! judge is `bins/rezidentd/tests/dr059_patch_e2e.rs` (unix) with the
//! host-side backstop `dr059_patch_structure.rs`. An implementer landing only
//! one site leaves the other board red — that is the symmetry judge, split
//! across the two emitters' own harnesses exactly like the subject's own
//! two-emitter ownership pins are.
//!
//! RED MODE (verified against the tree at cut time): ASSERT-RED, every test.
//! `summarize_to_cas` (`crates/rezidnt-adapters/git/src/lib.rs`) pins ONE
//! blob under the FALSE mime `text/x-diff` and the emitted payload carries no
//! `"patch"` key (the quoted literal appears nowhere in the adapter source —
//! grep-verified this session). The patch tests die on the missing payload
//! key; the mime test dies asserting `text/x-diff-summary` against the old
//! literal. Green requires the watcher to pin real diff bytes as a second
//! blob AND correct the summary mime — the two changes DR-059 §Decision 1/4
//! rule as one landing.

mod util;

use std::time::Duration;

use rezidnt_adapter_git::{GitAdapter, RepoSubstrate, WorktreeReq};
use rezidnt_cas::Cas;
use rezidnt_types::refs::CasRef;

const OUTER: Duration = Duration::from_secs(5);

fn branch_req(name: &str, branch: &str) -> WorktreeReq {
    WorktreeReq {
        name: name.to_string(),
        branch: Some(branch.to_string()),
        detach: false,
        ..WorktreeReq::default()
    }
}

fn ref_field(ev: &rezidnt_types::Event, field: &str) -> CasRef {
    let value = &ev.payload()[field];
    assert!(
        value.is_object(),
        "diff.ready payload must carry `{field}` as a CasRef object — got: {:#}",
        ev.payload()
    );
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("`{field}` must parse as a full CasRef ({e}): {value:#}"))
}

/// THE PATCH REF (criterion 1, watcher side; ontology `diff.ready.patch?`) —
/// a write to a tracked file yields a `diff.ready` carrying `patch`: a SECOND
/// CAS blob of real `git diff` unified-format bytes under mime `text/x-diff`,
/// alongside (never replacing) the summary ref. The blob must be an actual
/// diff: header line, context, and the changed line marked with `+` — the
/// exact things the e2e finding proved absent.
#[tokio::test]
async fn a_tracked_change_yields_a_real_patch_ref_alongside_the_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-patch", "feat/patch"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    // Modify the TRACKED file the fixture repo commits, so `git diff` has an
    // unstaged modification to render.
    std::fs::write(
        wt.path.join("README.md"),
        "# oracle fixture repo\nreviewable-change-under-test\n",
    )
    .unwrap();

    let ev = util::recv_subject(&mut rx, "diff.ready", OUTER).await;

    let summary = ref_field(&ev, "diff");
    let patch = ref_field(&ev, "patch");

    assert_eq!(
        patch.mime, "text/x-diff",
        "the patch ref's mime is `text/x-diff` — the label finally attached \
         to bytes that are actually diff-formatted (DR-059 §Decision 5)"
    );
    assert!(patch.bytes > 0, "a real change produces a non-empty patch");
    assert_ne!(
        patch.hash, summary.hash,
        "the patch is a SECOND blob, not the summary re-labeled — a summary \
         under a diff mime is exactly the lie DR-059 exists to end"
    );

    let blob = Cas::open(&cas_root).unwrap().get(&patch).unwrap();
    let text = String::from_utf8_lossy(&blob);
    assert!(
        text.contains("diff --git"),
        "the patch blob is unified `git diff` output (header line) — got:\n{text}"
    );
    assert!(
        text.contains("+reviewable-change-under-test"),
        "the ADDED line rides the patch with its `+` marker — the reviewable \
         content the summary can never carry — got:\n{text}"
    );
    assert!(
        text.contains("README.md"),
        "the patch names the changed file — got:\n{text}"
    );
}

/// AN ADDED FILE IS REVIEWABLE TOO — the ontology's `patch?` bullet rules the
/// patch carries "the change the `diff` summary describes", and the summary
/// describes untracked additions (`git status --untracked-files=all` feeds
/// it). A patch that silently omits a brand-new file re-opens the finding for
/// creations: the summary says `A <file>` and Review still shows nothing.
/// Plain `git diff` does NOT show untracked files — covering them is the
/// emitter's problem, and this judge is what makes skipping it red.
#[tokio::test]
async fn an_added_files_content_is_reviewable_in_the_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-added", "feat/added"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    std::fs::write(wt.path.join("created_by_agent.rs"), "fn created() {}\n").unwrap();

    let ev = util::recv_subject(&mut rx, "diff.ready", OUTER).await;
    let patch = ref_field(&ev, "patch");
    let blob = Cas::open(&cas_root).unwrap().get(&patch).unwrap();
    let text = String::from_utf8_lossy(&blob);
    assert!(
        text.contains("created_by_agent.rs"),
        "the patch names the added file — got:\n{text}"
    );
    assert!(
        text.contains("+fn created() {}"),
        "the added file's CONTENT rides the patch as `+` lines; an emitter \
         that shells out to bare `git diff` drops untracked files and leaves \
         Review blind for every file an agent creates — got:\n{text}"
    );
}

/// THE MIME FINALLY TELLS THE TRUTH, AND THE SUMMARY IS OTHERWISE UNTOUCHED
/// (criterion 3, watcher side; DR-059 §Decision 4) — the summary ref's mime
/// is corrected to `text/x-diff-summary` while its BYTES stay exactly what
/// `summary::diff_summary_text` renders today: the `# rezidnt diff summary
/// v1` header and status lines, no unified-diff markup. `DiffScope` and
/// `ForbiddenPath` never read the mime (`resolve_ref` ignores it), so this
/// correction moves no verifier — the content-unchanged leg here is what pins
/// that nothing widened the summary in place (the rejected alternative).
#[tokio::test]
async fn the_summary_mime_is_corrected_and_its_content_is_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cas_root = tmp.path().join("cas");
    util::init_committed_repo(&repo);

    let adapter = GitAdapter::open(&repo, &cas_root).await.unwrap();
    let mut rx = adapter.subscribe();
    let wt = adapter
        .alloc_worktree(branch_req("feat-mime", "feat/mime"))
        .await
        .unwrap();
    util::recv_subject(&mut rx, "worktree.allocated", OUTER).await;

    std::fs::write(wt.path.join("README.md"), "# oracle fixture repo\nedited\n").unwrap();
    let ev = util::recv_subject(&mut rx, "diff.ready", OUTER).await;

    let summary = ref_field(&ev, "diff");
    assert_eq!(
        summary.mime, "text/x-diff-summary",
        "the summary's mime is the corrected `text/x-diff-summary` — the old \
         `text/x-diff` label was FALSE and load-bearing (cas_read echoes the \
         stored mime back as the caller's claim; DR-059 §Decision 4)"
    );

    let blob = Cas::open(&cas_root).unwrap().get(&summary).unwrap();
    let text = String::from_utf8_lossy(&blob);
    assert!(
        text.starts_with("# rezidnt diff summary v1\n"),
        "the summary bytes are UNCHANGED — same header, same format the S2 \
         fixtures pin; only the label moved — got:\n{text}"
    );
    assert!(
        text.contains("README.md"),
        "the summary still names the changed file — got:\n{text}"
    );
    assert!(
        !text.contains("diff --git") && !text.contains("@@"),
        "no unified-diff markup leaks into the summary blob — widening the \
         summary in place is the alternative DR-059 REJECTED because \
         DiffScope/ForbiddenPath parse its line shape — got:\n{text}"
    );
}
