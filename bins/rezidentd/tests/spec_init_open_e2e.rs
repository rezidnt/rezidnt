//! DR-036 sub-slice `spec-init` ORACLE — CRITERION 2, the BINDING
//! "generated-file-untouched" clause, END-TO-END: a spec produced by
//! `rezidnt spec init --defaults` is handed to `rezidnt open` (the socket `open`
//! op) and reaches a first `agent.spawned` on the fabric. This needs a LIVE
//! daemon, so it is `#![cfg(unix)]` and lives in the daemon crate's tests
//! (where `rezidnt_testkit` + the sibling-binary locator are available). Modeled
//! on `operator_liveops_e2e.rs` / `permit_live_unblock.rs`.
//!
//! ## Why the CLI, not a hand-authored spec
//! The whole point of criterion 2 is that the GENERATOR's output — not a snapshot
//! of §13 prose — drives `open`. So this shells out to the REAL
//! `rezidnt spec init --defaults` binary (`cli_bin()`) to write the spec, then
//! opens THAT file and tails for `agent.spawned`. A generator that emits a spec
//! `open` rejects fails here (DR-036 §Consequences: "the generator is pinned to
//! the consumer, not to a snapshot of §13").
//!
//! ## The honest stub-harness seam (discussed, per the work order)
//! The DEFAULT spec `spec init --defaults` emits is a REALISTIC minimal operator
//! spec: `harness = "claude-code"` with NO `bin_override`. On a CI/test box there
//! is no real `claude` binary, so that spec would not actually spawn a process —
//! which is a property of the test ENVIRONMENT, not a generator defect. The
//! sibling live tests (`permit_live_unblock.rs`, `operator_liveops_e2e.rs`) all
//! spawn via a `bin_override` stub harness for exactly this reason.
//!
//! So this test takes the HONEST middle path the work order sanctions: it keeps
//! everything the generator OWNS (the `[project]` shape, the `[[agent]]`
//! name/harness/worktree, the file the operator would `open`) and substitutes only
//! the two ENVIRONMENT-bound values a headless test box cannot take at face value.
//! First, `repo = "."` becomes this tempdir's absolute path — the generator's "."
//! means "the operator's project dir", but the daemon canonicalizes `spec.repo`
//! against ITS OWN cwd (`runs.rs`), and the testkit daemon's cwd is the
//! `-p rezidentd` runner dir (the source tree), NOT this tempdir; resolving "." to
//! the scaffolded tempdir is what makes the test open the OPERATOR's project
//! (honest criterion 2) instead of the daemon's cwd repo, and keeps
//! `.rezidnt/worktrees` out of the tracked source tree (see `splice_repo`).
//! Second, `harness = "claude-code"` gains a `bin_override` stub harness the box
//! can spawn — there is no real `claude` binary on CI (see `splice_bin_override`).
//! Both are the smallest deviations that let a headless box reach `agent.spawned`
//! deterministically; the generator's §13 SHAPE drives the materialization. The
//! parse-shape fidelity of the whole generated file (nothing spliced away) is
//! pinned host-side in
//! `bins/rezidnt/tests/spec_init_cli.rs::defaults_writes_parseable_section13_spec`.
//! The load-bearing assertion is: the GENERATED spec, materialized by `open`,
//! reaches `agent.spawned`.
//!
//! Cross-platform: `#![cfg(unix)]` (needs the daemon + UDS). Host `/vet` cannot
//! reach this; lint/run on WSL per the project's host-vs-WSL rule.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{cli_bin, connect, read_until, send_line, start_daemon};
use serde_json::json;

const TAIL_DEADLINE: Duration = Duration::from_secs(20);

