//! DR-048 slice A oracle (phase 1): `AgentSubstrate` becomes a REAL trait
//! (I4). Until now the trait existed only in prose (doc comments in spec.rs
//! and the daemon); the sole implementation, `ClaudeCodeAdapter`, is a bare
//! struct. This suite exercises adapter behavior THROUGH a
//! `dyn AgentSubstrate` trait object, with `ClaudeCodeAdapter` as one impl.
//!
//! Trait shape these tests pin (the implementer's target; dyn-safe by
//! construction — every test drives a `dyn` object):
//!
//! ```ignore
//! pub trait AgentSubstrate {
//!     fn version_gate(&self, version: &str) -> Result<(), AdapterError>;
//!     fn map_line(&mut self, line: &str) -> Result<Vec<MappedFact>, AdapterError>;
//!     fn session_id(&self) -> Option<&str>;
//! }
//! ```
//!
//! RED MODE: compile-red today. `rezidnt_run::adapter::AgentSubstrate` does
//! not exist — this file does not compile until the trait is extracted and
//! implemented for `ClaudeCodeAdapter`. That IS the failing state.
//!
//! Stay-green criterion (DR-048 consequences): the existing concrete suites
//! (`tests/adapter.rs` and friends) are UNMODIFIED and must stay green through
//! the extraction — the trait goes in BEHIND the existing constructor API
//! (`ClaudeCodeAdapter::new(run)` unchanged; the free `version_gate` fn stays).

use rezidnt_run::RunId;
use rezidnt_run::adapter::{AdapterError, AgentSubstrate, ClaudeCodeAdapter, MappedFact};
use ulid::Ulid;

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/fixtures/transcripts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"))
}

/// Drive any substrate through the SEAM, not the concrete type. This helper
/// taking `&mut dyn AgentSubstrate` is itself part of the proof: it is the
/// shape the daemon's harness dispatch will hold, and it only compiles if the
/// trait is dyn-safe.
fn drive(substrate: &mut dyn AgentSubstrate, transcript: &str) -> Vec<MappedFact> {
    transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| {
            substrate
                .map_line(l)
                .expect("recorded lines must map cleanly")
        })
        .collect()
}

/// The recorded claude-code transcript maps to the SAME subject sequence
/// through the trait object as the concrete suite pins — the extraction
/// changes the seam, never the behavior.
#[test]
fn claude_code_maps_identically_through_the_trait_object() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(48, 1))));
    let facts = drive(
        boxed.as_mut(),
        &fixture("claude_code_stream_v2.1.191.jsonl"),
    );

    let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["agent.status.changed", "agent.message", "agent.completed"],
        "the trait object must reproduce the concrete adapter's pinned mapping"
    );
}

/// Version gating is a trait method: the daemon must be able to gate ANY
/// substrate without knowing its concrete type. Claude-code's recorded major
/// passes; an untested major refuses; garbage is an honest error.
#[test]
fn version_gate_is_interrogable_through_the_trait_object() {
    let adapter: Box<dyn AgentSubstrate> =
        Box::new(ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(48, 2))));

    adapter
        .version_gate("2.1.191")
        .expect("the recorded claude-code major must pass through the seam");
    match adapter.version_gate("999.0.0") {
        Err(AdapterError::UntestedMajor { major: 999, .. }) => {}
        other => panic!("untested major must refuse through the seam, got {other:?}"),
    }
    match adapter.version_gate("not-a-version") {
        Err(AdapterError::BadVersion { .. }) => {}
        other => panic!("garbage version must be BadVersion through the seam, got {other:?}"),
    }
}

/// The checkpoint/resume seam crosses the trait too: after replaying the
/// recorded stream through `dyn`, the captured session id is readable without
/// downcasting (the daemon checkpoints runs generically).
#[test]
fn session_id_is_readable_through_the_trait_object() {
    let mut boxed: Box<dyn AgentSubstrate> =
        Box::new(ClaudeCodeAdapter::new(RunId::new(Ulid::from_parts(48, 3))));
    assert_eq!(boxed.session_id(), None);
    drive(
        boxed.as_mut(),
        &fixture("claude_code_stream_v2.1.191.jsonl"),
    );
    assert_eq!(
        boxed.session_id(),
        Some("83c61e05-aecf-4c70-93f4-ada974db33df"),
        "session capture must survive the move behind the trait"
    );
}
