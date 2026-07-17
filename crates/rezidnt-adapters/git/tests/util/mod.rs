//! Shared helpers for the S2 git-adapter oracle tests. Each integration-test
//! crate uses a subset, hence the module-wide dead_code allow.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rezidnt_types::Event;
use tokio::sync::broadcast;

/// Run `git` in `dir`, panic on failure, return trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = git_status(dir, args);
    assert!(ok, "git {args:?} in {} failed: {err}", dir.display());
    out
}

/// Run `git` in `dir`, return (success, stdout, stderr) — for assertions that
/// EXPECT failure (e.g. detached HEAD has no symbolic ref).
pub fn git_status(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git CLI must be runnable in the test environment");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    )
}

/// Materialize a real repo with one commit at `dir` — the "committed repo"
/// the S2 triage requires for the worktree lifecycle tests.
pub fn init_committed_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "user.email", "oracle@rezidnt.test"]);
    git(dir, &["config", "user.name", "rezidnt oracle"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("README.md"), "# oracle fixture repo\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "initial commit"]);
}

/// Canonicalize, panicking with the path on failure. Comparisons canonicalize
/// BOTH sides so the assertion pins path identity, not platform spelling
/// (Windows `\\?\` prefixes must not fail an otherwise-correct adapter).
pub fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|e| panic!("canonicalize {}: {e}", p.display()))
}

/// Receive events until one matches `subject` (others are passed over), or
/// panic when `deadline` elapses. The deadline is the OUTER tolerance; timing
/// criteria are asserted by the caller on wall-clock elapsed, not here.
pub async fn recv_subject(
    rx: &mut broadcast::Receiver<Event>,
    subject: &str,
    deadline: Duration,
) -> Event {
    let deadline = tokio::time::Instant::now() + deadline;
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("no `{subject}` event before the outer deadline"))
            .expect("adapter event channel must stay open (test subscriber must not lag)");
        if ev.subject.as_str() == subject {
            return ev;
        }
    }
}

/// Collect every event delivered within `window` (then stop). Used for
/// exactly-once and quiescence assertions.
pub async fn drain_for(rx: &mut broadcast::Receiver<Event>, window: Duration) -> Vec<Event> {
    let deadline = tokio::time::Instant::now() + window;
    let mut events = Vec::new();
    while let Ok(res) = tokio::time::timeout_at(deadline, rx.recv()).await {
        events.push(
            res.expect("adapter event channel must stay open (test subscriber must not lag)"),
        );
    }
    events
}

/// Count events in a slice bearing `subject`.
pub fn count_subject(events: &[Event], subject: &str) -> usize {
    events
        .iter()
        .filter(|e| e.subject.as_str() == subject)
        .count()
}

/// Parse the JSONL worktree registry and return the entries (one JSON value
/// per line). Panics if the file is missing or a line is not valid JSON.
pub fn registry_entries(repo_root: &Path) -> Vec<serde_json::Value> {
    let path = repo_root.join(rezidnt_adapter_git::REGISTRY_PATH);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("registry file {} must exist: {e}", path.display()));
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("registry line must be JSON ({e}): {l}"))
        })
        .collect()
}

/// Registry entries whose `path` field canonicalizes to the same file as
/// `target` (the registry keys canonicalized paths — DR-001 BINDING rule).
pub fn registry_entries_for(repo_root: &Path, target: &Path) -> Vec<serde_json::Value> {
    let want = canon(target);
    registry_entries(repo_root)
        .into_iter()
        .filter(|e| {
            e["path"]
                .as_str()
                .is_some_and(|p| std::fs::canonicalize(p).is_ok_and(|c| c == want))
        })
        .collect()
}
