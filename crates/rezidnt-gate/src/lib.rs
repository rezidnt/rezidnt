//! rezidnt gate engine (doc §8 — the differentiation layer; over-invest
//! here).
//!
//! ## ORACLE SKELETON — S4 board (types real, behavior `todo!()`-stubbed)
//!
//! This crate is scaffolded by the oracle so the S4 failing tests are
//! assert-red rather than compile-red (the S3 `rezidnt-mcp` precedent).
//! Every `todo!()` is implementer work; the shapes around them are pinned by
//! the board and by the §8 BINDING contract:
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

use std::collections::BTreeMap;

use rezidnt_cas::Cas;
use rezidnt_types::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
pub fn parse_verifier_output(stdout: &[u8]) -> Result<VerifierOutput, GateError> {
    let _ = stdout;
    todo!("S4 implementer: strict §8 stdout parse — malformed is Err, never a coerced verdict")
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
        todo!("S4 implementer: BINDING defaults — network: false, timeout_ms: DEFAULT_TIMEOUT_MS")
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
        let _ = (workspace, refs, params);
        todo!("S4 implementer: assemble the §8 stdin doc from the gate def")
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

/// Built-in: is every touched path inside `params.allow` (glob list)?
/// Reads `refs["diff"]` — the S2 `diff.ready` summary format, one
/// `<status>\t<path>` line per touched file.
pub struct DiffScope;

impl NativeVerifier for DiffScope {
    fn name(&self) -> &'static str {
        "diff-scope"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let _ = (input, cas);
        todo!("S4 implementer: diff-scope over the CAS-pinned diff summary")
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
        let _ = (input, cas);
        todo!("S4 implementer: forbidden-path touch over the CAS-pinned diff summary")
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
        let _ = (input, cas);
        todo!("S4 implementer: bare-mode vet check over the CAS-pinned agent spec")
    }
}

/// Vet native: the agent spec must pin `harness_version` (risk register:
/// harness CLI churn — the adapter refuses untested majors).
pub struct PinnedVersion;

impl NativeVerifier for PinnedVersion {
    fn name(&self) -> &'static str {
        "pinned-version"
    }
    fn verify(&self, input: &VerifierInput, cas: &Cas) -> Result<VerifierOutput, GateError> {
        let _ = (input, cas);
        todo!("S4 implementer: pinned-version vet check over the CAS-pinned agent spec")
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
        let _ = (input, cas);
        todo!("S4 implementer: allowed-tools vet check over the CAS-pinned agent spec")
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
    pub async fn run(&self, input: &VerifierInput) -> VerdictRecord {
        let _ = input;
        todo!("S4 implementer: exec runner — spawn argv, feed stdin, enforce timeout, never coerce")
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
    let _ = (events, cas);
    todo!("S4 implementer: debrief replay — re-execute natives from log + CAS, alarm on divergence")
}
