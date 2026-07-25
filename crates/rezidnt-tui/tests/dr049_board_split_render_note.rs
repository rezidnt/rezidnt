//! DR-049 §Decision 2 RE-BLESS NOTE — why the two S5 render goldens changed in
//! the lifecycle/outcome-split slice, and the guard that keeps the re-bless
//! honest.
//!
//! ## Why a note file exists at all
//!
//! Re-blessing is the one operation in this suite that can silently launder a
//! render regression. `assert_or_bless_golden`
//! (`crates/rezidnt-tui/tests/board_render_golden.rs`) overwrites the committed
//! snapshot with whatever the render currently produces when
//! `REZIDNT_BLESS_GOLDEN=1` is set, so a byte snapshot re-blessed for one
//! reason absorbs every OTHER change to the render in the same stroke, and the
//! diff that lands looks exactly like an intended one. The mitigation the house
//! uses is not a mechanism but a record: the reason is written down next to the
//! goldens, and a check pins the specific content the re-bless was FOR.
//!
//! ## The re-bless, stated
//!
//! `spec/fixtures/s5_board_render.golden.txt` and
//! `spec/fixtures/s5b_board_permit_render.golden.txt` were re-blessed with
//! `REZIDNT_BLESS_GOLDEN=1` against the real render — never hand-edited — for
//! ONE change: the worktrees table moved off the single collapsed `status`
//! column onto DR-049's split pair. Concretely, in `crates/rezidnt-tui/src/lib.rs`:
//!
//! - the header row became `path, lifecycle, outcome, branch, last diff`,
//!   replacing `path, status, branch, last diff`, with the column widths
//!   re-apportioned to fit;
//! - an ABSENT `outcome` renders `-`. The board never invents an outcome the
//!   fold does not hold (I3), so a tree that is merely allocated shows
//!   `allocated` / `-` rather than a guess.
//!
//! Nothing else about the render moved in that slice: no panel was added,
//! removed, retitled or resized, and the runs / fleet / permit sections are
//! byte-identical across the re-bless.
//!
//! ## Why one column could not stay
//!
//! `lifecycle` (allocated -> released) and `outcome` (merged | failed |
//! abandoned | absent) are independent axes. A single column can show only one
//! of them, so on a merged-then-released tree it must drop either the merge or
//! the release — the derived-state clobber DR-049 §Decision 2 dissolves and
//! that DR-047 §Decision 5 declined to trade one way or the other.
//!
//! ## What this file asserts
//!
//! The committed goldens, read as TEXT, still carry the split the re-bless was
//! performed for. This is deliberately NOT a second byte snapshot (that is
//! `board_render_golden.rs`'s job, and duplicating it would just double the
//! re-bless surface). It is the narrow guard that a FUTURE re-bless cannot
//! quietly carry the worktrees table back to a collapsed column while the byte
//! snapshot keeps passing — because a byte snapshot always passes immediately
//! after a bless, whatever it captured.

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

/// The two goldens `board_render_golden.rs` snapshots, both re-blessed by the
/// DR-049 split slice.
const GOLDENS: [&str; 2] = [
    "s5_board_render.golden.txt",
    "s5b_board_permit_render.golden.txt",
];

fn golden_text(name: &str) -> String {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden {} must exist: {e}", path.display()))
}

/// The worktrees header row of a golden: the one line carrying the `path`
/// column label under the `worktrees` panel border.
fn worktrees_header(text: &str, name: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let panel = lines
        .iter()
        .position(|l| l.contains("worktrees ("))
        .unwrap_or_else(|| {
            panic!("golden {name} must render a titled `worktrees` panel; got:\n{text}")
        });
    lines
        .get(panel + 1)
        .unwrap_or_else(|| panic!("golden {name} ends before the worktrees header row"))
        .to_string()
}

/// The re-bless GUARD — both goldens carry DR-049's split column pair in the
/// worktrees header.
///
/// Non-vacuity: the header row is located structurally (the line after the
/// `worktrees` panel title) rather than by searching the whole file, so a
/// `lifecycle`/`outcome` occurrence anywhere else in the buffer — a subject
/// histogram entry, a run row — cannot satisfy this.
#[test]
fn both_reblessed_goldens_carry_the_split_worktrees_columns() {
    for name in GOLDENS {
        let text = golden_text(name);
        let header = worktrees_header(&text, name);
        for column in ["path", "lifecycle", "outcome", "branch", "last diff"] {
            assert!(
                header.contains(column),
                "golden {name} lost the `{column}` worktrees column. These goldens were \
                 re-blessed for exactly one reason — DR-049 §Decision 2 splitting the collapsed \
                 `status` column into `lifecycle` + `outcome` — and a re-bless that drops one of \
                 them has carried the render back to a single axis, where a merged-then-released \
                 tree can only report one of the two. Header row was:\n{header}"
            );
        }
    }
}

/// And the collapsed column is GONE — the half of the change a
/// "contains the new labels" check alone would miss, since a render that added
/// the split pair while KEEPING `status` would satisfy it.
///
/// Scoped to the worktrees header on purpose: the RUNS table legitimately keeps
/// a `status` column (a run's status is one axis), and
/// `board_render_golden.rs:235` asserts it. Only the worktree row was split.
#[test]
fn the_collapsed_status_column_is_gone_from_the_worktrees_header() {
    for name in GOLDENS {
        let text = golden_text(name);
        let header = worktrees_header(&text, name);
        assert!(
            !header.contains("status"),
            "golden {name} still renders a `status` column in the WORKTREES header. DR-049 \
             §Decision 2 replaced it — a board that shows both the split pair and the collapsed \
             field is serving two answers to one question, and the collapsed one is the field \
             the fold no longer holds. Header row was:\n{header}"
        );
    }
}
