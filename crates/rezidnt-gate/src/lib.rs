//! rezidnt gate engine (doc §8 — the differentiation layer; over-invest
//! here).
//!
//! ## S4 engine (implemented against the oracle board)
//!
//! The oracle scaffolded the type shapes; this crate now carries the real
//! behavior: strict §8 stdout parsing, the native pack, the exec runner, and
//! debrief replay. The shapes are pinned by the board and by the §8 BINDING
//! contract:
//!
//! - Verdicts are `pass | fail | inconclusive` — NEVER a bare boolean, never
//!   coerced (I6). `inconclusive` carries a reason
//!   (`timeout | nonzero_exit | malformed_output`).
//! - Two verifier kinds (BINDING): *native* ([`NativeVerifier`] trait) and
//!   *exec* ([`ExecVerifier`], any argv program speaking the §8 JSON contract
//!   over stdin/stdout).
//! - Determinism (BINDING): inputs pinned by content hash — verifiers receive
//!   CAS refs (`cas:blake3:<hex>` strings), not mutable paths; no network by
//!   default; 120 s DEFAULT timeout; nonzero exit or malformed output =
//!   `inconclusive`, never `pass`. Evidence blobs go to the CAS; events carry
//!   refs only (I2).
//! - Replay (the compliance sentence): [`replay`] re-executes recorded
//!   verdicts from log + CAS; divergence between recorded and replayed
//!   verdict is an [`IntegrityAlarm`] — verifier nondeterminism or log
//!   tampering, never silently reconciled.
//!
//! Exec verifiers run with a scrubbed environment (doc §12) and, in v1,
//! network disabled unless the gate def opts in — the opt-in is recorded, the
//! plumbing beyond recording is out of S4 scope.

pub mod permit;

use std::collections::BTreeMap;
use std::time::Duration;

use rezidnt_cas::Cas;
use rezidnt_types::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::Instrument;

/// Wall-clock timeout DEFAULT for a verifier (doc §8 BINDING: 120 s).
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// The verdict vocabulary (BINDING, I6): three-valued, never a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
    Inconclusive,
}

/// Why a verdict is `inconclusive` (ontology `gate.inconclusive` v1
/// `reason` vocabulary; new causes arrive additively as strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InconclusiveReason {
    Timeout,
    NonzeroExit,
    MalformedOutput,
    /// The verifier could not be run at all (spawn failed, or a wait-io error
    /// before any output): nothing executed, so "malformed output" would be
    /// untruthful. Distinguishes "your argv is wrong" from "your program
    /// printed garbage." Additive value in the `gate.inconclusive` reason vocab.
    CouldNotRun,
}

/// One evidence item (§8 stdout contract). `ref` is a `cas:blake3:<hex>`
/// string when the evidence blob lives in the CAS (I2: refs, never bytes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,
    pub msg: String,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub cas_ref: Option<String>,
}

/// The §8 stdin document — the EXACT bytes a verifier receives, recorded
/// verbatim on the verdict fact (ontology: `inputs`), returned verbatim by
/// `gate_explain` (I6 interrogability).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierInput {
    pub gate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Content-hash-pinned inputs: name → `cas:blake3:<hex>` string.
    pub refs: BTreeMap<String, String>,
    pub params: Value,
    pub timeout_ms: u64,
}

/// The §8 stdout document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierOutput {
    pub verdict: Verdict,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    pub cost_ms: u64,
}

/// What the engine records for one verifier execution: the §8 output plus
/// the engine-side honesty fields (`reason` when inconclusive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictRecord {
    pub verifier: String,
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<InconclusiveReason>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    pub cost_ms: u64,
}

/// Gate-engine errors (thiserror per lib convention). NOTE the deliberate
/// asymmetry: a verifier that cannot run or cannot decide is NOT an error —
/// it is an `inconclusive` verdict. Errors are engine defects only.
#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("cas: {0}")]
    Cas(#[from] rezidnt_cas::CasError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed verifier output: {0}")]
    Malformed(String),
    #[error("replay: {0}")]
    Replay(String),
}

/// Strict parse of a §8 stdout document. Anything that is not exactly the
/// contract — non-JSON, a bare boolean verdict, an unknown verdict string —
/// is `Err` (the engine maps that to `inconclusive { malformed_output }`,
/// NEVER to pass).
///
/// The verdict enum's serde derive is the strictness: `#[serde(rename_all =
/// "lowercase")]` accepts EXACTLY `pass|fail|inconclusive`. A boolean, a
/// near-miss string (`passed`, `PASS`, `ok`), prose, or empty input all fail
/// the deserialize — there is no path from garbage to a synthesized verdict.
pub fn parse_verifier_output(stdout: &[u8]) -> Result<VerifierOutput, GateError> {
    let text = std::str::from_utf8(stdout).map_err(|e| GateError::Malformed(e.to_string()))?;
    serde_json::from_str::<VerifierOutput>(text).map_err(|e| GateError::Malformed(e.to_string()))
}

/// A gate definition: a named policy point plus its execution policy.
/// `Default` MUST pin the BINDING defaults (network off, 120 s timeout) —
/// the derive is deliberately absent so the implementer writes it against
/// the red `gate_def_defaults` test.
#[derive(Debug, Clone, PartialEq)]
pub struct GateDef {
    /// `vet` | `pre_merge` | `post_run` — a string, not a closed enum.
    pub name: String,
    /// Exec verifiers run with network disabled unless this opts in;
    /// the opt-in is recorded in the gate event (doc §8).
    pub network: bool,
    pub timeout_ms: u64,
}

