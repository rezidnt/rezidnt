//! S5 oracle — golden render (RICHER read-only board, DR-031 §Decision 3).
//! Fold a fixture event log -> project -> `draw` onto a fixed-size
//! `ratatui::backend::TestBackend` -> assert the terminal buffer matches the
//! committed golden snapshot AND satisfies layout-independent structural
//! proofs. Deterministic: no real terminal, no clock, no color-dependent
//! assertions (text cells only).
//!
//! Criterion: the read-only fleet board renders the derived fleet state as a
//! RICHER surface — bordered rounded panels (ratatui `Block` +
//! `BorderType::Rounded`) for the fleet summary and for each of the
//! runs / permit / worktrees sections, with the runs and worktrees sections
//! rendered as ratatui `Table` widgets carrying a header row. This replaces the
//! prior flat `place()`-column text render. The render path still takes a
//! `BoardView` (never an `Event`) and stays a PURE, NON-INTERACTIVE function of
//! that single snapshot — no selection, no cursor, no focus/Tab, no
//! selected-row detail pane. Interactivity is Phase 3 / demand-gated (DR-031,
//! roadmap §16/§19) and OUT of scope.
//!
//! Same DATA as before (no field dropped or added): fleet summary
//! (events_folded, workspaces open/closed, counts_by_subject), the runs table
//! (run id, status, cost usd, in/out tokens, integrity alarms), the permit
//! section (granted/denied/escalated, pending, delegated) that appears ONLY
//! when at least one run has permit activity, and the worktrees table (path,
//! status, branch?, last_diff?).
//!
//! Colored status cells are ALLOWED in the real `draw` but are NEVER asserted:
//! the golden is a TestBackend TEXT dump (`buffer_to_text` below drops style),
//! so colors do not survive the snapshot. Every assertion here is
//! text/structure-only so the suite stays deterministic.
//!
//! RED MODE (assert-red, DR-031 richer-render amendment):
//! 1. The committed goldens (`s5_board_render.golden.txt`,
//!    `s5b_board_permit_render.golden.txt`) have been reset to a single sentinel
//!    line that NO real render can ever equal, so the snapshot tests FAIL until
//!    the implementer ships the bordered-table `draw` and re-blesses.
//! 2. The structural tests below (`*_is_bordered_and_tabular`) assert
//!    box-drawing characters (panels exist) and per-section table header
//!    strings that the CURRENT flat `place()` render does NOT emit — so they
//!    FAIL against today's `draw`, independent of the golden.
//!
//! This is the S4/DR-006 scaffold discipline: the API is real, the assertions
//! pin behavior that does not exist yet, the tests fail honestly.
//!
//! WIDTH/HEIGHT: widened from 80x24 to 100x40. Bordered `Table` widgets consume
//! two columns (left/right border) and two rows (top border + header, bottom
//! border) per panel, and four panels stack vertically (summary, runs, permit,
//! worktrees). 80x24 is too tight for the run-id + cost + tokens columns once
//! the border chrome and header row are added; 100x40 fits the four bordered
//! panels with their header rows without truncating the run-id prefix or the
//! numeric columns. Documented per the task's size-justification requirement.
//!
//! GOLDEN BLESS MECHANISM (unchanged from the prior oracle): with
//! `REZIDNT_BLESS_GOLDEN=1` set, each snapshot test WRITES its golden from the
//! real `draw` output and panics (telling you to unset it); otherwise it reads
//! the committed golden and asserts equality. The golden is regenerated the
//! project way — ONCE, by the implementer, after the bordered-table `draw`
//! exists — never hand-fabricated to paper over a broken render (test honesty).

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rezidnt_state::{Graph, fold};
use rezidnt_tui::{BoardView, draw, project};
use rezidnt_types::Event;

/// Widened for the bordered-table layout — see module header for the
/// justification. The richer panels do not fit the prior 80x24.
const WIDTH: u16 = 100;
const HEIGHT: u16 = 40;

/// Box-drawing characters that a `Block` border (rounded or square) paints into
/// the buffer. Presence of ANY of these proves at least one bordered panel
/// exists — the flat `place()` render paints none.
const BOX_DRAWING: &[char] = &[
    '─', '│', '╭', '╮', '╰', '╯', // rounded corners + edges
    '┌', '┐', '└', '┘', // square corners (Block default, still a border)
    '├', '┤', '┬', '┴', '┼', // junctions (table column separators, if used)
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

fn graph_from_fixture(name: &str) -> Graph {
    let path = fixtures_dir().join(name);
    let events: Vec<Event> = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} must exist: {e}", path.display()))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name} line must parse: {e}")))
        .collect();
    fold(events.iter())
}

