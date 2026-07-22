//! DR-036 sub-slice `quickstart` ORACLE — the HOST-runnable LOCKSTEP judge for
//! `docs/quickstart.md`, the operator quickstart that IS the narrated one-take
//! golden-path demo (DR-036 §Design line 42; Slicing `quickstart` criteria 1/2).
//!
//! ## What this judges (and why it is a genuine judge, not theater)
//! The quickstart doc's whole value is that it stays in LOCKSTEP with the real CLI
//! and the §1/§18 BINDING golden path — if a future CLI change (a renamed verb, a
//! dropped `init`) is NOT reflected in the doc, the doc is STALE. A prose file has
//! no compiler, so drift is invisible until an operator copy-pastes a command the
//! binary no longer has. This test is that compiler: it EXTRACTS every
//! `rezidnt <verb> …` invocation from the doc's fenced code blocks and asserts each
//! `<verb>` is a REAL subcommand the shipped CLI recognizes (drives the REAL binary
//! `CARGO_BIN_EXE_rezidnt` with `<verb> --help` and asserts clap does NOT answer
//! with "unrecognized subcommand"). A documented command the CLI lacks FAILS here.
//!
//! ## Strictness — the judge-not-brittle line (documented per the slice contract)
//! Two kinds of assertion, deliberately at DIFFERENT strictness:
//!   - COMMANDS (exact, machine-checked): every `rezidnt <verb>` in a fenced block
//!     MUST be a real CLI verb. This is exact because it is machine-verifiable off
//!     the binary itself — a reword cannot change which verbs are real, so pinning
//!     it exactly is not brittle, it is the anti-drift core. `init` specifically
//!     MUST appear among the doc's commands (the golden-path entry this arc built)
//!     AND be a real verb.
//!   - GOLDEN-PATH ANCHORS (loose, case-insensitive token/substring): the doc must
//!     MENTION each golden-path step — install, `rezidnt init`, reaching a
//!     run/gate/permit (the "first gated run"), and the golden-path bar ("zero
//!     config edits" + "single-digit minutes"). These are matched as
//!     case-insensitive substrings / token alternatives, NOT exact sentences, so a
//!     reworded-but-faithful doc still PASSES while a doc MISSING a golden-path step
//!     FAILS. We never assert an exact prose sentence — that would be brittle
//!     theater that breaks on an editorial pass.
//!
//! The result: DRIFT (a doc verb the CLI lacks) fails; a MISSING step fails; a
//! faithful REWORD passes. That is the judge the slice asks for.
//!
//! ## The §1/§18 golden path this doc is pinned to (authoritative, for the scribe)
//! `docs/rezidnt-architecture.md` §1 line 18 (BINDING): "cold machine → `curl`
//! install → `rezidnt open <repo>` → worktrees allocated, agents spawned under
//! gates, fleet state visible, first verified diff merged — one take, zero config
//! edits, single-digit minutes." §18 (risk register) makes the phase-exit demo the
//! ONLY definition of done. DR-036 makes `rezidnt init` (doctor → spec init → open)
//! the zero-config-edits ENTRY to that path. So the narrated sequence the doc must
//! walk is: curl install → `rezidnt init` (which runs the preflight, generates the
//! spec untouched, and opens the repo) → worktrees/agents under gates → a first
//! verified/gated run — one take, zero config edits, single-digit minutes.
//!
//! ## How commands are extracted from the doc
//! `extract_rezidnt_commands` scans the doc line by line, tracking fenced-block
//! state (a line whose trimmed start is ```` ``` ```` toggles in/out of a fence,
//! regardless of the info string — ```bash / ```text / ```console all count). INSIDE
//! a fence, each line is stripped of a leading shell prompt marker (`$ ` or `# `),
//! then split on whitespace; if the first token is `rezidnt`, the SECOND token is
//! taken as the subcommand verb (skipping tokens that look like `--flags` so an
//! invocation like `rezidnt --json init` still yields `init`). Verbs are collected
//! deduplicated. Prose OUTSIDE fences is never mined for commands (only fenced,
//! copy-pasteable blocks are the contract).
//!
//! ## How this test locates docs/quickstart.md
//! `quickstart_path()` walks UP from `CARGO_MANIFEST_DIR` (this crate lives at
//! `<repo>/bins/rezidnt`) to the repo root and joins `docs/quickstart.md`. It does
//! NOT hardcode an absolute path, so the test is portable across checkouts.
//!
//! ## RED anchor (honesty) — past-tense, contract-true messages
//! When this board was written, `docs/quickstart.md` did NOT exist yet, so
//! `read_quickstart` panics with "docs/quickstart.md not found (quickstart not
//! written yet)" — the honest RED. That message states a CONTRACT (the file must
//! exist), so it stays true after the doc is written: if it ever fires again it is
//! because the doc was deleted. Every OTHER assertion likewise states the CONTRACT
//! it pins ("the quickstart must reference `rezidnt init`", "every rezidnt command
//! in the doc must be a real CLI verb"), so no message makes a false present-tense
//! claim once the doc exists. This file asserts the doc's CONTRACT; it does NOT
//! write the doc (the scribe's job) and does NOT green-pass on an empty/missing doc.
//!
//! Cross-platform on purpose (no `#![cfg(unix)]`, no socket, no daemon): it only
//! reads a file and shells `rezidnt <verb> --help`, so host `/vet` covers it.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// The doc under judgment, relative to the repo root.
const QUICKSTART_REL: &str = "docs/quickstart.md";

