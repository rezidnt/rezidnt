//! S1 oracle: agent-run reducers (the dossier's accounting seed).
//! Payload shapes here are the oracle's proposal pending warden ratification
//! (see the S1 warden work order); events are built through the public wire
//! codec so the test exercises exactly what the log replays.

use rezidnt_state::fold;
use rezidnt_types::Event;

const RUN: &str = "01ARZ3NDEKTSV4RRFFQ69G5A01";

fn event(id_suffix: char, subject: &str, payload: &str) -> Event {
    let line = format!(
        r#"{{"id":"01ARZ3NDEKTSV4RRFFQ69G5FA{id_suffix}","ts":"2026-07-16T12:00:0{id_suffix}Z","v":1,"source":"rezidnt-run","subject":"{subject}","correlation":"01ARZ3NDEKTSV4RRFFQ69G5C00","payload":{payload}}}"#
    );
    Event::from_json_line(&line).expect("well-formed test event")
}

#[test]
fn agent_lifecycle_folds_into_run_state() {
    let events = [
        event(
            '0',
            "agent.spawned",
            &format!(
                r#"{{"run":"{RUN}","agent":"impl","harness":"claude-code","harness_version":"2.1.191","pid":4242,"badge_id":"deadbeef01234567"}}"#
            ),
        ),
        event(
            '1',
            "agent.status.changed",
            &format!(r#"{{"run":"{RUN}","from":"spawning","to":"running"}}"#),
        ),
        event(
            '2',
            "agent.completed",
            &format!(
                r#"{{"run":"{RUN}","status":"success","cost":{{"total_usd":0.190075,"input_tokens":7441,"output_tokens":45}},"num_turns":1,"duration_ms":6199,"session_id":"83c61e05-aecf-4c70-93f4-ada974db33df"}}"#
            ),
        ),
    ];
    let graph = fold(events.iter());

    let run = graph
        .agent_runs
        .get(RUN)
        .expect("run must materialize under its ULID key");
    assert_eq!(run.status, "completed");
    assert_eq!(run.total_usd, Some(0.190075));
    assert_eq!(run.input_tokens, Some(7441));
    assert_eq!(run.output_tokens, Some(45));
    assert_eq!(
        run.session_id.as_deref(),
        Some("83c61e05-aecf-4c70-93f4-ada974db33df")
    );

    // S0 envelope-level semantics are untouched by S1 reducers (conservation).
    assert_eq!(graph.events_folded, 3);
    assert_eq!(graph.counts_by_subject.len(), 3);
}

#[test]
fn status_changed_tracks_the_payload_to_field() {
    let events = [
        event(
            '0',
            "agent.spawned",
            &format!(r#"{{"run":"{RUN}","agent":"impl","harness":"claude-code"}}"#),
        ),
        event(
            '1',
            "agent.status.changed",
            &format!(r#"{{"run":"{RUN}","from":"spawning","to":"running"}}"#),
        ),
    ];
    let graph = fold(events.iter());
    assert_eq!(
        graph.agent_runs.get(RUN).expect("run exists").status,
        "running"
    );
}

/// A spawned run starts in "spawning" — the status the fabric later moves.
#[test]
fn spawned_alone_materializes_in_spawning_state() {
    let events = [event(
        '0',
        "agent.spawned",
        &format!(r#"{{"run":"{RUN}","agent":"impl","harness":"claude-code"}}"#),
    )];
    let graph = fold(events.iter());
    let run = graph.agent_runs.get(RUN).expect("run exists");
    assert_eq!(run.status, "spawning");
    assert_eq!(run.total_usd, None);
}
