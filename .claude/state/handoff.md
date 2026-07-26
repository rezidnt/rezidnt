# Handoff — 2026-07-26 (session 33)

## ► THE GOAL — and it is MET

**rezidnt is a fast, lightweight tool for agentic engineering, and the measure is: STOP NEEDING AN IDE.**

| Verb | State |
|---|---|
| **Watch** | DONE — `board_view`, `tail_events`, `orchestration_graph`, wired in the cockpit |
| **Steer** | DONE — `kill_run`, `resolve_permit`, `allow`/`deny`, escalations, wired |
| **Compare** | PARKED — `open_trial` (DR-055). Deliberate: Compare doesn't close the IDE |
| **Review** | **DONE — the panel renders a real diff, proven end to end today** |

Session 32 found the last defect by running the golden path and reading the bytes: `cas_read` returned a
file-status list, not a diff. This session closed it. **Verified by re-running the same repro, not by a test.**

## ► WHAT SHIPPED

The full DR-059 sequence, in the order the last handoff specified:

1. **DR-059 ACCEPTED** (`ce952e2`) — ratified under the standing autonomy grant: it mints no invariant, no
   subject, no trait method, no dep. §20 row flipped in the same commit.
2. **`/subject`** (`0ae61de`) — the warden minted `patch?: CasRef` on `diff.ready` v1 and `diff.merged` v1,
   `v` stays 1, and corrected the summary's mime `text/x-diff` → `text/x-diff-summary`. **No `rezidnt-types`
   edit was owed** — the crate carries no typed event-payload structs (payloads ride `serde_json::Value`,
   `taxonomy.rs` holds subject NAMES only, and no table row was added), so nothing serialized moved.
3. **`/oracle`** (`10c8a45`) — five boards, 12 judges, every one verified red by running it.
4. **Implementer** (`35aedae`) — both emitters, the fold, `diff_view`, and the mime recut.
5. **`/gauntlet`** — host `/vet` pass, WSL `/vet` pass, `/debrief` inconclusive-then-closed (below).
6. **Remediation** (`e6e84b6`) — the one runtime-impact finding.
7. **The cockpit one-liner** — `d81bdfd` in `D:\github\rezidnt-operator`, pushed.

`main` at `e6e84b6`, **pushed**, working tree clean. `rezidnt-operator` at `d81bdfd`, pushed.

### The design decision worth knowing

`crates/rezidnt-adapters/git/src/patch.rs` is a NEW shared renderer both emitters call, so they cannot drift.
It renders through a **scratch `GIT_INDEX_FILE`** in a temp dir — `read-tree HEAD`, then intent-to-add for
**untracked paths only**, then `git diff`. The repository's own index is never touched, and the scratch file
sits outside the worktree so no watcher wakes.

**The obvious `git add -A -N` is WRONG and was rejected on evidence:** `-A` stages removals, which silently
drops every DELETED file from the patch. The e2e below proves all three cases render.

### The debrief, and why it shipped

`/debrief` returned **inconclusive** for exactly one reason: the gate-time emitter's behavioural judge is
`#[cfg(unix)]` and the WSL gauntlet had not reported when the auditor wrote. **That closed** — WSL `/vet` is
green and `dr059_patch_e2e::the_golden_path_pins_and_republishes_the_real_patch` was confirmed genuinely
running (`--nocapture`), not cfg'd away. The auditor explicitly said "flip to pass on a green WSL run."

It also verified the serialized-axis claim **holds, for the reason stated** — the first time in this arc.
`#[serde(default, skip_serializing_if)]`, no `deny_unknown_fields`, and **no `WorktreeState { .. }` struct
literal anywhere in the workspace** (the exact DR-057 failure mode, absent here). Zero golden recuts owed for
the field; the 23 golden/suite edits are the disclosed mime-literal recut only.

