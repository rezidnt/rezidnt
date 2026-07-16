# Handoff — 2026-07-16

## State of play
**Current slice: S1** (native run substrate) — not started. Pointer advanced this session after S0 closed clean: vet pass on Windows host AND WSL2, auditor /debrief pass (second round; first round failed on a real defect, remediated), composed exit demo on the record (301 events → two identical concurrent subscribers → kill -9 → byte-identical rebuilds).

## What changed this session (all of git history is this session)
- `320886b` foundation: harness, docs/rezidnt-architecture.md (DR-002 flipped to ACCEPTED 2026-07-16 by owner), spec/ontology.md (warden-bootstrapped, 33 subjects, v=1).
- `cfbc270` S0 oracle board: 33 oracle tests + 3 implementer regression files (all failing-first), golden fixtures, replay script.
- `c1a890d` S0 implementation: types/fabric/state/proto crates + rezidentd/rezidnt bins. Delivery adjudicates by log seq, never id order (the debrief fail-driver).
- Environment: WSL Ubuntu-24.04 got rustup (stable+fmt+clippy). NOT the default distro — invoke `wsl.exe -d Ubuntu-24.04`, use `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`.

## Next action
**`/oracle run`** — oracle writes failing S1 tests: rezidnt-run (DR-001: spawner/capture/persistence/reaper), `rezidnt open` materialization, kill-client-survives, attach tail replay. Prereq inside that round: record a REAL `claude -p --output-format stream-json --verbose` transcript once as the adapter contract fixture (testing-oracles skill).

## Open /debrief findings (non-blocking, need slice assignment during S1 planning)
1. Streaming replay — tail currently builds O(log) String per connection; needed before S1 event volume (likely S1 scope).
2. §12 fabric-ingress redaction pass — DEFAULT-on in doc, unbuilt; assign a slice or /dr it out.
3. `tail --subject` has zero automated coverage.
4. Nits: bind→chmod window on the UDS; stale "todo stub" doc comment in rezidnt-types; fixture_replay.rs comment slightly inaccurate post-I2-hardening.

## Decisions needing /dr
None pending. Standing gates (owner-only, unchanged): employer IP memo (push blocked until `.claude/state/ip-memo-cleared`), name registry checks (fallback `rezident`).