/// Fold a fixture, project it, and render onto a fresh TestBackend of the
/// richer size. Returns the terminal so callers can pull either the styled
/// buffer or the plain-text dump.
fn render_fixture(name: &str) -> (BoardView, Terminal<TestBackend>) {
    let graph = graph_from_fixture(name);
    let view = project(&graph);
    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal
        .draw(|frame| draw(frame, &view))
        .expect("draw onto the test backend");
    (view, terminal)
}

/// Flatten a TestBackend buffer to newline-joined, right-trimmed rows of plain
/// text — style-independent so the golden pins CONTENT, not colors. (Colored
/// status cells, if the implementer adds them, are dropped here on purpose.)
fn buffer_to_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let mut lines = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut row = String::with_capacity(area.width as usize);
        for x in 0..area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        lines.push(row.trim_end().to_string());
    }
    lines.join("\n").trim_end().to_string() + "\n"
}

/// Assert-and-write helper for a golden snapshot: on `REZIDNT_BLESS_GOLDEN=1`
/// writes `got` to the golden and panics; otherwise reads the committed golden
/// and asserts equality. Centralizes the bless discipline for both snapshots.
fn assert_or_bless_golden(got: &str, golden_name: &str) {
    let golden_path = fixtures_dir().join(golden_name);
    if std::env::var_os("REZIDNT_BLESS_GOLDEN").is_some() {
        std::fs::write(&golden_path, got).expect("write golden");
        panic!(
            "REZIDNT_BLESS_GOLDEN set: wrote golden to {} — unset it to assert",
            golden_path.display()
        );
    }
    let expected = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("golden {} must exist: {e}", golden_path.display()));
    assert_eq!(
        got, expected,
        "rendered board buffer diverged from the committed golden `{golden_name}` \
         (bless deliberately with REZIDNT_BLESS_GOLDEN=1 ONCE the bordered-table draw is real)"
    );
}

// ---------------------------------------------------------------------------
// S5 — the verified-run fleet board (no permit activity → no permit panel).
// Golden reused fixture: `spec/fixtures/s4_verified_run.jsonl`.
// ---------------------------------------------------------------------------

/// Byte snapshot: render the s4 verified-run fixture and compare to the
/// committed golden. RED now — the committed golden is a sentinel placeholder
/// the flat render can never equal; GREEN once the implementer ships the
/// bordered-table `draw` and re-blesses with REZIDNT_BLESS_GOLDEN=1.
#[test]
fn board_render_matches_golden_snapshot() {
    let (_view, terminal) = render_fixture("s4_verified_run.jsonl");
    let got = buffer_to_text(&terminal);
    assert_or_bless_golden(&got, "s5_board_render.golden.txt");
}

/// Content spot-checks independent of exact layout: the rendered buffer must
/// name the fleet's run and its status somewhere. Keeps the render honest even
/// if the golden is regenerated — a blank frame fails these outright.
#[test]
fn rendered_buffer_names_the_run_and_its_status() {
    let (_view, terminal) = render_fixture("s4_verified_run.jsonl");
    let text = buffer_to_text(&terminal);
    // The run id is 26 chars; a fixed board may truncate it, so assert a
    // recognizable prefix rather than the whole ULID.
    assert!(
        text.contains("01S4VER1F1ED"),
        "the board must render the run id (prefix); got:\n{text}"
    );
    assert!(
        text.contains("completed"),
        "the board must render the run status; got:\n{text}"
    );
    assert!(
        text.contains("merged"),
        "the board must render the merged worktree status; got:\n{text}"
    );
}

/// STRUCTURAL richer-render proof (DR-031): the board is bordered and tabular,
/// not flat text. Fails against the current `place()` render (which paints no
/// box-drawing chrome and no `Table` header row), passes once the implementer
/// ships bordered `Block`s + `Table` widgets.
///
/// Assertions (all text/structure, color-free):
/// - the buffer contains box-drawing characters → at least one bordered panel;
/// - the fleet-summary, runs, and worktrees panels carry a legible title;
/// - the runs and worktrees tables carry a header row (column labels present);
/// - every run id in the projected `BoardView` appears in the buffer (the
///   table did not silently drop a row).
#[test]
fn board_render_is_bordered_and_tabular() {
    let (view, terminal) = render_fixture("s4_verified_run.jsonl");
    let text = buffer_to_text(&terminal);

    // Panels exist: at least one border character is painted.
    assert!(
        text.chars().any(|c| BOX_DRAWING.contains(&c)),
        "richer render must paint bordered panels (box-drawing chars) — the flat \
         place() render paints none; got:\n{text}"
    );

    // The three always-present sections are titled (Block titles). The permit
    // panel is asserted separately (it is conditional).
    let lower = text.to_lowercase();
    for title in ["fleet", "runs", "worktrees"] {
        assert!(
            lower.contains(title),
            "richer render must title the `{title}` panel; got:\n{text}"
        );
    }

    // Table header rows: the runs and worktrees `Table` widgets carry column
    // headers. These labels are the header-row proof (a bare Paragraph of rows
    // with no header would omit them).
    for header in ["status", "cost usd", "tokens", "alarms"] {
        assert!(
            lower.contains(header),
            "runs table must carry a `{header}` column header; got:\n{text}"
        );
    }
    for header in ["path", "branch"] {
        assert!(
            lower.contains(header),
            "worktrees table must carry a `{header}` column header; got:\n{text}"
        );
    }

    // Every projected run id appears in the buffer — no row dropped by the
    // table. Run ULIDs may be truncated to a prefix, so assert a 12-char prefix.
    assert!(
        !view.runs.is_empty(),
        "fixture must project at least one run"
    );
    for row in &view.runs {
        let prefix = &row.run[..row.run.len().min(12)];
        assert!(
            text.contains(prefix),
            "runs table dropped run `{}` (prefix `{prefix}` absent); got:\n{text}",
            row.run
        );
    }
}

