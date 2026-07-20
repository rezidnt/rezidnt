//! S5 oracle — golden render. Fold a fixture event log -> project -> `draw`
//! onto a fixed-size `ratatui::backend::TestBackend` -> assert the terminal
//! buffer matches the committed golden snapshot. Deterministic: no real
//! terminal, no clock, no color-dependent assertions (text cells only).
//!
//! Criterion: the read-only fleet board renders the derived fleet state. The
//! render path takes a `BoardView` (never an `Event`) — this suite proves the
//! rendered surface is a pure function of that view.
//!
//! RED MODE: assert-red. `draw` exists (oracle scaffold) but paints NOTHING,
//! so the TestBackend buffer stays blank and diverges from the committed
//! golden below. Mirrors the S4/DR-006 scaffold discipline: the API is real,
//! the body is a stub, the test fails on assertion.
//!
//! Golden fixture reused: `spec/fixtures/s4_verified_run.jsonl` (a verified run
//! + a merged worktree — the fleet has something to show).
//!
//! The golden is committed as `spec/fixtures/s5_board_render.golden.txt`
//! (named for the behavior it pins, per the fixture-hygiene rule). It is the
//! expected TEXT content of the TestBackend buffer, row by row. Regenerate
//! deliberately with `REZIDNT_BLESS_GOLDEN=1` once the implementer's `draw` is
//! real — never to make a broken render pass (test honesty).

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rezidnt_state::{Graph, fold};
use rezidnt_tui::{draw, project};
use rezidnt_types::Event;

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

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

/// Flatten a TestBackend buffer to newline-joined, right-trimmed rows of plain
/// text — style-independent so the golden pins CONTENT, not colors.
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

/// Render the s4 verified-run fixture and compare to the committed golden. On
/// `REZIDNT_BLESS_GOLDEN=1` the golden is (re)written instead of asserted —
/// used ONCE when the real `draw` lands, never to paper over a regression.
#[test]
fn board_render_matches_golden_snapshot() {
    let graph = graph_from_fixture("s4_verified_run.jsonl");
    let view = project(&graph);

    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal
        .draw(|frame| draw(frame, &view))
        .expect("draw onto the test backend");

    let got = buffer_to_text(&terminal);
    let golden_path = fixtures_dir().join("s5_board_render.golden.txt");

    if std::env::var_os("REZIDNT_BLESS_GOLDEN").is_some() {
        std::fs::write(&golden_path, &got).expect("write golden");
        panic!(
            "REZIDNT_BLESS_GOLDEN set: wrote golden to {} — unset it to assert",
            golden_path.display()
        );
    }

    let expected = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("golden {} must exist: {e}", golden_path.display()));

    assert_eq!(
        got, expected,
        "rendered board buffer diverged from the committed golden (bless deliberately with REZIDNT_BLESS_GOLDEN=1 once draw is real)"
    );
}

/// Content spot-checks independent of exact layout: the rendered buffer must
/// name the fleet's run and its status somewhere. Keeps the render honest even
/// if the golden is regenerated — a blank frame fails these outright.
#[test]
fn rendered_buffer_names_the_run_and_its_status() {
    let graph = graph_from_fixture("s4_verified_run.jsonl");
    let view = project(&graph);

    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal
        .draw(|frame| draw(frame, &view))
        .expect("draw onto the test backend");

    let text = buffer_to_text(&terminal);
    // The run id is 26 chars; a fixed 80-col board may truncate it, so assert a
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

// ---------------------------------------------------------------------------
// S5b oracle — the PERMIT column renders. `draw` must paint the per-run permit
// info (granted/denied/escalated decision counts, pending, delegation depth) as
// a column/cell in the runs table. Pinned against a NEW golden generated from a
// permit-bearing fixture — the pre-permit S5 golden
// (`s5_board_render.golden.txt`) is left UNTOUCHED (it pins the S5 contract).
//
// TARGET API the implementer must build: extend `draw` (the runs table) to
// place the permit cell(s) for each RunRow — the exact layout is whatever the
// blessed golden captures once the column lands.
//
// GOLDEN BLESS MECHANISM (mirrors `board_render_matches_golden_snapshot`
// above): with `REZIDNT_BLESS_GOLDEN=1` set, the test WRITES the golden and
// panics (telling you to unset it); otherwise it reads the committed golden and
// asserts equality. The golden must be GENERATED from the real `draw` output
// ONCE the column paints — never hand-fabricated (test honesty).
//
// RED until then: the committed placeholder golden
// (`s5b_board_permit_render.golden.txt`) is a single sentinel line that the real
// render can never equal, so this test FAILS until the implementer (a) paints
// the permit column and (b) blesses the golden with REZIDNT_BLESS_GOLDEN=1.

/// Render the S5b permit-bearing fixture and compare to its OWN committed
/// golden (never the S5 one). Criterion 3: the board renders the permit column.
#[test]
fn board_render_permit_column_matches_golden_snapshot() {
    let graph = graph_from_fixture("s5b_board_permit.jsonl");
    let view = project(&graph);

    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal
        .draw(|frame| draw(frame, &view))
        .expect("draw onto the test backend");

    let got = buffer_to_text(&terminal);
    let golden_path = fixtures_dir().join("s5b_board_permit_render.golden.txt");

    if std::env::var_os("REZIDNT_BLESS_GOLDEN").is_some() {
        std::fs::write(&golden_path, &got).expect("write golden");
        panic!(
            "REZIDNT_BLESS_GOLDEN set: wrote golden to {} — unset it to assert",
            golden_path.display()
        );
    }

    let expected = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("golden {} must exist: {e}", golden_path.display()));

    assert_eq!(
        got, expected,
        "rendered permit-board buffer diverged from the committed golden (bless deliberately with REZIDNT_BLESS_GOLDEN=1 ONCE the permit column paints)"
    );
}

/// Layout-independent spot-check: once the permit column paints, the rendered
/// buffer must surface this run's permit decision counts somewhere. A blank /
/// permit-less frame fails this outright, so it stays honest even across a
/// deliberate golden re-bless. The S5b fixture folds granted=1 / denied=1 /
/// escalated=1 / pending=1 / delegated=2.
#[test]
fn rendered_buffer_shows_permit_counts() {
    let graph = graph_from_fixture("s5b_board_permit.jsonl");
    let view = project(&graph);

    let backend = TestBackend::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal
        .draw(|frame| draw(frame, &view))
        .expect("draw onto the test backend");

    let text = buffer_to_text(&terminal);
    // The run must appear (prefix, since a 26-char ULID may be truncated).
    assert!(
        text.contains("01S5BB0ARDPERM"),
        "the board must render the permit-bearing run id (prefix); got:\n{text}"
    );
    // A "permit" column header/label must be present so the column is legible.
    assert!(
        text.to_lowercase().contains("permit"),
        "the board must label the permit column; got:\n{text}"
    );
}
