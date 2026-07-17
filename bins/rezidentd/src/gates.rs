//! S4 gate engine wiring in the daemon (doc §8 — the differentiation layer).
//!
//! The pure verdict logic lives in `rezidnt-gate` (the `NativeVerifier`
//! trait, the exec runner, the §8 verdict contract, and debrief replay). This
//! module is the *daemon-side lifecycle*: it pins the CAS-hashed inputs a
//! verifier receives, runs the configured verifier set for a gate, emits the
//! `gate.entered`/`gate.passed`/`gate.failed`/`gate.inconclusive` facts onto
//! the fabric in causal order, and — for `pre_merge` on the golden path —
//! performs the git-CLI merge mutation that emits `diff.merged`.
//!
//! Determinism (BINDING): a verifier never receives a mutable path — it
//! receives a `cas:blake3:<hex>` ref pinned by content hash (§8). Evidence
//! blobs are CAS refs, never bytes on the fact (I2). `inconclusive` is never
//! coerced (I6).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use rezidnt_gate::{
    AllowedTools, BareMode, DiffScope, ExecVerifier, ForbiddenPath, GateDef, NativeVerifier,
    PinnedVersion, Verdict, VerdictRecord, VerifierInput,
};
use rezidnt_run::spec::{AgentSpec, GateSpec, VerifierSpec, agent_spec_toml};
use rezidnt_types::{Event, SourceId, Subject, WorkspaceId};
use serde_json::{Value, json};
use ulid::Ulid;

use crate::runs::{Daemon, publish};

/// The verdict a gate reached and the id of its terminal verdict fact.
pub struct GateOutcome {
    pub verdict: Verdict,
    /// The id of the terminal verdict fact (`gate.passed`/`failed`/
    /// `inconclusive`), for causation chaining of what follows.
    pub verdict_id: Ulid,
}

/// Look up a native verifier by name (the v1 built-in pack).
fn native_by_name(name: &str) -> Option<Box<dyn NativeVerifier>> {
    match name {
        "diff-scope" => Some(Box::new(DiffScope)),
        "forbidden-path" => Some(Box::new(ForbiddenPath)),
        "bare-mode" => Some(Box::new(BareMode)),
        "pinned-version" => Some(Box::new(PinnedVersion)),
        "allowed-tools" => Some(Box::new(AllowedTools)),
        _ => None,
    }
}

/// The §8 `inputs` document as a JSON value recorded verbatim on the fact.
fn inputs_value(input: &VerifierInput) -> Value {
    serde_json::to_value(input).unwrap_or_else(|_| json!({}))
}

/// Run one native verifier against a CAS-pinned input, producing the recorded
/// verdict + timing. Blocking CAS IO runs on a blocking thread.
async fn run_native(
    daemon: &Arc<Daemon>,
    name: String,
    input: VerifierInput,
) -> anyhow::Result<VerdictRecord> {
    let cas = Arc::clone(&daemon.cas);
    let started = std::time::Instant::now();
    let out = tokio::task::spawn_blocking(move || {
        let native = native_by_name(&name)
            .unwrap_or_else(|| unreachable!("native verifier {name} must be a built-in"));
        native.verify(&input, &cas)
    })
    .await
    .context("native verify task panicked")?
    .context("native verify")?;
    let cost_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    // A native's own cost is 0 (it does not self-report); the engine records
    // the wall-clock it took (the "recorded cost" the exit asserts).
    let cost_ms = out.cost_ms.max(cost_ms).max(1);
    // Find the native's name again for the record.
    Ok(VerdictRecord {
        verifier: String::new(), // filled by the caller (it owns the name)
        verdict: out.verdict,
        reason: verdict_reason(&out),
        evidence: out.evidence,
        cost_ms,
    })
}

/// A native's inconclusive reason is carried in its evidence `kind` (the
/// engine maps a cannot-run to `malformed_output`-adjacent honesty); natives
/// only ever pass/fail/inconclusive-cannot-run, so map accordingly.
fn verdict_reason(out: &rezidnt_gate::VerifierOutput) -> Option<rezidnt_gate::InconclusiveReason> {
    if out.verdict == Verdict::Inconclusive {
        // A native that cannot run (missing CAS blob) is malformed-input class.
        Some(rezidnt_gate::InconclusiveReason::MalformedOutput)
    } else {
        None
    }
}

