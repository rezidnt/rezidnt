//! S0 oracle — golden event-log fixture replay (testing-oracles skill).
//! Every `spec/fixtures/<name>.jsonl` with a `<name>.expected.json` companion
//! is folded and compared against its expected graph. Run by
//! `scripts/replay-fixtures.sh` (the /vet gauntlet) and by every release.
//!
//! Note: events are parsed with plain serde here (not `Event::from_json_line`)
//! so a failure in this test isolates the *reducers*, not the wire codec.

use std::path::PathBuf;

use rezidnt_state::{Graph, fold};
use rezidnt_types::Event;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures")
}

#[test]
fn golden_fixtures_fold_to_their_expected_graphs() {
    let dir = fixtures_dir();
    let mut replayed = 0usize;
    for entry in std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("fixtures dir {} must exist: {e}", dir.display()))
    {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let expected_path = path.with_extension("expected.json");
        if !expected_path.exists() {
            continue; // chain/envelope fixtures are owned by other suites
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();

        let events: Vec<Event> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| {
                serde_json::from_str(l)
                    .unwrap_or_else(|e| panic!("{name}: fixture line must parse ({e}): {l}"))
            })
            .collect();
        let expected: Graph =
            serde_json::from_str(&std::fs::read_to_string(&expected_path).unwrap())
                .unwrap_or_else(|e| panic!("{name}: expected graph must parse: {e}"));

        let got = fold(events.iter());
        assert_eq!(
            got, expected,
            "{name}: fold(fixture) diverged from the committed expected graph"
        );
        replayed += 1;
    }
    assert!(
        replayed >= 1,
        "no <name>.jsonl + <name>.expected.json fixture pairs found — the golden fixture set is broken"
    );
}