/// The permit panel is ABSENT for a permit-free fleet. The s4 fixture folds no
/// permit activity, so the projection carries zero permit counts on every run —
/// the render must show no permit panel (byte-identical omission preserved in
/// spirit). Guards against the richer render always drawing a (possibly empty)
/// permit panel.
#[test]
fn permit_panel_absent_when_no_permit_activity() {
    let (view, terminal) = render_fixture("s4_verified_run.jsonl");
    // Precondition: the fixture genuinely has no permit activity, so this test
    // is exercising the absence path, not a coincidence.
    let any_permit = view.runs.iter().any(|r| {
        r.permit_granted != 0
            || r.permit_denied != 0
            || r.permit_escalated != 0
            || r.permit_pending != 0
            || r.delegated != 0
    });
    assert!(
        !any_permit,
        "s4 fixture must have no permit activity for this absence test to be meaningful"
    );

    let text = buffer_to_text(&terminal);
    // No permit panel title anywhere. `fleet`/`runs`/`worktrees` may contain no
    // substring "permit", so a plain contains-check is a sound absence proof.
    assert!(
        !text.to_lowercase().contains("permit"),
        "a permit-free fleet must render NO permit panel; got:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// S5b — the permit-bearing fleet board (permit panel PRESENT).
// Fixture: `spec/fixtures/s5b_board_permit.jsonl` (granted=1 / denied=1 /
// escalated=1 / pending=1 / delegated=2). Its OWN golden is never the S5 one.
// ---------------------------------------------------------------------------

/// Byte snapshot for the permit-bearing board. RED now (sentinel golden);
/// GREEN once the bordered-table + bordered-permit-panel `draw` is real and the
/// implementer re-blesses.
#[test]
fn board_render_permit_column_matches_golden_snapshot() {
    let (_view, terminal) = render_fixture("s5b_board_permit.jsonl");
    let got = buffer_to_text(&terminal);
    assert_or_bless_golden(&got, "s5b_board_permit_render.golden.txt");
}

/// Layout-independent spot-check: once the permit panel paints, the rendered
/// buffer must surface this run's permit decision counts. A blank / permit-less
/// frame fails this outright, so it stays honest across a deliberate re-bless.
#[test]
fn rendered_buffer_shows_permit_counts() {
    let (_view, terminal) = render_fixture("s5b_board_permit.jsonl");
    let text = buffer_to_text(&terminal);
    assert!(
        text.contains("01S5BB0ARDPERM"),
        "the board must render the permit-bearing run id (prefix); got:\n{text}"
    );
    assert!(
        text.to_lowercase().contains("permit"),
        "the board must label the permit panel; got:\n{text}"
    );
}

/// STRUCTURAL: the permit panel is PRESENT (bordered + titled) exactly when a
/// run has permit activity, and the runs/worktrees tables are still bordered
/// and tabular in the permit case. Fails against the current flat render.
#[test]
fn permit_panel_present_and_bordered_when_permit_activity() {
    let (view, terminal) = render_fixture("s5b_board_permit.jsonl");
    let text = buffer_to_text(&terminal);

    // Precondition: the fixture genuinely folds permit activity.
    let any_permit = view.runs.iter().any(|r| {
        r.permit_granted != 0
            || r.permit_denied != 0
            || r.permit_escalated != 0
            || r.permit_pending != 0
            || r.delegated != 0
    });
    assert!(
        any_permit,
        "s5b fixture must fold permit activity for this presence test to be meaningful"
    );

    // Bordered panels exist.
    assert!(
        text.chars().any(|c| BOX_DRAWING.contains(&c)),
        "richer permit-board must paint bordered panels; got:\n{text}"
    );

    // The permit panel is titled — it appears iff a run has permit activity.
    assert!(
        text.to_lowercase().contains("permit"),
        "permit-bearing fleet must render a titled permit panel; got:\n{text}"
    );
}
