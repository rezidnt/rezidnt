# Recorded harness transcripts (adapter contract fixtures)

Contract per the testing-oracles skill: the claude-code adapter is tested against
RECORDED stream-json output, never against a live CLI. A CLI version bump that
breaks a recording blocks the adapter, not the daemon (version-gated hello).
Adapter tests replay these files with zero network.

| file | provenance | CLI version | captured |
|---|---|---|---|
| `claude_code_stream_v2.1.191.jsonl` | **REAL** — recorded via `claude -p "Say exactly: rezidnt transcript probe" --output-format stream-json --verbose` (session `83c61e05-…`); single-run verbatim, the same run every S1 test pin and the `s1_agent_run` golden pair derive from | 2.1.191 | 2026-07-16 |
| `claude_code_stream_tool_use.jsonl` | **CONSTRUCTED from vendor docs + observed real shapes** (the recorded probe used no tools). PROVISIONAL — re-record from a real tool-using run and replace. DR-002 rule 5: vendor docs are a primary source. | 2.1.191 (shape) | 2026-07-16 |
| `codex_exec_v0.145.0.jsonl` | **REAL** — recorded via `codex exec --json --skip-git-repo-check --sandbox read-only "Say exactly: rezidnt codex transcript probe"` in a scratch dir (thread `019f99a4-…`); single-run verbatim, DR-048 slice A codex adapter contract source | codex-cli 0.145.0 | 2026-07-25 |
| `codex_exec_v0.145.0_turn_failed.jsonl` | **REAL** — recorded via `codex exec --json --skip-git-repo-check -m definitely-not-a-real-model-xyz "reply with the single word: hi"` in a scratch dir (thread `019f99d8-…`), stdin closed; single-run verbatim. The **failing-turn arm** DR-050 §Decision 3 required before any Trials scoring may trust a codex verdict | codex-cli 0.145.0 | 2026-07-25 |

Notes on the codex recording: line types present: `thread.started` (carries
`thread_id`, the resume identity), `turn.started`, `item.completed` twice (one
`error` item — machine-local skills-context noise the adapter must tolerate
unmapped — and one `agent_message` carrying the probe text), `turn.completed`
carrying `usage` token counts. The format carries NO `duration_ms`, NO dollar
cost, and NO aggregate turn count — the adapter's zero-default/derivation
behavior for those is pinned by the contract tests, not invented here.

**The failing-turn recording settles DR-050 §Decision 3 empirically.** That record
left the `turn.completed` → `status:"success"` mapping flagged-not-settled: the
implementer read `turn.completed` as the harness positively asserting the turn
finished, the auditor held that it asserts only that the turn TERMINATED while
`status` is the ontology's OUTCOME, and the evidence available then was
*consistent with* the mapping without establishing it. This recording establishes
it. A codex turn that ends badly emits a top-level `{"type":"error"}` line followed
by **`turn.failed`** carrying `error.message` — it does **NOT** emit
`turn.completed`. So the success mapping is sound, and the auditor's
mismapped-failure branch is refuted for this CLI version.

Two further things the recording pins, both previously unverified:
- The `item.completed` items of type `error` are confirmed **noise, not outcome
  signals** — this failing run carries two of them (a model-metadata warning and
  the same skills-context notice the successful probe carries) alongside a turn
  that genuinely failed. Tolerating them unmapped is correct.
- `turn.failed` carries **no `usage` object**, so a failed codex turn reports no
  token accounting at all. Any collator (DR-048 slice C) must treat a failed
  candidate's cost as absent rather than zero — absence is the honest
  representation; a zero would read as a free run.

Notes on the real claude recording: it was captured from the rezidnt repo cwd, so it
contains project-level SessionStart `hook_started`/`hook_response` lines —
legitimate real-world noise the adapter must tolerate unmapped. Line types
present: system (hook_started ×3, hook_response ×3, init), assistant
(thinking + text), rate_limit_event, result. The result envelope carries
`total_cost_usd`, `usage`, `num_turns`, `duration_ms`, `session_id` — the
dossier accounting source (DR-001).

**Provenance history (oracle, 2026-07-16, S1):** the oracle round recorded two
probe runs; the test pins were written from the first (session `83c61e05-…`)
while the second (session `9a7baf75-…`) was mistakenly installed as this
fixture. The implementer detected the mismatch during S1 and reconciled the
scalars as a stopgap (documented, tests untouched); the oracle then replaced
the file with the first recording verbatim, restoring single-run provenance.
No test or golden fixture was changed at any point.