impl Default for GateDef {
    fn default() -> Self {
        // BINDING (doc §8): no network by default, 120 s wall-clock timeout.
        Self {
            name: String::new(),
            network: false,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

impl GateDef {
    /// Build the §8 stdin document for one verifier run: refs are CAS-ref
    /// strings (inputs pinned by content hash, BINDING), the timeout is this
    /// def's (DEFAULT 120 s).
    pub fn input_for(
        &self,
        workspace: Option<String>,
        refs: BTreeMap<String, String>,
        params: Value,
    ) -> VerifierInput {
        VerifierInput {
            gate: self.name.clone(),
            workspace,
            refs,
            params,
            timeout_ms: self.timeout_ms,
        }
    }
}

/// A native verifier (BINDING kind 1): a deterministic Rust check. Same
/// content-hashed inputs → same verdict and same evidence, every time.
/// Evidence blobs are written to `cas`; the returned [`Evidence`] carries
/// refs. A missing input blob is an `Inconclusive` OUTPUT, not an `Err`.
pub trait NativeVerifier {
    fn name(&self) -> &'static str;
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError>;
}

/// Resolve a `cas:blake3:<hex>` ref string to its blob. A MISSING blob is a
/// can't-run signal (`Ok(None)`), never an engine error — the caller maps it
/// to `inconclusive` (I6 honesty). A malformed ref string is likewise a
/// can't-run.
fn resolve_ref(cas: &Cas, ref_str: &str) -> Result<Option<Vec<u8>>, GateError> {
    let Some(hash) = ref_str.strip_prefix("cas:blake3:") else {
        return Ok(None);
    };
    let want = rezidnt_types::refs::CasRef {
        hash: hash.to_string(),
        bytes: 0,
        mime: String::new(),
    };
    match cas.get(&want) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(rezidnt_cas::CasError::NotFound { .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The `inconclusive { malformed_output }`-adjacent verdict a native returns
/// when its pinned input blob is absent: it cannot decide, so it says so.
fn cannot_run(msg: &str) -> VerifierOutput {
    VerifierOutput {
        verdict: Verdict::Inconclusive,
        evidence: vec![Evidence {
            kind: "cannot-run".to_string(),
            msg: msg.to_string(),
            cas_ref: None,
        }],
        cost_ms: 0,
    }
}

/// Extract the repo-relative path from one diff-summary line, tolerating both
/// the oracle fixture format (`M\tsrc/x.rs`) and the S2 git-adapter format
/// (`M src/x.rs blake3:<hex>`, with a leading `# rezidnt diff summary v1`
/// header line). Returns `None` for blank / comment / header lines.
///
/// The path is the token AFTER the single-character status letter and its
/// separator (a tab or a run of spaces), up to the next whitespace (which,
/// in the adapter format, precedes the ` blake3:<hex>` content hash).
fn diff_line_path(line: &str) -> Option<&str> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // status letter, then a separator, then the path.
    let mut chars = line.char_indices();
    let (_i0, letter) = chars.next()?;
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let (sep_start, sep_char) = chars.next()?;
    if sep_char != '\t' && sep_char != ' ' {
        return None;
    }
    let rest = &line[sep_start..];
    let rest = rest.trim_start();
    // The path runs up to the next whitespace (the adapter appends
    // ` blake3:<hex>`; the fixture format has nothing after the path).
    let path = rest.split_whitespace().next()?;
    if path.is_empty() { None } else { Some(path) }
}

/// Every touched path in a diff-summary blob, in first-seen order.
fn touched_paths(blob: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(blob);
    let mut paths = Vec::new();
    for line in text.lines() {
        if let Some(p) = diff_line_path(line) {
            let owned = p.to_string();
            if !paths.contains(&owned) {
                paths.push(owned);
            }
        }
    }
    paths
}

/// Two-star glob match over a `/`-segmented path (the subset the board pins:
/// `src/checkout/**`, `.env`, `secrets/**`, `**`). Semantics:
/// - `**` matches any number of path segments (including zero);
/// - `*` matches within one segment;
/// - `?` matches one non-`/` character; literals match themselves.
///
/// Hand-rolled per the root-manifest note (the pinned patterns are a tiny
/// two-star subset; a full globset dep would be attack surface, I7).
fn glob_match(pattern: &str, path: &str) -> bool {
    glob_seg(pattern.as_bytes(), path.as_bytes())
}

fn glob_seg(pat: &[u8], text: &[u8]) -> bool {
    // Iterative backtracking matcher with `**` (crosses `/`), `*` (does not
    // cross `/`), `?` (one non-`/`), literals.
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti): (Option<usize>, usize) = (None, 0);
    let mut star_crosses_slash = false;
    while ti < text.len() {
        if pi < pat.len() {
            match pat[pi] {
                b'*' => {
                    let double = pi + 1 < pat.len() && pat[pi + 1] == b'*';
                    star_crosses_slash = double;
                    star_pi = Some(pi);
                    star_ti = ti;
                    pi += if double { 2 } else { 1 };
                    // Skip a `/` immediately after `**/` so `src/**` matches
                    // `src/a` and `**` matches everything.
                    if double && pi < pat.len() && pat[pi] == b'/' {
                        pi += 1;
                    }
                    continue;
                }
                b'?' => {
                    if text[ti] != b'/' {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                }
                c => {
                    if c == text[ti] {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                }
            }
        }
        // Mismatch: backtrack to the last star if the char it may consume is
        // legal (a single `*` never consumes `/`).
        if let Some(sp) = star_pi {
            if !star_crosses_slash && text[star_ti] == b'/' {
                return false;
            }
            star_ti += 1;
            ti = star_ti;
            pi = sp + if star_crosses_slash { 2 } else { 1 };
            if star_crosses_slash && pi < pat.len() && pat[pi] == b'/' {
                pi += 1;
            }
            continue;
        }
        return false;
    }
    // Consume trailing stars/slashes in the pattern.
    while pi < pat.len() {
        match pat[pi] {
            b'*' => pi += 1,
            b'/' if pi > 0 && pat[pi - 1] == b'*' => pi += 1,
            _ => break,
        }
    }
    pi == pat.len()
}

/// `params[key]` as a `Vec<String>` of glob patterns (absent ⇒ empty).
fn glob_list(params: &Value, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Read the CAS-pinned agent-spec TOML (`refs["spec"]`) as a parsed table.
/// Missing/malformed ⇒ `Ok(None)` (cannot-run → inconclusive).
fn read_spec(input: &VerifierInput, cas: &Cas) -> Result<Option<toml::Value>, GateError> {
    let Some(ref_str) = input.refs.get("spec") else {
        return Ok(None);
    };
    let Some(bytes) = resolve_ref(cas, ref_str)? else {
        return Ok(None);
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    Ok(toml::from_str::<toml::Value>(text).ok())
}

/// The `[agent]` table of an agent-spec blob (the natives read this).
fn agent_table(spec: &toml::Value) -> Option<&toml::value::Table> {
    spec.get("agent").and_then(toml::Value::as_table)
}

/// Built-in: is every touched path inside `params.allow` (glob list)?
/// Reads `refs["diff"]` — the S2 `diff.ready` summary format, one
/// `<status>\t<path>` line per touched file.
pub struct DiffScope;

impl NativeVerifier for DiffScope {
    fn name(&self) -> &'static str {
        "diff-scope"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(ref_str) = input.refs.get("diff") else {
            return Ok(cannot_run("no diff ref in inputs"));
        };
        let Some(blob) = resolve_ref(cas, ref_str)? else {
            return Ok(cannot_run("diff blob absent from CAS"));
        };
        let allow = glob_list(&input.params, "allow");
        // Deterministic scan: first out-of-scope path in summary order is the
        // named offender (same inputs → same evidence, I6).
        let out_of_scope: Vec<String> = touched_paths(&blob)
            .into_iter()
            .filter(|p| !allow.iter().any(|g| glob_match(g, p)))
            .collect();
        if out_of_scope.is_empty() {
            return Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            });
        }
        let msg = format!("out-of-scope paths: {}", out_of_scope.join(", "));
        // Evidence blob → CAS; the fact carries the ref only (I2).
        let ev = cas.put(msg.as_bytes(), "text/plain")?;
        Ok(VerifierOutput {
            verdict: Verdict::Fail,
            evidence: vec![Evidence {
                kind: "scope-violation".to_string(),
                msg,
                cas_ref: Some(format!("cas:blake3:{}", ev.hash)),
            }],
            cost_ms: 0,
        })
    }
}

/// Built-in: does the diff touch any `params.forbid` (glob list) path?
/// Reads `refs["diff"]` (same summary format as [`DiffScope`]).
pub struct ForbiddenPath;

impl NativeVerifier for ForbiddenPath {
    fn name(&self) -> &'static str {
        "forbidden-path"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(ref_str) = input.refs.get("diff") else {
            return Ok(cannot_run("no diff ref in inputs"));
        };
        let Some(blob) = resolve_ref(cas, ref_str)? else {
            return Ok(cannot_run("diff blob absent from CAS"));
        };
        let forbid = glob_list(&input.params, "forbid");
        let touched: Vec<String> = touched_paths(&blob)
            .into_iter()
            .filter(|p| forbid.iter().any(|g| glob_match(g, p)))
            .collect();
        if touched.is_empty() {
            return Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            });
        }
        let msg = format!("forbidden paths touched: {}", touched.join(", "));
        let ev = cas.put(msg.as_bytes(), "text/plain")?;
        Ok(VerifierOutput {
            verdict: Verdict::Fail,
            evidence: vec![Evidence {
                kind: "forbidden-touch".to_string(),
                msg,
                cas_ref: Some(format!("cas:blake3:{}", ev.hash)),
            }],
            cost_ms: 0,
        })
    }
}

/// Vet native: governed spawns require `bare = true` in the agent spec
/// (DR-001: `--bare` is the determinism knob a vet gate can require).
/// Reads `refs["spec"]` — the CAS-pinned agent-spec TOML (an `[agent]`
/// table).
pub struct BareMode;

impl NativeVerifier for BareMode {
    fn name(&self) -> &'static str {
        "bare-mode"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(spec) = read_spec(input, cas)? else {
            return Ok(cannot_run("agent spec absent from CAS or unparseable"));
        };
        let bare = agent_table(&spec)
            .and_then(|t| t.get("bare"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        if bare {
            Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            })
        } else {
            fail_evidence(
                cas,
                "bare-mode",
                "governed spawn requires `bare = true` in the agent spec (DR-001)",
            )
        }
    }
}

/// Build a `fail` output whose evidence blob (in the CAS, ref-carried, I2)
/// names the missing knob — the refusal is interrogable (I6).
fn fail_evidence(cas: &Cas, kind: &str, msg: &str) -> Result<VerifierOutput, GateError> {
    let ev = cas.put(msg.as_bytes(), "text/plain")?;
    Ok(VerifierOutput {
        verdict: Verdict::Fail,
        evidence: vec![Evidence {
            kind: kind.to_string(),
            msg: msg.to_string(),
            cas_ref: Some(format!("cas:blake3:{}", ev.hash)),
        }],
        cost_ms: 0,
    })
}

/// Vet native: the agent spec must pin `harness_version` (risk register:
/// harness CLI churn — the adapter refuses untested majors).
pub struct PinnedVersion;

impl NativeVerifier for PinnedVersion {
    fn name(&self) -> &'static str {
        "pinned-version"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(spec) = read_spec(input, cas)? else {
            return Ok(cannot_run("agent spec absent from CAS or unparseable"));
        };
        let pinned = agent_table(&spec)
            .and_then(|t| t.get("harness_version"))
            .and_then(toml::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if pinned {
            Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            })
        } else {
            fail_evidence(
                cas,
                "pinned-version",
                "governed spawn requires a pinned `harness_version` (risk register: harness CLI churn)",
            )
        }
    }
}

