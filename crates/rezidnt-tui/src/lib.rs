//! rezidnt read-only fleet board (S5). Proof that I1 holds: the daemon renders
//! nothing; this is a PURE socket CLIENT. The board rides the EXISTING
//! `rezidnt_proto::Request::Tail { subject: None }` op (replay-from-seq-0 then
//! live — the same path `rezidnt tail` uses), folds each event into a
//! [`rezidnt_state::Graph`] with the existing pure reducers, and publishes each
//! snapshot onto a `tokio::sync::watch<Graph>`. The RENDER LOOP consumes ONLY
//! the watch channel and never touches a raw `Event` — that is the literal
//! "consuming only watch channels" I1 proof.
//!
//! ## Three pure pieces
//!
//! The crate is deliberately a pure, in-memory-testable core (the socket wiring
//! is the `rezidnt board` subcommand in `bins/rezidnt`):
//! - [`project`] — `&Graph` -> [`BoardView`], the read-only fleet projection;
//! - [`draw`] — `&BoardView` -> ratatui frame, testable via `TestBackend`;
//! - [`ingest_into_watch`] — folds an event iterator onto a `watch::Sender<Graph>`.
//!
//! The S5 tests pin each against the S4/S1 golden fixtures.
//!
//! ## Structural read-only proof (I1)
//!
//! See `Cargo.toml`: this crate depends only on `rezidnt-state` +
//! `rezidnt-types` + ratatui/crossterm — never the fabric writer or any
//! socket-write path. The board cannot emit an event because it does not link
//! anything that can.

use rezidnt_state::{Graph, WorkspaceStatus};
use tokio::sync::watch;

/// The read-only fleet projection: everything the board renders, computed as a
/// PURE function of a [`Graph`] snapshot. The render path takes a `BoardView`,
/// never an `Event` — the type system is the I1 seam.
///
/// S5 projection semantics (pinned by `tests/board_projection.rs`):
/// - `events_folded` = `graph.events_folded` (fleet heartbeat);
/// - `workspaces_open` / `workspaces_closed` = counts over
///   `graph.workspaces` by [`WorkspaceStatus`];
/// - `counts_by_subject` = `graph.counts_by_subject` (subject histogram),
///   carried verbatim for the fleet summary line;
/// - `runs` = one [`RunRow`] per `graph.agent_runs` entry, in the map's
///   deterministic (ULID-string) key order;
/// - `worktrees` = one [`WorktreeRow`] per `graph.worktrees` entry, in
///   deterministic (path) key order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoardView {
    pub events_folded: u64,
    pub workspaces_open: usize,
    pub workspaces_closed: usize,
    /// Subject histogram, verbatim from the graph (deterministic order).
    pub counts_by_subject: Vec<(String, u64)>,
    pub runs: Vec<RunRow>,
    pub worktrees: Vec<WorktreeRow>,
}

/// One agent run's row in the fleet board — the read-only accounting shadow.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunRow {
    /// The run's ULID key (graph `agent_runs` key).
    pub run: String,
    /// Recorded status string, verbatim (`spawning` | `running` | `completed`
    /// | …) — never re-interpreted (I3: reducers fold every live payload).
    pub status: String,
    pub total_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// How many replay-divergence alarms are on this run (DR-006). Zero for a
    /// healthy run; the board surfaces a nonzero count.
    pub integrity_alarms: usize,
    /// Permit decisions folded onto this run, carried verbatim from
    /// [`rezidnt_state::AgentRunState::permit_accumulators`] (I3: no
    /// re-interpretation). Grant count.
    pub permit_granted: u64,
    /// Permit denial count, from `permit_accumulators.denied`.
    pub permit_denied: u64,
    /// Permit escalation count, from `permit_accumulators.escalated`.
    /// `escalated` is never coerced to `granted` (I6) — it surfaces on its own.
    pub permit_escalated: u64,
    /// Requested-but-undecided permits: ledger entries whose `decision` is
    /// `None`. Honest zero when the ledger is empty.
    pub permit_pending: usize,
    /// Delegation-chain depth for this run, from
    /// [`rezidnt_state::AgentRunState::delegations`] length.
    pub delegated: usize,
}

/// One worktree's row in the fleet board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorktreeRow {
    /// The worktree's path key (graph `worktrees` key).
    pub path: String,
    pub status: String,
    pub branch: Option<String>,
    /// Most recent `diff.ready`/`diff.merged` summary hash, if any.
    pub last_diff: Option<String>,
}

