//! Crash-test harness bin: `burst-writer <db-path> <count>`.
//! Exists to be SIGKILLed mid-append by `tests/crash_safety.rs`.

use std::path::Path;
use std::process::exit;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(count)) = (args.next(), args.next()) else {
        eprintln!("usage: burst-writer <db-path> <count>");
        exit(2);
    };
    let Ok(count) = count.parse::<u64>() else {
        eprintln!("burst-writer: <count> must be a u64, got {count:?}");
        exit(2);
    };
    if let Err(e) = rezidnt_fabric::burst::run_burst(Path::new(&db), count) {
        eprintln!("burst-writer: {e}");
        exit(1);
    }
}