/// Vet native: the agent spec must carry an explicit, non-empty
/// `allowed_tools` list (DR-001: permission composition recorded in events).
pub struct AllowedTools;

impl NativeVerifier for AllowedTools {
    fn name(&self) -> &'static str {
        "allowed-tools"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(spec) = read_spec(input, cas)? else {
            return Ok(cannot_run("agent spec absent from CAS or unparseable"));
        };
        let has_explicit_list = agent_table(&spec)
            .and_then(|t| t.get("allowed_tools"))
            .and_then(toml::Value::as_array)
            .is_some_and(|a| !a.is_empty());
        if has_explicit_list {
            Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            })
        } else {
            fail_evidence(
                cas,
                "allowed-tools",
                "governed spawn requires an explicit non-empty `allowed_tools` list (DR-001)",
            )
        }
    }
}

/// `params[key]` as a plain `Vec<String>` (absent ⇒ empty). Unlike
/// [`glob_list`] this is intent-neutral — the caller decides whether the items
/// are globs or literals.
fn string_list(params: &Value, key: &str) -> Vec<String> {
    glob_list(params, key)
}

/// Permit native (SP1): the requested action's `tool` (a `params` scalar) must
/// be in `params.allow` (a glob/name list). Listed → Pass; unlisted → Fail
/// (evidence names the offending tool, I6); tool ABSENT → Inconclusive
/// (cannot-run — the native never SYNTHESIZES a pass, I6).
///
/// The tool descriptor rides inline in `params` (not a CAS ref): it is a small
/// scalar (ontology `permit.requested.target`), so it is content-hash-pinned
/// via the verbatim `inputs.params` like every other native input (design §8).
pub struct ToolAllowlist;