/// Run `rezidnt spec init --defaults <dir>` via the REAL CLI binary and return
/// (exit, stderr). No daemon env is needed — the generator is purely local (that
/// independence is criterion 4's job, pinned host-side); here we only need the
/// file it writes.
fn cli_spec_init_defaults(dir: &Path) -> (Option<i32>, String) {
    let out = Command::new(cli_bin())
        .arg("spec")
        .arg("init")
        .arg("--defaults")
        .arg(dir.to_str().expect("utf8 dir"))
        .output()
        .expect("spawn rezidnt spec init --defaults");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Splice a `bin_override = "<stub>"` line into the FIRST `[[agent]]` table of a
/// generated spec so a headless test box can actually spawn (see the module doc's
/// "honest stub-harness seam"). Everything else the generator emitted is left
/// verbatim. Inserts the line immediately after the `[[agent]]` header.
fn splice_bin_override(spec_toml: &str, stub: &Path) -> String {
    let inject = format!("bin_override = \"{}\"\n", stub.display());
    let mut out = String::with_capacity(spec_toml.len() + inject.len());
    let mut injected = false;
    for line in spec_toml.lines() {
        out.push_str(line);
        out.push('\n');
        if !injected && line.trim() == "[[agent]]" {
            out.push_str(&inject);
            injected = true;
        }
    }
    assert!(
        injected,
        "the generated spec must contain an [[agent]] table to splice a stub harness into — \
         generator emitted no [[agent]]:\n{spec_toml}"
    );
    out
}

/// Rewrite the `[project]` `repo = …` line to `abs` (an ABSOLUTE path). The
/// generator emits `repo = "."` — correct for the PRODUCT (the operator runs
/// `open` from their project dir, where "." is that project). But the daemon
/// canonicalizes `spec.repo` against ITS OWN cwd (`runs.rs`: "relative paths
/// resolve against the daemon cwd in S1"), and the testkit daemon inherits the
/// `-p rezidentd` runner cwd — the tracked source tree, NOT this tempdir. So to
/// honestly open the OPERATOR's scaffolded project (this tempdir) — and to keep
/// the daemon from materializing worktrees into the source tree — we express what
/// "." MEANS in the operator's context by resolving it to the tempdir's absolute
/// path. This is the same class of environment substitution as `splice_bin_override`
/// (the harness executable the headless box lacks): the generator's §13 SHAPE is
/// untouched; only the two environment-bound values ("." → this box's project
/// dir; `claude-code` → a spawnable stub) are made real. Replaces only the FIRST
/// `repo =` line (the `[project]` one; the default spec has no other).
fn splice_repo(spec_toml: &str, abs: &Path) -> String {
    let line = format!("repo = \"{}\"", abs.display());
    let mut out = String::with_capacity(spec_toml.len() + line.len());
    let mut replaced = false;
    for src in spec_toml.lines() {
        if !replaced && src.trim_start().starts_with("repo =") {
            out.push_str(&line);
            out.push('\n');
            replaced = true;
        } else {
            out.push_str(src);
            out.push('\n');
        }
    }
    assert!(
        replaced,
        "the generated spec must carry a `[project]` repo line to point at the scaffolded \
         project — generator emitted none:\n{spec_toml}"
    );
    out
}

/// Open a spec (as a TOML string) over the bare socket and tail until
/// `agent.spawned`; return the spawned run ulid. Mirrors the sibling live tests'
/// `open_and_get_run`.
fn open_and_get_run(socket: &Path, spec: &str) -> String {
    let mut opener = connect(socket);
    send_line(
        &mut opener,
        &serde_json::to_string(&json!({"op": "open", "spec_toml": spec})).unwrap(),
    );
    let mut tail = connect(socket);
    send_line(&mut tail, r#"{"op":"tail"}"#);
    let lines = read_until(&mut tail, TAIL_DEADLINE, |v| {
        v["subject"] == "agent.spawned"
    });
    lines
        .iter()
        .find(|v| v["subject"] == "agent.spawned")
        .expect("agent.spawned on the fabric")["payload"]["run"]
        .as_str()
        .expect("agent.spawned carries the run ulid")
        .to_string()
}

/// CRITERION 2 — the BINDING "generated file untouched" clause, end to end. Drive
/// the REAL `rezidnt spec init --defaults` to write the spec; hand THAT generated
/// file to `rezidnt open`; assert it reaches a first `agent.spawned` on the
/// fabric. Only the two ENVIRONMENT-bound values are substituted (the generator's
/// §13 SHAPE is untouched, driving the materialization): `repo = "."` → this
/// tempdir's absolute path (so `open` materializes the OPERATOR's scaffolded
/// project, not the daemon's cwd — see `splice_repo` + the module doc) and
/// `harness = "claude-code"` → a spawnable `bin_override` stub (see
/// `splice_bin_override`).
#[test]
fn generated_default_spec_opens_and_spawns() {
    let daemon = start_daemon();

    // 1. GENERATE: drive the real `rezidnt spec init --defaults` into a temp dir.
    let dir = tempfile::tempdir().expect("tempdir");
    let (code, stderr) = cli_spec_init_defaults(dir.path());
    assert_eq!(
        code,
        Some(0),
        "rezidnt spec init --defaults must succeed and write the spec; stderr: {stderr}"
    );
    let spec_path = dir.path().join("rezidnt.toml");
    let generated = std::fs::read_to_string(&spec_path).unwrap_or_else(|e| {
        panic!(
            "spec init --defaults must WRITE {} — got none (subcommand absent?): {e}; \
             stderr: {stderr}",
            spec_path.display()
        )
    });

    // 2. Make the generated spec's two ENVIRONMENT-bound values real for a headless
    // box, leaving its §13 SHAPE untouched: (a) resolve `repo = "."` to THIS
    // tempdir (the operator's scaffolded project) so `open` materializes the
    // scaffolded repo — NOT the daemon's cwd — and confines `.rezidnt/worktrees`
    // to the tempdir; (b) splice a `bin_override` stub harness the box can spawn.
    // Both are documented environment substitutions; the generated project/agent
    // shape drives the materialization.
    let canonical_dir = std::fs::canonicalize(dir.path()).expect("canonicalize tempdir");
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&canonical_dir)
        .status()
        .expect("git init the scaffolded project dir");
    assert!(git.success(), "git init the scaffolded repo dir");

    let stub = common::stub_harness(dir.path(), 4_000);
    let spec_for_open = splice_bin_override(&splice_repo(&generated, &canonical_dir), &stub);

    // 3. OPEN the generated spec and tail for the first agent.spawned — the
    // materialization proof (the "generated file untouched" BINDING clause).
    let run = open_and_get_run(&daemon.socket, &spec_for_open);
    assert_eq!(
        run.len(),
        26,
        "the generated spec, opened, reached a first agent.spawned carrying a 26-char run \
         ULID — the golden-path 'generated file untouched' clause holds end to end: run={run:?}"
    );
}
