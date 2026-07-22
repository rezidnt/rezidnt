//! PROTOTYPE (outside the slice loop) — an interactive, chrome-rich fleet board.
//!
//! The shipped S5 board (`rezidnt_tui::draw`) is a deliberately chrome-free
//! `Paragraph` whose golden buffer is byte-stable. This example explores what a
//! richer, *interactive* board could look like WITHOUT touching that golden:
//! bordered panels, colored status/alarm cells, a selectable runs table, and a
//! live detail pane. It stays read-only by construction (I1) — it consumes a
//! `BoardView` and renders it; it never mints or appends an `Event`.
//!
//! Run it two ways:
//!   * `cargo run -p rezidnt-tui --example board_rich`            (interactive)
//!   * `cargo run -p rezidnt-tui --example board_rich -- snapshot` (one frame to
//!     stdout via `TestBackend` — no TTY needed, handy for review)
//!
//! Interactive keys: ↑/↓ or j/k move the selection · Tab switches the focused
//! table (runs ↔ worktrees) · space pauses the simulated live feed · q quits.
//!
//! The data here is SYNTHETIC (a hand-built `BoardView`) so the prototype runs
//! with no daemon. Wiring this to the real `watch<Graph>` seam is a one-liner
//! in `bins/rezidnt board`: project the live snapshot instead of the demo view.

use std::io;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Table, TableState,
};
use ratatui::{Frame, symbols};
use rezidnt_tui::{BoardView, RunRow, WorktreeRow};

/// Which table currently owns the selection cursor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Runs,
    Worktrees,
}

/// Prototype UI state — a read-only view plus cursor/selection bookkeeping. A
/// tiny LCG drives the simulated live feed so the board *feels* live without a
/// clock source that varies per build.
struct App {
    view: BoardView,
    focus: Focus,
    runs_state: TableState,
    wt_state: TableState,
    paused: bool,
    ticks: u64,
    rng: u64,
    /// Recent per-tick event-fold deltas — the sparkline's data (newest last).
    feed_history: Vec<u64>,
}

/// How many recent deltas the feed sparkline keeps.
const FEED_HISTORY: usize = 96;

impl App {
    fn new() -> Self {
        let mut runs_state = TableState::default();
        runs_state.select(Some(0));
        let mut wt_state = TableState::default();
        wt_state.select(Some(0));
        Self {
            view: demo_view(),
            focus: Focus::Runs,
            runs_state,
            wt_state,
            paused: false,
            ticks: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
            feed_history: Vec::new(),
        }
    }

    /// Cheap deterministic-per-step pseudo-randomness for the live sim (no
    /// wall-clock / OS rng — keeps the prototype self-contained).
    fn roll(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Advance the simulated live feed: bump the fleet heartbeat, nudge one
    /// active run's tokens/cost, and occasionally raise an alarm. This mimics
    /// what folding a fresh event into the `Graph` would do to the projection.
    fn tick(&mut self) {
        if self.paused {
            return;
        }
        self.ticks += 1;
        let inc = 1 + (self.roll() % 6);
        self.view.events_folded += inc;
        self.feed_history.push(inc);
        if self.feed_history.len() > FEED_HISTORY {
            let overflow = self.feed_history.len() - FEED_HISTORY;
            self.feed_history.drain(0..overflow);
        }

        if self.view.runs.is_empty() {
            return;
        }
        let idx = (self.roll() as usize) % self.view.runs.len();
        let bump_in = self.roll() % 900;
        let bump_out = self.roll() % 400;
        let alarm = self.roll().is_multiple_of(23);
        let run = &mut self.view.runs[idx];
        if run.status == "running" || run.status == "spawning" {
            run.input_tokens = Some(run.input_tokens.unwrap_or(0) + bump_in);
            run.output_tokens = Some(run.output_tokens.unwrap_or(0) + bump_out);
            let add_cost = (bump_in + bump_out) as f64 * 0.000_003;
            run.total_usd = Some(run.total_usd.unwrap_or(0.0) + add_cost);
            if alarm {
                run.integrity_alarms += 1;
            }
        }

        // Nudge a subject counter so the histogram moves too.
        if !self.view.counts_by_subject.is_empty() {
            let s = (self.roll() as usize) % self.view.counts_by_subject.len();
            self.view.counts_by_subject[s].1 += 1;
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let (state, len) = match self.focus {
            Focus::Runs => (&mut self.runs_state, self.view.runs.len()),
            Focus::Worktrees => (&mut self.wt_state, self.view.worktrees.len()),
        };
        if len == 0 {
            return;
        }
        let cur = state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32);
        state.select(Some(next as usize));
    }

    fn selected_run(&self) -> Option<&RunRow> {
        self.view.runs.get(self.runs_state.selected().unwrap_or(0))
    }
}

fn main() -> io::Result<()> {
    if std::env::args().any(|a| a == "snapshot") {
        return snapshot();
    }
    interactive()
}

/// One-frame render to a `TestBackend`, printed to stdout as text. No raw mode,
/// no alternate screen — safe to run without a TTY (and how the reviewer sees
/// the layout). Colors don't survive the text dump; the structure does.
fn snapshot() -> io::Result<()> {
    let mut app = App::new();
    // Advance a few ticks so the board isn't all-zero.
    for _ in 0..12 {
        app.tick();
    }
    let backend = TestBackend::new(118, 34);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| ui(f, &mut app))?;
    println!("{}", terminal.backend());
    Ok(())
}