impl NativeVerifier for ToolAllowlist {
    fn name(&self) -> &'static str {
        "tool-allowlist"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(tool) = input.params.get("tool").and_then(Value::as_str) else {
            return Ok(cannot_run(
                "no tool in params — undecidable, not a pass (I6)",
            ));
        };
        let allow = string_list(&input.params, "allow");
        if allow.iter().any(|g| glob_match(g, tool)) {
            return Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            });
        }
        fail_evidence(
            cas,
            "tool-not-allowed",
            &format!("tool {tool} not in allowlist"),
        )
    }
}

/// Permit native (SP1): every target path in `params.paths` must be inside
/// `params.allow` (glob list, the two-star matcher the diff natives use).
/// In-scope → Pass; out-of-scope → Fail naming the FIRST offending path in
/// order (deterministic, I6); paths ABSENT → Inconclusive (cannot-run, I6).
pub struct PathScope;

impl NativeVerifier for PathScope {
    fn name(&self) -> &'static str {
        "path-scope"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let Some(paths) = input.params.get("paths").and_then(Value::as_array) else {
            return Ok(cannot_run(
                "no target paths in params — undecidable, not a pass (I6)",
            ));
        };
        let allow = glob_list(&input.params, "allow");
        // Deterministic scan: the first out-of-scope path in list order is the
        // named offender (same params → same verdict AND same evidence, I6).
        let offender = paths
            .iter()
            .filter_map(Value::as_str)
            .find(|p| !allow.iter().any(|g| glob_match(g, p)));
        match offender {
            None => Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            }),
            Some(path) => fail_evidence(
                cas,
                "path-out-of-scope",
                &format!("path {path} outside allowed scope"),
            ),
        }
    }
}

/// Permit native (SP1, DR-009 C1): the running per-session spend + rate vs. a
/// soft/hard cap. The running totals arrive as PINNED INPUTS in `params` (the
/// daemon folds `PermitAccumulators` from the log, I3, and passes the snapshot
/// verbatim — the native never touches mutable state; determinism BINDING).
///
/// Verdicts: projected spend (`cumulative_spend_usd + action_cost_usd`) under
/// the soft cap → Pass; soft ≤ projected < hard → **Inconclusive** (escalate to
/// a human, NEVER coerced, I6, DR-008 §4); projected ≥ hard → Fail; the window
/// action count ≥ the rate limit → Fail (independent of spend); caps MISSING →
/// Inconclusive (cannot-run — garbage never coerces to a pass, I6).
pub struct SpendCap;

