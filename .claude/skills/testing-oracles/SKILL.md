---
name: testing-oracles
description: >-
  The oracle-first testing strategy for rezidnt: reducer property tests, golden event-log
  fixtures, recorded-transcript adapter contracts, and the benchmark seam. This skill should
  be used when writing tests before implementation, designing fixtures, or reviewing test
  honesty. Load for the oracle agent and for any test work. Encodes the "build where a
  deterministic judge exists" sequencing law.
user-invocable: false
version: 0.1.0
---

# Testing oracles

Ordered by the project's sequencing principle — build where a deterministic oracle exists. Full text: architecture doc §15.

## Reducer determinism (Phase-1 oracle)
proptest asserts, over arbitrary event interleavings: `fold(log) == snapshot`, and `rebuild()` equals live materialized state. This is the release-blocking property — a divergence is a reducer bug, not a flaky test. Generators produce well-formed event sequences from the subject taxonomy.

## Golden event-log fixtures
Committed under `spec/fixtures/*.jsonl`, minimal and named for the behavior they pin (`s0_rebuild_equality.jsonl`, `s2_worktree_conflict.jsonl` — never `test2.jsonl`). A replay script at `scripts/replay-fixtures.sh` folds each fixture and asserts the expected graph; `/vet` runs it. Every release replays all fixtures.

## Recorded-transcript adapter contracts
Harness and (future) substrate adapters are tested against RECORDED stdout/stream-json transcripts captured from real harness runs, stored as fixtures. A harness CLI version bump that breaks a recording blocks the adapter, not the daemon (the version-gated hello). This is how a weekly-changing external CLI is prevented from silently breaking the fabric. Record real `claude -p --output-format stream-json` output once; assert the adapter maps it to the right subjects.

## Verifier conformance
A suite feeding exec verifiers malformed input, timeouts, and nondeterminism traps, asserting `inconclusive`-not-`pass` behavior every time. If a verifier can be made to emit `pass` on garbage, it is broken.

## Test honesty (what the oracle refuses)
- A new test that passes before its implementation exists tests nothing — rewrite it to fail first.
- A test that asserts around the claim instead of exercising it is theater — the auditor flags it.
- Weakening a test to make an implementation pass is refused; if the criterion itself is wrong, route to `/dr`.

## Benchmark seam (GTM-grade)
The harness lives in `bench/harness` and is public. The held-out labeled case set is PRIVATE — contamination destroys a measuring stick's authority. Metrics: orchestrated task completion rate, gate precision/recall against labeled defects, worktree merge success, cost per merged verified diff (the last is free from the harness cost fields).