/// The interactive event loop: draw, poll input at ~20fps, tick the live sim
/// every ~700ms. `ratatui::init()` handles raw mode + alternate screen and
/// `ratatui::restore()` puts the terminal back, even on the `?` early return.
fn interactive() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let tick_rate = Duration::from_millis(700);
    let mut last_tick = Instant::now();

    let res = loop {
        if let Err(e) = terminal.draw(|f| ui(f, &mut app)) {
            break Err(e);
        }
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        match event::poll(timeout) {
            Ok(true) => match event::read() {
                Ok(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Char(' ') => app.paused = !app.paused,
                    KeyCode::Tab => {
                        app.focus = match app.focus {
                            Focus::Runs => Focus::Worktrees,
                            Focus::Worktrees => Focus::Runs,
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                    KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                    _ => {}
                },
                Ok(_) => {}
                Err(e) => break Err(e),
            },
            Ok(false) => {}
            Err(e) => break Err(e),
        }
        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }
    };

    ratatui::restore();
    res
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

const ACCENT: Color = Color::Cyan;

fn ui(f: &mut Frame, app: &mut App) {
    let root = Layout::vertical([
        Constraint::Length(6), // summary
        Constraint::Min(6),    // body
        Constraint::Length(1), // footer
    ])
    .split(f.area());

    draw_summary(f, root[0], app);

    let body =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(root[1]);
    let left =
        Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)]).split(body[0]);

    draw_runs(f, left[0], app);
    draw_worktrees(f, left[1], app);
    draw_detail(f, body[1], app);
    draw_footer(f, root[2], app);
}

