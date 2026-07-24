//! rezidnt materialized state: pure reducers folding the event log into the
//! entity graph (CQRS-lite, doc §6). I3: the log is truth, this is derived —
//! the whole crate can be deleted and rebuilt from the log.
//!
//! ## S0 graph scope (deliberate)
//!
//! S0 materializes only what the *envelope itself* provides plus the
//! `workspace.*` lifecycle; payload-schema-driven entities (worktrees, agent
//! runs, dossiers) arrive with their slices (S1/S2) as additive fields. The
//! S0 reducer semantics pinned by the oracle tests and golden fixtures:
//!
//! - every event: `events_folded += 1`, `last_event = Some(event.id)`,
//!   `counts_by_subject[subject] += 1`;
//! - `workspace.opened` with an envelope workspace id: status → `Open`;
//! - `workspace.closed` with an envelope workspace id: status → `Closed`
//!   (inserted even if never opened — the log is truth);
//! - every other subject: counters only.

use std::collections::BTreeMap;

use rezidnt_types::{Event, Subject, WorkspaceId};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Workspace lifecycle status derived from `workspace.opened` / `.closed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceStatus {
    Open,
    Closed,
}

/// One gate's recorded verdict state on a run (S4 — the ORACLE SCAFFOLD:
/// fields exist so the board is assert-red; the reducer arm is implementer
/// work).
///
/// S4 reducer semantics (pinned by `tests/s4_gates.rs` and the
/// `s4_verified_run` / `s4_vet_refusal` golden fixtures; keyed under
/// `AgentRunState::gates` by the payload `gate` name, last write wins):
/// - `gate.entered` `{run, gate}` → `verdict = "entered"`;
/// - `gate.passed` `{run, gate, verifiers: [{verifier, cost_ms, evidence,
///   inputs}]}` → `verdict = "pass"`, `verifier = None`, `evidence` = the
///   verifiers' evidence hashes flattened in order;
/// - `gate.failed` `{run, gate, verifier, evidence, inputs}` →
///   `verdict = "fail"`, the FAILING verifier named, evidence hashes copied;
/// - `gate.inconclusive` (failed shape + `reason`) →
///   `verdict = "inconclusive"` plus `reason` — never coerced (I6).
///
/// Verdicts stay payload-strings (never an enum gate) for the same I3 reason
/// as statuses: reducers fold every live payload version, they never choke.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GateState {
    /// `"entered" | "pass" | "fail" | "inconclusive"` — recorded verbatim.
    pub verdict: String,
    /// The FAILING verifier's name (fail/inconclusive only).
    pub verifier: Option<String>,
    /// Evidence blob hashes (blake3 hex) — refs, never bytes (I2).
    #[serde(default)]
    pub evidence: Vec<String>,
    /// `gate.inconclusive` v1 `reason` (`timeout | nonzero_exit |
    /// malformed_output`), recorded verbatim.
    pub reason: Option<String>,
}

/// One replay-divergence alarm folded onto a run's dossier (DR-006 — the
/// ORACLE SCAFFOLD: the type + field exist so `tests/dr006_integrity_alarms.rs`
/// is assert-red, mirroring the S4 `GateState` scaffold; the reducer arm for
/// `integrity.alarm` is implementer work).
///
/// DR-006 reducer semantics (pinned by `tests/dr006_integrity_alarms.rs`):
/// `integrity.alarm` `{run, gate, verifier, recorded, replayed}` folds onto
/// `AgentRunState::integrity_alarms`, DEDUPED by (gate, verifier) — the log is
/// append-only and debrief is re-runnable, so duplicate facts collapse to one
/// queryable record. Deterministic order (by (gate, verifier)) keeps
/// whole-graph equality stable. Payload shape is an ORACLE PROPOSAL pending
/// the warden `/subject` for `integrity.alarm` (flagged in the work order);
/// verdicts stay payload-strings (never an enum gate) for the same I3 reason
/// as gate verdicts — reducers fold every live payload version.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntegrityAlarmRecord {
    /// The gate the diverging verifier ran under (`vet` | `pre_merge` | …).
    pub gate: String,
    /// The diverging verifier's name.
    pub verifier: String,
    /// Recorded verdict, verbatim (`pass` | `fail` | `inconclusive`).
    pub recorded: String,
    /// Replayed verdict, verbatim — divergence means `recorded != replayed`.
    pub replayed: String,
}

/// DR-017 §Decision 2 (SP4b): one folded `permit.delegated` fact — a capability
/// edge where a lead agent's badge was attenuated (a narrowing caveat appended,
/// re-keyed offline) into a child badge for a sub-agent spawn. Folded onto the
/// run's dossier so the capability chain replays (I3). `added_caveats` is folded
/// VERBATIM (the tagged `Caveat` JSON objects) — the reducer never re-derives
/// the crypto; the log is truth. Rebuild-stable via the same `#[serde(default)]`
/// discipline `integrity_alarms` / `permit_ledger` / `intent` use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DelegationRecord {
    /// Loggable badge id of the PARENT (lead-agent) badge that was attenuated
    /// (`hex(blake3(sig)[..8])`, sig-derived — DR-018 §(a); never the token — I2/§12).
    pub parent_badge_id: String,
    /// Loggable badge id of the CHILD (sub-agent) badge minted by the attenuation.
    pub child_badge_id: String,
    /// The narrowing caveats appended at this step, folded verbatim (I3) — the
    /// tagged first-party `Caveat` JSON objects the macaroon carries.
    pub added_caveats: Vec<serde_json::Value>,
}

/// DR-029 §Decision 6: a run's current composed egress/sandbox POSTURE, folded
/// LAST-WRITE-WINS from `egress.mediated` / `egress.unavailable` facts (a run has
/// one current posture; the latest fact overwrites). The warden taxonomy folds
/// the sandbox posture in as a FIELD (`sandbox`), not a parallel noun — one
/// `compose_degrade` decision, one folded posture. Rebuild-stable via the same
/// `#[serde(default)]` discipline the other DR-* fields use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EgressPostureState {
    /// The network posture (`"mediated"` on `egress.mediated`; `"sealed"`/absent
    /// on `egress.unavailable`), verbatim from the fact. `None` when the fact
    /// omitted it (the Unsandboxed floor — readers key on `sandbox`).
    pub network: Option<String>,
    /// The sandbox discriminator (`"available"` | `"unavailable"`) — the field
    /// that tells the ConfinedClosed floor (sandbox held) from the Unsandboxed
    /// floor (sandbox down). Verbatim from the fact.
    pub sandbox: Option<String>,
    /// Whether egress is enforceable in this posture — `true` only on
    /// `egress.mediated` (the sealed netns is the sole route out); `false` on
    /// both `egress.unavailable` floors. The honesty anchor.
    #[serde(default)]
    pub egress_enforceable: bool,
    /// The composed backend label (`"pasta+bwrap"`, `"bwrap"`, `"none"`),
    /// recorded for replay. `None` when the fact omitted it.
    pub backend: Option<String>,
}

/// DR-029 §Decision 6: one folded `egress.denied` fact — an off-allowlist
/// per-connection denial (`EgressScope` `fail`). Appended in log order so the
/// denial trail replays (many denials per run). By-reference only: the `dest`
/// host + the deciding `policy_ref` hash.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EgressDenial {
    /// The denied (off-allowlist) destination host.
    pub dest: String,
    /// The deciding egress policy's CAS hash (`policy_ref.hash`), so
    /// `gate_explain` can answer WHY denied (I6). `None` if the fact omitted it.
    pub policy_ref: Option<String>,
}

/// DR-029 §Decision 6: one folded `credential.injected` fact — the by-reference
/// audit trail of a brokered secret injected upstream on an approved mediated
/// egress (DR-026 crit 5). Records the `dest` + the `secret_ref` LABEL ONLY —
/// the fact carries NO value, so neither can the fold. Appended in log order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CredentialInjection {
    /// The allowlisted destination the secret was brokered toward.
    pub dest: String,
    /// The brokered secret's LABEL/HASH (`secret_ref`) — the by-reference
    /// contract. NEVER the value (it is not in the fact).
    pub secret_ref: String,
}