**One finding had runtime impact and was fixed before the gate** (loop policy: security and correctness fix,
prose batches). `summarize_worktree` propagated a render failure with `?` and `DiffPins::patch` was
non-optional, so a patch-render failure aborted the whole `pre_merge` — a review nicety became a merge
blocker, and it was asymmetric with the sibling emitter DR-059 rules must move together. Now `Option<CasRef>`,
infallible by type, key OMITTED when absent (never nulled, never synthesized). Judge:
`gates.rs::patch_degrades_tests`, failure injected for real, mutation-proven red both ways.

**I did NOT run a second debrief round.** One remediation round per slice is the standing policy, and the
auditor had already classified everything else as batch.

### The end-to-end, re-run and passing

Same repro as last session (WSL daemon, fresh DB, `open_project` → worktree → edit → `diff_view` →
badged `cas_read`). `diff_view` served both refs with honest mimes; `cas_read` on `patch` returned:

```
diff --git a/README.md b/README.md
@@ -1,2 +1,6 @@
 # e2e
+## Review
+The cockpit reads the patch now.
diff --git a/added.rs b/added.rs
new file mode 100644
+fn added() { … }
diff --git a/main.rs b/main.rs
deleted file mode 100644
-fn main() { … }
```

A modification with context, an **untracked addition** (bare `git diff` omits these — the scratch index earns
its keep), and a **deletion with its removed lines** (`git add -A -N` would have dropped it). Unbadged
`cas_read` still answers `badge.required`. **Note `cas_read` takes the CasRef fields FLATTENED**
(`{badge, hash, bytes, mime}`), not a nested `ref` — a nested one answers `-32602 missing field hash`.

## ► NEXT ACTION — the batched cleanup slice, then pick the next verb

Nothing is blocking. The obvious next move is the **cleanup slice**, which now carries two arcs' findings.
None of these changes runtime behaviour; none blocks a ship.

**From this slice's `/debrief` (all LOW or NONE impact, auditor-triaged as batch):**

1. `crates/rezidnt-adapters/git/src/patch.rs::render_with_index` — non-UTF-8 untracked paths are dropped
   silently via `filter_map(String::from_utf8().ok())`, with no log line. Such a file shows in the summary as
   `A <path>` but contributes nothing to the patch — **the same summary-says-one-thing class DR-059 exists to
   close**, narrowed to non-UTF-8 names. Batch pathspecs as `OsString`.
2. `patch.rs::PATHSPEC_BATCH` — bounds by pathspec COUNT (100), but its doc comment claims it prevents
   overrunning the platform's command-line limit, which is a BYTE limit (~32 KiB on Windows). Accumulate
   byte length, not count.
3. `crates/rezidnt-state/src/lib.rs::apply` — reducer asymmetry: the `diff.ready` arm assigns `last_patch`
   INSIDE the `if let Some(diff)` guard, the `diff.merged` arm OUTSIDE. Unreachable today (no emitter produces
   it), but it falsifies the doc claim that the pair always describes one fact.
4. `crates/rezidnt-types/src/mcp.rs::DiffViewArgs` doc — still states the response is
   `{worktree, lifecycle, outcome, diff}`. Now incomplete, asserts another module's behaviour, unpinned.
   **DR-056 §Decision 2 class.**
5. **`spec/ontology.md`'s own wiring-status prose is now FALSE** — the `diff.ready.diff`, `diff.ready.patch?`
   and `diff.merged.patch?` bullets all say "NOT wired this session" / "both pin sites still write the old
   literal". All three ARE wired. **This is the arc's described-vs-tree defect INVERTED** — the spec now
   understates the tree. Needs a warden `/subject` (hook-blocked otherwise).
6. `patch.rs` module doc claims determinism ("the same tree state renders the same bytes"). The bytes are
   config-dependent — `diff.algorithm`, `diff.context`, `diff.renames`, `core.autocrlf` all move them.
   **Harmless while the patch is NOT a gate input** (`refs["diff"]` is unchanged, so I6 is not engaged) —
   **load-bearing the moment any record wires the patch into a verifier.** Pin the config with `-c` flags, or
   narrow the sentence.