fn draw_summary(f: &mut Frame, area: Rect, app: &App) {
    let view = &app.view;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " rezidnt fleet board · prototype ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(ratatui::layout::Alignment::Center);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(28),
        Constraint::Percentage(32),
    ])
    .split(inner);

    // Left: heartbeat number, live/paused state, and a feed sparkline.
    let live = if app.paused {
        Span::styled("‖ paused", Style::default().fg(Color::Yellow))
    } else {
        Span::styled("● live", Style::default().fg(Color::Green))
    };
    let spark_w = cols[0].width.saturating_sub(1) as usize;
    let heartbeat = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("events folded  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                view.events_folded.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            live,
        ]),
        Line::from(Span::styled(
            "feed / tick",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            spark(&app.feed_history, spark_w),
            Style::default().fg(ACCENT),
        )),
    ]);
    f.render_widget(heartbeat, cols[0]);

    // Middle: workspaces open ratio. Title above the bar so the label doesn't
    // collide with the fill.
    let g = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(cols[1]);
    f.render_widget(
        Paragraph::new(Span::styled(
            "workspaces open",
            Style::default().fg(Color::DarkGray),
        )),
        g[0],
    );
    let total = (view.workspaces_open + view.workspaces_closed).max(1);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(ACCENT).bg(Color::Black))
        .ratio(view.workspaces_open as f64 / total as f64)
        .label(format!(
            "{} / {}",
            view.workspaces_open,
            view.workspaces_open + view.workspaces_closed
        ));
    f.render_widget(gauge, g[1]);

    // Right: top subjects (three, sorted by count desc).
    let mut subjects: Vec<Line> = vec![Line::from(Span::styled(
        "top subjects",
        Style::default().fg(Color::DarkGray),
    ))];
    let mut sorted = view.counts_by_subject.clone();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (subject, count) in sorted.iter().take(3) {
        subjects.push(Line::from(vec![
            Span::styled(format!("{count:>5}  "), Style::default().fg(ACCENT)),
            Span::raw(subject.clone()),
        ]));
    }
    f.render_widget(Paragraph::new(subjects), cols[2]);
}

/// A block-character sparkline of `data` (newest last), right-aligned to
/// `width`. Pure text so it renders in the `snapshot` dump too. Each sample
/// maps to one of eight partial blocks scaled against the window's max.
fn spark(data: &[u64], width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return String::new();
    }
    let tail = if data.len() > width {
        &data[data.len() - width..]
    } else {
        data
    };
    let max = tail.iter().copied().max().unwrap_or(0).max(1);
    let pad = width.saturating_sub(tail.len());
    let mut s = " ".repeat(pad);
    for &v in tail {
        let idx = (v.saturating_sub(1) as usize * (BARS.len() - 1)) / max as usize;
        s.push(BARS[idx.min(BARS.len() - 1)]);
    }
    s
}

