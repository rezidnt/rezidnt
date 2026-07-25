---
name: "source-command-pickup"
description: "Resume from the last session's handoff and continue the work"
---

# source-command-pickup

Use this skill when the user asks to run the migrated source command `pickup`.

## Command Template

Cold-start resume. Read `.Codex/state/handoff.md` in full — it is the state of play written by the previous session. Cross-check it against reality before trusting it: `git log --oneline -5` and `git status -sb` (flag stale SHAs, uncommitted work, or an unpushed branch), and confirm `.Codex/state/current-slice` matches the handoff's stated slice. Then give the user a three-line summary — current slice, boards/verdict state at last close, the handoff's "next action" — and proceed with that next action without waiting to be asked, unless the handoff names a blocker that is owner-only (standing gates, /dr decisions, credentials), in which case surface it and start whatever work is not blocked. If the handoff is missing or older than the latest commit implies, say so and reconstruct the state from git log + the tracked-items list before doing anything.