impl NativeVerifier for SpendCap {
    fn name(&self) -> &'static str {
        "spend-cap"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let p = &input.params;
        let num = |key: &str| p.get(key).and_then(Value::as_f64);
        // The caps are the load-bearing inputs: absent caps are undecidable.
        let (Some(soft), Some(hard)) = (num("soft_cap_usd"), num("hard_cap_usd")) else {
            return Ok(cannot_run(
                "no soft/hard cap in params — undecidable, not a pass (I6)",
            ));
        };
        let cumulative = num("cumulative_spend_usd").unwrap_or(0.0);
        let cost = num("action_cost_usd").unwrap_or(0.0);
        let projected = cumulative + cost;
        let window = p
            .get("window_action_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let rate_limit = p.get("rate_limit").and_then(Value::as_u64);

        // Rate limit: at/over the per-window limit → deny, independent of spend.
        if let Some(limit) = rate_limit
            && window >= limit
        {
            return fail_evidence(
                cas,
                "rate-limit-exceeded",
                &format!("window action count {window} at/over rate limit {limit}"),
            );
        }

        // Hard cap: at/over → deny.
        if projected >= hard {
            return fail_evidence(
                cas,
                "hard-cap-exceeded",
                &format!("projected spend {projected:.2} at/over hard cap {hard:.2}"),
            );
        }
        // Soft band (soft ≤ projected < hard): escalate to a human. NEVER
        // coerced to a pass or an auto-deny (I6, DR-008 §4). The evidence names
        // the crossing so the escalation is interrogable.
        if projected >= soft {
            return Ok(VerifierOutput {
                verdict: Verdict::Inconclusive,
                evidence: vec![Evidence {
                    kind: "soft-cap-crossed".to_string(),
                    msg: format!(
                        "projected spend {projected:.2} crossed soft cap {soft:.2} (< hard {hard:.2}) — escalate"
                    ),
                    cas_ref: None,
                }],
                cost_ms: 0,
            });
        }
        // Under the soft cap: allow.
        Ok(VerifierOutput {
            verdict: Verdict::Pass,
            evidence: vec![],
            cost_ms: 0,
        })
    }
}

/// The SHARED deterministic risk scorer (DR-024 C6, Q1/Q5). Scores THIS action's
/// risk from the pinned request `axis` (`tool`/`paths`/`role`) against the config
/// `table`, as the SUM of three independent factors:
///
/// - per-tool base risk: `table.base[tool]` (an unlisted tool → 0.0);
/// - a path-sensitivity modifier: `table.path_modifier` added ONCE if ANY of the
///   axis `paths` matches ANY glob in `table.sensitive_paths` (no match → 0.0);
/// - a role modifier: `table.role_modifier[role]` (an unlisted role → 0.0).
///
/// Pure, deterministic, NO network/inference/IO (I6/I7): the SAME axis + table
/// yield the SAME scalar, replayable from content-pinned params. The WEIGHTS live
/// in the config `table` and are NEVER hardcoded (DR-024 "does NOT decide" — the
/// numbers are tuning, the STRUCTURE is the contract). Both [`RiskCap::verify`]
/// (for its soft/hard verdict) and the emit site (to stamp `risk_delta` on the
/// `permit.granted` fact) call THIS fn on the identical content-pinned inputs, so
/// the verdict and the folded delta CANNOT diverge (DR-024 Q5 option iii — the
/// contract-free producer seam).
pub fn risk_score(axis: &Value, table: &Value) -> f64 {
    let base = axis
        .get("tool")
        .and_then(Value::as_str)
        .and_then(|tool| table.pointer("/base").and_then(|b| b.get(tool)))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    let sensitive = glob_list(table, "sensitive_paths");
    let touches_sensitive = axis
        .get("paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .any(|p| sensitive.iter().any(|g| glob_match(g, p)))
        })
        .unwrap_or(false);
    let path_modifier = if touches_sensitive {
        table
            .get("path_modifier")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    } else {
        0.0
    };

    let role_modifier = axis
        .get("role")
        .and_then(Value::as_str)
        .and_then(|role| table.pointer("/role_modifier").and_then(|r| r.get(role)))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

    base + path_modifier + role_modifier
}

/// Permit native (DR-024 C6): the running per-run RISK score vs. a soft/hard cap
/// — the RISK analogue of [`SpendCap`]. THIS action's risk is COMPUTED inside the
/// verifier by the shared [`risk_score`] fn from the pinned request axis
/// (`tool`/`paths`/`role`) + the config `risk_table` (DR-024 Q4 — NOT injected).
/// The folded `cumulative_risk_score` (prior GRANTED actions) arrives as a PINNED
/// input the PDP injects from `PermitAccumulators` (I3, determinism BINDING — the
/// native never touches live state).
///
/// Verdicts mirror [`SpendCap`] EXACTLY: projected (`cumulative_risk_score +
/// this-action risk`) under the soft cap → Pass; soft ≤ projected < hard →
/// **Inconclusive** (escalate to a human, NEVER coerced, I6, DR-008 §4);
/// projected ≥ hard → Fail; caps MISSING → Inconclusive (cannot-run — garbage
/// never coerces to a pass, I6). The soft-band/hard-cap evidence NAMES each
/// contributing factor (per-tool base, path modifier, role modifier) so
/// `gate_explain` answers "why this risk" (I6 interrogability, DR-024 crit 3).
pub struct RiskCap;

