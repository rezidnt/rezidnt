//! Demo/dev helper: seed an event db from a golden fixture BEFORE the daemon
//! starts — the same move the S3 board makes in `tests/common/mod.rs`
//! (`start_daemon_with_mcp`), exposed for the Phase-1 exit demo so the
//! `gate_explain` forced failure has a stub verdict on the log (I3: the log
//! is truth; the verdict is seeded there and nowhere else).
//!
//! Usage: cargo run -p rezidentd --example seed_fixture -- <db-path> <fixture.jsonl>

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args
        .next()
        .context("usage: seed_fixture <db-path> <fixture.jsonl>")?;
    let fixture = args
        .next()
        .context("usage: seed_fixture <db-path> <fixture.jsonl>")?;

    let text = std::fs::read_to_string(&fixture).with_context(|| format!("read {fixture}"))?;
    let mut log = rezidnt_fabric::EventLog::open(std::path::Path::new(&db))
        .with_context(|| format!("open event log {db}"))?;
    let mut n = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let event = rezidnt_types::Event::from_json_line(line)
            .with_context(|| format!("fixture line {} parses", n + 1))?;
        log.append(&event).context("append seeded event")?;
        n += 1;
    }
    println!("seeded {n} events from {fixture} into {db}");
    Ok(())
}