fn draw_runs(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.focus == Focus::Runs;
    let header = Row::new(
        [
            "run",
            "status",
            "cost usd",
            "in/out tok",
            "alarms",
            "permit g/d/e",
        ]
        .into_iter()
        .map(|h| {
            Cell::from(Span::styled(
                h,
                Style::default().add_modifier(Modifier::BOLD),
            ))
        }),
    )
    .style(Style::default().fg(Color::DarkGray));

    let rows = app.view.runs.iter().map(|r| {
        let alarms = if r.integrity_alarms > 0 {
            Cell::from(r.integrity_alarms.to_string())
                .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        } else {
            Cell::from("0").style(Style::default().fg(Color::Green))
        };
        Row::new(vec![
            Cell::from(truncate(&r.run, 14).to_string()),
            status_cell(&r.status),
            Cell::from(
                r.total_usd
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "-".into()),
            ),
            Cell::from(tokens(r.input_tokens, r.output_tokens)),
            alarms,
            Cell::from(format!(
                "{}/{}/{}",
                r.permit_granted, r.permit_denied, r.permit_escalated
            )),
        ])
    });

    let widths = [
        Constraint::Length(16),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(13),
        Constraint::Length(7),
        Constraint::Min(10),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(panel_block(
            "runs",
            app.view.runs.len(),
            focused,
            app.runs_state.selected(),
        ))
        .row_highlight_style(
            Style::default()
                .bg(if focused { ACCENT } else { Color::DarkGray })
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎");
    f.render_stateful_widget(table, area, &mut app.runs_state);
}

fn draw_worktrees(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.focus == Focus::Worktrees;
    let header = Row::new(
        ["path", "status", "branch", "last diff"]
            .into_iter()
            .map(|h| {
                Cell::from(Span::styled(
                    h,
                    Style::default().add_modifier(Modifier::BOLD),
                ))
            }),
    )
    .style(Style::default().fg(Color::DarkGray));

    let rows = app.view.worktrees.iter().map(|w| {
        Row::new(vec![
            Cell::from(truncate(&w.path, 26).to_string()),
            status_cell(&w.status),
            Cell::from(w.branch.clone().unwrap_or_else(|| "-".into())),
            Cell::from(
                w.last_diff
                    .as_deref()
                    .map(|d| truncate(d, 12).to_string())
                    .unwrap_or_else(|| "-".into()),
            ),
        ])
    });

    let widths = [
        Constraint::Length(28),
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Min(12),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(panel_block(
            "worktrees",
            app.view.worktrees.len(),
            focused,
            app.wt_state.selected(),
        ))
        .row_highlight_style(
            Style::default()
                .bg(if focused { ACCENT } else { Color::DarkGray })
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎");
    f.render_stateful_widget(table, area, &mut app.wt_state);
}

/// The detail pane: everything the projection knows about the selected run,
/// laid out as a read-only inspector. This is the drill-down the flat S5 board
/// can't show without chrome.
fn draw_detail(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " run detail ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(run) = app.selected_run() else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "no run selected",
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
        return;
    };

    let kv = |k: &str, v: Span<'static>| {
        Line::from(vec![
            Span::styled(format!("{k:<16}"), Style::default().fg(Color::DarkGray)),
            v,
        ])
    };
    let plain = |s: String| Span::styled(s, Style::default().fg(Color::White));

    let mut lines = vec![
        kv("run", plain(run.run.clone())),
        kv("status", status_span(&run.status)),
        kv(
            "cost usd",
            plain(
                run.total_usd
                    .map(|v| format!("{v:.6}"))
                    .unwrap_or_else(|| "-".into()),
            ),
        ),
        kv("input tokens", plain(opt_num(run.input_tokens))),
        kv("output tokens", plain(opt_num(run.output_tokens))),
        Line::raw(""),
        kv(
            "integrity",
            if run.integrity_alarms > 0 {
                Span::styled(
                    format!("{} alarm(s) — DR-006 divergence", run.integrity_alarms),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("clean", Style::default().fg(Color::Green))
            },
        ),
        Line::raw(""),
        Line::from(Span::styled(
            "permits — verbatim, I6",
            Style::default().fg(Color::DarkGray),
        )),
        kv(
            "granted",
            Span::styled(
                run.permit_granted.to_string(),
                Style::default().fg(Color::Green),
            ),
        ),
        kv(
            "denied",
            Span::styled(
                run.permit_denied.to_string(),
                Style::default().fg(Color::Red),
            ),
        ),
        kv(
            "escalated",
            Span::styled(
                run.permit_escalated.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ),
        kv("pending", plain(run.permit_pending.to_string())),
        kv("delegated depth", plain(run.delegated.to_string())),
    ];

    // A little bar for the granted/denied/escalated split.
    let g = run.permit_granted;
    let d = run.permit_denied;
    let e = run.permit_escalated;
    let sum = (g + d + e).max(1);
    let bar_w = inner.width.saturating_sub(0) as u64;
    let seg = |n: u64| ((n * bar_w) / sum) as usize;
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("█".repeat(seg(g)), Style::default().fg(Color::Green)),
        Span::styled("█".repeat(seg(d)), Style::default().fg(Color::Red)),
        Span::styled("█".repeat(seg(e)), Style::default().fg(Color::Yellow)),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let focus = match app.focus {
        Focus::Runs => "runs",
        Focus::Worktrees => "worktrees",
    };
    let key = |k: &str, d: &str| {
        vec![
            Span::styled(
                format!(" {k} "),
                Style::default().bg(Color::DarkGray).fg(Color::White),
            ),
            Span::styled(format!(" {d}   "), Style::default().fg(Color::DarkGray)),
        ]
    };
    let mut spans = Vec::new();
    spans.extend(key("↑/↓ j/k", "move"));
    spans.extend(key("Tab", &format!("focus: {focus}")));
    spans.extend(key("space", if app.paused { "resume" } else { "pause" }));
    spans.extend(key("q", "quit"));
    // Trailing read-only reassurance — the whole point of I1.
    spans.push(Span::styled(
        format!(
            "{}read-only · zero writer deps (I1)",
            symbols::line::VERTICAL
        ),
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn panel_block(
    title: &str,
    count: usize,
    focused: bool,
    selected: Option<usize>,
) -> Block<'static> {
    let color = if focused { ACCENT } else { Color::DarkGray };
    // Focused panel shows the 1-based cursor position (2/4); unfocused shows
    // just the count.
    let label = match (focused, selected) {
        (true, Some(i)) if count > 0 => format!(" {title} ({}/{count}) ", i + 1),
        _ => format!(" {title} ({count}) "),
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
}

fn status_cell(status: &str) -> Cell<'static> {
    Cell::from(status_span(status))
}

fn status_span(status: &str) -> Span<'static> {
    let color = match status {
        "completed" | "merged" | "verified" => Color::Green,
        "running" | "open" => ACCENT,
        "spawning" | "pending" => Color::Yellow,
        "failed" | "closed" => Color::Red,
        _ => Color::Gray,
    };
    Span::styled(status.to_string(), Style::default().fg(color))
}

fn tokens(input: Option<u64>, output: Option<u64>) -> String {
    let cell = |n: Option<u64>| n.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    format!("{}/{}", cell(input), cell(output))
}

fn opt_num(n: Option<u64>) -> String {
    n.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

/// Synthetic fleet state so the prototype runs with no daemon. Mirrors the
/// shape the real `project(&Graph)` produces.
fn demo_view() -> BoardView {
    let runs = vec![
        RunRow {
            run: "01JADR4ALPHA7QF2".into(),
            status: "running".into(),
            total_usd: Some(0.184_2),
            input_tokens: Some(48_120),
            output_tokens: Some(9_430),
            integrity_alarms: 0,
            permit_granted: 12,
            permit_denied: 1,
            permit_escalated: 2,
            permit_pending: 1,
            delegated: 2,
        },
        RunRow {
            run: "01JADR4BETA88KM3".into(),
            status: "completed".into(),
            total_usd: Some(0.402_9),
            input_tokens: Some(121_800),
            output_tokens: Some(31_200),
            integrity_alarms: 0,
            permit_granted: 40,
            permit_denied: 0,
            permit_escalated: 0,
            permit_pending: 0,
            delegated: 0,
        },
        RunRow {
            run: "01JADR4GAMMA9XZ1".into(),
            status: "spawning".into(),
            total_usd: None,
            input_tokens: Some(210),
            output_tokens: None,
            integrity_alarms: 0,
            permit_granted: 0,
            permit_denied: 0,
            permit_escalated: 0,
            permit_pending: 3,
            delegated: 1,
        },
        RunRow {
            run: "01JADR4DELTA5PQ7".into(),
            status: "running".into(),
            total_usd: Some(0.093_1),
            input_tokens: Some(22_400),
            output_tokens: Some(4_100),
            integrity_alarms: 2,
            permit_granted: 5,
            permit_denied: 3,
            permit_escalated: 1,
            permit_pending: 0,
            delegated: 0,
        },
    ];

    let worktrees = vec![
        WorktreeRow {
            path: "wt/c3-op-secrets".into(),
            status: "merged".into(),
            branch: Some("c3-op-secrets".into()),
            last_diff: Some("b3a91f0c22de".into()),
        },
        WorktreeRow {
            path: "wt/board-rich-proto".into(),
            status: "open".into(),
            branch: Some("board-rich".into()),
            last_diff: None,
        },
        WorktreeRow {
            path: "wt/gate-egress".into(),
            status: "open".into(),
            branch: Some("egress-fold".into()),
            last_diff: Some("7fe10a99c401".into()),
        },
    ];

    let counts_by_subject = vec![
        ("run.progress".into(), 812),
        ("permit.decided".into(), 60),
        ("diff.ready".into(), 14),
        ("session.opened".into(), 6),
        ("gate.verdict".into(), 22),
    ];

    BoardView {
        events_folded: 1_284,
        workspaces_open: 3,
        workspaces_closed: 7,
        counts_by_subject,
        runs,
        worktrees,
    }
}