/// DR-029 §Decision 2/6: one folded `credential.dropped` fact — the honest-floor
/// audit trail of a `secret_ref` the `SecretSource` could not resolve, so the
/// mapping was dropped (never a fake secret). Records the `dest` + the
/// unresolvable `secret_ref` LABEL. Appended in log order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CredentialDrop {
    /// The destination host that LOST its injection.
    pub dest: String,
    /// The unresolvable `secret_ref` LABEL — carries no value (there is none).
    pub secret_ref: String,
}

/// One entry in a run's permit ledger: an authorization request and, once a
/// decision lands, its outcome (DR-008/DR-009 — the pre-hoc "may" axis). SP5
/// REDUCER SCAFFOLD: the type + fields exist so the permit subjects have a
/// folding consumer (no consumer-less subjects — DR-006 precedent); the
/// contextual C1/C7 permit-verifiers that *read* this ledger are SP0–SP4.
///
/// Reducer semantics (keyed under [`AgentRunState::permit_ledger`] by the
/// payload `request_id`, so request and decision fold onto the same entry):
/// - `permit.requested` `{run, request_id, action, target, ...}` → insert with
///   `action` recorded, `decision = None` (pending);
/// - `permit.granted`/`permit.denied`/`permit.escalated`
///   `{run, request_id, ...}` → set `decision` to `"granted"`/`"denied"`/
///   `"escalated"` on the matching entry (created if the decision is seen
///   before the request — the log is truth, I3, never gatekeeps).
///
/// Decisions stay payload-strings (never an enum gate) for the same I3 reason
/// as gate verdicts — reducers fold every live payload version. `granted`=allow
/// (`pass`), `denied`=deny (`fail`), `escalated`=inconclusive→human (`ask`);
/// `escalated` is never coerced to `granted` (I6, DR-008 §4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PermitLedgerEntry {
    /// The requested action kind (`permit.requested.action`, e.g.
    /// `"tool.invoke"`), recorded verbatim.
    pub action: String,
    /// The requested action target (`permit.requested.target`, e.g.
    /// `{"tool": "Bash"}`), recorded verbatim so `resolve_permit` can DERIVE the
    /// `(action, target)` match key by `request_id` (DR-033 §Design). `None`
    /// while the request has not folded (a decision may fold first, out of
    /// order). Additive; `#[serde(default)]` keeps pre-existing fixtures parsing.
    #[serde(default)]
    pub target: Option<serde_json::Value>,
    /// The decision, once one lands: `"granted" | "denied" | "escalated"`.
    /// `None` while the request is pending (requested but not yet decided).
    pub decision: Option<String>,
    /// The deciding policy's CAS hash (`policy_ref.hash`), recorded so
    /// `gate why` / `gate_explain` can resolve the deciding policy (I6). Set
    /// on the decision fact; `None` while pending.
    pub policy_ref: Option<String>,
    /// Denial / escalation reason, recorded verbatim (`permit.denied.reason`
    /// / `permit.escalated.reason`); `None` for grants and while pending.
    pub reason: Option<String>,
}

/// DR-033 §Decision 1 (slice 2): one folded `permit.resolved` fact — the durable
/// HUMAN-OVERRIDE record that an operator resolved a previously-escalated permit
/// via the `resolve_permit` operator-badged MCP tool. NOT a PDP verdict: the
/// recorded human decision the PDP's pre-verifier ledger-check APPLIES on the
/// agent's NEXT ask for the same action `(run, tool, action/target)`, emitting
/// the corresponding `permit.granted`/`permit.denied` citing this resolution
/// (`resolved_from` = [`Self::request_id`]) as authority (I6). Folded VERBATIM —
/// the reducer never re-derives: `decision` stays the human INPUT VERB
/// (`"allow"`/`"deny"`), NEVER coerced to `granted`/`denied` (that coercion is
/// the PDP's, on the next ask). Appended in log order onto
/// [`AgentRunState::resolutions`]; a new resolution for the same action stands as
/// the applied one (last-matching-wins, DR-033 §Decision 2 "a new resolve
/// overrides"). Rebuild-stable via the same `#[serde(default)]` discipline the
/// other DR-* fields use (I3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PermitResolution {
    /// The ESCALATED ask's `request_id` — the AUDIT correlation (which escalation
    /// this resolution answers), NOT the match key (`request_id` is re-minted per
    /// ask, DR-033 §Context). The applied grant/denial cites this as
    /// `resolved_from`.
    pub request_id: String,
    /// The requested action kind (`permit.requested.action`, e.g.
    /// `"tool.invoke"`) — half the `(run, tool, action/target)` match key.
    pub action: String,
    /// The action target descriptor (`permit.requested.target`, e.g.
    /// `{"tool": "Bash"}`) — the other half of the match key, folded VERBATIM as
    /// the raw JSON so the PDP compares descriptor-to-descriptor with no shape
    /// translation (I2: a small inline descriptor, never bulk bytes).
    pub target: serde_json::Value,
    /// The human's binding choice, the override the PDP applies — the INPUT VERB
    /// `"allow"`/`"deny"`, folded VERBATIM (never coerced to `granted`/`denied`;
    /// that is the PDP's job on the next ask, I6).
    pub decision: String,
    /// The loggable operator badge id of the human who resolved this
    /// (`hex(blake3(sig)[..8])` / DR-005 opaque operator id), folded VERBATIM —
    /// NEVER the token (§12/I2). `None` when the fact omitted it.
    pub operator_badge_id: Option<String>,
    /// The operator-supplied reason, folded VERBATIM (I6 interrogability). `None`
    /// when absent — never synthesized.
    pub reason: Option<String>,
    /// DR-035 §Decision 1 — the optional TTL, a millisecond DURATION relative to
    /// this resolution's OWN envelope-ULID timestamp ([`Self::resolved_at_ms`]).
    /// The expiry deadline is `resolved_at_ms + ttl_ms`; a resolution applies to
    /// an incoming request iff the request's envelope-ULID timestamp is `<=` that
    /// deadline (inclusive), else it is skipped and the request re-escalates.
    /// `None` = permanent (DR-033 §Decision 2 behavior, never filtered). Folded
    /// VERBATIM from the fact; `#[serde(default)]` so a pre-DR-035 fixture (no
    /// `ttl_ms`) parses unchanged and folds identically (additive, rebuild-stable,
    /// I3).
    #[serde(default)]
    pub ttl_ms: Option<u64>,
    /// DR-035 §Decision 1 — T0, the anchor the TTL deadline is measured from: this
    /// resolution's OWN envelope-ULID timestamp (`event.id.timestamp_ms()`),
    /// captured at fold time. NOT a payload field (there is no `created_at` on the
    /// fact — DR-035 §Decision 1 derives it from the envelope ULID already on the
    /// log, so expiry stays a pure fold with no hidden clock, I3). `#[serde(default)]`
    /// = `0` for a pre-DR-035 fixture: harmless because such a fixture also has no
    /// `ttl_ms`, so the deadline is never consulted (permanent).
    #[serde(default)]
    pub resolved_at_ms: u64,
    /// DR-035 §Decision 2 — the optional grant-all scope: a single-axis wildcard
    /// widening the match from the exact `(run, tool, action/target)` to a class.
    /// The only value in v1 is `"run_tool"` = "any action on this `(run, tool)`";
    /// `None` = exact request-scoped match (DR-033 behavior). Folded VERBATIM from
    /// the fact; `#[serde(default)]` so a pre-DR-035-grant-all fixture (no `scope`)
    /// parses unchanged and matches exactly as before (additive, rebuild-stable,
    /// I3). The broadening of `resolution_for`'s match predicate that CONSUMES
    /// this is implementer scope (NOT folded-into behavior yet): today the field is
    /// carried inert. COUPLING (DR-035 §Decision 3): a `Some("run_tool")` obligates
    /// a `Some(ttl_ms)` — but that is enforced at the `resolve_permit` tool
    /// boundary, so a broad-and-permanent resolution can never reach this fold.
    #[serde(default)]
    pub scope: Option<String>,
}

