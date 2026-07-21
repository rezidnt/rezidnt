//! The run substrate (DR-001's `rezidnt-run`): spawner, capture, persistence,
//! reaper, plus the claude-code adapter and the §13 project-spec parser.

pub mod adapter;
pub mod badge;
pub mod capture;
pub mod compose;
pub mod egress;
pub mod reaper;
pub mod sandbox;
pub mod secret;
pub mod spawner;
pub mod spec;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// One agent run's identity. Newtyped per rust-conventions.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct RunId(Ulid);

impl RunId {
    pub fn new(id: Ulid) -> Self {
        Self(id)
    }

    pub fn ulid(&self) -> Ulid {
        self.0
    }
}

/// Agent run status (fabric fact `agent.status.changed` carries from/to).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Spawning,
    Running,
    Completed,
    Failed,
    Signaled,
}

/// Errors for the run substrate (thiserror per lib convention).
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("project spec: {0}")]
    Spec(String),
    #[error("spawn: {0}")]
    Spawn(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