/// PURE projection: `&Graph` -> [`BoardView`]. No IO, no clocks, no
/// randomness. The render path never sees an `Event` — it sees this. Every
/// field is carried verbatim from derived state (I3): the board re-interprets
/// nothing.
pub fn project(graph: &Graph) -> BoardView {
    // Fleet summary: heartbeat + workspace open/closed split. BTreeMap<Subject,_>
    // iterates in deterministic key order, so the histogram is stable.
    let mut workspaces_open = 0usize;
    let mut workspaces_closed = 0usize;
    for status in graph.workspaces.values() {
        match status {
            WorkspaceStatus::Open => workspaces_open += 1,
            WorkspaceStatus::Closed => workspaces_closed += 1,
        }
    }
    let counts_by_subject = graph
        .counts_by_subject
        .iter()
        .map(|(subject, count)| (subject.as_str().to_string(), *count))
        .collect();

    // Per-run rows, in the agent_runs map's deterministic (ULID-string) key
    // order. Every field is carried verbatim from derived state (I3) — the
    // board re-interprets nothing.
    let runs = graph
        .agent_runs
        .iter()
        .map(|(run, state)| RunRow {
            run: run.clone(),
            status: state.status.clone(),
            total_usd: state.total_usd,
            input_tokens: state.input_tokens,
            output_tokens: state.output_tokens,
            integrity_alarms: state.integrity_alarms.len(),
            // Permit state, carried verbatim from the ALREADY-FOLDED run
            // (I3 — the board re-derives nothing). `permit_pending` counts
            // ledger entries still awaiting a decision.
            permit_granted: state.permit_accumulators.granted,
            permit_denied: state.permit_accumulators.denied,
            permit_escalated: state.permit_accumulators.escalated,
            permit_pending: state
                .permit_ledger
                .values()
                .filter(|entry| entry.decision.is_none())
                .count(),
            delegated: state.delegations.len(),
        })
        .collect();

    // Per-worktree rows, in deterministic (path) key order.
    let worktrees = graph
        .worktrees
        .iter()
        .map(|(path, state)| WorktreeRow {
            path: path.clone(),
            status: state.status.clone(),
            branch: state.branch.clone(),
            last_diff: state.last_diff.clone(),
        })
        .collect();

    BoardView {
        events_folded: graph.events_folded,
        workspaces_open,
        workspaces_closed,
        counts_by_subject,
        runs,
        worktrees,
    }
}