/// Run one gate: emit `gate.entered`, execute the verifier set in order
/// (first failure short-circuits, §8), emit the terminal verdict fact, and
/// return the outcome. `refs` is the shared CAS-ref map every verifier's
/// input carries (e.g. `{"spec": …}` for vet, `{"diff": …}` for pre_merge);
/// each verifier's own `params` come from its spec entry.
#[allow(clippy::too_many_arguments)]
pub async fn run_gate(
    daemon: &Arc<Daemon>,
    workspace: WorkspaceId,
    correlation: Ulid,
    causation: Option<Ulid>,
    run: &str,
    gate_name: &str,
    refs: BTreeMap<String, String>,
    verifiers: &[ResolvedVerifier],
) -> anyhow::Result<GateOutcome> {
    let def = GateDef {
        name: gate_name.to_string(),
        ..GateDef::default()
    };

    // gate.entered — the lifecycle fact precedes every verdict.
    let entered_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("rezidnt-gate"),
            Some(workspace),
            Subject::new("gate.entered"),
            correlation,
            causation,
            1,
            json!({"run": run, "gate": gate_name}),
        )?,
    )
    .await?;

    let mut passed_verifiers: Vec<Value> = Vec::new();

    for resolved in verifiers {
        let input = def.input_for(Some(run.to_string()), refs.clone(), resolved.params.clone());
        let inputs_doc = inputs_value(&input);

        let mut record = match &resolved.kind {
            VerifierKind::Native(name) => {
                let mut r = run_native(daemon, name.clone(), input).await?;
                r.verifier = name.clone();
                r
            }
            VerifierKind::Exec(argv) => {
                ExecVerifier {
                    name: resolved.name.clone(),
                    argv: argv.clone(),
                }
                .run(&input)
                .await
            }
        };
        // Belt-and-suspenders: an exec verifier's record already carries its
        // own name; a native's is set above.
        if record.verifier.is_empty() {
            record.verifier = resolved.name.clone();
        }

        match record.verdict {
            Verdict::Pass => {
                passed_verifiers.push(json!({
                    "verifier": record.verifier,
                    "cost_ms": record.cost_ms,
                    "evidence": evidence_refs(&record),
                    "inputs": inputs_doc,
                }));
            }
            Verdict::Fail => {
                let verdict_id = publish(
                    &daemon.fabric,
                    Event::new(
                        SourceId::new("rezidnt-gate"),
                        Some(workspace),
                        Subject::new("gate.failed"),
                        correlation,
                        Some(entered_id),
                        1,
                        json!({
                            "run": run,
                            "gate": gate_name,
                            "verifier": record.verifier,
                            "evidence": evidence_refs(&record),
                            "inputs": inputs_doc,
                        }),
                    )?,
                )
                .await?;
                return Ok(GateOutcome {
                    verdict: Verdict::Fail,
                    verdict_id,
                });
            }
            Verdict::Inconclusive => {
                let reason = match record.reason {
                    Some(rezidnt_gate::InconclusiveReason::Timeout) => "timeout",
                    Some(rezidnt_gate::InconclusiveReason::NonzeroExit) => "nonzero_exit",
                    Some(rezidnt_gate::InconclusiveReason::CouldNotRun) => "could_not_run",
                    Some(rezidnt_gate::InconclusiveReason::MalformedOutput) | None => {
                        "malformed_output"
                    }
                };
                let verdict_id = publish(
                    &daemon.fabric,
                    Event::new(
                        SourceId::new("rezidnt-gate"),
                        Some(workspace),
                        Subject::new("gate.inconclusive"),
                        correlation,
                        Some(entered_id),
                        1,
                        json!({
                            "run": run,
                            "gate": gate_name,
                            "verifier": record.verifier,
                            "reason": reason,
                            "evidence": evidence_refs(&record),
                            "inputs": inputs_doc,
                        }),
                    )?,
                )
                .await?;
                return Ok(GateOutcome {
                    verdict: Verdict::Inconclusive,
                    verdict_id,
                });
            }
        }
    }

    // Every verifier passed — gate.passed carries ALL records (asymmetry with
    // gate.failed, which carries the one failing verifier).
    let verdict_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("rezidnt-gate"),
            Some(workspace),
            Subject::new("gate.passed"),
            correlation,
            Some(entered_id),
            1,
            json!({"run": run, "gate": gate_name, "verifiers": passed_verifiers}),
        )?,
    )
    .await?;
    Ok(GateOutcome {
        verdict: Verdict::Pass,
        verdict_id,
    })
}

