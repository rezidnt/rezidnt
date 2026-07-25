---
name: "source-command-handoff"
description: "Write a session handoff so the next session resumes with zero recontextualizing"
---

# source-command-handoff

Use this skill when the user asks to run the migrated source command `handoff`.

## Command Template

Capture the state of play into `.Codex/state/handoff.md` (overwrite): current slice and how far through its criteria; what changed this session (from `git status` and `git log` since the last handoff); the exact next action; any open /debrief findings; any decisions that still need a /dr. Keep it to a screen. End by printing the next action so the human sees it immediately.
