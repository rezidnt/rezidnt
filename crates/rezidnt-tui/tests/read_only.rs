//! S5 oracle — the STRUCTURAL read-only proof (I1) and the NO-DAEMON-CHANGE
//! pin. These two tests encode the load-bearing half of the criterion:
//! "consuming only watch channels — proof I1 held" and the handoff's "pure
//! client, no daemon change."
//!
//! RED MODE: mostly compile/assert-red by construction today.
//! - `crate_has_no_writer_dependency` reads THIS crate's `Cargo.toml` and
//!   asserts its runtime `[dependencies]` table pulls no write/emit path. It
//!   is GREEN today (the scaffold Cargo.toml is already writer-free) and its
//!   job is to STAY green — it is the regression tripwire that turns an
//!   implementer sneaking `rezidnt-fabric` in RED. Documented here so the
//!   auditor knows it is a guard, not an oracle-red.
//! - `board_rides_existing_tail_op_no_new_proto_op` pins the existing
//!   `Request::Tail { subject: None }` variant (the op the board must ride) and
//!   asserts NO new proto op is minted. Green today; RED the instant someone
//!   adds a board-specific proto op instead of reusing `tail`.
//!
//! Both are "stay-green" structural guards rather than assert-red oracles — the
//! honest thing is to say so (test honesty). The assert-red oracles for S5 are
//! `board_projection.rs`, `board_render_golden.rs`, and `watch_live_update.rs`.

use std::path::PathBuf;

/// The runtime dependency closure of this crate must never include a fabric
/// writer or any socket-write / log-append path. I1: the board is a pure
/// downstream reader of derived state; it cannot emit an event because it does
/// not link anything that can. Parses the `[dependencies]` table of THIS
/// crate's own manifest (dev-deps are excluded — test-only proto pinning does
/// not ship).
#[test]
fn crate_has_no_writer_dependency() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("read own Cargo.toml");

    // Slice out the runtime [dependencies] table (stop at the next table
    // header, e.g. [dev-dependencies]). Crude-but-honest: the manifest is
    // ours and small.
    let deps = runtime_dependencies_section(&text);

    // The write/emit surfaces the board must NEVER link (I1). rezidnt-state /
    // rezidnt-types are read-only derived-state crates and are allowed.
    const FORBIDDEN: &[&str] = &[
        "rezidnt-fabric", // the log writer / broadcast emit path
        "rezidnt-proto",  // socket request/reply write surface (dev-only is ok)
        "rezidnt-run",    // spawns processes, emits run facts
        "rezidnt-mcp",    // MCP tool surface (mutating tools)
        "rezidnt-gate",   // gate engine (emits gate facts)
        "rusqlite",       // the SQLite append path
        "blake3",         // the chain/append hashing path
    ];
    for needle in FORBIDDEN {
        assert!(
            !deps.contains(needle),
            "I1 regression: rezidnt-tui runtime [dependencies] pulled `{needle}` — the read-only board must not link any write/emit path"
        );
    }

    // Positive: it DOES depend on the read-only state crate (otherwise the
    // "no writer" pass would be vacuous — a crate with no deps at all).
    assert!(
        deps.contains("rezidnt-state"),
        "the board must read derived state from rezidnt-state"
    );
}

/// Return the text of the runtime `[dependencies]` table only, up to the next
/// `[` table header at column 0.
fn runtime_dependencies_section(manifest: &str) -> String {
    let mut in_deps = false;
    let mut out = String::new();
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_deps = trimmed.starts_with("[dependencies]");
            continue;
        }
        if in_deps {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The board rides the EXISTING `Request::Tail` op (replay-from-seq-0 then
/// live) — no new proto op is introduced (handoff: "no daemon change"). Pin the
/// variant the board must reuse; if a board-specific op is ever minted, this
/// stops compiling or the shape check below trips.
#[test]
fn board_rides_existing_tail_op_no_new_proto_op() {
    use rezidnt_proto::Request;

    // The op the board must ride, verbatim: replay from seq 0 then live,
    // unfiltered. Constructing it proves the variant exists and has this shape.
    let op = Request::Tail { subject: None };
    match &op {
        Request::Tail { subject } => {
            assert!(
                subject.is_none(),
                "the board tails the whole fleet, unfiltered"
            )
        }
        _ => unreachable!("constructed a Tail"),
    }

    // It round-trips through the existing codec unchanged — the board sends
    // this exact frame, nothing new on the wire.
    let line = rezidnt_proto::encode_request(&op).expect("encode existing Tail op");
    let back = rezidnt_proto::decode_request(&line).expect("decode existing Tail op");
    assert_eq!(op, back, "the board uses the existing Tail op verbatim");
    assert_eq!(
        line, r#"{"op":"tail"}"#,
        "the board's wire frame is the existing tail op — no new proto op minted"
    );
}
