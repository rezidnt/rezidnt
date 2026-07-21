//! c3-egress-fold oracle (DR-029) — CRITERION 5 (HOST-provable, the REDUCER half):
//! the five minted `egress.*`/`credential.*` subjects fold onto `AgentRunState`
//! (`crates/rezidnt-state/src/lib.rs`), each with the fold shape the warden's DR-029
//! §"Reducer obligation" (ontology line 534) spec'd — discharging DR-006 (no
//! consumer-less subjects). The reducer arms + fields do NOT exist yet.
//!
//! The fold, keyed on the payload `run` (a keyless payload ⇒ counters-only no-op —
//! the established permit/intent/action discipline, I3):
//!   - `egress.mediated` / `egress.unavailable` → `egress: Option<EgressPostureState>`
//!     (LAST-WRITE-WINS the composed posture: network/sandbox/egress_enforceable/backend);
//!   - `egress.denied` → `Vec<EgressDenial>` (APPEND order — replayable);
//!   - `credential.injected` → `Vec<CredentialInjection>` (APPEND order — the by-ref
//!     audit trail: secret_ref/dest only, NEVER the value);
//!   - `credential.dropped` → `Vec<CredentialDrop>` (APPEND order — the honest-floor
//!     audit trail).
//!
//! Mirrors how `role_fold.rs` / `permit_ledger.rs` assert `fold(log)==snapshot` and
//! the `#[serde(default)]` rebuild-stability the taxonomy demands (ontology line 534).
//!
//! ## RED MODE — COMPILE-RED then behavior-red: `AgentRunState` has no `egress`
//! /`egress_denials`/`credential_injections`/`credential_drops` fields and `apply`
//! has no arms for the five subjects. This file cannot compile until the fields land;
//! the fold assertions decide green after.
//!
//! IMPLEMENTER ADDS (the seam this pins) — new `#[serde(default)]` fields on
//! `AgentRunState` + `match` arms in `apply` keyed on `payload_run(event)`:
//!   - `pub egress: Option<EgressPostureState>` where `EgressPostureState` carries
//!     `{ network, sandbox, egress_enforceable, backend }` (last-write-wins);
//!   - `pub egress_denials: Vec<EgressDenial>` —
//!     `EgressDenial { dest: String, policy_ref: Option<String> }` (append);
//!   - `pub credential_injections: Vec<CredentialInjection>` —
//!     `CredentialInjection { dest: String, secret_ref: String }` (append; NO value);
//!   - `pub credential_drops: Vec<CredentialDrop>` —
//!     `CredentialDrop { dest: String, secret_ref: String }` (append).
//!
//! Exact struct names/accessors are the implementer's oracle-first call; this file
//! reads them through the public `AgentRunState` fields the reducer must surface.

use rezidnt_state::{Materializer, fold};
use rezidnt_types::Event;

// A valid 26-char Crockford-base32 ULID (alphabet 0123456789ABCDEFGHJKMNPQRSTVWXYZ
// — no I/L/O/U). The run key every fact in this suite folds on.
const RUN: &str = "01C3EGRESSF0DREDCERRN00001";

fn event(id_suffix: char, subject: &str, payload: &str) -> Event {
    // `id` is a valid 26-char Crockford ULID: a fixed 25-char valid prefix plus the
    // per-event suffix digit (time-ordered by suffix — callers pass 0..=3, which are
    // valid Crockford AND keep the `ts` second field below valid). The 25-char prefix
    // uses only Crockford chars (no I/L/O/U).
    let line = format!(
        r#"{{"id":"01C3EGRESSF0DREDCEREVENT0{id_suffix}","ts":"2026-07-21T12:00:0{id_suffix}Z","v":1,"source":"rezidnt-run","subject":"{subject}","correlation":"01C3EGRESSF0DREDCERC0RR001","payload":{payload}}}"#
    );
    Event::from_json_line(&line).expect("well-formed test event")
}