impl NativeVerifier for RiskCap {
    fn name(&self) -> &'static str {
        "risk-cap"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let p = &input.params;
        let num = |key: &str| p.get(key).and_then(Value::as_f64);
        // The caps are the load-bearing inputs: absent caps are undecidable.
        let (Some(soft), Some(hard)) = (num("soft_cap_risk"), num("hard_cap_risk")) else {
            return Ok(cannot_run(
                "no soft/hard risk cap in params — undecidable, not a pass (I6)",
            ));
        };
        // The scorer table rides the verifier's own spec params (merged over the
        // request axis). This-action risk is COMPUTED here (never injected) from
        // the pinned axis + table via the SHARED fn — the same scalar the emit
        // site stamps (DR-024 Q5).
        let table = p.get("risk_table").cloned().unwrap_or(Value::Null);
        let cumulative = num("cumulative_risk_score").unwrap_or(0.0);
        let this_action = risk_score(p, &table);
        let projected = cumulative + this_action;

        // Name the request axis for interrogable evidence (I6, DR-024 crit 3).
        let tool = p.get("tool").and_then(Value::as_str).unwrap_or("(none)");
        let role = p.get("role").and_then(Value::as_str).unwrap_or("(none)");
        let sensitive = glob_list(&table, "sensitive_paths");
        let touched: Vec<String> = p
            .get("paths")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|path| sensitive.iter().any(|g| glob_match(g, path)))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        // The three contributing factors, each NAMED (per-tool base, path
        // modifier, role modifier), so `gate_explain` surfaces the breakdown.
        let breakdown = format!(
            "per-tool base tool={tool}; path modifier sensitive-paths=[{}]; role modifier role={role}",
            touched.join(", ")
        );

        // Hard cap: at/over → deny. Evidence blob → CAS, ref carried (I2).
        if projected >= hard {
            return fail_evidence(
                cas,
                "hard-risk-cap-exceeded",
                &format!("projected risk {projected:.2} at/over hard cap {hard:.2} ({breakdown})"),
            );
        }
        // Soft band (soft ≤ projected < hard): escalate to a human. NEVER coerced
        // to a pass or an auto-deny (I6, DR-008 §4). The evidence names the
        // crossing AND each factor so the escalation is interrogable.
        if projected >= soft {
            let msg = format!(
                "projected risk {projected:.2} crossed soft cap {soft:.2} (< hard {hard:.2}) — escalate ({breakdown})"
            );
            let ev = cas.put(msg.as_bytes(), "text/plain")?;
            return Ok(VerifierOutput {
                verdict: Verdict::Inconclusive,
                evidence: vec![Evidence {
                    kind: "soft-risk-cap-crossed".to_string(),
                    msg,
                    cas_ref: Some(format!("cas:blake3:{}", ev.hash)),
                }],
                cost_ms: 0,
            });
        }
        // Under the soft cap: allow.
        Ok(VerifierOutput {
            verdict: Verdict::Pass,
            evidence: vec![],
            cost_ms: 0,
        })
    }
}

/// Permit native (SP-intent, DR-010 C7): the `intent-lock` run-intent check.
/// The requested action's `tool` (a `params` scalar, same key
/// [`ToolAllowlist`] uses) must be in the run's DECLARED, content-pinned intent
/// allowlist `params.allowed_tools`. The allowlist is folded from the log
/// (`AgentRunState.intent`, I3) and passed verbatim in the PINNED `inputs.params`
/// — the native NEVER re-derives intent and NEVER touches live state
/// (determinism BINDING, DR-010 §3; a live LLM inference would break I6 replay).
///
/// Verdicts (DR-010 §8 crit 2):
/// - requested tool ∈ `allowed_tools` → Pass (on-task → allow).
/// - off-task (∉) under the DEFAULT / `escalate` knob → **Inconclusive**
///   (escalate to a human, NEVER coerced to pass or deny — the load-bearing I6
///   honesty guard). Evidence names BOTH the off-task tool AND the declared
///   intent allowlist so `gate_explain` can surface WHY; the evidence blob goes
///   to the CAS, the fact carries the ref only (I2).
/// - off-task under the hardened knob `on_off_task = deny` → Fail (deny) for
///   high-assurance runs, with the same interrogable evidence.
/// - intent ABSENT (the `allowed_tools` KEY omitted from params) →
///   Inconclusive via cannot-run, NEVER a synthesized pass (same discipline as
///   [`SpendCap`] with missing caps, I6). A DECLARED-empty allowlist
///   (`allowed_tools: []`, key present) is NOT cannot-run: it is a lockdown
///   where every tool is off-task, routed through the off-task path above
///   (escalate/deny per `on_off_task`), per DR-012 option B.
pub struct IntentLock;

impl NativeVerifier for IntentLock {
    fn name(&self) -> &'static str {
        "intent-lock"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let p = &input.params;
        let Some(tool) = p.get("tool").and_then(Value::as_str) else {
            return Ok(cannot_run(
                "no tool in params — undecidable, not a pass (I6)",
            ));
        };
        // The DECLARED, content-pinned intent allowlist. Key-ABSENCE is
        // intent-absent: the verifier cannot decide whether the tool is on-task
        // → cannot-run (never a synthesized pass, DR-010 §8 crit 2d, I6). A
        // key-PRESENT-but-empty `[]` is a DECLARED-empty lockdown — the run
        // declared it may use NO tools, so every tool is off-task and falls
        // through the off-task path below (DR-012 option B). The discriminator
        // is key-presence, NOT emptiness.
        if p.get("allowed_tools").is_none() {
            return Ok(cannot_run(
                "no intent allowlist pinned — undecidable, not a pass (I6)",
            ));
        }
        let allowed = string_list(p, "allowed_tools");

