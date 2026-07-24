---
description: Parallel-lane build — fan out independent work items to implementer agents in isolated worktrees, single merge gate
argument-hint: "<item-1> | <item-2> | ..."
---
The multi-lane pattern as a routine verb, not a remembered trick. Use whenever the current arc has ≥2 work items whose file sets are disjoint; default to it, don't save it for special occasions.

0. Commit (or stash) any WIP first — worktrees fork committed HEAD, not the dirty tree. A lane spawned over uncommitted work silently builds against the wrong base.
1. Plan the file set of each item ($ARGUMENTS, split on `|`) and verify disjointness. Items with overlapping file sets stay serial in one lane; only genuinely disjoint items get parallel lanes.
2. Spawn one implementer agent per lane in a SINGLE message (so they run concurrently), each with `isolation: "worktree"`. Each prompt carries: the item's acceptance criteria, its file-set boundary stated as a prohibition ("touch nothing outside <paths>"), and the oracle tests it must turn green. Lanes run tests only for their own crates, never the whole workspace.
3. The orchestrator (this session) holds the only gate: /gauntlet runs here, one lane at a time — never concurrently with another lane's test run (core contention flakes the exec-verifier spawn tests, host and WSL alike).
4. Merge lanes in dependency order. Re-run /gauntlet after EACH merge, not just the last — a lane green in isolation can fail against a sibling's merged changes.
5. A lane that stalls or drifts out of its file boundary is killed and its item goes serial; the boundary is the contract that makes the parallelism safe.
