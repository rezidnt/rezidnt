# Handoff — 2026-07-26 (session 32)

## ► THE GOAL — read this before anything else

**rezidnt is a fast, lightweight tool for agentic engineering, and the measure is: STOP NEEDING AN IDE.**
Owner direction, this session. This is not the compliance/audit-trail product the README and arch §1 still
describe — that framing is stale and owes a DR.

The origin, in the owner's words: combine herdr + Omnigent into one product *and get rid of IDE use*. What
actually happened is that rezidnt shipped the **Omnigent half** (permits, gates, containment, evidence) and
deferred the **herdr half** — the place you sit and watch agents. DR-001 replaced herdr's plumbing; herdr's
*product* was never built. Net effect: **rezidnt has only ever ADDED to the owner's workflow and never removed
anything.** That is the whole problem, and closing it is the whole job.

Don't try to *replace* the IDE (panes/tree/editor — DR-038 priced that at Tauri scale). **Make it unnecessary.**
If agents write the code you need four verbs, not an editor:

| Verb | State |
|---|---|
| **Watch** | DONE — `board_view`, `tail_events`, `orchestration_graph`, wired in the cockpit |
| **Steer** | DONE — `kill_run`, `resolve_permit`, `allow`/`deny`, escalations, wired |
| **Compare** | PARKED — `open_trial` (DR-055). Deliberate: Compare doesn't close the IDE |
| **Review** | **MCP tools SHIPPED today. The cockpit panel is THE LAST THING.** |

## ► NEXT ACTION — the panel is BUILT and the chain WORKS, but it shows the wrong thing

**Run end-to-end 2026-07-26, WSL daemon + Windows client, and it proved the goal is NOT met.** The whole path
works: daemon in WSL, `REZIDNT_MCP_LOCKFILE` pointed at a `/mnt/c/...` path so Windows can read it, loopback
HTTP reaches it across the boundary, `open_project` → worktree allocated → files edited → notify watcher folds
`diff.ready` → `diff_view` returns a `CasRef` → badged `cas_read` returns content, unbadged refuses
`badge.required`. Every link verified from Windows.

**But the content is not a diff.** `cas_read` returned, verbatim:

```
# rezidnt diff summary v1
M README.md blake3:64e4c611…
M main.rs blake3:c2e424b6…
```

**`gates::summarize_worktree` pins `git_diff_summary` — a file-status list with content hashes — and labels it
`text/x-diff`.** It has never been a diff. DR-057's premise ("diff bytes are already CAS-pinned since S2") is
FALSE; every citation in it is literally correct and the conclusion is wrong. The sibling
`summarize_worktree_content` → `git_added_content` (DR-043, for secret-scan) is added-content only — no
removals, no context, also not reviewable.

**So the Review panel renders a list of filenames.** It cannot close an IDE. Both gates passed, two audit
rounds hardened it, and none of that could catch this — only running it did.

**THE WORK IS SCOPED AND DRAFTED — [DR-059](../../docs/decisions/DR-059-real-diff-sibling-ref.md) is PROPOSED
(`5f6b12b`).** Run it in this order; the first step is not optional:

1. **Owner ratifies DR-059.** It AUTHORIZES but does not mint the field — that is deliberate.
2. **`/subject` (warden)** mints `patch?: CasRef` on `diff.ready` v1 and `diff.merged` v1, plus the mime
   correction. `v` stays 1. **An implementer landing the payload key before this session runs is working
   outside the record's authorization.**
3. **`/oracle`** for the landing slice.
4. **Implementer:** pin real `git diff` bytes at **BOTH** emitters — `gates::summarize_worktree` AND
   `crates/rezidnt-adapters/git/src/lib.rs::summarize_to_cas` (the watcher's continuous emitter, a second site
   producing the identical format under the identical false mime — fix one alone and the other keeps lying).
   Widen `diff_view` with `patch: CasRef|null`. **Check whether that widening is additive or BREAKING on the
   serialized axis and disclose it** — that is the exact question DR-057 and DR-058 each got wrong.
5. **Recut the mime literals** — four goldens plus `dr057_partial_diff_ref_folds_none.rs:156` pin
   `text/x-diff`. Behaviourally safe (nothing branches on the literal), not free.
6. **One line in the panel** to read `patch` instead of `diff`. Nothing else in `rezidnt-operator` changes.

`refs["diff"]` stays EXACTLY as it is — `DiffScope` and `ForbiddenPath` parse its line shape and are BINDING
`pre_merge` inputs. This is DR-043's sibling-ref pattern, not a change in place.

### Reproducing the end-to-end (it takes about two minutes)