/// CRITERION 5 (posture fold, last-write-wins) — an `egress.mediated` folds the
/// composed posture onto the run's `egress` field; a later `egress.unavailable` on
/// the same run OVERWRITES it (last-write-wins the posture — a run has one current
/// posture). Mints the run entry if absent (log is truth, I3 — a posture fact needs
/// no prior `agent.spawned`, ontology line 495).
///
/// COMPILE-RED until `AgentRunState.egress` + the two posture arms exist.
#[test]
fn egress_posture_folds_last_write_wins() {
    let events = [
        event(
            '0',
            "egress.mediated",
            &format!(
                r#"{{"run":"{RUN}","network":"mediated","sandbox":"available","egress_enforceable":true,"backend":"pasta+bwrap"}}"#
            ),
        ),
        event(
            '1',
            "egress.unavailable",
            &format!(
                r#"{{"run":"{RUN}","network":"sealed","sandbox":"available","egress_enforceable":false,"injected":false,"reason":"egress backend unavailable"}}"#
            ),
        ),
    ];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("a posture fact mints the run entry (log is truth, I3)");
    let posture = run
        .egress
        .as_ref()
        .expect("the composed posture folds onto AgentRunState.egress");
    assert!(
        !posture.egress_enforceable,
        "last-write-wins: the later egress.unavailable posture (not enforceable) is the current one"
    );
    assert_eq!(
        posture.sandbox.as_deref(),
        Some("available"),
        "the sandbox discriminator field folds onto the posture (DR-029 taxonomy: sandbox is a field)"
    );
}

/// CRITERION 5 (denial fold, append order) — each `egress.denied` appends an
/// `EgressDenial` recording the off-allowlist `dest` (+ policy_ref), in log order so
/// the denial trail replays (many denials per run — ontology line 513).
///
/// COMPILE-RED until `AgentRunState.egress_denials` + the arm exist.
#[test]
fn egress_denials_append_in_log_order() {
    let events = [
        event(
            '0',
            "egress.denied",
            &format!(
                r#"{{"run":"{RUN}","dest":"evil.example.com","policy_ref":{{"hash":"po1"}},"reason":"off allowlist"}}"#
            ),
        ),
        event(
            '1',
            "egress.denied",
            &format!(
                r#"{{"run":"{RUN}","dest":"tracker.example.net","policy_ref":{{"hash":"po1"}}}}"#
            ),
        ),
    ];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("the denial fact mints the run");
    let dests: Vec<&str> = run.egress_denials.iter().map(|d| d.dest.as_str()).collect();
    assert_eq!(
        dests,
        vec!["evil.example.com", "tracker.example.net"],
        "egress.denied facts append in log order (replayable denial trail, DR-029)"
    );
}

