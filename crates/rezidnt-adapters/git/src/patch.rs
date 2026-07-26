//! Real unified-diff rendering (DR-059 §Decision 1) — the bytes a human can
//! actually review, as distinct from [`crate::summary`]'s path-status listing.
//!
//! Rendered by the `git` CLI rather than gix. Doc §7 puts reads on gix and
//! mutations on the CLI; a unified diff is a read, and this module is the
//! stated exception: `git diff`'s output format IS the artifact — the exact
//! bytes every reviewer, patch tool and editor already understands — so
//! re-deriving it from a blob differ would be re-implementing a wire format,
//! not reading a repository.
//!
//! ## Why a scratch index
//!
//! Bare `git diff` compares the worktree against the index, and an untracked
//! file is in neither: a file an agent CREATED renders as nothing at all,
//! while the summary beside it says the file was added. Making them describe
//! the same change needs the new paths marked intent-to-add, which mutates an
//! index.
//!
//! So this renders against a SCRATCH index — a throwaway file seeded from
//! `HEAD`, named by `GIT_INDEX_FILE` — and the repository's own index is never
//! touched. Two properties fall out of that choice:
//!
//! - the diff is taken against `HEAD`, matching what the summary describes
//!   (staged-but-uncommitted work shows up as the change it is, not as
//!   nothing);
//! - intent-to-add is recorded for untracked paths ONLY. Recording it for
//!   everything (`git add -A -N`) would also stage removals into the scratch
//!   index, which silently drops every DELETED file from the rendered patch.
//!
//! Determinism: `git diff` emits its file sections in sorted path order, so
//! the same tree state renders the same bytes and therefore the same CAS ref.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::Instrument;

use crate::GitError;

/// Distinguishes concurrent scratch indexes inside one process; the pid
/// distinguishes them across processes.
static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

/// How many pathspecs ride one `git add` invocation. Bounded so a worktree
/// with thousands of new files cannot overrun the platform's command-line
/// limit.
const PATHSPEC_BATCH: usize = 100;

/// Render `worktree`'s changes as real `git diff` unified-format bytes,
/// relative to `HEAD`, including files git does not yet track.
///
/// Empty output is a legitimate answer: a tree with no changes has no patch,
/// and zero bytes says so honestly.
pub async fn render_patch(worktree: &Path) -> Result<Vec<u8>, GitError> {
    let span = tracing::info_span!("adapter", kind = "git", op = "render_patch");
    async move {
        let index = scratch_index_path();
        let rendered = render_with_index(worktree, &index).await;
        if let Err(e) = tokio::fs::remove_file(&index).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(error = %e, path = %index.display(), "scratch index not removed");
        }
        rendered
    }
    .instrument(span)
    .await
}

/// A process-and-call-unique path for the scratch index, outside the worktree
/// so writing it wakes no filesystem watcher.
fn scratch_index_path() -> PathBuf {
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rezidnt-patch-{}-{seq}.index", std::process::id()))
}

async fn render_with_index(worktree: &Path, index: &Path) -> Result<Vec<u8>, GitError> {
    // Seed the scratch index from HEAD. A repository with no commit yet has no
    // HEAD to read: the scratch index simply stays empty and every file then
    // renders as an addition, which is exactly what such a tree contains.
    let seeded = git(worktree, index, &["read-tree", "HEAD"]).await?;
    if !seeded.status.success() {
        tracing::debug!(
            worktree = %worktree.display(),
            "no HEAD to seed the scratch index from; rendering against an empty tree"
        );
    }

    // Mark the untracked paths intent-to-add, and ONLY those (see module doc).
    let listed = git(
        worktree,
        index,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    if !listed.status.success() {
        return Err(git_failed(worktree, "ls-files --others", &listed));
    }
    let untracked: Vec<String> = listed
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| String::from_utf8(p.to_vec()).ok())
        .collect();
    for batch in untracked.chunks(PATHSPEC_BATCH) {
        let mut args: Vec<&str> = vec!["add", "-N", "--"];
        args.extend(batch.iter().map(String::as_str));
        let added = git(worktree, index, &args).await?;
        if !added.status.success() {
            return Err(git_failed(worktree, "add -N", &added));
        }
    }

    // The patch itself. Colour and external/textconv drivers are refused so
    // the bytes are the plain, deterministic, machine-readable form.
    let diffed = git(
        worktree,
        index,
        &["diff", "--no-color", "--no-ext-diff", "--no-textconv"],
    )
    .await?;
    if !diffed.status.success() {
        return Err(git_failed(worktree, "diff", &diffed));
    }
    Ok(diffed.stdout)
}

/// Run one `git -C <worktree> <args>` against the scratch index. A non-zero
/// exit is returned to the caller to judge, not treated as an error here.
async fn git(worktree: &Path, index: &Path, args: &[&str]) -> Result<Output, GitError> {
    tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .output()
        .await
        .map_err(|e| GitError::Git(format!("run git {args:?} (is git on PATH?): {e}")))
}

fn git_failed(worktree: &Path, what: &str, out: &Output) -> GitError {
    GitError::Git(format!(
        "git {what} failed in {}: {}",
        worktree.display(),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}