/// Evidence blob hashes rendered as `CasRef`-shaped objects for the fact
/// (`{hash, bytes, mime}`), from a verifier record's `cas:blake3:<hex>` refs.
fn evidence_refs(record: &VerdictRecord) -> Vec<Value> {
    record
        .evidence
        .iter()
        .filter_map(|e| e.cas_ref.as_deref())
        .filter_map(|r| r.strip_prefix("cas:blake3:"))
        .map(|hash| json!({"hash": hash, "bytes": 0, "mime": "text/plain"}))
        .collect()
}

/// A verifier resolved from its spec entry into an executable form.
pub struct ResolvedVerifier {
    pub name: String,
    pub kind: VerifierKind,
    pub params: Value,
}

pub enum VerifierKind {
    Native(String),
    Exec(Vec<String>),
}

/// Resolve a gate's `[gates.<name>]` verifier specs into runnable verifiers.
/// An entry naming an unknown native is skipped with a warning (never a
/// silent pass — the gate simply has fewer verifiers, and vet/pre_merge each
/// build their own default set below).
pub fn resolve_verifiers(gate: &GateSpec) -> Vec<ResolvedVerifier> {
    let mut out = Vec::new();
    for v in &gate.verifiers {
        if let Some(resolved) = resolve_one(v) {
            out.push(resolved);
        }
    }
    out
}

fn resolve_one(v: &VerifierSpec) -> Option<ResolvedVerifier> {
    if let Some(name) = &v.native {
        if native_by_name(name).is_none() {
            tracing::warn!(verifier = %name, "unknown native verifier in gate spec; skipping");
            return None;
        }
        Some(ResolvedVerifier {
            name: name.clone(),
            kind: VerifierKind::Native(name.clone()),
            params: v.params.clone(),
        })
    } else if let Some(exec) = &v.exec {
        let name = v.name.clone().unwrap_or_else(|| exec.display().to_string());
        Some(ResolvedVerifier {
            name,
            kind: VerifierKind::Exec(vec![exec.display().to_string()]),
            params: v.params.clone(),
        })
    } else {
        tracing::warn!("verifier spec names neither `native` nor `exec`; skipping");
        None
    }
}

/// The three vet natives, in the pinned order (bare-mode, pinned-version,
/// allowed-tools). Vet is a fixed policy point — its verifier set is not
/// spec-configurable in v1 (the agent's governed fields ARE the inputs).
pub fn vet_verifiers() -> Vec<ResolvedVerifier> {
    ["bare-mode", "pinned-version", "allowed-tools"]
        .into_iter()
        .map(|name| ResolvedVerifier {
            name: name.to_string(),
            kind: VerifierKind::Native(name.to_string()),
            params: json!({}),
        })
        .collect()
}

/// Pin the agent-spec TOML into the CAS and return its `cas:blake3:<hex>`
/// ref — the vet gate's `refs["spec"]` (determinism BINDING).
pub async fn pin_agent_spec(daemon: &Arc<Daemon>, agent: &AgentSpec) -> anyhow::Result<String> {
    let toml = agent_spec_toml(agent);
    let cas = Arc::clone(&daemon.cas);
    let ref_ = tokio::task::spawn_blocking(move || cas.put(toml.as_bytes(), "application/toml"))
        .await
        .context("cas put task panicked")?
        .context("pin agent spec into cas")?;
    Ok(format!("cas:blake3:{}", ref_.hash))
}

/// Summarize a worktree's working-tree changes into the CAS and return the
/// `cas:blake3:<hex>` ref — the pre_merge gate's `refs["diff"]`. The summary
/// format matches the S2 git-adapter `diff.ready` shape: one
/// `<status>\t<path>` line per touched file (deterministic, content-stable).
pub async fn summarize_worktree(
    daemon: &Arc<Daemon>,
    worktree: &Path,
) -> anyhow::Result<(String, rezidnt_types::refs::CasRef)> {
    let summary = git_diff_summary(worktree).await?;
    let cas = Arc::clone(&daemon.cas);
    let bytes = summary.into_bytes();
    let r = tokio::task::spawn_blocking(move || cas.put(&bytes, "text/x-diff"))
        .await
        .context("cas put task panicked")?
        .context("pin diff summary into cas")?;
    let ref_str = format!("cas:blake3:{}", r.hash);
    Ok((ref_str, r))
}

