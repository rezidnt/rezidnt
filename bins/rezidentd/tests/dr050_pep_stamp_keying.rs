//! TRIALS-SLICE-B ENTRY ORACLE — criterion (a) of DR-050 §Decision 2: the
//! `agent.spawned.pep = "enforced"` stamp must be keyed on
//! `plan.permit_hook_config().is_some()`, NOT on gate-name presence in the
//! spec's gates list.
//!
//! THE DEFECT (DR-050 §Context finding 2, first trap): the daemon derives the
//! stamp from `agent.gates.iter().any(|g| g == "permit")` — the spec's REQUEST
//! to be governed — while the thing the stamp claims is that the PEP was WIRED,
//! which only the plan knows. For claude-code the two signals coincide by
//! construction (`launch_agent` builds the plan via `for_claude_code_permit`
//! exactly when the gates list names `permit`), which is what hides the
//! mis-keying: a future substrate whose PEP-equivalent is not gate-name-shaped
//! can log itself `pep = "enforced"` while running un-intercepted.
//! `SpawnPlan::for_codex` refuses a declared `[gates.permit]` for exactly this
//! reason — the crate closed the trap from its side; the daemon still reads the
//! wrong signal.
//!
//! ## Why the judge is a SOURCE-TEXT guard (disclosure, house style of
//! `registry_convergence_structure.rs`)
//!
//! No behavioral test can go red on this tree: the divergent case (gates name
//! `permit`, plan carries no hook config) is UNREACHABLE through the daemon's
//! public surface, because the same gates-list scan that mis-keys the stamp
//! also selects the plan constructor — the two signals cannot disagree on any
//! spawn the daemon will actually perform today. The fix changes no observable
//! claude-code behavior; only the KEYING changes. So the red judge reads
//! `bins/rezidentd/src/runs.rs` as text, finds the guard of the `"pep"` insert,
//! resolves the binding it keys on, and demands the defining expression derive
//! from `permit_hook_config`. A daemon that laundered the gates scan through an
//! intermediate binding chain deeper than one `let` would slip past — the
//! window is disclosed, not hidden. HOST-RUNNABLE on purpose: the daemon's
//! `mod runs` is `#[cfg(unix)]` and invisible to host `/vet`.
//!
//! ## RED MODE (stated plainly, per test)
//!
//! - `pep_stamp_is_keyed_on_the_plans_permit_hook_config` — ASSERT-RED today:
//!   the guard binding `pep_enforced` is defined from
//!   `agent.gates.iter().any(|g| g == "permit")`, which mentions
//!   `permit_hook_config` nowhere. Green when the stamp's key derives from the
//!   plan.
//! - the two premise guards were never red and say so: they pin, at the
//!   `SpawnPlan` seam, that the two signals ARE distinct (a gates-permit plan
//!   with no hook config is constructible) and that the claude-code permit
//!   path does wire one — the facts the red test's meaning rests on.

use std::path::{Path, PathBuf};

use rezidnt_run::spawner::SpawnPlan;
use rezidnt_run::spec::AgentSpec;

fn runs_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runs.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The condition of the `if` that guards the `"pep"` insert, plus the index the
/// guard starts at (so the binding lookup below can stay left of the use site).
fn pep_stamp_guard(source: &str) -> (String, usize) {
    let insert = source.find(r#"insert("pep""#).expect(
        "runs.rs no longer inserts a \"pep\" key — the stamp site moved; re-anchor this oracle",
    );
    let guard_anchor = "\n    if ";
    let guard_start = source[..insert]
        .rfind(guard_anchor)
        .expect("no `if` guard found above the \"pep\" insert — re-anchor this oracle");
    let guard = &source[guard_start + guard_anchor.len()..insert];
    // The condition runs to the block opener; strip a `&& let` destructure tail
    // (the payload-object borrow, not part of the keying).
    let cond = guard.split('{').next().unwrap_or(guard);
    let cond = cond.split("&& let").next().unwrap_or(cond).trim();
    (cond.to_string(), guard_start)
}