/// A run's per-session permit accumulators: the running state the *contextual*
/// (stateful) permit-verifiers read to make C6/C7 decisions — a pure fold over
/// the log (I3), not held imperatively as Omnigent does (intel memo 001 C6).
/// SP5 REDUCER SCAFFOLD: fields exist so the accumulator inputs on the permit
/// decision payloads have a folding consumer; the verifiers that read these
/// are SP0–SP4.
///
/// Reducer semantics: every `permit.granted`/`.denied`/`.escalated` fold adds
/// the payload's optional `spend_delta_usd` / `risk_delta` (when present) to
/// the running totals and increments the decision counters. Sized per the
/// intel-memo C6/C7 note so the payload supports contextual decisions without
/// a `v+1`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PermitAccumulators {
    /// Running cumulative spend charged to this run's granted/denied actions
    /// (`sum(spend_delta_usd)`); read by the C1 spend-cap verifiers.
    #[serde(default)]
    pub cumulative_spend_usd: f64,
    /// Running per-session risk score (`sum(risk_delta)`); read by the C6
    /// contextual policy.
    #[serde(default)]
    pub risk_score: f64,
    /// Count of decisions folded, by outcome — cheap contextual signal
    /// (e.g. escalation rate) and a rebuild-stable counter.
    #[serde(default)]
    pub granted: u64,
    #[serde(default)]
    pub denied: u64,
    #[serde(default)]
    pub escalated: u64,
}

/// A run's declared intent — the initiating task + the intent-scoped tool set
/// the `intent-lock` permit-verifier enforces (DR-010; the run-intent axis).
/// SP-intent REDUCER SCAFFOLD: the type + field exist so `run.intent.declared`
/// has a folding consumer (no consumer-less subjects — DR-006 precedent); the
/// `intent-lock` verifier that *reads* this pinned state is the NEXT slice
/// (SP-intent, oracle-first — NOT built here).
///
/// Reducer semantics (folded onto [`AgentRunState::intent`], keyed by the
/// payload `run`, last write wins):
/// - `run.intent.declared` `{run, intent_ref: CasRef, allowed_tools: [string]}`
///   → set `intent = Some(IntentState { allowed_tools, intent_ref })`.
///
/// The enforced set is the DECLARED list read verbatim (never re-derived — the
/// determinism BINDING forbids a live inference at decision time; DR-010 §3).
/// **Distinct from `agent.spawned.allowed_tools?`**: that is the *composed
/// harness allowlist* (what the agent was configured with); this is the
/// *intent-derived least-privilege set* the verifier checks against (DR-010 §4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntentState {
    /// The intent-scoped tool names the `intent-lock` verifier enforces
    /// (in-set → allow; off-task → escalate / deny under the knob).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// blake3 hex of the initiating task/prompt text persisted to the CAS
    /// (`intent_ref.hash`) — a ref, never inline bytes (I2). The interrogable
    /// "what was this run for" `gate_explain` names alongside the off-task tool.
    pub intent_ref: Option<String>,
}

/// One agent run's derived state (S1: the dossier's accounting seed).
///
/// S1 reducer semantics (pinned by `tests/s1_agent_runs.rs` and the
/// `s1_agent_run` golden fixture; payload schemas pending warden ratification):
/// - `agent.spawned` `{run, ...}` → insert with `status = "spawning"`;
/// - `agent.status.changed` `{run, from, to}` → `status = to`;
/// - `agent.completed` `{run, status, cost{total_usd,input_tokens,
///   output_tokens}, session_id, ...}` → `status = "completed"`, accounting
///   fields recorded.
///
/// Statuses stay payload-strings in the graph: reducers must fold any live
/// payload version, so they do not gatekeep through an enum (I3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentRunState {
    pub status: String,
    pub total_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub session_id: Option<String>,
    /// S4: recorded gate verdicts, keyed by gate name (see [`GateState`]).
    /// `#[serde(default)]` keeps every pre-S4 golden fixture parsing
    /// unedited; a gate fact on a run the log never spawned still creates
    /// the entry (default `status: ""`) — the log is truth (I3), and a
    /// pre-spawn vet refusal is exactly such a run.
    #[serde(default)]
    pub gates: BTreeMap<String, GateState>,
    /// DR-006: replay-divergence alarms on this run (see
    /// [`IntegrityAlarmRecord`]). ORACLE SCAFFOLD — field present so the
    /// DR-006 board is assert-red; the `integrity.alarm` reducer arm is
    /// implementer work. `#[serde(default)]` keeps every pre-DR-006 golden
    /// fixture parsing (and comparing equal) unedited. Deduped by
    /// (gate, verifier), deterministic order.
    #[serde(default)]
    pub integrity_alarms: Vec<IntegrityAlarmRecord>,
    /// DR-008/DR-009: this run's permit ledger, keyed by `request_id`
    /// (request→decision folds onto one entry). SP5 REDUCER SCAFFOLD — the
    /// permit subjects' folding consumer (no consumer-less subjects, DR-006
    /// precedent). `#[serde(default)]` keeps every pre-permit golden fixture
    /// parsing (and comparing equal) unedited. `BTreeMap` for deterministic
    /// whole-graph equality.
    #[serde(default)]
    pub permit_ledger: BTreeMap<String, PermitLedgerEntry>,
    /// DR-008/DR-009: this run's per-session permit accumulators (running
    /// spend / risk / decision counts) — the state the contextual C1/C7
    /// permit-verifiers read, folded purely from the log (I3). SP5 REDUCER
    /// SCAFFOLD. `#[serde(default)]` keeps every pre-permit golden fixture
    /// unedited.
    #[serde(default)]
    pub permit_accumulators: PermitAccumulators,
    /// DR-010: this run's declared intent (see [`IntentState`]) — the pinned
    /// state the future `intent-lock` permit-verifier reads. SP-intent REDUCER
    /// SCAFFOLD. `None` until a `run.intent.declared` fact folds. `#[serde(default)]`
    /// keeps every pre-DR-010 golden fixture parsing (and comparing equal)
    /// unedited.
    #[serde(default)]
    pub intent: Option<IntentState>,
    /// DR-014 §Decision 5: this run's PEP enforcement mode, folded from
    /// `agent.spawned.pep?`. `Some("enforced")` iff the spawn wired the permit
    /// PEP (the spec declared a `[gates.permit]` gate); `None` = edge-gated-only
    /// (no mid-run interception). The value is recorded VERBATIM as a string
    /// (never a bool) so a future degraded/partial mode arrives additively; the
    /// ABSENT case is never synthesized to a truthy value (DR-012 declared-vs-
    /// absent; the honest "no PEP wired"). `#[serde(default)]` keeps every
    /// pre-DR-014 golden fixture parsing (and comparing equal) unedited (I3
    /// rebuild-stability). Read through [`AgentRunState::pep_enforced`].
    #[serde(default)]
    pub pep: Option<String>,
    /// DR-016 §Decision 2 (SP4a): this run's RBAC role, folded from
    /// `agent.spawned.role?` (ontology line 195). `Some(role)` iff the spawn
    /// carried a role — taken VERBATIM (an opaque string the policy interprets;
    /// rezidnt mints no role vocabulary). `None` = no role declared: ABSENT,
    /// never synthesized to a default like `"contributor"` (DR-012 declared-vs-
    /// absent; the honest "no role"). This is the permit input axis
    /// `decide_permit` injects into its content-pinned per-run params so a
    /// role-keyed policy can decide on role + workspace + action. `#[serde(default)]`
    /// keeps every pre-DR-016 golden fixture parsing and comparing equal
    /// unedited (I3 rebuild-stability), exactly as `pep` does.
    #[serde(default)]
    pub role: Option<String>,
    /// DR-017 §Decision 2 (SP4b): this run's capability-delegation chain, folded
    /// from `permit.delegated` facts (ontology). Each attenuation-and-handoff at
    /// a sub-agent spawn earns one [`DelegationRecord`], in append (log) order so
    /// the chain replays deterministically (I3). `#[serde(default)]` keeps every
    /// pre-DR-017 golden fixture parsing (and comparing equal) unedited — the
    /// exact rebuild-stability discipline `integrity_alarms` / `permit_ledger` /
    /// `intent` / `role` already use.
    #[serde(default)]
    pub delegations: Vec<DelegationRecord>,
    /// DR-029 §Decision 6: this run's current composed egress/sandbox posture,
    /// folded LAST-WRITE-WINS from `egress.mediated` / `egress.unavailable`
    /// facts (a run has one current posture). `None` until a posture fact folds.
    /// `#[serde(default)]` keeps every pre-DR-029 golden fixture parsing (and
    /// comparing equal) unedited — the exact rebuild-stability discipline the
    /// other DR-* fields use (I3).
    #[serde(default)]
    pub egress: Option<EgressPostureState>,
    /// DR-029 §Decision 6: this run's off-allowlist denials, folded from
    /// `egress.denied` facts in append (log) order — the replayable denial
    /// trail. `#[serde(default)]` keeps pre-DR-029 fixtures unedited (I3).
    #[serde(default)]
    pub egress_denials: Vec<EgressDenial>,
    /// DR-029 §Decision 6: this run's by-reference credential-injection audit
    /// trail, folded from `credential.injected` facts in append order —
    /// `secret_ref`/`dest` ONLY, never a value (DR-026 crit 5). `#[serde(default)]`
    /// keeps pre-DR-029 fixtures unedited (I3).
    #[serde(default)]
    pub credential_injections: Vec<CredentialInjection>,
    /// DR-029 §Decision 2/6: this run's dropped-credential audit trail, folded
    /// from `credential.dropped` facts in append order — the honest floor (a
    /// mapping was dropped, never a fake secret injected). `#[serde(default)]`
    /// keeps pre-DR-029 fixtures unedited (I3).
    #[serde(default)]
    pub credential_drops: Vec<CredentialDrop>,
    /// DR-032 §Decision 5: the loggable operator badge id of the human who
    /// KILLED this run, folded VERBATIM from an `agent.signaled` fact carrying
    /// `operator_badge_id` (`hex(blake3(sig)[..8])`, the loggable id — NEVER the
    /// token, I2/§12). `None` = NOT operator-killed: a daemon-initiated reaper
    /// TERM→KILL stop carries no `operator_badge_id`, so this stays `None` —
    /// ABSENCE is the honest representation, NEVER synthesized to a sentinel
    /// (DR-012 declared-vs-absent). This is the interrogable "a human killed this
    /// run" record `debrief` / `gate_explain` reads, DISTINCT from a
    /// daemon-timeout stop (I6). `#[serde(default)]` keeps every pre-DR-032 golden
    /// fixture parsing (and comparing equal) unedited — I3 rebuild-stability.
    #[serde(default)]
    pub killed_by: Option<String>,
    /// DR-032 §Decision 5: the operator-supplied kill reason, folded VERBATIM
    /// from an `agent.signaled` fact's `reason`. `None` when absent (a daemon
    /// stop has no operator and no reason) — never synthesized to an empty
    /// string. Interrogable alongside [`AgentRunState::killed_by`] (I6).
    /// `#[serde(default)]` keeps every pre-DR-032 golden fixture unedited (I3).
    #[serde(default)]
    pub kill_reason: Option<String>,
    /// DR-033 §Decision 1 (slice 2): this run's folded `permit.resolved` human
    /// overrides (see [`PermitResolution`]), in append (log) order so the
    /// override history replays deterministically (I3). The PDP ledger-check finds
    /// the applicable resolution by ACTION identity `(run, tool, action/target)`
    /// via [`AgentRunState::resolution_for`] — a re-ask carries a fresh
    /// `request_id`, so the match keys on the action descriptor, not the id
    /// (DR-033 §Context/§Decision 3). A NEW resolution for the same action appends
    /// and wins (last-matching-wins, DR-033 §Decision 2). `#[serde(default)]` keeps
    /// every pre-DR-033 golden fixture parsing (and comparing equal) unedited — the
    /// exact rebuild-stability discipline `delegations` / `integrity_alarms` /
    /// `permit_ledger` already use (I3).
    #[serde(default)]
    pub resolutions: Vec<PermitResolution>,
}