```bash
# 1. daemon in WSL, lockfile on a path Windows can read
wsl.exe -d Ubuntu-24.04 -e bash -lc '
  export PATH=$HOME/.cargo/bin:$PATH
  export REZIDNT_MCP_LOCKFILE=/mnt/c/Users/dakot/AppData/Local/Temp/rezidnt-mcp.lock
  export REZIDNT_DB=$HOME/.local/state/rezidnt-e2e/events.db
  nohup $HOME/.cache/rezidnt-target/debug/rezidentd >/tmp/rezidentd.log 2>&1 & sleep 4
  cat $REZIDNT_MCP_LOCKFILE'
# 2. the cockpit, pointed at the same lockfile (Windows path form)
cd /d/github/rezidnt-operator/src-tauri && \
  REZIDNT_MCP_LOCKFILE='C:\Users\dakot\AppData\Local\Temp\rezidnt-mcp.lock' ./target/debug/app.exe
```

The test repo is at `~/e2e-repo` in WSL with a worktree already carrying edits; `REZIDNT_DB` above rebuilds
that state from the log, so `diff_view` answers immediately. In the app: **Look inside rezidnt → Worktrees →
review**. The chain works; the bytes are the summary. That is the whole finding, visible in about two minutes.

**The panel itself is DONE and correct** — `366a0bd` in `D:\github\rezidnt-operator`, commits `3d5577d`/`366a0bd`.
It renders whatever bytes it is handed, handles all seven refusal codes honestly, and shows
"No diff was read, so none is shown." with no editor on screen for every refusal. Do not rebuild it.

### Previous next-action, kept for context