        // On-task: the requested tool is in the declared intent allowlist → allow.
        if allowed.iter().any(|t| t == tool) {
            return Ok(VerifierOutput {
                verdict: Verdict::Pass,
                evidence: vec![],
                cost_ms: 0,
            });
        }

        // Off-task. The evidence names BOTH the off-task tool AND the declared
        // intent so the escalation/denial is interrogable (I6, DR-010 §8).
        let msg = format!(
            "off-task tool {tool} not in declared intent [{}]",
            allowed.join(", ")
        );
        // The knob is read from the PINNED params (never live state, DR-010 §3);
        // ABSENT ⇒ escalate (the honest default — deny is opt-in only).
        let deny = p
            .get("on_off_task")
            .and_then(Value::as_str)
            .is_some_and(|k| k == "deny");
        if deny {
            // Hardened knob → Fail (deny). Evidence blob → CAS, ref carried (I2).
            return fail_evidence(cas, "off-task-tool", &msg);
        }
        // DEFAULT / `escalate` → Inconclusive (escalate to a human). NEVER
        // coerced to pass or auto-deny (I6, DR-010 §3). The evidence blob goes
        // to the CAS; the fact carries the ref only (I2).
        let ev = cas.put(msg.as_bytes(), "text/plain")?;
        Ok(VerifierOutput {
            verdict: Verdict::Inconclusive,
            evidence: vec![Evidence {
                kind: "off-task-tool".to_string(),
                msg,
                cas_ref: Some(format!("cas:blake3:{}", ev.hash)),
            }],
            cost_ms: 0,
        })
    }
}

/// The v1 native pack, by name — also the registry [`replay`] re-executes
/// against.
pub fn builtin_natives() -> Vec<Box<dyn NativeVerifier>> {
    vec![
        Box::new(DiffScope),
        Box::new(ForbiddenPath),
        Box::new(BareMode),
        Box::new(PinnedVersion),
        Box::new(AllowedTools),
        Box::new(ToolAllowlist),
        Box::new(PathScope),
        Box::new(SpendCap),
        Box::new(RiskCap),
        Box::new(IntentLock),
    ]
}

/// An exec verifier (BINDING kind 2): any argv program speaking the §8 JSON
/// contract — stdin [`VerifierInput`], stdout [`VerifierOutput`].
///
/// Engine obligations pinned by the board:
/// - the child receives EXACTLY the serialized input document on stdin;
/// - a SCRUBBED environment (doc §12) — no ambient secrets reach a verifier;
/// - `input.timeout_ms` is enforced by wall clock; overrun ⇒
///   `inconclusive { timeout }`;
/// - nonzero exit ⇒ `inconclusive { nonzero_exit }` — even when stdout says
///   `pass`;
/// - unparseable stdout ⇒ `inconclusive { malformed_output }`;
/// - failures are VERDICTS, not errors: `run` is infallible by design.
pub struct ExecVerifier {
    pub name: String,
    pub argv: Vec<String>,
}

impl ExecVerifier {
    /// Run the argv program under the §8 contract. Infallible by design: a
    /// verifier that cannot run or cannot decide yields an `inconclusive`
    /// [`VerdictRecord`], never an error — the only honest mappings are the
    /// three verdicts (I6).
    pub async fn run(&self, input: &VerifierInput) -> VerdictRecord {
        let span = tracing::info_span!("adapter", kind = "exec-verifier", verifier = %self.name);
        self.run_inner(input).instrument(span).await
    }

    async fn run_inner(&self, input: &VerifierInput) -> VerdictRecord {
        use tokio::io::AsyncWriteExt;

        let inconclusive = |reason: InconclusiveReason| VerdictRecord {
            verifier: self.name.clone(),
            verdict: Verdict::Inconclusive,
            reason: Some(reason),
            evidence: vec![],
            cost_ms: 0,
        };

        let Some((program, args)) = self.argv.split_first() else {
            // A verifier with no argv cannot run — malformed config.
            return inconclusive(InconclusiveReason::MalformedOutput);
        };

        // §8 stdin document, serialized verbatim (what the log records).
        let stdin_doc = match serde_json::to_vec(input) {
            Ok(bytes) => bytes,
            Err(_) => return inconclusive(InconclusiveReason::MalformedOutput),
        };

        // Scrubbed environment (doc §12): no ambient secret reaches the child.
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .env_clear()
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            // The program never started (ENOENT, EACCES, …): nothing ran, so
            // the honest reason is could_not_run, not malformed_output.
            Err(_) => return inconclusive(InconclusiveReason::CouldNotRun),
        };

        if let Some(mut stdin) = child.stdin.take() {
            // Write the stdin doc; a broken pipe (child exited early) is not a
            // hard error — the exit-status branch decides the verdict.
            let _ = stdin.write_all(&stdin_doc).await;
            let _ = stdin.shutdown().await;
        }

