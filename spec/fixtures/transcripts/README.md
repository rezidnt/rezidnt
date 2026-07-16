# Recorded harness transcripts (adapter contract fixtures)

Contract per the testing-oracles skill: the claude-code adapter is tested against
RECORDED stream-json output, never against a live CLI. A CLI version bump that
breaks a recording blocks the adapter, not the daemon (version-gated hello).
Adapter tests replay these files with zero network.

| file | provenance | CLI version | captured |
|---|---|---|---|
| `claude_code_stream_v2.1.191.jsonl` | **REAL** — recorded via `claude -p "Say exactly: rezidnt transcript probe" --output-format stream-json --verbose` (session `83c61e05-…`); single-run verbatim, the same run every S1 test pin and the `s1_agent_run` golden pair derive from | 2.1.191 | 2026-07-16 |
| `claude_code_stream_tool_use.jsonl` | **CONSTRUCTED from vendor docs + observed real shapes** (the recorded probe used no tools). PROVISIONAL — re-record from a real tool-using run and replace. DR-002 rule 5: vendor docs are a primary source. | 2.1.191 (shape) | 2026-07-16 |

Notes on the real recording: it was captured from the rezidnt repo cwd, so it
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
