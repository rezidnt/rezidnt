//! Diff-summary read path (doc §7: reads via gix; the CLI is for mutations).
//!
//! The summary is a deterministic, sorted, text rendering of the worktree's
//! changed paths relative to HEAD/index — one line per change, each carrying
//! a blake3 content hash of the file's current bytes (when it still exists on
//! disk) so the summary is *content-stable*: the same tree state always
//! renders to the same bytes (and therefore the same CAS ref), and a content
//! change always renders differently (I6-adjacent — this ref is future
//! gate-verifier input).
//!
//! All functions here are blocking (gix walks the filesystem); callers run
//! them inside `spawn_blocking` (rust-conventions: no blocking in async).

use std::path::Path;

use crate::GitError;

/// Render the deterministic diff summary text for `worktree`.
pub(crate) fn diff_summary_text(worktree: &Path) -> Result<String, GitError> {
    let repo = gix::open(worktree)
        .map_err(|e| GitError::Git(format!("gix open {}: {e}", worktree.display())))?;
    let iter = repo
        .status(gix::progress::Discard)
        .map_err(|e| GitError::Git(format!("gix status: {e}")))?
        // Per-file listing: a collapsed directory would hide the changed
        // filenames the summary exists to name.
        .untracked_files(gix::status::UntrackedFiles::Files)
        .into_iter(None::<gix::bstr::BString>)
        .map_err(|e| GitError::Git(format!("gix status iter: {e}")))?;

    let mut lines = Vec::new();
    for item in iter {
        let item = item.map_err(|e| GitError::Git(format!("gix status item: {e}")))?;
        if let Some(line) = render_item(worktree, &item) {
            lines.push(line);
        }
    }
    // gix yields items in a parallelism-dependent order; sorting (plus dedup
    // for paths reported by both comparisons) makes the rendering
    // deterministic for identical tree states.
    lines.sort();
    lines.dedup();

    let mut text = String::from("# rezidnt diff summary v1\n");
    for line in &lines {
        text.push_str(line);
        text.push('\n');
    }
    Ok(text)
}

/// One summary line: `<status-letter> <repo-relative-path> [blake3:<hex>]`.
/// `None` for items that are bookkeeping rather than changes (e.g. an index
/// entry that merely needs a stat refresh).
fn render_item(worktree: &Path, item: &gix::status::Item) -> Option<String> {
    use gix::status::index_worktree::iter::Summary;

    let (letter, rela_path) = match item {
        gix::status::Item::IndexWorktree(item) => {
            let letter = match item.summary()? {
                Summary::Removed => 'D',
                Summary::Added | Summary::IntentToAdd => 'A',
                Summary::Modified => 'M',
                Summary::TypeChange => 'T',
                Summary::Renamed => 'R',
                Summary::Copied => 'C',
                Summary::Conflict => 'U',
            };
            (letter, item.rela_path().to_string())
        }
        gix::status::Item::TreeIndex(change) => {
            use gix::diff::index::ChangeRef;
            let letter = match change {
                ChangeRef::Addition { .. } => 'A',
                ChangeRef::Deletion { .. } => 'D',
                ChangeRef::Modification { .. } => 'M',
                ChangeRef::Rewrite { .. } => 'R',
            };
            (letter, change.location().to_string())
        }
    };

    let mut line = format!("{letter} {rela_path}");
    // Content hash of the file as it exists NOW, so identical change-sets
    // with different contents never collide onto one ref. Missing files
    // (deletions) simply carry no hash.
    let on_disk = worktree.join(&rela_path);
    if let Ok(bytes) = std::fs::read(&on_disk) {
        line.push_str(" blake3:");
        line.push_str(&blake3::hash(&bytes).to_hex());
    }
    Some(line)
}

/// Branch checked out in `worktree`, when the head is a symbolic ref
/// (detached trees yield `None`). Blocking; callers use `spawn_blocking`.
pub(crate) fn read_branch(worktree: &Path) -> Option<String> {
    let repo = gix::open(worktree).ok()?;
    let name = repo.head_name().ok()??;
    Some(name.shorten().to_string())
}