/// DR-035 §Decision 2 — the action-axis match predicate shared by the applied
/// (`resolution_for`) and interrogability (`expired_resolution_for`) scans. A
/// resolution with `scope == Some("run_tool")` is BROAD: it matches ANY incoming
/// action (the action/target axis is wildcarded; the tool axis stays exact,
/// enforced by the caller's separate `target.tool` check). Absent (or any other)
/// scope stays DR-033's EXACT `action` match — a sibling action never matches, so
/// the request-scoped default is un-broadened. Pure, no clock (I3): the branch
/// reads only the folded record and the incoming action string.
fn action_matches(r: &PermitResolution, action: &str) -> bool {
    match r.scope.as_deref() {
        Some("run_tool") => true,
        _ => r.action == action,
    }
}

impl AgentRunState {
    /// Whether this run was mid-run-PEP-enforced — `true` iff the spawn folded
    /// `pep == "enforced"` (DR-014 §Decision 5). ABSENCE folds `false`: a run
    /// with no `pep` on its spawn is edge-gated-only, NEVER synthesized to
    /// enforced (the honesty the `gate_explain` distinction rests on, I4).
    pub fn pep_enforced(&self) -> bool {
        self.pep.as_deref() == Some("enforced")
    }

    /// DR-033 §Decision 1/3 (slice 2): the resolution the PDP would APPLY for an
    /// incoming ask matching `(action, tool)` on this run — the LATEST folded
    /// `permit.resolved` whose `action` and `target.tool` match (last-matching-
    /// wins, DR-033 §Decision 2). `None` when no resolution answers this action
    /// (the request-scoped guard: a resolution for one action never grants
    /// another, DR-033 §Decision 3). The match keys on ACTION identity, NOT
    /// `request_id` (which is re-minted per ask, DR-033 §Context).
    ///
    /// DR-035 §Decision 1 — the log-derived EXPIRY FILTER. `incoming_ms` is the
    /// incoming `permit.requested`'s OWN envelope-ULID timestamp (both sides of the
    /// comparison come from event ULIDs already on the log, so this stays a pure
    /// fold — no `SystemTime::now()`, replay-deterministic, I3). A resolution with
    /// `ttl_ms == Some(n)` is SKIPPED when `incoming_ms > resolved_at_ms + n`
    /// (past its deadline → the request re-escalates); the deadline is INCLUSIVE
    /// (`<=` applies). A resolution with `ttl_ms == None` is PERMANENT and never
    /// filtered (DR-033 §Decision 2, unchanged). Last-matching-wins holds among the
    /// still-VALID resolutions (an expired one is passed over, so an older
    /// permanent resolution below it can still win — the reverse scan filters, it
    /// does not stop). Saturating arithmetic on `resolved_at_ms + n` so a
    /// pathological `u64` ttl cannot overflow (a saturated deadline of `u64::MAX`
    /// simply means "always applies", the intended sense of an absurd ttl).
    pub fn resolution_for(
        &self,
        action: &str,
        tool: &str,
        incoming_ms: u64,
    ) -> Option<&PermitResolution> {
        self.resolutions.iter().rev().find(|r| {
            action_matches(r, action)
                && r.target.get("tool").and_then(serde_json::Value::as_str) == Some(tool)
                && match r.ttl_ms {
                    // Permanent (DR-033 §Decision 2) — never filtered.
                    None => true,
                    // Expiry deadline is INCLUSIVE (`<=`, DR-035 §Decision 1).
                    Some(n) => incoming_ms <= r.resolved_at_ms.saturating_add(n),
                }
        })
    }

    /// DR-035 §Invariants I6 — the interrogability COMPANION to
    /// [`Self::resolution_for`]: the resolution that WOULD have matched
    /// `(action, tool)` at `incoming_ms` but was SKIPPED because its TTL had
    /// expired (`ttl_ms == Some(n)` AND `incoming_ms > resolved_at_ms + n`). This
    /// is what lets `gate why`/`debrief` say "not applied: resolution expired at
    /// ULID T → re-escalated" instead of a silent vanish — an expired override is
    /// EXPLAINABLE, never a silent coercion (mirrors how `resolved_from` makes an
    /// APPLIED resolution interrogable). Returns the NEWEST such expired match
    /// (reverse scan, same last-matching order as the applied path), or `None`
    /// when no matching resolution exists OR the newest match is still live (in
    /// which case `resolution_for` returns it — the two are complementary). A
    /// permanent resolution (`ttl_ms == None`) is never "expired", so it is never
    /// reported here. Same saturating-deadline discipline as the filter.
    pub fn expired_resolution_for(
        &self,
        action: &str,
        tool: &str,
        incoming_ms: u64,
    ) -> Option<&PermitResolution> {
        self.resolutions.iter().rev().find(|r| {
            action_matches(r, action)
                && r.target.get("tool").and_then(serde_json::Value::as_str) == Some(tool)
                && matches!(r.ttl_ms, Some(n) if incoming_ms > r.resolved_at_ms.saturating_add(n))
        })
    }
}