/// Render the deterministic diff summary for a worktree via git-CLI
/// (`git status --porcelain`), one `<status>\t<path>` line per touched file,
/// sorted for determinism. The tab-delimited shape is the format the native
/// verifiers and the S2 fixtures pin.
async fn git_diff_summary(worktree: &Path) -> anyhow::Result<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .await
        .context("run git status (is git on PATH?)")?;
    anyhow::ensure!(
        out.status.success(),
        "git status failed in {}: {}",
        worktree.display(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let porcelain = String::from_utf8_lossy(&out.stdout);
    let mut lines: Vec<String> = Vec::new();
    for line in porcelain.lines() {
        if line.len() < 4 {
            continue;
        }
        // Porcelain v1: `XY <path>` (X = index, Y = worktree). Collapse to a
        // single status letter, tab, path.
        let xy = &line[..2];
        let path = line[3..].trim();
        let letter = porcelain_letter(xy);
        lines.push(format!("{letter}\t{path}"));
    }
    lines.sort();
    lines.dedup();
    Ok(lines.join("\n") + if lines.is_empty() { "" } else { "\n" })
}

/// Map a porcelain-v1 `XY` status pair to a single summary status letter.
fn porcelain_letter(xy: &str) -> char {
    let bytes = xy.as_bytes();
    // Untracked (`??`) and added map to A; deletions D; else M.
    if xy == "??" || bytes.contains(&b'A') {
        'A'
    } else if bytes.contains(&b'D') {
        'D'
    } else {
        'M'
    }
}

/// Merge a worktree's committed change into the repo's checked-out branch via
/// git-CLI, then emit `diff.merged` (the golden-path worktree-lifecycle-close
/// fact). Returns the `diff.merged` event id.
///
/// Mutation shape: commit every change in the worktree onto its own head, then
/// `git merge` that head into the repo's current branch. The change reaches the
/// repo's checked-in history (the golden-path exit reads `git show HEAD:…`).
#[allow(clippy::too_many_arguments)]
pub async fn merge_worktree(
    daemon: &Arc<Daemon>,
    workspace: WorkspaceId,
    correlation: Ulid,
    causation: Option<Ulid>,
    run: &str,
    repo: &Path,
    worktree: &Path,
    diff_ref: &rezidnt_types::refs::CasRef,
) -> anyhow::Result<Ulid> {
    // 1. Stage + commit the worktree's change onto its head (detached or a
    //    rezidnt/<agent> branch — either way it becomes a mergeable commit).
    git_in(worktree, &["add", "-A"]).await?;
    // A worktree with nothing to commit is not a merge — but the golden path
    // always writes a change, so an empty commit here would be a defect.
    git_in(
        worktree,
        &[
            "-c",
            "user.email=rezidnt@localhost",
            "-c",
            "user.name=rezidnt",
            "commit",
            "-q",
            "-m",
            "rezidnt: verified merge",
        ],
    )
    .await?;
    let head = git_in(worktree, &["rev-parse", "HEAD"]).await?;
    let head = head.trim().to_string();

    // 2. Merge that commit into the repo's checked-out branch.
    git_in(
        repo,
        &[
            "-c",
            "user.email=rezidnt@localhost",
            "-c",
            "user.name=rezidnt",
            "merge",
            "--no-ff",
            "-q",
            "-m",
            "rezidnt: verified merge",
            &head,
        ],
    )
    .await?;

    // 3. diff.merged — closes the worktree lifecycle (folded to status
    //    "merged" by the S4 reducer).
    let merged_id = publish(
        &daemon.fabric,
        Event::new(
            SourceId::new("rezidnt-adapter-git"),
            Some(workspace),
            Subject::new("diff.merged"),
            correlation,
            causation,
            1,
            json!({
                "run": run,
                "worktree": worktree.display().to_string(),
                "diff": diff_ref,
            }),
        )?,
    )
    .await?;
    Ok(merged_id)
}

/// Run one git-CLI command in `dir`, returning its trimmed stdout.
async fn git_in(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .await
        .with_context(|| format!("run git {args:?}"))?;
    anyhow::ensure!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
