---
description: Show the current slice and its acceptance criteria
allowed-tools: Read, Bash(cat:*)
---
Read `.claude/state/current-slice` and print the current slice ID. Then from the slice-discipline skill, print that slice's exact acceptance criteria and its exit demo. Remind that "done" equals these criteria passing /vet and /debrief — nothing more, nothing less. If the argument `$1` names a different slice, show that slice's criteria instead but do not change the current-slice state (that requires an explicit advance).