/// One worktree's derived state (S2: the sole-allocator registry's shadow in
/// the graph — the log is truth, the `.rezidnt/worktrees` file is the
/// adapter's working copy).
///
/// S2 reducer semantics (pinned by `tests/s2_worktrees.rs` and the
/// `s2_worktree_conflict` / `s2_diff_ready` golden fixtures). Entries key on
/// the payload's canonicalized path string:
/// - `worktree.allocated` `{path, branch?, allocator}` → insert with
///   `status = "allocated"`, `branch`/`allocator` copied from the payload;
/// - `worktree.observed` `{path, allocator}` → insert with
///   `status = "observed"`, allocator copied (out-of-band guard, DR-001);
/// - `worktree.conflict` `{path}` → `conflicts += 1` on the existing entry
///   (first claim's fields untouched — never double-tracked); the
///   exactly-once emission obligation is the ADAPTER's, so the reducer counts
///   every logged conflict honestly (I3);
/// - `worktree.released` `{path}` → `status = "released"` (inserted even if
///   never allocated — the log is truth);
/// - `diff.ready` `{worktree, diff: CasRef}` → `last_diff = Some(diff.hash)`
///   on the entry keyed by `worktree`.
///
/// S4 addition (pinned by `tests/s4_gates.rs` and the `s4_verified_run`
/// fixture; payload shape PENDING warden ratification — `diff.merged` is a
/// proposed subject, flagged in the S4 work order):
/// - `diff.merged` `{run, worktree, diff: CasRef}` → `status = "merged"`,
///   `last_diff = Some(diff.hash)` (inserted even if never allocated — the
///   log is truth, I3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorktreeState {
    pub status: String,
    pub branch: Option<String>,
    /// `"rezidnt"` (sole allocator) or `"human"` (out-of-band observation).
    pub allocator: Option<String>,
    #[serde(default)]
    pub conflicts: u64,
    /// blake3 hex of the most recent `diff.ready` summary ref.
    pub last_diff: Option<String>,
}

/// The entity graph. `BTreeMap` everywhere so equality and serialization
/// are deterministic (the property tests compare whole graphs).
///
/// S1 adds `agent_runs`, S2 adds `worktrees` — both additively:
/// `#[serde(default)]` keeps every earlier golden fixture parsing (and
/// comparing equal) unedited.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub events_folded: u64,
    pub last_event: Option<Ulid>,
    pub counts_by_subject: BTreeMap<Subject, u64>,
    pub workspaces: BTreeMap<WorkspaceId, WorkspaceStatus>,
    /// Keyed by the run ULID's canonical text form (payload `run` field).
    #[serde(default)]
    pub agent_runs: BTreeMap<String, AgentRunState>,
    /// Keyed by the canonicalized worktree path (payload `path` /
    /// `worktree` field).
    #[serde(default)]
    pub worktrees: BTreeMap<String, WorktreeState>,
}

