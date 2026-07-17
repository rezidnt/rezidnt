# Golden event-log fixtures

Committed, minimal, and named for the behavior they pin (never `test2.jsonl`).
Replayed by `scripts/replay-fixtures.sh` (the /vet gauntlet) and by every
release. Fixture values (ULIDs, blake3 chain links) were computed
independently of the implementation — they are the oracle, not its echo.

## Formats

- `<name>.jsonl` + `<name>.expected.json` — event envelopes (doc §5), one per
  line, folded by `rezidnt-state`; the companion file is the expected `Graph`.
- `s0_chain_*.jsonl` — log rows `{"seq": N, "chain": "<blake3 hex>", "event": {…}}`
  loaded verbatim into a doc §6 database by `rezidnt-fabric/tests/chain_fixtures.rs`.
  Chain rule: `chain = blake3(prev.chain ‖ id ‖ payload)` — prev chain as 32 raw
  bytes (genesis = 32 zero bytes), id as the 26-char ULID text, payload as the
  exact `payload` column text (compact JSON, keys in serde_json's sorted order).
- `s0_envelope_additive.jsonl` — envelopes carrying unknown fields at both the
  envelope and payload level; must always decode (additive evolution, doc §5).

## Current set

| Fixture | Pins |
|---|---|
| `s0_rebuild_equality.jsonl` (+ `.expected.json`) | `fold(log)` reproduces the committed graph: per-subject counts, workspace open/close lifecycle, `last_event`, `events_folded` |
| `s0_chain_valid.jsonl` | the exact chain formula — an honest log with precomputed links verifies end-to-end |
| `s0_chain_tamper.jsonl` | tamper-evidence — row 4's payload was edited after the chain was written; verification must name seq 4 |
| `s0_envelope_additive.jsonl` | unknown envelope/payload fields never break deserialization |
| `s1_agent_run.jsonl` (+ `.expected.json`) | S1 agent-run reducers: `agent.spawned` / `agent.status.changed` / `agent.completed` fold into `agent_runs` keyed by payload `run` — status transitions plus dossier accounting (cost, tokens, session id) |
| `s2_worktree_conflict.jsonl` (+ `.expected.json`) | S2 sole-allocator guard (DR-001): `worktree.allocated` / `worktree.observed` (human) / `worktree.conflict` fold into `worktrees` keyed by canonicalized path — one logged collision counts once, the first claim is never double-tracked |
| `s2_diff_ready.jsonl` (+ `.expected.json`) | S2 worktree lifecycle: allocate → `diff.ready` (summary as CAS ref, I2) → release; `last_diff` pins the ref hash, release closes the entry. The diff ref is a REAL blake3: hash of the 20-byte preimage `M\toracle_change.txt\n`, computed with the reference blake3 crate independently of any rezidnt code |
| `s3_gate_forced_failure.jsonl` (+ `.expected.json`) | S3's honest "forced failure": a STUB `gate.failed` verdict on the log (no S4 verifier engine yet) that `gate_explain` must interrogate — failing verifier, evidence CAS refs, exact §8 verifier inputs. Evidence ref is a REAL blake3 of the 29-byte preimage `test regression: auth::login\n`; the inputs' diff ref hashes the 14-byte preimage `M\tsrc/auth.rs\n` — both computed with the reference blake3 crate, independent of any rezidnt code. `gate.*` payload shapes are oracle proposals PENDING warden ratification |
| `s3_gate_inconclusive.jsonl` (+ `.expected.json`) | I6 honesty: a `gate.inconclusive` verdict (timeout) that `gate_explain` must report as `inconclusive` — never coerced toward pass. Evidence ref hashes the 35-byte preimage `verifier timed out after 120000 ms\n` (reference blake3, independent) |
| `s4_verified_run.jsonl` (+ `.expected.json`) | the S4 exit shape on the log: vet passed pre-spawn → spawn (governed fields recorded) → completed (cost) → `diff.ready` → pre_merge passed → `diff.merged`. Pins the S4 gate reducers (`AgentRunState::gates`, worktree `status = "merged"`), the proposed `gate.passed` v1 per-verifier records (verifier, cost_ms, evidence, inputs — PENDING warden ratification, like `diff.merged` v1), and per-verifier recorded cost. Diff ref is a REAL blake3 of the 23-byte preimage `M\tsrc/checkout/cart.rs\n`; the vet inputs' spec ref hashes the 119-byte conforming agent-spec TOML (`SPEC_CONFORMING` in `crates/rezidnt-gate/tests/native_verifiers.rs`) — reference blake3 crate, independent of any rezidnt code |
| `s4_vet_refusal.jsonl` (+ `.expected.json`) | vet enforcement pre-spawn: `gate.entered` + `gate.failed` (verifier `bare-mode`) with NO `agent.spawned` — the refusal is interrogable from the log alone (run entry exists with default status, I3). Evidence ref hashes the 47-byte preimage `bare-mode: governed spawn requires bare = true\n`; the inputs' spec ref hashes the 59-byte unbared spec (`SPEC_UNBARED`, same file) |
| `s4_replay_verified.jsonl` | debrief replay equality (doc §8, the compliance sentence): a recorded diff-scope `pass` whose inputs pin the committed diff preimage — re-execution from log + CAS reproduces the verdict, zero alarms. No `.expected.json`: owned by `rezidnt-gate/tests/replay.rs`, which seeds the CAS from the documented preimage |
| `s4_replay_divergence_alarm.jsonl` | the INTEGRITY ALARM: identical inputs, but the recorded verdict was flipped to `fail` (with a fabricated evidence ref hashing the 18-byte preimage `tampered evidence\n`) AFTER recording — re-execution over the committed CAS preimage yields `pass`; the divergence must alarm, naming the verifier and both verdicts. Owned by `rezidnt-gate/tests/replay.rs` and the daemon `debrief` CLI board |
| `transcripts/` | recorded claude-code stream-json for the adapter contract — see `transcripts/README.md` for provenance |

Regenerating or editing a fixture is an oracle act: expected values must be
derived independently of `rezidnt-fabric`/`rezidnt-state` internals.

S4 board note: the two `s3_gate_*.expected.json` graphs were EXTENDED (not
weakened) at S4 with the `gates` fold the new reducer semantics imply — the
same `gate.*` facts now materialize into `AgentRunState::gates`, so those
fixture replays are deliberately red until the S4 reducers land.