/// CRITERION 5 (injection audit-trail fold, by-ref, NEVER the value) — each
/// `credential.injected` appends a `CredentialInjection` recording `dest` +
/// `secret_ref` ONLY; the value is not in the fact, so it cannot be in the folded
/// state. The audit trail is the by-reference record (ontology line 520/534).
///
/// COMPILE-RED until `AgentRunState.credential_injections` + the arm exist.
#[test]
fn credential_injections_fold_by_reference_never_the_value() {
    let events = [event(
        '0',
        "credential.injected",
        &format!(
            r#"{{"run":"{RUN}","dest":"github.com","secret_ref":"gh_token","policy_ref":{{"hash":"po1"}}}}"#
        ),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("the injection fact mints the run");
    assert_eq!(run.credential_injections.len(), 1);
    let inj = &run.credential_injections[0];
    assert_eq!(inj.dest, "github.com");
    assert_eq!(
        inj.secret_ref, "gh_token",
        "the injection audit trail records the secret_ref BY REFERENCE (DR-026 crit 5 — the fact \
         carries no value, so neither can the fold)"
    );
}

/// CRITERION 5 (drop audit-trail fold) — each `credential.dropped` appends a
/// `CredentialDrop` recording the `dest` + the unresolvable `secret_ref` (the
/// honest-floor audit trail — a mapping was dropped, never a fake secret injected,
/// ontology line 527/534).
///
/// COMPILE-RED until `AgentRunState.credential_drops` + the arm exist.
#[test]
fn credential_drops_fold_the_honest_floor_audit_trail() {
    let events = [event(
        '0',
        "credential.dropped",
        &format!(
            r#"{{"run":"{RUN}","dest":"github.com","secret_ref":"gh_token","reason":"unresolvable by the configured SecretSource"}}"#
        ),
    )];
    let graph = fold(events.iter());
    let run = graph
        .agent_runs
        .get(RUN)
        .expect("the drop fact mints the run");
    assert_eq!(run.credential_drops.len(), 1);
    assert_eq!(run.credential_drops[0].dest, "github.com");
    assert_eq!(run.credential_drops[0].secret_ref, "gh_token");
}

/// CRITERION 5 (keyless payload ⇒ no-op — the established discipline) — a fact of
/// each new subject with NO `run` key folds as counters-only: it mints no run entry,
/// never panics (I3 — the reducer never guesses a key, the permit/intent/action
/// discipline). Non-vacuous: `events_folded` still advances.
///
/// COMPILE-RED until the arms exist (a missing arm folds counters-only too, so this
/// stays honest against a partially-added reducer).
#[test]
fn keyless_new_subject_facts_fold_counters_only_no_op() {
    let events = [
        event('0', "egress.mediated", r#"{"network":"mediated"}"#),
        event('1', "egress.denied", r#"{"dest":"evil.example.com"}"#),
        event(
            '2',
            "credential.injected",
            r#"{"dest":"github.com","secret_ref":"gh_token"}"#,
        ),
        event(
            '3',
            "credential.dropped",
            r#"{"dest":"github.com","secret_ref":"gh_token"}"#,
        ),
    ];
    let graph = fold(events.iter());
    assert!(
        graph.agent_runs.is_empty(),
        "CRITERION 5: a keyless (no `run`) egress.*/credential.* fact mints NO run entry — \
         counters-only no-op, never a guessed key (I3, the established permit/intent discipline)"
    );
    assert_eq!(
        graph.events_folded, 4,
        "conservation: every event still folds as a counter (non-vacuous)"
    );
}

/// CRITERION 5 (rebuild-stability, I3, release-blocking) — the whole
/// `egress.*`/`credential.*` log folds equal under both fold-from-zero and
/// incremental application. The new `#[serde(default)]` fields are what keep
/// `rezidnt rebuild` reproducing identical graph state across the schema addition.
///
/// COMPILE-RED until the fields land; the equality is the I3 pin (the property test
/// discipline — a divergence is a reducer bug, not a flaky test).
#[test]
fn egress_credential_log_folds_rebuild_stable() {
    let events = [
        event(
            '0',
            "egress.mediated",
            &format!(
                r#"{{"run":"{RUN}","network":"mediated","sandbox":"available","egress_enforceable":true}}"#
            ),
        ),
        event(
            '1',
            "credential.injected",
            &format!(
                r#"{{"run":"{RUN}","dest":"github.com","secret_ref":"gh_token","policy_ref":{{"hash":"po1"}}}}"#
            ),
        ),
        event(
            '2',
            "egress.denied",
            &format!(
                r#"{{"run":"{RUN}","dest":"evil.example.com","policy_ref":{{"hash":"po1"}}}}"#
            ),
        ),
        event(
            '3',
            "credential.dropped",
            &format!(
                r#"{{"run":"{RUN}","dest":"api.github.com","secret_ref":"gh_api_token","reason":"unresolvable"}}"#
            ),
        ),
    ];

    let folded = fold(events.iter());
    let mut live = Materializer::new();
    for e in &events {
        live.apply(e);
    }
    assert_eq!(
        live.snapshot(),
        folded,
        "incremental == fold-from-zero across the egress/credential field additions — rebuild is \
         stable (I3, release blocker; the #[serde(default)] rebuild-stability the taxonomy demands)"
    );
}