7. Plan §201 owes a one-line note that unified-diff rendering is CLI-side. **The auditor RULED gix-vs-CLI
   acceptable as built, not DR-shaped**: §201's only BINDING clause is the worktree-registry rule, "reads via
   gix" is DEFAULT prose, and `gates.rs::git_diff_summary` already shells `git status --porcelain` for a read.

**Carried from the `cas-badge-door` slice's FAILed-but-shipped debrief** — five prose items, still owed, listed
in full in the previous handoff (`git show 2563772:.claude/state/handoff.md`, §BATCHED CLEANUP): the "no echo"
comment in `read_bounded`, the `dr058_path_for_address_guard.rs` board header, two mislabelled "unpinnable"
items, the drifted `file:line` citations in `dr058_invalid_ref_honesty.rs`, and the one-word badge-message
blacklist in `badge_enforcement.rs`.

## ► DO NOT — each of these will burn a session

1. **Do not propose making `delivery-harness` a rezidnt client.** The owner used it and rejected it: didn't
   beat plain Claude Code, too much ceremony, too slow.
2. **Do not build approve / reject / redirect buttons.** Read-only Review is enough. `approve` needs a merge
   hold that does not exist; **`redirect` is BLOCKED AT THE SUBSTRATE** — agents spawn `.stdin(Stdio::null())`,
   `attach` is one-way, `claude -p` is one-shot. `kill_run` and DR-049 §D3 keep-for-triage already exist.
3. **Do not restart slice B without reconciling TWO contradictory boards** — `ddc892d` on
   `worktree-agent-ac2146777f0403fee`, and branch `slice-b-board-onmain`. They disagree on which crate owns
   key-derivation (`rezidnt-run` vs `rezidnt-mcp`).
4. **Do not chase audit findings that don't change runtime behaviour.** Loop policy below.
5. **Do not re-run `/debrief` to see if the remediation is clean.** One round per slice. That is the policy,
   and asking the auditor progressively more meta questions reliably manufactures findings.

## ► LOOP POLICY (standing)

**Triage `/debrief` findings by runtime impact. Security and correctness get fixed before the gate; prose,
pinning, and doc-comment accuracy get BATCHED into a cleanup slice and never block a ship. ONE remediation
round per slice, not N.** This session ran exactly one and shipped. It worked.

## ► OPEN, none blocking

- **DR-053 is still PROPOSED** since 2026-07-25 — the last unresolved record status in the tree. Owner's call.
- **The macaroon verb for `cas_read` is unruled** — the implementer picked `"read"`; no test pins it, so a
  `Verb`-caveated agent badge is refused today. Rule it before an agent consumer ships.
- **`CasReadArgs.bytes`' doc serves `rezidnt_mcp::MAX_CAS_READ_BYTES_DEFAULT`** — a private symbol, over the
  wire, to every MCP client. Same class DR-058 fixed for `hash`; owed to a later record.
- **arch §1 still names "Microsoft-stack enterprises that need an audit trail a compliance reviewer will
  sign"** as a wedge buyer, and the README leads with compliance evidence. **Both contradict the stated goal,
  and the goal is now DEMONSTRABLY met** — the thesis change is a BINDING one and owes a DR. With Review
  landed, this is the most valuable record left to write.
- **The "described, tree lacks" pattern** claimed four records this arc. DR-059 was the fourth and the only
  one to reach a user-facing surface. **Item 5 in the cleanup list is the fifth, inverted** — worth noticing
  that the mitigation (source-text guards, DR-056 §2) does not cover spec prose about wiring status.

## Also (carried)

- `.claude/worktrees/agent-ab4e17a54fbbdb421/` is still an empty dir a Windows handle refused to delete.
  Gitignored, deregistered, harmless.
- If rust-analyzer reports phantom errors, restart it — the running instance can predate the pinned toolchain.
- **`pkill -f rezidentd` from a `bash -lc` string kills your own shell** (the pattern matches the command
  line). Use `pkill -x rezidentd`.

**Anchor discipline (warden-ratified 2026-07-24):** cite by SYMBOL, not line. A line number is admissible only
bolted to a commit hash.
