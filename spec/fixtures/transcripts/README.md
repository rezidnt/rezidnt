# Recorded harness transcripts (adapter contract fixtures)

Contract per the testing-oracles skill: the claude-code adapter is tested against
RECORDED stream-json output, never against a live CLI. A CLI version bump that
breaks a recording blocks the adapter, not the daemon (version-gated hello).
Adapter tests replay these files with zero network.

| file | provenance | CLI version | captured |
|---|---|---|---|
| `claude_code_stream_v2.1.191.jsonl` | **REAL** — recorded via `claude -p "Say exactly: rezidnt transcript probe" --output-format stream-json --verbose` from a neutral cwd | 2.1.191 | 2026-07-16 |
| `claude_code_stream_tool_use.jsonl` | **CONSTRUCTED from vendor docs + observed real shapes** (the recorded probe used no tools). PROVISIONAL — re-record from a real tool-using run and replace. DR-002 rule 5: vendor docs are a primary source. | 2.1.191 (shape) | 2026-07-16 |

Notes on the real recording: user-level SessionStart hooks fire even from a
neutral cwd, so the transcript legitimately contains `system/hook_started` and
`system/hook_response` lines — the adapter must tolerate system lines it does
not map. Line types present: system (hook_started, hook_response, init),
assistant (thinking + text), rate_limit_event, result. The result envelope
carries `total_cost_usd`, `usage`, `num_turns`, `duration_ms`, `session_id` —
the dossier accounting source (DR-001).