        let timeout = Duration::from_millis(input.timeout_ms);
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            // Wall-clock overrun: the child is killed by kill_on_drop when the
            // future is dropped here.
            Err(_elapsed) => return inconclusive(InconclusiveReason::Timeout),
            // A wait-io error means the run itself broke, not that the child
            // produced malformed output — could_not_run is the truthful reason.
            Ok(Err(_io)) => return inconclusive(InconclusiveReason::CouldNotRun),
        };

        // Nonzero exit ⇒ inconclusive, NEVER pass — even if stdout says pass.
        if !output.status.success() {
            return inconclusive(InconclusiveReason::NonzeroExit);
        }

        match parse_verifier_output(&output.stdout) {
            Ok(doc) => VerdictRecord {
                verifier: self.name.clone(),
                verdict: doc.verdict,
                reason: None,
                evidence: doc.evidence,
                cost_ms: doc.cost_ms,
            },
            Err(_) => inconclusive(InconclusiveReason::MalformedOutput),
        }
    }
}

/// One replayed verdict. `replayed: None` means the record was reported but
/// not re-executed — v1 replay policy (stated in the S4 board): native
/// verifiers are re-executed from log + CAS; exec verifiers are reported
/// from the record; recorded `inconclusive` is honest can't-decide and has
/// nothing deterministic to reproduce, so it is never re-executed and never
/// alarms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayedVerdict {
    pub run: String,
    pub gate: String,
    pub verifier: String,
    pub recorded: Verdict,
    pub replayed: Option<Verdict>,
}

/// Recorded ≠ replayed: verifier nondeterminism (a verifier bug) or an
/// altered log (§12). Raised, named, never silently reconciled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntegrityAlarm {
    pub run: String,
    pub gate: String,
    pub verifier: String,
    pub recorded: Verdict,
    pub replayed: Verdict,
}

/// The debrief replay report (`rezidnt debrief <session|run>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayReport {
    pub verdicts: Vec<ReplayedVerdict>,
    pub alarms: Vec<IntegrityAlarm>,
}

/// Re-execute the recorded verdicts found in `events` (gate.passed /
/// gate.failed / gate.inconclusive facts) from their recorded `inputs`
/// against `cas`, per the v1 replay policy on [`ReplayedVerdict`].
pub fn replay(events: &[Event], cas: &Cas) -> Result<ReplayReport, GateError> {
    let natives = builtin_natives();
    let is_native = |name: &str| natives.iter().any(|n| n.name() == name);

    let mut verdicts = Vec::new();
    let mut alarms = Vec::new();

    for event in events {
        let payload = event.payload();
        let run = payload["run"].as_str().unwrap_or_default().to_string();
        let gate = payload["gate"].as_str().unwrap_or_default().to_string();

        // Collect (verifier, recorded verdict, recorded inputs) tuples for
        // whichever gate.* fact this is.
        let recorded: Vec<(String, Verdict, Value)> = match event.subject.as_str() {
            "gate.failed" => vec![(
                payload["verifier"].as_str().unwrap_or_default().to_string(),
                Verdict::Fail,
                payload["inputs"].clone(),
            )],
            "gate.inconclusive" => vec![(
                payload["verifier"].as_str().unwrap_or_default().to_string(),
                Verdict::Inconclusive,
                payload["inputs"].clone(),
            )],
            "gate.passed" => payload["verifiers"]
                .as_array()
                .map(|records| {
                    records
                        .iter()
                        .map(|r| {
                            (
                                r["verifier"].as_str().unwrap_or_default().to_string(),
                                Verdict::Pass,
                                r["inputs"].clone(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => continue,
        };

        for (verifier, recorded_verdict, inputs) in recorded {
            // v1 replay policy: inconclusive is honest can't-decide (nothing
            // deterministic to reproduce) → reported, never re-executed,
            // never an alarm. Exec verifiers' argv is not on the v1 payload →
            // reported (`replayed: None`). Only natives re-execute.
            let replayed = if recorded_verdict == Verdict::Inconclusive || !is_native(&verifier) {
                None
            } else {
                reexecute_native(&natives, &verifier, &inputs, cas)?
            };

            if let Some(replayed_verdict) = replayed
                && replayed_verdict != recorded_verdict
            {
                alarms.push(IntegrityAlarm {
                    run: run.clone(),
                    gate: gate.clone(),
                    verifier: verifier.clone(),
                    recorded: recorded_verdict,
                    replayed: replayed_verdict,
                });
            }

            verdicts.push(ReplayedVerdict {
                run: run.clone(),
                gate: gate.clone(),
                verifier,
                recorded: recorded_verdict,
                replayed,
            });
        }
    }

    Ok(ReplayReport { verdicts, alarms })
}

/// Re-execute a named native verifier against its recorded §8 inputs. The
/// recorded `inputs` object deserializes to a [`VerifierInput`] verbatim (the
/// determinism BINDING: content-hash-pinned inputs reproduce the verdict).
/// Returns `None` when the inputs are unrecoverable — a can't-run, never a
/// synthesized verdict.
fn reexecute_native(
    natives: &[Box<dyn NativeVerifier>],
    name: &str,
    inputs: &Value,
    cas: &Cas,
) -> Result<Option<Verdict>, GateError> {
    let Some(native) = natives.iter().find(|n| n.name() == name) else {
        return Ok(None);
    };
    let Ok(input) = serde_json::from_value::<VerifierInput>(inputs.clone()) else {
        return Ok(None);
    };
    Ok(Some(native.verify(&input, cas)?.verdict))
}
