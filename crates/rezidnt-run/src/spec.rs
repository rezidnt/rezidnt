//! §13 project spec: the TOML file `rezidnt open` materializes from.
//!
//! `[[workspace.tab]]` layout intent is Phase-3 surface: parsed and preserved,
//! never an error, never acted on in Phase 1.

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
}

#[derive(Deserialize)]
struct RawProject {
    name: Option<String>,
    repo: Option<PathBuf>,
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
        })
    }
}