**The Review panel in `D:\github\rezidnt-operator`** (separate repo, sibling on disk, was at `7068b7f`
with 8 commits, outside the rezidnt gauntlet by DR-038 — it does not inherit rezidnt's loop).

An implementer was building it when this session ended; **check `git log` there first** — it may be done,
partly done, or untouched. It consumes two tools that are live and green on rezidnt `main`:

- **`diff_view`** — unbadged. `{worktree}` → `{worktree, lifecycle, outcome, diff: CasRef|null}`.
  `null` means no diff folded — render an honest empty state, **never an empty diff**.
- **`cas_read`** — **BADGED** (DR-058; `badge` is first in the args schema). Pass the whole `CasRef`, not a
  bare hash. → `{content, bytes_returned, truncated}`. The cockpit already threads an operator badge for
  `kill_run` — reuse it.

**The one failure mode this surface exists to prevent:** a refused read rendering as an empty or partial diff.
`cas.too_large` means the daemon REFUSED (256 KiB bound; it never truncates) — show the size, never a partial
diff presented as whole. Same for `cas.not_found` / `corrupt` / `not_text` / `not_utf8` / `hash_invalid` and
`badge.required` / `badge.invalid`.

**When that panel renders a diff, the IDE is closed and the goal is met.** Everything below is secondary.

## ► DO NOT — each of these will burn a session

1. **Do not propose making `delivery-harness` a rezidnt client.** The owner used it and rejected it: didn't
   beat plain Claude Code, too much ceremony, too slow. It is rezidnt's own loop with the serial numbers filed
   off, so that critique lands here too.
2. **Do not build approve / reject / redirect buttons.** Read-only Review is enough to close the editor.
   `approve` needs a merge hold that does not exist (merge is automatic on a verified pass) plus its own DR;
   **`redirect` is BLOCKED AT THE SUBSTRATE** — agents spawn `.stdin(Stdio::null())`, `attach` is one-way, and
   `claude -p` is one-shot. Reject (`kill_run`) and keep-for-triage (DR-049 §D3 default) already exist if you
   want them cheaply.
3. **Do not restart slice B without reconciling TWO contradictory boards** — `ddc892d` on
   `worktree-agent-ac2146777f0403fee`, and branch `slice-b-board-onmain`. They disagree on which crate owns
   key-derivation (`rezidnt-run` vs `rezidnt-mcp`). Handing an implementer both gives it two work orders.
4. **Do not chase audit findings that don't change runtime behaviour.** See the loop policy.

## ► LOOP POLICY (new this session — the owner called out looping)

**Triage `/debrief` findings by runtime impact. Security and correctness get fixed before the gate; prose,
pinning, and doc-comment accuracy get BATCHED into a cleanup slice and never block a ship. ONE remediation
round per slice, not N.**

This session ran two full debrief→remediate→re-debrief cycles. Round 1 found a real security hole (worth it).
Round 2 found a vacated mutation proof (real, but meta). Returns were clearly diminishing and the loop should
have been called sooner. Asking the auditor progressively more meta questions reliably manufactures more
findings — don't.

## ► STATE

`main` at `f7f8562`, working tree clean, **27 commits ahead of `origin/main` — NOT PUSHED.**

**Shipped:** `diff_view` + `cas_read` (DR-057; both gates passed) · the `cas_read` badge door + crate-level
`Cas::path_for` address guard + honest `InvalidAddress` mapping in `resolve_ref`/`honest_evidence_ref`
(DR-058; `/vet` green, **final `/debrief` was still running at session end — check it**).

**Records:** DR-056 (prose tax, ACCEPTED — its ~800-word cap was revised to ~1500 before ratification because
it was 0-for-5, and rejected-alternatives/counterargument text is excluded from the count), DR-057 (ACCEPTED),
DR-058 (ACCEPTED). **Next record is DR-059.**

**§20 index convention CHANGED:** a record takes its row and moves the pointer **when the file lands, whatever
its status.** The old withhold-until-acceptance practice went three records stale and would have minted DR-056
twice. Back-pointers into *other* records still wait for acceptance — that part was always right.

**DR-056 §Decision 2 is now a standing rule** and is in the rust-conventions skill: a doc comment asserting
another module's behaviour, or citing `file:line`, must be pinned by a source-text guard proven red by
deletion, or not written.

## ► BATCHED CLEANUP — `cas-badge-door`'s final /debrief FAILed and the slice SHIPPED ANYWAY

**This is a deliberate departure from "done = both gates pass," made under the loop policy above, and it needs
the owner's ratification if it becomes standing practice.** The final `/debrief` returned FAIL with five
findings. **All five are prose; ZERO change runtime behaviour.** F1 (the restored mutation proof), F3 (the
honest `InvalidAddress` arm), F4's core, and the new fact-bearing test all HOLD — the auditor verified every
mutation leg by tracing the code path rather than trusting the report. The security work is intact.

Fix these together in one cleanup pass; none blocks anything:

1. `crates/rezidnt-mcp/src/lib.rs` — the `// No echo of the argument.` comment above the `Internal` arm in
   `read_bounded`. `format!("resolve blob path: {e}")` interpolates `rezidnt_cas::CasError`'s Display, and
   `NotFound { hash }` renders the hash — so "no echo" is a claim about another crate, true only because the
   arm is unreachable. State what the line guarantees, or drop the absolute.
2. `crates/rezidnt-cas/tests/dr058_path_for_address_guard.rs` — the board header calls its own fragile leg
   unpinnable; it is already pinned by `path_for_refuses_every_non_address_with_invalid_address`. Cite the
   judge. Zero test code.
3. Relabel the other two "unpinnable" items **"behaviourally unpinnable, deliberately unpinned by source
   text"** — both ARE source-text pinnable by the instrument this slice built (~8 lines). One of three
   misclassified is the answer to "is that category growing": yes, watch it.
4. `crates/rezidnt-gate/tests/dr058_invalid_ref_honesty.rs` header — two `file:line` citations drifted when
   this slice's own doc paragraphs pushed the targets down. **Strip line numbers, keep file+symbol** (the
   warden's anchor discipline, at the bottom of this file, already says so).
5. `crates/rezidnt-mcp/tests/badge_enforcement.rs` — the badge-message pin is a one-word blacklist on
   "mutating"; "write tools require a badge" would pass while being false to a refused `cas_read` caller. The
   doc asserts the general property; say what is actually pinned.

## ► OPEN, none blocking

- **DR-053 is still PROPOSED** since 2026-07-25, back-pointers correctly withheld. Last unresolved record
  status in the tree. Owner's call.
- **The macaroon verb for `cas_read` is unruled** — the implementer picked `"read"`; no test pins it, so a
  `Verb`-caveated agent badge is refused today. Rule it before an agent consumer ships.
- **`CasReadArgs.bytes`' doc serves `rezidnt_mcp::MAX_CAS_READ_BYTES_DEFAULT`** — a private symbol, over the
  wire, to every MCP client. Same class DR-058 fixed for `hash`; owed to a later record.
- **arch §1 still names "Microsoft-stack enterprises that need an audit trail a compliance reviewer will
  sign"** as a wedge buyer, and the README leads with compliance evidence. Both contradict the stated goal.
  Owes a DR — it is a BINDING thesis change, not a slice.
- **A pattern worth its own rule:** three records this arc claimed a shape was unchanged when it wasn't
  (DR-049 via DR-052 item (d); DR-057's "additive"; DR-058's "shape unchanged"). All three reasoned about the
  *information* a change carries rather than the *serialized surface* it moves across. A ledger's shape claim
  should be checked against goldens, schemas, and wire responses — DR-053's five-limb form has no slot for it.

## Also (carried)

- `.claude/worktrees/agent-ab4e17a54fbbdb421/` is still an empty dir a Windows handle refused to delete.
  Gitignored, deregistered, harmless.
- If rust-analyzer reports phantom errors, restart it — the running instance can predate the pinned toolchain.

**Anchor discipline (warden-ratified 2026-07-24):** cite by SYMBOL, not line. A line number is admissible only
bolted to a commit hash.