/// Render a [`BoardView`] onto a ratatui frame. PURE given the view — testable
/// with `ratatui::backend::TestBackend` golden buffers, no real terminal.
///
/// S5 RICHER render semantics (DR-031 §Decision 3, pinned by
/// `tests/board_render_golden.rs`): a stack of bordered rounded panels
/// (`Block` + `BorderType::Rounded`) — a fleet-summary panel (events folded,
/// open/closed workspace counts), a subjects `Table` (subject, count — one row
/// per `counts_by_subject` entry so EVERY subject is visible, not clipped off a
/// single summary line), a runs `Table` (run id, status, cost usd, tokens,
/// alarms), a permit `Table` rendered ONLY when at least one run has permit
/// activity ([`run_has_permit_activity`]) so a permit-free fleet shows NO permit
/// panel, and a worktrees `Table` (path, status, branch, last diff). Colored
/// status cells are allowed but never asserted — the golden is a text-only
/// `TestBackend` dump.
///
/// This is a PURE, NON-INTERACTIVE function of ONE `BoardView` snapshot: no
/// selection, cursor, focus, or detail pane (interactivity is Phase 3 /
/// demand-gated, out of scope per DR-031). Every value is carried verbatim
/// from the projected view (I3): the render re-interprets nothing.
pub fn draw(frame: &mut ratatui::Frame, view: &BoardView) {
    use ratatui::layout::{Constraint, Layout};
    use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

    // Rounded bordered panel with a titled top edge. The title strings carry
    // the section words the golden's structural proof asserts (`fleet`, `runs`,
    // `permit`, `worktrees`) — ONLY the permit panel ever contains "permit".
    let panel = |title: String| -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
    };

    // The permit panel appears iff a run has permit activity, so a permit-free
    // fleet renders no permit chrome at all (the golden's absence proof).
    let any_permit = view.runs.iter().any(run_has_permit_activity);

    // Stack the panels vertically, sized for the 100x40 canvas the oracle sets.
    // Each Table panel spends 2 rows on border + 1 on its header row; the
    // summary panel holds a fixed 2-line body. The subjects panel is sized to
    // its own row count so EVERY subject is visible (no clip — the histogram
    // moved off the single clipping summary line into its own bordered Table).
    // The runs/permit/worktrees panels take the remaining space split
    // proportionally so long fleets scroll within their own panel.
    //
    // Subjects panel height: border(2) + header(1) + one row per subject. A
    // permit-free fleet (s4) stacks 4 panels; a permit-bearing fleet (s5b)
    // stacks 5. Both fixtures carry 7 subjects → the subjects panel is 10 rows;
    // fixed chrome is 4 (summary) + 10 (subjects) = 14, leaving 26 rows on the
    // 40-row canvas for the remaining Min-sized tables. Comfortable.
    let subjects_body = view.counts_by_subject.len().max(1) as u16;
    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(4), // fleet summary: border(2) + 2 body lines
        Constraint::Length(subjects_body + 3), // subjects: border(2) + header(1) + rows
        Constraint::Min(4),    // runs table
    ];
    if any_permit {
        constraints.push(Constraint::Min(4)); // permit table
    }
    constraints.push(Constraint::Min(4)); // worktrees table
    let areas = Layout::vertical(constraints).split(frame.area());

    // --- Fleet summary panel -------------------------------------------------
    // Carries the heartbeat + workspace split ONLY; the subject histogram now
    // lives in its own bordered panel below so no entry clips off the right edge.
    let summary_block = panel(" fleet summary ".to_string());
    let summary = Paragraph::new(format!(
        "events folded: {}\nworkspaces: {} open / {} closed",
        view.events_folded, view.workspaces_open, view.workspaces_closed
    ))
    .block(summary_block);
    frame.render_widget(summary, areas[0]);

    // --- Subjects panel ------------------------------------------------------
    // One row per `counts_by_subject` entry, in the projection's existing
    // deterministic order. Rendered as a `Table` (like runs/worktrees) so every
    // short `noun.verb[.qualifier]` subject appears in FULL — the clip-regression
    // guard (`every_projected_subject_is_visible_not_clipped`) asserts each
    // subject string reaches the buffer. Values carried verbatim (I3).
    let subjects_widths = [
        Constraint::Length(40), // subject (short noun.verb[.qualifier])
        Constraint::Length(12), // count
    ];
    let subjects_header = Row::new(["subject", "count"]);
    let subjects_rows = view.counts_by_subject.iter().map(|(subject, count)| {
        Row::new([Cell::from(subject.clone()), Cell::from(count.to_string())])
    });
    let subjects_table = Table::new(subjects_rows, subjects_widths)
        .header(subjects_header)
        .block(panel(format!(
            " subjects ({}) ",
            view.counts_by_subject.len()
        )));
    frame.render_widget(subjects_table, areas[1]);

    // --- Runs table ----------------------------------------------------------
    let runs_widths = [
        Constraint::Length(28), // run id
        Constraint::Length(11), // status
        Constraint::Length(12), // cost usd
        Constraint::Length(16), // tokens (in/out)
        Constraint::Length(8),  // alarms
    ];
    let runs_header = Row::new(["run", "status", "cost usd", "tokens", "alarms"]);
    let runs_rows = view.runs.iter().map(|row| {
        Row::new([
            Cell::from(truncate(&row.run, 26).to_string()),
            status_cell(&row.status),
            Cell::from(
                row.total_usd
                    .map(|v| format!("{v:.6}"))
                    .unwrap_or_else(|| "-".to_string()),
            ),
            Cell::from(tokens(row.input_tokens, row.output_tokens)),
            Cell::from(row.integrity_alarms.to_string()),
        ])
    });
    let runs_table = Table::new(runs_rows, runs_widths)
        .header(runs_header)
        .block(panel(format!(" runs ({}) ", view.runs.len())));
    frame.render_widget(runs_table, areas[2]);

    // --- Permit table (conditional) ------------------------------------------
    // Rendered ONLY when at least one run has permit activity. A permit-free
    // fleet shows no permit panel — and no other panel/label ever contains the
    // word "permit", so the absence is a clean text proof. Values are carried
    // verbatim from the RunRow (I3); `escalated` is surfaced on its own, never
    // coerced into `granted` (I6).
    let next = if any_permit {
        let permit_widths = [
            Constraint::Length(28), // run id
            Constraint::Length(10), // granted
            Constraint::Length(10), // denied
            Constraint::Length(11), // escalated
            Constraint::Length(9),  // pending
            Constraint::Length(11), // delegated
        ];
        let permit_header = Row::new([
            "run",
            "granted",
            "denied",
            "escalated",
            "pending",
            "delegated",
        ]);
        let permit_rows = view
            .runs
            .iter()
            .filter(|row| run_has_permit_activity(row))
            .map(|row| {
                Row::new([
                    Cell::from(truncate(&row.run, 26).to_string()),
                    Cell::from(row.permit_granted.to_string()),
                    Cell::from(row.permit_denied.to_string()),
                    Cell::from(row.permit_escalated.to_string()),
                    Cell::from(row.permit_pending.to_string()),
                    Cell::from(row.delegated.to_string()),
                ])
            });
        let permit_table = Table::new(permit_rows, permit_widths)
            .header(permit_header)
            .block(panel(" permit decisions ".to_string()));
        frame.render_widget(permit_table, areas[3]);
        4
    } else {
        3
    };

    // --- Worktrees table -----------------------------------------------------
    let wt_widths = [
        Constraint::Length(34), // path
        Constraint::Length(11), // status
        Constraint::Length(18), // branch
        Constraint::Min(14),    // last diff
    ];
    let wt_header = Row::new(["path", "status", "branch", "last diff"]);
    let wt_rows = view.worktrees.iter().map(|wt| {
        Row::new([
            Cell::from(truncate(&wt.path, 32).to_string()),
            status_cell(&wt.status),
            Cell::from(wt.branch.clone().unwrap_or_else(|| "-".to_string())),
            Cell::from(
                wt.last_diff
                    .as_deref()
                    .map(|d| truncate(d, 12).to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ])
    });
    let wt_table = Table::new(wt_rows, wt_widths)
        .header(wt_header)
        .block(panel(format!(" worktrees ({}) ", view.worktrees.len())));
    frame.render_widget(wt_table, areas[next]);
}

/// A status `Cell`, colored by status kind. The color is decorative only — the
/// golden is a text dump that drops style, so correctness never depends on it.
fn status_cell(status: &str) -> ratatui::widgets::Cell<'static> {
    use ratatui::style::{Color, Style};
    let color = match status {
        "completed" | "merged" | "verified" => Color::Green,
        "running" | "open" => Color::Cyan,
        "spawning" | "pending" => Color::Yellow,
        "failed" | "closed" => Color::Red,
        _ => Color::Gray,
    };
    ratatui::widgets::Cell::from(status.to_string()).style(Style::default().fg(color))
}

