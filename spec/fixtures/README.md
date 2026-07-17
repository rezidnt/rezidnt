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
| `transcripts/` | recorded claude-code stream-json for the adapter contract — see `transcripts/README.md` for provenance |

Regenerating or editing a fixture is an oracle act: expected values must be
derived independently of `rezidnt-fabric`/`rezidnt-state` internals.