/// Locate `docs/quickstart.md` by walking UP from this crate's manifest dir
/// (`<repo>/bins/rezidnt`) to the repo root, then joining the doc path. Portable
/// across checkouts — no absolute path is hardcoded.
fn quickstart_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // <repo>/bins/rezidnt
    let repo_root = manifest_dir
        .parent() // <repo>/bins
        .and_then(|p| p.parent()) // <repo>
        .expect("CARGO_MANIFEST_DIR should have a repo-root grandparent (<repo>/bins/rezidnt)");
    repo_root.join(QUICKSTART_REL)
}

/// Read the quickstart doc, or panic with the honest RED anchor. The panic states
/// the CONTRACT (the doc must exist), so it stays true after the doc is written.
fn read_quickstart() -> String {
    let path = quickstart_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => panic!(
            "{QUICKSTART_REL} not found (quickstart not written yet): expected the operator \
             quickstart at {} — the DR-036 `quickstart` slice must deliver it",
            path.display()
        ),
    }
}

/// Extract the set of `rezidnt <verb>` subcommand verbs referenced inside the doc's
/// FENCED code blocks (```… fences of any info string). Prose outside fences is not
/// mined. A leading shell prompt (`$ ` / `# `) is stripped; leading `--flags` before
/// the verb are skipped so `rezidnt --json init` still yields `init`.
fn extract_rezidnt_commands(doc: &str) -> BTreeSet<String> {
    let mut verbs = BTreeSet::new();
    let mut in_fence = false;
    for raw in doc.lines() {
        let trimmed = raw.trim_start();
        // A fence delimiter toggles block state, whatever the info string.
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }
        // Strip a leading shell prompt marker so `$ rezidnt init` parses.
        let line = trimmed
            .strip_prefix("$ ")
            .or_else(|| trimmed.strip_prefix("# "))
            .unwrap_or(trimmed);
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("rezidnt") {
            continue;
        }
        // The verb is the first non-flag token after `rezidnt`.
        if let Some(verb) = tokens.find(|t| !t.starts_with('-')) {
            // Trim trailing shell punctuation (`,` `;` `\`) a narrated command
            // might carry, so the verb matches the CLI's spelling.
            let verb = verb.trim_end_matches([',', ';', '\\']);
            if !verb.is_empty() {
                verbs.insert(verb.to_string());
            }
        }
    }
    verbs
}

/// Drive the REAL binary with `<verb> --help` and report whether clap RECOGNIZES
/// the subcommand — i.e. it did NOT answer with an "unrecognized subcommand" usage
/// error. A recognized verb prints its help (exit 0) or at worst a different usage
/// error; only clap's unknown-subcommand message means the verb is not real.
fn cli_recognizes_verb(verb: &str) -> bool {
    let out = Command::new(env!("CARGO_BIN_EXE_rezidnt"))
        .arg(verb)
        .arg("--help")
        .output()
        .expect("spawn the rezidnt binary for a --help probe");
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    // clap's phrasing for an unknown verb. If neither appears, the verb resolved.
    !(stderr.contains("unrecognized subcommand")
        || stderr.contains("invalid subcommand")
        || stderr.contains("unexpected argument"))
}

/// Lowercased doc, for case-insensitive anchor matching.
fn lc(doc: &str) -> String {
    doc.to_lowercase()
}

