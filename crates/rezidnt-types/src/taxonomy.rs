//! Subject taxonomy v0 — transcription of `spec/ontology.md` (the canonical
//! copy; edited only via `/subject`). 47 subjects, all payload `v = 1`.
//!
//! Sync discipline: when the warden changes the ontology, this list changes in
//! the same commit. (An automated ontology↔const drift test is implementer
//! scope — land it with the S0 implementation, not before.)

/// Taxonomy version string.
pub const ONTOLOGY_VERSION: &str = "v0";

/// Every subject minted in taxonomy v0, in ontology-table order.
pub const SUBJECTS_V0: &[&str] = &[
    // workspace
    "workspace.opened",
    "workspace.closed",
    "workspace.spec.applied",
    // worktree
    "worktree.allocated",
    "worktree.observed",
    "worktree.conflict",
    "worktree.released",
    // session (Phase 3 seam; subjects reserved now)
    "session.created",
    "session.attached",
    // pane (Phase 3 seam)
    "pane.spawned",
    "pane.exited",
    // agent
    "agent.spawned",
    "agent.status.changed",
    "agent.blocked",
    "agent.completed",
    "agent.signaled",
    "agent.tool.invoked",
    "agent.message",
    // gate
    "gate.entered",
    "gate.passed",
    "gate.failed",
    "gate.inconclusive",
    "gate.explained",
    // artifact
    "artifact.captured",
    // diff
    "diff.ready",
    "diff.merged",
    // merge
    "merge.completed",
    "merge.rejected",
    // adapter
    "adapter.health.changed",
    // daemon
    "daemon.started",
    "daemon.warning",
    "daemon.error",
    // badge
    "badge.issued",
    "badge.revoked",
    // integrity
    "integrity.alarm",
    // permit (DR-008 / DR-009 — the pre-hoc authorization axis)
    "permit.requested",
    "permit.granted",
    "permit.denied",
    "permit.escalated",
    // DR-017 SP4b — macaroon-attenuated delegation (capability-chain fact)
    "permit.delegated",
    // run (DR-010 — the run-intent axis; least-privilege in time)
    "run.intent.declared",
    // action (DR-021 — the post-action metering axis; C1 live spend-cap fold source)
    "action.metered",
    // egress (DR-029 — the C3 egress-mediation axis; composed posture + off-allowlist denial)
    "egress.mediated",
    "egress.unavailable",
    "egress.denied",
    // credential (DR-029 — the brokered-credential axis; by-ref, never the value)
    "credential.injected",
    "credential.dropped",
];
