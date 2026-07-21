//! §13 project spec: the TOML file `rezidnt open` materializes from.
//!
//! `[[workspace.tab]]` layout intent is Phase-3 surface: parsed and preserved,
//! never an error, never acted on in Phase 1.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::RunError;

/// Parsed project spec (doc §13 shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSpec {
    pub name: String,
    /// Repo path, relative to the spec file's directory ("." is common).
    pub repo: PathBuf,
    pub agents: Vec<AgentSpec>,
    /// `[gates.<name>]` verifier sets (S4). Keyed by gate name; an agent's
    /// `gates` list names which of these run. Empty in pre-S4 specs.
    #[serde(default)]
    pub gates: std::collections::BTreeMap<String, GateSpec>,
    /// `[egress]` block (DR-029 §Decision 1): the project-DECLARED folded egress
    /// authority — the allowlist of reachable hosts + the `host → secret_ref`
    /// LABEL map the daemon-side [`crate::secret::SecretSource`] resolves values
    /// for. ABSENT ⇒ default ⇒ empty allowlist ⇒ deny-all (absent NEVER means
    /// open, the DR-028 honest default preserved). The map holds only labels
    /// (`secret_ref`s), never a value — repo-safe. C6/DR-024: this is folded
    /// authority (the sole `EgressPolicy::from_folded_authority` door), never a
    /// run-supplied/request-time value.
    #[serde(default)]
    pub egress: EgressSpec,
}

/// The `[egress]` block (DR-029 §Decision 1). The allowlist hosts + the
/// `[egress.secrets]` `host → secret_ref` LABEL map (repo-safe — a label, never a
/// value). Absent block ⇒ default (both empty) ⇒ deny-all. A malformed block
/// (e.g. `allowlist` given as a string, not an array) surfaces as
/// [`RunError::Spec`], never a silently-empty allowlist that would drop the
/// deny-all boundary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EgressSpec {
    /// The allowlisted hosts (`allowlist = ["github.com", …]`), in declared
    /// order. Empty ⇒ deny-all.
    #[serde(default)]
    pub allowlist: Vec<String>,
    /// The `[egress.secrets]` `host → secret_ref` LABEL map — a label the
    /// daemon-side `SecretSource` resolves to a value, NEVER a value itself
    /// (repo-safe). `BTreeMap` for deterministic fold order.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
}

/// One `[[agent]]` table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    /// AgentSubstrate impl selector (`claude-code` is the S1 native adapter).
    pub harness: String,
    /// `"auto"` = rezidnt allocates (sole-allocator model, DR-001).
    pub worktree: String,
    /// Gate names — parsed and preserved in S1, enforced from Phase 2 (S4).
    #[serde(default)]
    pub gates: Vec<String>,
    /// Test/pin seam: run this executable instead of the harness default.
    /// Recorded-transcript contract tests and version pinning both need it.
    #[serde(default)]
    pub bin_override: Option<PathBuf>,
    /// Governed-spawn field (S4): `--bare` enforcement decision the vet
    /// gate's bare-mode verifier checks (DR-001; ontology `agent.spawned`
    /// additive field). Recorded on the fact so the posture is log-derivable.
    #[serde(default)]
    pub bare: bool,
    /// Governed-spawn field (S4): the pinned harness version the vet gate's
    /// pinned-version verifier requires (risk register: harness CLI churn).
    #[serde(default)]
    pub harness_version: Option<String>,
    /// Governed-spawn field (S4): the composed allow-list the vet gate's
    /// allowed-tools verifier requires (DR-001: permission composition).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// SP4a permit input axis (DR-016 §Decision 2; ontology `agent.spawned.role?`):
    /// the RBAC role the agent is authorized as, an opaque string the policy
    /// interprets (rezidnt mints no role vocabulary). Recorded on `agent.spawned`
    /// and injected into `decide_permit`'s content-pinned per-run params so a
    /// role-keyed policy can decide on role + workspace + action. ABSENT = no role
    /// declared — never synthesized to a default like `"contributor"` (DR-012
    /// declared-vs-absent; mirrors `harness_version`). A declared empty string is
    /// `Some("")`, distinct from absent (the policy interprets it, not rezidnt).
    #[serde(default)]
    pub role: Option<String>,
}

/// One `[gates.<name>]` table: the ordered verifier set a gate runs (S4).
/// Unknown gate names are tolerated; only gates listed in an agent's `gates`
/// are executed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GateSpec {
    #[serde(default)]
    pub verifiers: Vec<VerifierSpec>,
}