/// The pure reducer (doc §6: `fn apply(&mut Graph, &Event)`). No IO, no
/// clocks, no randomness — same event, same graph delta, every time.
pub fn apply(graph: &mut Graph, event: &Event) {
    graph.events_folded += 1;
    graph.last_event = Some(event.id);
    *graph
        .counts_by_subject
        .entry(event.subject.clone())
        .or_insert(0) += 1;

    match event.subject.as_str() {
        "workspace.opened" => {
            if let Some(ws) = event.workspace {
                graph.workspaces.insert(ws, WorkspaceStatus::Open);
            }
        }
        "workspace.closed" => {
            // Inserted even if never opened — the log is truth (I3).
            if let Some(ws) = event.workspace {
                graph.workspaces.insert(ws, WorkspaceStatus::Closed);
            }
        }
        // S1 agent-run reducers, keyed by the payload `run` string. A payload
        // without a `run` (pre-ratification fixture lines, foreign versions)
        // folds as counters-only — reducers never choke, never guess (I3).
        "agent.spawned" => {
            if let Some(run) = payload_run(event) {
                let state = graph.agent_runs.entry(run).or_default();
                state.status = "spawning".to_string();
                // DR-014 §Decision 5: fold the enforcement mode VERBATIM when
                // present (`pep: "enforced"`); ABSENT stays `None` — never
                // synthesized to a truthy value (DR-012; the honest "no PEP
                // wired"). A pre-DR-014 spawn (no `pep`) folds edge-gated-only.
                if let Some(pep) = event.payload()["pep"].as_str() {
                    state.pep = Some(pep.to_string());
                }
                // DR-016 §Decision 2 (SP4a): fold the RBAC role VERBATIM when
                // present; ABSENT stays `None` — never synthesized to a default
                // (DR-012; the honest "no role declared"). A pre-DR-016 spawn (no
                // `role`) folds role-less, keeping rebuild stable (I3).
                if let Some(role) = event.payload()["role"].as_str() {
                    state.role = Some(role.to_string());
                }
            }
        }
        "agent.status.changed" => {
            if let Some(run) = payload_run(event)
                && let Some(to) = event.payload()["to"].as_str()
            {
                graph.agent_runs.entry(run).or_default().status = to.to_string();
            }
        }
        "agent.completed" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let state = graph.agent_runs.entry(run).or_default();
                state.status = "completed".to_string();
                state.total_usd = payload["cost"]["total_usd"].as_f64();
                state.input_tokens = payload["cost"]["input_tokens"].as_u64();
                state.output_tokens = payload["cost"]["output_tokens"].as_u64();
                state.session_id = payload["session_id"].as_str().map(String::from);
            }
        }
        // DR-032 §Decision 5: the `agent.signaled` kill-attribution fold, keyed
        // on the payload `run`. An OPERATOR kill carries `operator_badge_id` +
        // `reason`, which fold VERBATIM onto `killed_by` / `kill_reason`
        // (mirroring the `pep`/`role` optional-field fold above). A DAEMON stop
        // (reaper TERM→KILL) carries NEITHER, so both stay `None` — ABSENCE is
        // the honest representation, NEVER synthesized to a sentinel (DR-012;
        // the daemon-vs-operator distinction `debrief` reads, I6). This arm does
        // NOT touch `status` — the signaled run-status transition rides
        // `agent.status.changed`, untouched here. A keyless fact folds
        // counters-only (never guesses a key, I3). `#[serde(default)]` on the
        // fields keeps rebuild stable across the schema addition (I3).
        "agent.signaled" => {
            if let Some(run) = payload_run(event) {
                let state = graph.agent_runs.entry(run).or_default();
                if let Some(id) = event.payload()["operator_badge_id"].as_str() {
                    state.killed_by = Some(id.to_string());
                }
                if let Some(reason) = event.payload()["reason"].as_str() {
                    state.kill_reason = Some(reason.to_string());
                }
            }
        }
        // S2 worktree reducers, keyed by the payload's canonicalized path
        // string. A payload without its key folds as counters-only —
        // reducers never choke, never gatekeep (I3).
        "worktree.allocated" => {
            if let Some(path) = payload_path(event) {
                let payload = event.payload();
                let wt = graph.worktrees.entry(path).or_default();
                wt.status = "allocated".to_string();
                wt.branch = payload["branch"].as_str().map(String::from);
                wt.allocator = payload["allocator"].as_str().map(String::from);
            }
        }
        "worktree.observed" => {
            if let Some(path) = payload_path(event) {
                let payload = event.payload();
                let wt = graph.worktrees.entry(path).or_default();
                wt.status = "observed".to_string();
                wt.branch = payload["branch"].as_str().map(String::from);
                wt.allocator = payload["allocator"].as_str().map(String::from);
            }
        }
        "worktree.conflict" => {
            // Never mints a second entry (no double-tracking) and never
            // touches the first claim's fields; every logged fact counts
            // once — exactly-once emission is the adapter's obligation (I3).
            if let Some(path) = payload_path(event) {
                graph.worktrees.entry(path).or_default().conflicts += 1;
            }
        }
        "worktree.released" => {
            // Inserted even if never allocated — the log is truth (I3).
            if let Some(path) = payload_path(event) {
                graph.worktrees.entry(path).or_default().status = "released".to_string();
            }
        }
        "diff.ready" => {
            if let Some(path) = event.payload()["worktree"].as_str()
                && let Some(hash) = event.payload()["diff"]["hash"].as_str()
            {
                graph
                    .worktrees
                    .entry(path.to_string())
                    .or_default()
                    .last_diff = Some(hash.to_string());
            }
        }
        "diff.merged" => {
            // Golden-path merge/worktree-lifecycle-close fact; inserted even
            // if the worktree was never allocated — the log is truth (I3).
            if let Some(path) = event.payload()["worktree"].as_str() {
                let wt = graph.worktrees.entry(path.to_string()).or_default();
                wt.status = "merged".to_string();
                if let Some(hash) = event.payload()["diff"]["hash"].as_str() {
                    wt.last_diff = Some(hash.to_string());
                }
            }
        }
        // S4 gate reducers, keyed under `AgentRunState::gates` by the payload
        // `gate` name (last write wins). A gate fact creates the run entry
        // with no synthesized status — a pre-spawn vet refusal is exactly such
        // a run (I3: the log is truth, gate facts do not require a spawn).
        "gate.entered" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "entered".to_string(),
                    ..GateState::default()
                };
            }
        }
        "gate.passed" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                // Flatten every verifier record's evidence hashes in order.
                let evidence = event.payload()["verifiers"]
                    .as_array()
                    .map(|records| {
                        records
                            .iter()
                            .flat_map(|r| r["evidence"].as_array().cloned().unwrap_or_default())
                            .filter_map(|e| e["hash"].as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "pass".to_string(),
                    verifier: None,
                    evidence,
                    reason: None,
                };
            }
        }
        "gate.failed" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "fail".to_string(),
                    verifier: event.payload()["verifier"].as_str().map(String::from),
                    evidence: evidence_hashes(event),
                    reason: None,
                };
            }
        }
        "gate.inconclusive" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let g = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .gates
                    .entry(gate)
                    .or_default();
                *g = GateState {
                    verdict: "inconclusive".to_string(),
                    verifier: event.payload()["verifier"].as_str().map(String::from),
                    evidence: evidence_hashes(event),
                    // Recorded verbatim — never coerced (I6).
                    reason: event.payload()["reason"].as_str().map(String::from),
                };
            }
        }
        // DR-006 replay-divergence alarm, folded onto the run's dossier. A
        // payload without a `run` folds as counters-only (never guesses a key,
        // I3). Deduped by (gate, verifier) within the run — the log is
        // append-only and debrief is re-runnable, so duplicate facts collapse
        // to one queryable record. Deterministic order (by (gate, verifier))
        // keeps whole-graph equality stable.
        "integrity.alarm" => {
            if let Some(run) = payload_run(event)
                && let Some(gate) = payload_gate(event)
            {
                let payload = event.payload();
                if let Some(verifier) = payload["verifier"].as_str() {
                    let alarms = &mut graph.agent_runs.entry(run).or_default().integrity_alarms;
                    // Dedup by (gate, verifier); duplicate facts do not grow
                    // the vec (the raw log still holds every fact).
                    if !alarms
                        .iter()
                        .any(|a| a.gate == gate && a.verifier == verifier)
                    {
                        let record = IntegrityAlarmRecord {
                            gate,
                            verifier: verifier.to_string(),
                            recorded: payload["recorded"].as_str().unwrap_or_default().to_string(),
                            replayed: payload["replayed"].as_str().unwrap_or_default().to_string(),
                        };
                        // Insert in deterministic (gate, verifier) order so
                        // whole-graph equality is interleaving-independent.
                        let pos = alarms
                            .binary_search_by(|a| {
                                (a.gate.as_str(), a.verifier.as_str())
                                    .cmp(&(record.gate.as_str(), record.verifier.as_str()))
                            })
                            .unwrap_or_else(|e| e);
                        alarms.insert(pos, record);
                    }
                }
            }
        }
        // DR-008/DR-009 permit reducers (the pre-hoc "may" axis). Keyed under
        // the run by the payload `request_id` (request and decision fold onto
        // one ledger entry). A payload without `run`/`request_id` folds as
        // counters-only — reducers never choke, never guess a key (I3). The
        // decision facts also accumulate per-session running state (spend/risk)
        // that the contextual C1/C7 verifiers read (SP0–SP4 work; this is the
        // fold that feeds them).
        "permit.requested" => {
            if let Some(run) = payload_run(event)
                && let Some(request_id) = payload_request_id(event)
            {
                let entry = graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .permit_ledger
                    .entry(request_id)
                    .or_default();
                // Record the requested action and target; a decision may
                // already have folded first (out-of-order log), so only fill the
                // request fields — never clobber a decision (the log is truth,
                // I3). The `target` is what `resolve_permit` DERIVES the match
                // key from by `request_id` (DR-033 §Design).
                entry.action = event.payload()["action"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let target = &event.payload()["target"];
                if !target.is_null() {
                    entry.target = Some(target.clone());
                }
            }
        }
        "permit.granted" => apply_permit_decision(graph, event, "granted"),
        "permit.denied" => apply_permit_decision(graph, event, "denied"),
        "permit.escalated" => apply_permit_decision(graph, event, "escalated"),
        // DR-033 §Decision 1 (slice 2): the `permit.resolved` human-override
        // fact. Keyed on the payload `run`; appends one [`PermitResolution`] in
        // log order so the override history replays and a NEW resolution for the
        // same action wins (last-matching-wins via `resolution_for`'s reverse
        // scan, DR-033 §Decision 2). The fact folds VERBATIM (the reducer never
        // re-derives — `decision` stays the human input verb `allow`/`deny`, I3;
        // `operator_badge_id`/`reason` are the loggable attribution, never the
        // token). A keyless fact (missing `run`) folds counters-only / no-op,
        // never mints a run, never panics — the established permit/intent
        // discipline (I3, never guess a key). `target` folds as the raw JSON
        // descriptor (the next-ask match key).
        "permit.resolved" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let record = PermitResolution {
                    request_id: payload["request_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    action: payload["action"].as_str().unwrap_or_default().to_string(),
                    target: payload["target"].clone(),
                    decision: payload["decision"].as_str().unwrap_or_default().to_string(),
                    operator_badge_id: payload["operator_badge_id"].as_str().map(String::from),
                    reason: payload["reason"].as_str().map(String::from),
                    // DR-035 §Decision 1: the optional TTL folds VERBATIM (absent =
                    // permanent, DR-033 §Decision 2). `resolved_at_ms` is T0 — this
                    // resolution's OWN envelope-ULID timestamp, captured from
                    // `event.id` (NOT a payload field; DR-035 derives the anchor from
                    // the envelope ULID already on the log so expiry is a pure fold).
                    ttl_ms: payload["ttl_ms"].as_u64(),
                    resolved_at_ms: event.id.timestamp_ms(),
                    // DR-035 §Decision 2 (slice `escalation-grant-all`): the grant-all
                    // scope folds VERBATIM from the fact (absent = exact request-scoped
                    // match, DR-033 §Decision 3; `Some("run_tool")` = the single-axis
                    // wildcard `resolution_for` consumes to grant any action on this
                    // `(run, tool)`). Additive/rebuild-stable: a legacy resolution with
                    // no `scope` folds to `None` and matches exactly as before (I3).
                    scope: payload["scope"].as_str().map(String::from),
                };
                graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .resolutions
                    .push(record);
            }
        }
        // DR-010 run-intent reducer (the run-intent axis). Keyed by the payload
        // `run`; folds onto `AgentRunState::intent` the pinned state the future
        // `intent-lock` verifier reads. A payload without `run` folds as
        // counters-only — the reducer never guesses a key, never chokes (I3,
        // the permit-reducer discipline). The enforced set is the DECLARED list
        // read verbatim (never re-derived; DR-010 §3).
        "run.intent.declared" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let allowed_tools = payload["allowed_tools"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let intent_ref = payload["intent_ref"]["hash"].as_str().map(String::from);
                graph.agent_runs.entry(run).or_default().intent = Some(IntentState {
                    allowed_tools,
                    intent_ref,
                });
            }
        }
        // DR-017 §Decision 2 (SP4b): the `permit.delegated` capability-chain
        // fact. Keyed by the payload `run` (the lead agent's run under which the
        // attenuation happened); appends one DelegationRecord in log order so the
        // chain replays (I3). A keyless fact (missing `run`) folds counters-only
        // / no-op, never panics — the established permit-reducer discipline
        // (mirrors `apply_permit_decision`). `added_caveats` folds VERBATIM (the
        // tagged Caveat JSON); an absent array folds empty, never chokes.
        "permit.delegated" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let added_caveats = payload["added_caveats"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let record = DelegationRecord {
                    parent_badge_id: payload["parent_badge_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    child_badge_id: payload["child_badge_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    added_caveats,
                };
                graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .delegations
                    .push(record);
            }
        }
        // DR-029 §Decision 6: the composed egress/sandbox POSTURE facts. Both
        // fold LAST-WRITE-WINS onto `AgentRunState::egress` (a run has one current
        // posture). Keyed on the payload `run`; a keyless fact folds counters-only
        // / no-op, never mints a run, never panics — the established
        // permit/intent discipline (I3, never guess a key). A posture fact needs
        // no prior `agent.spawned` — the log is truth (I3), so it mints the run.
        "egress.mediated" | "egress.unavailable" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                graph.agent_runs.entry(run).or_default().egress = Some(EgressPostureState {
                    network: payload["network"].as_str().map(String::from),
                    sandbox: payload["sandbox"].as_str().map(String::from),
                    egress_enforceable: payload["egress_enforceable"].as_bool().unwrap_or(false),
                    backend: payload["backend"].as_str().map(String::from),
                });
            }
        }
        // DR-029 §Decision 6: an off-allowlist per-connection denial. Appends an
        // EgressDenial in log order (many denials per run — the replayable trail).
        // Keyless fact → counters-only no-op (I3).
        "egress.denied" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let record = EgressDenial {
                    dest: payload["dest"].as_str().unwrap_or_default().to_string(),
                    policy_ref: payload["policy_ref"]["hash"].as_str().map(String::from),
                };
                graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .egress_denials
                    .push(record);
            }
        }
        // DR-029 §Decision 6: the by-reference credential-injection audit trail.
        // Appends `dest` + `secret_ref` ONLY (the fact carries no value, so
        // neither can the fold — DR-026 crit 5). Keyless fact → no-op (I3).
        "credential.injected" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let record = CredentialInjection {
                    dest: payload["dest"].as_str().unwrap_or_default().to_string(),
                    secret_ref: payload["secret_ref"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                };
                graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .credential_injections
                    .push(record);
            }
        }
        // DR-029 §Decision 2/6: the honest-floor dropped-credential audit trail
        // (a mapping the SecretSource could not resolve, dropped — never a fake
        // secret). Appends `dest` + `secret_ref`. Keyless fact → no-op (I3).
        "credential.dropped" => {
            if let Some(run) = payload_run(event) {
                let payload = event.payload();
                let record = CredentialDrop {
                    dest: payload["dest"].as_str().unwrap_or_default().to_string(),
                    secret_ref: payload["secret_ref"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                };
                graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .credential_drops
                    .push(record);
            }
        }
        // DR-021 B2 (C1): the post-action `action.metered` fact is the C1 spend
        // fold source. Keyed by the payload `run`; folds the MEASURED
        // `spend_delta_usd` into the SAME `cumulative_spend_usd` accumulator the
        // permit path used to feed — only the SOURCE fact moved (off the
        // pre-action permit decision, onto this measured post-action fact). A
        // keyless fact (missing `run`) folds counters-only / no-op, never mints a
        // run, never panics — the established permit-reducer discipline (I3, never
        // guess a key). `input_tokens`/`output_tokens` are RECORDED-only: there is
        // no cumulative-tokens accumulator, so they never fold.
        "action.metered" => {
            if let Some(run) = payload_run(event)
                && let Some(spend) = event.payload()["spend_delta_usd"].as_f64()
            {
                graph
                    .agent_runs
                    .entry(run)
                    .or_default()
                    .permit_accumulators
                    .cumulative_spend_usd += spend;
            }
        }
        _ => {} // every other subject: counters only (S0 scope)
    }
}

