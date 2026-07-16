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
}

/// One `[[agent]]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    /// AgentSubstrate impl selector (`claude-code` is the S1 native adapter).
    pub harness: String,
    /// `"auto"` = rezidnt allocates (sole-allocator model, DR-001).
    pub worktree: String,
    /// Gate names — parsed and preserved in S1, enforced from Phase 2.
    #[serde(default)]
    pub gates: Vec<String>,
    /// Test/pin seam: run this executable instead of the harness default.
    /// Recorded-transcript contract tests and version pinning both need it.
    #[serde(default)]
    pub bin_override: Option<PathBuf>,
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
        })
    }
}
