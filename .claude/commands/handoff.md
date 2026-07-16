---
description: Write a session handoff so the next session resumes with zero recontextualizing
allowed-tools: Read, Write, Bash(git status:*), Bash(git diff:*), Bash(git log:*)
---
Capture the state of play into `.claude/state/handoff.md` (overwrite): current slice and how far through its criteria; what changed this session (from `git status` and `git log` since the last handoff); the exact next action; any open /debrief findings; any decisions that still need a /dr. Keep it to a screen. End by printing the next action so the human sees it immediately.