/// Fold a permit decision (`permit.granted`/`.denied`/`.escalated`) onto the
/// run's ledger + per-session accumulators. `decision` is `"granted"` /
/// `"denied"` / `"escalated"` — `escalated` is NEVER coerced to `granted`
/// (I6, DR-008 §4). Pure: same event, same delta.
fn apply_permit_decision(graph: &mut Graph, event: &Event, decision: &str) {
    let (Some(run), Some(request_id)) = (payload_run(event), payload_request_id(event)) else {
        return; // counters-only; never guess a key (I3)
    };
    let payload = event.payload();
    let state = graph.agent_runs.entry(run).or_default();

    // Ledger: create the entry if the decision arrived before the request
    // (out-of-order log — the log is truth, I3).
    let entry = state.permit_ledger.entry(request_id).or_default();
    entry.decision = Some(decision.to_string());
    entry.policy_ref = payload["policy_ref"]["hash"].as_str().map(String::from);
    entry.reason = payload["reason"].as_str().map(String::from);

    // Accumulators: fold the optional per-session deltas + decision counters,
    // the state the contextual permit-verifiers read (C1/C6/C7).
    let acc = &mut state.permit_accumulators;
    // DR-021 B2 (C1): `spend_delta_usd` is RETIRED as the permit-path fold source;
    // the permit fact now folds ZERO spend. Measured spend rides the post-action
    // `action.metered` fact instead (the `action.metered` arm above).
    match decision {
        // DR-024 Q3 (C6): fold `risk_delta` on the GRANTED arm ONLY. Only an
        // action that ACTUALLY RAN contributes running risk; a denied or escalated
        // action never ran, so folding its assessed risk would be a phantom charge
        // — the exact I3 dishonesty DR-021 B2 refused for spend. The permit fact
        // still RECORDS the pre-action assessment on every arm; the accumulator
        // COUNTS only granted risk.
        "granted" => {
            if let Some(risk) = payload["risk_delta"].as_f64() {
                acc.risk_score += risk;
            }
            acc.granted += 1;
        }
        "denied" => acc.denied += 1,
        "escalated" => acc.escalated += 1,
        _ => {}
    }
}

/// The `run` key every `agent.*` payload carries (ontology v1 baselines).
fn payload_run(event: &Event) -> Option<String> {
    event.payload()["run"].as_str().map(String::from)
}

/// The `path` key every `worktree.*` payload carries (ontology v1 baselines).
fn payload_path(event: &Event) -> Option<String> {
    event.payload()["path"].as_str().map(String::from)
}

/// The `gate` key every `gate.*` payload carries (ontology v1 baselines).
fn payload_gate(event: &Event) -> Option<String> {
    event.payload()["gate"].as_str().map(String::from)
}

/// The `request_id` key every `permit.*` payload carries (the ledger key;
/// ontology v1 permit baselines, DR-008/DR-009).
fn payload_request_id(event: &Event) -> Option<String> {
    event.payload()["request_id"].as_str().map(String::from)
}