/// Assert the doc contains AT LEAST ONE of the given case-insensitive substrings —
/// a loose golden-path anchor. `anchor` names the step for the failure message; the
/// message states the CONTRACT (the step must be mentioned), staying true post-write.
fn assert_mentions_any(doc_lc: &str, anchor: &str, needles: &[&str]) {
    let hit = needles.iter().any(|n| doc_lc.contains(&n.to_lowercase()));
    assert!(
        hit,
        "quickstart must narrate the golden-path step `{anchor}`: none of {needles:?} \
         appear in {QUICKSTART_REL} (§1/§18 lockstep — a missing golden-path step is drift)"
    );
}

// ===========================================================================
// LOCKSTEP CORE — every `rezidnt <verb>` in the doc is a real CLI verb, and the
// doc uses `rezidnt init` (criterion 2, the anti-drift heart).
// ===========================================================================

/// Every `rezidnt <verb>` referenced in the doc's fenced blocks MUST be a real
/// subcommand the shipped CLI recognizes. A documented command the CLI lacks is
/// DRIFT and fails here. (Exact/machine-checked strictness — see module docs.)
#[test]
fn every_documented_rezidnt_command_is_a_real_cli_verb() {
    let doc = read_quickstart();
    let verbs = extract_rezidnt_commands(&doc);
    assert!(
        !verbs.is_empty(),
        "quickstart must contain copy-pasteable `rezidnt <verb>` commands in fenced blocks, \
         but none were found in {QUICKSTART_REL} (the doc IS the narrated one-take demo — it \
         must show real commands)"
    );
    for verb in &verbs {
        assert!(
            cli_recognizes_verb(verb),
            "quickstart command `rezidnt {verb}` is not a real CLI subcommand — the doc has \
             DRIFTED from the shipped CLI (§1/§18 lockstep): every `rezidnt <verb>` in \
             {QUICKSTART_REL} must be a verb the binary recognizes"
        );
    }
}

/// The doc MUST use `rezidnt init` — the zero-config-edits golden-path entry this
/// DR-036 arc built (Slicing `quickstart` criterion 1) — AND `init` must be a real
/// verb (proven independently, so this cannot false-green if `init` were dropped).
#[test]
fn quickstart_uses_rezidnt_init_and_it_is_a_real_verb() {
    let doc = read_quickstart();
    let verbs = extract_rezidnt_commands(&doc);
    assert!(
        verbs.contains("init"),
        "quickstart must reference `rezidnt init` (the DR-036 zero-config-edits golden-path \
         entry): no `rezidnt init` invocation found in {QUICKSTART_REL}"
    );
    assert!(
        cli_recognizes_verb("init"),
        "the CLI must recognize `rezidnt init` (DR-036 entry verb) — the quickstart is pinned \
         to it; if this fails the wrapper verb regressed out of the binary"
    );
}

// ===========================================================================
// GOLDEN-PATH ANCHORS — the doc narrates every §1/§18 step (loose, reword-safe).
// ===========================================================================

/// The doc walks zero → first gated run: it must MENTION the install step, the
/// `rezidnt init` entry, and reaching a run / gate / permit — each as a loose
/// case-insensitive anchor (reword-safe; a missing step still fails).
#[test]
fn quickstart_narrates_the_golden_path_steps() {
    let doc = read_quickstart();
    let doc_lc = lc(&doc);

    // Install step (§1: "curl install"). A faithful doc may say `curl` or "install".
    assert_mentions_any(&doc_lc, "install", &["curl", "install"]);
    // The golden-path entry (DR-036). Loose: the token `init` in `rezidnt init`.
    assert_mentions_any(
        &doc_lc,
        "rezidnt init",
        &["rezidnt init", "`init`", "init "],
    );
    // Reaching a first GATED run (§1: agents under gates → first verified diff).
    assert_mentions_any(
        &doc_lc,
        "first gated run",
        &["gate", "gated", "permit", "verified"],
    );
}

/// The doc asserts the golden-path BAR (Slicing `quickstart` criterion 2): the
/// "zero config edits" clause and the "single-digit minutes" clause. Loose, so a
/// reworded bar ("no config to edit", "in minutes") still passes; a doc omitting the
/// bar fails.
#[test]
fn quickstart_asserts_the_golden_path_bar() {
    let doc = read_quickstart();
    let doc_lc = lc(&doc);

    // Zero config edits — the clause DR-036 makes independently testable.
    assert_mentions_any(
        &doc_lc,
        "zero config edits",
        &["zero config", "no config", "without editing", "untouched"],
    );
    // Single-digit / few minutes — the golden-path time bar.
    assert_mentions_any(
        &doc_lc,
        "single-digit minutes",
        &["single-digit minutes", "minutes", "single digit"],
    );
}