/// One verifier entry in a `[gates.<name>]` table. A verifier is EITHER a
/// built-in `native` (by name) or an `exec` program (argv path + a display
/// `name`); `params` (glob lists, opt-ins) ride the §8 stdin doc verbatim.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VerifierSpec {
    /// Built-in native verifier name (`diff-scope`, `forbidden-path`, …).
    #[serde(default)]
    pub native: Option<String>,
    /// Exec-verifier program path (any argv speaking the §8 JSON contract).
    #[serde(default)]
    pub exec: Option<PathBuf>,
    /// Display name for an exec verifier (recorded on the verdict fact).
    #[serde(default)]
    pub name: Option<String>,
    /// §8 params (glob `allow`/`forbid` lists, network opt-in) — verbatim.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Raw wire shape: every field optional so missing values surface as honest
/// `RunError::Spec` messages naming the field, never a serde panic or a
/// silently defaulted value. Unknown tables (`[[workspace.tab]]`, `[gates.*]`)
/// are tolerated because serde ignores unknown fields by default.
#[derive(Deserialize)]
struct RawSpec {
    project: Option<RawProject>,
    #[serde(default)]
    agent: Vec<AgentSpec>,
    #[serde(default)]
    gates: std::collections::BTreeMap<String, GateSpec>,
    /// The `[egress]` block, typed so a malformed body (e.g. `allowlist` as a
    /// string) is a `toml` deserialize error surfaced as [`RunError::Spec`] —
    /// never a silently-empty allowlist (DR-029 §Decision 1 honesty leg).
    #[serde(default)]
    egress: EgressSpec,
}

#[derive(Deserialize)]
struct RawProject {
    name: Option<String>,
    repo: Option<PathBuf>,
}

/// Serialize an agent's governed fields as the `[agent]` TOML blob the vet
/// natives read (the §8 `refs["spec"]` preimage). Only the fields the three
/// vet verifiers inspect — the pinned-input surface is intentionally small.
///
/// This is the SINGLE source of the preimage: the CLI's `vet` path and the
/// daemon's gate path both call it, so the CLI and daemon vet byte-identical
/// bytes by construction (I4-adjacent dedup). A change here moves the CAS hash
/// on BOTH paths together — it can never silently split them.
pub fn agent_spec_toml(spec: &AgentSpec) -> String {
    // Minimal TOML basic-string quoting (the values are short identifiers /
    // versions / tool names — no control chars): `\`→`\\` then `"`→`\"`.
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut s = String::from("[agent]\n");
    s.push_str(&format!("name = {}\n", q(&spec.name)));
    s.push_str(&format!("harness = {}\n", q(&spec.harness)));
    s.push_str(&format!("bare = {}\n", spec.bare));
    if let Some(v) = &spec.harness_version {
        s.push_str(&format!("harness_version = {}\n", q(v)));
    }
    if !spec.allowed_tools.is_empty() {
        let items: Vec<String> = spec.allowed_tools.iter().map(|t| q(t)).collect();
        s.push_str(&format!("allowed_tools = [{}]\n", items.join(", ")));
    }
    s
}

/// Parse a HOST-LEVEL permit config TOML (SP4c-wire, DR-020 §Decision 1): a file
/// carrying ONLY a top-level `[gates.permit]` block (the same
/// `verifiers = [{ native, params }]` shape a workspace `[gates.permit]` uses),
/// sourced OUTSIDE any workspace spec. There is no `[project]`/`[[agent]]` here —
/// this is the admin authority surface, not a project. Returns the `[gates.permit]`
/// [`GateSpec`] if present, `None` if the file declares no `permit` gate. A TOML
/// syntax error is an honest [`RunError::Spec`]; unknown tables are tolerated
/// (serde ignores unknown fields).
pub fn permit_gate_from_host_toml(input: &str) -> Result<Option<GateSpec>, RunError> {
    #[derive(Deserialize)]
    struct HostPermit {
        #[serde(default)]
        gates: std::collections::BTreeMap<String, GateSpec>,
    }
    let raw: HostPermit = toml::from_str(input).map_err(|e| RunError::Spec(e.to_string()))?;
    Ok(raw.gates.get("permit").cloned())
}

impl ProjectSpec {
    /// Parse the §13 TOML. Unknown tables (e.g. `[[workspace.tab]]`,
    /// `[gates.*]`) are tolerated; a missing `[project]` name/repo is an
    /// honest `RunError::Spec`.
    pub fn from_toml_str(input: &str) -> Result<Self, RunError> {
        let raw: RawSpec = toml::from_str(input).map_err(|e| RunError::Spec(e.to_string()))?;
        let project = raw
            .project
            .ok_or_else(|| RunError::Spec("missing [project] table (name, repo)".to_string()))?;
        let name = project
            .name
            .ok_or_else(|| RunError::Spec("missing field `name` in [project]".to_string()))?;
        let repo = project
            .repo
            .ok_or_else(|| RunError::Spec("missing field `repo` in [project]".to_string()))?;
        Ok(Self {
            name,
            repo,
            agents: raw.agent,
            gates: raw.gates,
            egress: raw.egress,
        })
    }
}