/// First `max` chars of `s` (byte-truncation is safe here: run ULIDs and
/// blake3 hashes are ASCII).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

/// `in/out` token cell (`-` for an absent count).
fn tokens(input: Option<u64>, output: Option<u64>) -> String {
    let cell = |n: Option<u64>| n.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());
    format!("{}/{}", cell(input), cell(output))
}

/// Whether a run has any folded permit activity to surface: a decision
/// (granted/denied/escalated), a pending request, or a delegation. A run with
/// none reads as all-zero and is omitted from the permit section so a
/// permit-free fleet renders byte-identically to the pre-permit board.
fn run_has_permit_activity(row: &RunRow) -> bool {
    row.permit_granted != 0
        || row.permit_denied != 0
        || row.permit_escalated != 0
        || row.permit_pending != 0
        || row.delegated != 0
}

/// Fold an event iterator into a `watch::Sender<Graph>`, publishing a fresh
/// [`Graph`] snapshot after each event. This is the WATCH SEAM the render loop
/// consumes: the render side holds a `watch::Receiver<Graph>` and calls
/// [`project`] on each observed snapshot — it never sees a raw `Event`.
///
/// The socket wiring (connect, read the hello, send `Request::Tail`, read the
/// JSONL event frames) is the implementer's `rezidnt board` subcommand in
/// `bins/rezidnt`; this helper is the pure, in-memory-testable core: feed it a
/// `Vec<Event>` and assert the receiver observes the transition.
pub fn ingest_into_watch<'a, I>(events: I, sender: &watch::Sender<Graph>)
where
    I: IntoIterator<Item = &'a rezidnt_types::Event>,
{
    // Fold onto the sender's current snapshot (the seam is resumable: the
    // render loop's initial seed rides through unchanged if there are no
    // events). One `send` per event so a receiver that redraws on every watch
    // change observes each transition, not just the terminal one.
    let mut graph = sender.borrow().clone();
    for event in events {
        rezidnt_state::apply(&mut graph, event);
        // The receiver only ever reads state — never a raw Event (I1).
        let _ = sender.send(graph.clone());
    }
}
