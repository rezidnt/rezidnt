//! Drift guard: `taxonomy::SUBJECTS_V0` must match the subjects declared in
//! `spec/ontology.md` (the canonical copy, warden-custody) — same set, same
//! ontology-table order. Deferred to the implementer per the oracle honesty
//! rule: it could only be written once the implementation existed to test
//! against.
//!
//! Parsing contract: a subject row in the ontology is a Markdown table row
//! whose first cell is a backticked subject and whose second cell is the
//! integer payload schema version, e.g. `| `workspace.opened` | 1 | … |`.
//! Prose mentions of subjects (grammar notes, changelog) are not table rows
//! and are ignored.

use std::path::PathBuf;

use rezidnt_types::taxonomy::SUBJECTS_V0;

fn ontology_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/ontology.md")
}

/// Extract `(subject, v)` from one line iff it is a subject table row.
fn parse_subject_row(line: &str) -> Option<(String, u16)> {
    let mut cells = line.strip_prefix('|')?.split('|').map(str::trim);
    let first = cells.next()?;
    let subject = first.strip_prefix('`')?.strip_suffix('`')?;
    let v: u16 = cells.next()?.parse().ok()?;
    Some((subject.to_owned(), v))
}

#[test]
fn subjects_v0_matches_the_canonical_ontology() {
    let path = ontology_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("canonical ontology {} must exist: {e}", path.display()));

    let parsed: Vec<(String, u16)> = text.lines().filter_map(parse_subject_row).collect();
    assert!(
        !parsed.is_empty(),
        "no subject table rows parsed out of {} — parser or ontology format drift",
        path.display()
    );

    let ontology_subjects: Vec<&str> = parsed.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        ontology_subjects, SUBJECTS_V0,
        "taxonomy::SUBJECTS_V0 has drifted from spec/ontology.md \
         (same subjects, ontology-table order); route the fix through /subject \
         if the ontology is what must change"
    );

    for (subject, v) in &parsed {
        assert_eq!(
            *v, 1,
            "taxonomy v0 mints every subject at v = 1, but {subject} declares v = {v}"
        );
    }
}