/// The expression the stamp is keyed on: the guard condition itself if it is
/// already an expression, else the right-hand side of the closest preceding
/// `let <binding> =` statement.
fn stamp_key_expression(source: &str, cond: &str, guard_start: usize) -> String {
    if !cond
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '!')
    {
        // The condition is an inline expression — it IS the key.
        return cond.to_string();
    }
    let binding = cond.trim_start_matches('!');
    let needle = format!("let {binding} ");
    let def = source[..guard_start].rfind(&needle).unwrap_or_else(|| {
        panic!("guard condition `{cond}` has no visible `let {binding}` definition — re-anchor this oracle")
    });
    let statement = &source[def..];
    let end = statement
        .find(';')
        .expect("unterminated let statement — re-anchor this oracle");
    statement[..end].to_string()
}

/// DR-050 §Decision 2(a), verbatim criterion: the `pep` stamp is keyed on
/// `plan.permit_hook_config().is_some()`, not gate-name presence in the spec's
/// gates list.
///
/// ASSERT-RED today: the key expression is
/// `agent.gates.iter().any(|g| g == "permit")`.
#[test]
fn pep_stamp_is_keyed_on_the_plans_permit_hook_config() {
    let source = runs_rs();
    let (cond, guard_start) = pep_stamp_guard(&source);
    let key = stamp_key_expression(&source, &cond, guard_start);

    assert!(
        key.contains("permit_hook_config"),
        "DR-050 §Decision 2(a): `agent.spawned.pep = \"enforced\"` must be keyed on \
         `plan.permit_hook_config().is_some()` — the signal that the PEP was actually WIRED \
         at spawn — never on gate-name presence in the spec's gates list, which is only the \
         REQUEST to be governed. A stamp keyed on the request lets a future substrate log \
         itself PEP-enforced while running un-intercepted (the exact trap \
         `SpawnPlan::for_codex` refuses from the crate side). The stamp's key expression \
         today is: `{key}` (guard condition: `{cond}`)"
    );
    assert!(
        !key.contains(".gates"),
        "the pep stamp's key expression still consults the spec's gates list (`{key}`); \
         the gates scan may keep selecting the plan CONSTRUCTOR, but the STAMP must read \
         only the plan (DR-050 §Decision 2(a))"
    );
}

/// PREMISE GUARD (never red — stated plainly): the two signals are genuinely
/// distinct. A plan whose agent's gates list names `permit` but which was built
/// without PEP wiring carries NO hook config — this is the divergent
/// construction DR-050 §Decision 2(a)'s test description names, expressible at
/// the `SpawnPlan` seam even though `launch_agent` cannot reach it today. A
/// stamp keyed on the gates list would claim `pep = "enforced"` for a spawn of
/// exactly this plan; a stamp keyed on the plan stays honestly silent.
#[test]
fn a_gates_permit_plan_without_hook_config_is_constructible() {
    let agent = AgentSpec {
        name: "impl".into(),
        harness: "claude-code".into(),
        worktree: "auto".into(),
        gates: vec!["permit".into()],
        ..AgentSpec::default()
    };
    let plan = SpawnPlan::for_claude_code(&agent, "badge", std::env::vars());
    assert!(
        plan.permit_hook_config().is_none(),
        "premise: the base claude-code plan never wires a PEP, whatever the gates list says \
         — if this changed, the divergence the red test guards against no longer exists and \
         this oracle must be re-derived"
    );
}

/// PREMISE GUARD (never red — stated plainly): the claude-code permit path DOES
/// wire a hook config for a permit-gated agent, so re-keying the stamp on the
/// plan keeps the existing claude-code behavior byte-identical: enforced runs
/// still stamp `pep = "enforced"`, and the honest-absence discipline for
/// non-permit runs is untouched (DR-012 declared-vs-absent).
#[test]
fn the_permit_path_wires_a_hook_config_so_rekeying_preserves_claude_code() {
    let agent = AgentSpec {
        name: "impl".into(),
        harness: "claude-code".into(),
        worktree: "auto".into(),
        gates: vec!["permit".into()],
        ..AgentSpec::default()
    };
    let plan = SpawnPlan::for_claude_code_permit(
        &agent,
        "badge",
        std::env::vars(),
        "run-ulid",
        Path::new("/tmp/rezidnt.sock").to_str().unwrap_or("sock"),
    );
    assert!(
        plan.permit_hook_config().is_some(),
        "premise: a permit-gated claude-code plan carries the PreToolUse hook config — \
         the plan-keyed stamp reads `enforced` from exactly this"
    );
}