/// Evidence blob hashes (blake3 hex) from a `gate.failed`/`gate.inconclusive`
/// fact — refs, never bytes (I2).
fn evidence_hashes(event: &Event) -> Vec<String> {
    event.payload()["evidence"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["hash"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Fold a whole event sequence from scratch. `rezidnt rebuild` is exactly
/// `fold(log from seq 0)`.
pub fn fold<'a, I>(events: I) -> Graph
where
    I: IntoIterator<Item = &'a Event>,
{
    let mut graph = Graph::default();
    for event in events {
        apply(&mut graph, event);
    }
    graph
}

/// Live materializer: incremental fold + snapshot/resume. A snapshot *is* a
/// [`Graph`] (it carries `last_event`/`events_folded`, so startup = load
/// snapshot, fold the tail).
pub struct Materializer {
    graph: Graph,
}

impl Materializer {
    pub fn new() -> Self {
        Self {
            graph: Graph::default(),
        }
    }

    /// Resume from a snapshot taken by [`Materializer::snapshot`].
    pub fn resume(snapshot: Graph) -> Self {
        Self { graph: snapshot }
    }

    /// Apply one live event (delegates to the pure [`apply`]).
    pub fn apply(&mut self, event: &Event) {
        apply(&mut self.graph, event);
    }

    /// Current graph.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Point-in-time snapshot. Property (release-blocking, doc §15):
    /// `fold(log) == snapshot` — resuming from this and folding the tail must
    /// equal folding everything from seq 0.
    pub fn snapshot(&self) -> Graph {
        self.graph.clone()
    }
}

impl Default for Materializer {
    fn default() -> Self {
        Self::new()
    }
}

// --- Fleet BoardView projection (DR-039) -----------------------------------
//
// Hoisted DOWN from `rezidnt-tui` into this crate (DR-039 Decision 3): the
// fleet view is materialized state, so it belongs with the reducers. ONE pure
// projection, `&Graph -> BoardView`, reused by both consumers — the read-only
// board (`rezidnt-tui`, which keeps only its ratatui `draw()` layer and
// re-exports these) and the `board_view` MCP tool (`rezidnt-mcp`). Neither
// re-implements the projection (I3: one derivation), and `rezidnt-mcp` never
// depends on `rezidnt-tui`, so the board's writer-free read-only proof
// (DR-031) is untouched.
//
// `BoardView`/`RunRow`/`WorktreeRow` derive `Serialize + Deserialize` so the
// tool result serializes through `tool_ok()` and the oracle can deserialize the
// served payload back into a `BoardView` for byte-for-byte projection equality.

/// The read-only fleet projection: everything the board renders, computed as a
/// PURE function of a [`Graph`] snapshot. Carries derived state verbatim (I3):
/// the projection re-interprets nothing.
///
/// Projection semantics (pinned by `rezidnt-tui/tests/board_projection.rs`):
/// - `events_folded` = `graph.events_folded` (fleet heartbeat);
/// - `workspaces_open` / `workspaces_closed` = counts over `graph.workspaces`
///   by [`WorkspaceStatus`];
/// - `counts_by_subject` = `graph.counts_by_subject` (subject histogram),
///   carried verbatim in deterministic key order;
/// - `runs` = one [`RunRow`] per `graph.agent_runs` entry, in the map's
///   deterministic (ULID-string) key order;
/// - `worktrees` = one [`WorktreeRow`] per `graph.worktrees` entry, in
///   deterministic (path) key order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BoardView {
    pub events_folded: u64,
    pub workspaces_open: usize,
    pub workspaces_closed: usize,
    /// Subject histogram, verbatim from the graph (deterministic order).
    pub counts_by_subject: Vec<(String, u64)>,
    pub runs: Vec<RunRow>,
    pub worktrees: Vec<WorktreeRow>,
}

/// One agent run's row in the fleet board — the read-only accounting shadow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunRow {
    /// The run's ULID key (graph `agent_runs` key).
    pub run: String,
    /// Recorded status string, verbatim (`spawning` | `running` | `completed`
    /// | …) — never re-interpreted (I3: reducers fold every live payload).
    pub status: String,
    pub total_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// How many replay-divergence alarms are on this run (DR-006). Zero for a
    /// healthy run; the board surfaces a nonzero count.
    pub integrity_alarms: usize,
    /// Permit decisions folded onto this run, carried verbatim from
    /// [`AgentRunState::permit_accumulators`] (I3: no re-interpretation). Grant
    /// count.
    pub permit_granted: u64,
    /// Permit denial count, from `permit_accumulators.denied`.
    pub permit_denied: u64,
    /// Permit escalation count, from `permit_accumulators.escalated`.
    /// `escalated` is never coerced to `granted` (I6) — it surfaces on its own.
    pub permit_escalated: u64,
    /// Requested-but-undecided permits: ledger entries whose `decision` is
    /// `None`. Honest zero when the ledger is empty.
    pub permit_pending: usize,
    /// Delegation-chain depth for this run, from
    /// [`AgentRunState::delegations`] length.
    pub delegated: usize,
}

/// One worktree's row in the fleet board.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorktreeRow {
    /// The worktree's path key (graph `worktrees` key).
    pub path: String,
    pub status: String,
    pub branch: Option<String>,
    /// Most recent `diff.ready`/`diff.merged` summary hash, if any.
    pub last_diff: Option<String>,
}

/// PURE projection: `&Graph` -> [`BoardView`]. No IO, no clocks, no
/// randomness. Every field is carried verbatim from derived state (I3): the
/// projection re-interprets nothing. Deterministic key orders are pinned by
/// the `BTreeMap` iteration order (I6).
pub fn project(graph: &Graph) -> BoardView {
    // Fleet summary: heartbeat + workspace open/closed split. BTreeMap<Subject,_>
    // iterates in deterministic key order, so the histogram is stable.
    let mut workspaces_open = 0usize;
    let mut workspaces_closed = 0usize;
    for status in graph.workspaces.values() {
        match status {
            WorkspaceStatus::Open => workspaces_open += 1,
            WorkspaceStatus::Closed => workspaces_closed += 1,
        }
    }
    let counts_by_subject = graph
        .counts_by_subject
        .iter()
        .map(|(subject, count)| (subject.as_str().to_string(), *count))
        .collect();

    // Per-run rows, in the agent_runs map's deterministic (ULID-string) key
    // order. Every field is carried verbatim from derived state (I3).
    let runs = graph
        .agent_runs
        .iter()
        .map(|(run, state)| RunRow {
            run: run.clone(),
            status: state.status.clone(),
            total_usd: state.total_usd,
            input_tokens: state.input_tokens,
            output_tokens: state.output_tokens,
            integrity_alarms: state.integrity_alarms.len(),
            // Permit state, carried verbatim from the ALREADY-FOLDED run
            // (I3 — the board re-derives nothing). `permit_pending` counts
            // ledger entries still awaiting a decision.
            permit_granted: state.permit_accumulators.granted,
            permit_denied: state.permit_accumulators.denied,
            permit_escalated: state.permit_accumulators.escalated,
            permit_pending: state
                .permit_ledger
                .values()
                .filter(|entry| entry.decision.is_none())
                .count(),
            delegated: state.delegations.len(),
        })
        .collect();

    // Per-worktree rows, in deterministic (path) key order.
    let worktrees = graph
        .worktrees
        .iter()
        .map(|(path, state)| WorktreeRow {
            path: path.clone(),
            status: state.status.clone(),
            branch: state.branch.clone(),
            last_diff: state.last_diff.clone(),
        })
        .collect();

    BoardView {
        events_folded: graph.events_folded,
        workspaces_open,
        workspaces_closed,
        counts_by_subject,
        runs,
        worktrees,
    }
}

/// DR-040: one outstanding permit escalation — the drill-down detail behind
/// `board_view`'s `permit_escalated` COUNT. A pure read of already-folded state
/// (I3): every field is carried VERBATIM from the folded [`PermitLedgerEntry`]
/// (`crates/rezidnt-state/src/lib.rs`) plus the run key and the `request_id`
/// map key — the projection re-interprets nothing. `reason`/`policy_ref` surface
/// verbatim so the escalation stays interrogable and is never coerced to
/// granted/denied (I6). Derives `Serialize + Deserialize` so `get_escalations`
/// serves it and the oracle deserializes the served payload back for equality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EscalationRow {
    /// The run's ULID key (graph `agent_runs` key).
    pub run: String,
    /// The escalated ask's `request_id` (the `permit_ledger` map key).
    pub request_id: String,
    /// The requested action kind, verbatim from the ledger entry.
    pub action: String,
    /// The requested action target descriptor, verbatim. `None` while the
    /// request has not folded (a decision may fold before its request, I3).
    pub target: Option<serde_json::Value>,
    /// The escalation reason, verbatim (I6 — interrogable, never coerced).
    pub reason: Option<String>,
    /// The deciding policy's CAS hash, verbatim, so the escalation is
    /// interrogable (I6). `None` if the fact omitted it.
    pub policy_ref: Option<String>,
}

/// PURE projection: the outstanding permit escalations across the fleet. No IO,
/// no clocks, no randomness — a read of already-folded state (I3): each row is
/// carried VERBATIM from a [`PermitLedgerEntry`] whose `decision ==
/// Some("escalated")`, re-interpreting nothing. `filter` scopes to one run's key
/// when `Some` (absent/other run → no rows), all runs when `None`. Deterministic
/// in `BTreeMap` key order (`agent_runs`, then `permit_ledger`) — same
/// content-hashed log yields the same rows (I6).
pub fn escalations(graph: &Graph, filter: Option<&str>) -> Vec<EscalationRow> {
    graph
        .agent_runs
        .iter()
        .filter(|(run, _)| filter.is_none_or(|want| want == run.as_str()))
        .flat_map(|(run, state)| {
            state
                .permit_ledger
                .iter()
                .filter(|(_, entry)| entry.decision.as_deref() == Some("escalated"))
                .map(move |(request_id, entry)| EscalationRow {
                    run: run.clone(),
                    request_id: request_id.clone(),
                    action: entry.action.clone(),
                    target: entry.target.clone(),
                    reason: entry.reason.clone(),
                    policy_ref: entry.policy_ref.clone(),
                })
        })
        .collect()
}
