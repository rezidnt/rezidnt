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

impl ProjectSpec {
    /// Parse the §13 TOML. Unknown tables (e.g. `[[workspace.tab]]`,
    /// `[gates.*]`) are tolerated; a missing `[project]` name/repo is an
    /// honest `RunError::Spec`.
    pub fn from_toml_str(input: &str) -> Result<Self, RunError> {
        let _ = input;
        todo!("S1: parse [project] + [[agent]], tolerate layout/gate tables")
    }
}
