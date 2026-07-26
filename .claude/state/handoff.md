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

## ► NEXT ACTION

**Finish the Review panel in `D:\github\rezidnt-operator`** (separate repo, sibling on disk, was at `7068b7f`
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
