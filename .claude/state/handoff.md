# Handoff — 2026-07-24 (session 26: registry-convergence SHIPPED · DR-047 accepted)

## State of play
`current-slice` = **`registry-convergence`** (DR-046 §Decision 8), and it is **DONE**: host `/vet` **pass** (638/0),
`/debrief` **PASS** on the fourth gate, WSL workspace **806/0**. Pushed and synced to `origin/main` at `8e1ef5c`
(+ the DR-047 commit). Tree clean except the pre-existing untracked `.playwright-mcp/` and `docs/site/` (leave them).
Nothing mid-flight, no agent running. High autonomy ON ([[autonomy-high-trust]]).

**Shipped:** `rezidentd` takes its first `rezidnt-adapter-git` dependency; **both** allocation paths (ordinary spawn and
fan-out) now allocate through `RepoSubstrate::alloc_worktree` on one `GitAdapter` per **canonicalized repo root**
(`Daemon::repo_adapter`); adapter facts **append** to the fabric through an injected `FactSink` whose `Err` fails the
allocation (the adapter still has **no** `rezidnt-fabric` dep — I4); exactly one `worktree.allocated` per allocation;
`codes::WORKTREE_CONFLICT` minted, so **DR-044 §Decision 3's refused-sub rule is REACHED for the first time** — a
contended task is refused alone and its siblings still spawn.

## Records
- **DR-047** (ACCEPTED, ratified under the standing autonomy grant — overturn knowingly if you disagree). Discharges
  DR-046 §Decision 8 in full and records that its brief, marked "all verified against the tree", was not: **one** stated
  premise false, **five** conditions unnamed, one a hard blocker. Ratifies the two DEFAULT layout rulings, rules
  `diff.ready`'s two emitters deliberate, records the post-merge clobber, and names the next slice.
- Ontology corrected by the warden (prose only; no subject, no field, vocabulary byte-identical, `v` stays 1).
  `:218-219` had been corrected that *morning* to say the registry was unreached — this slice falsified it the same
  afternoon. Recorded as **superseding**, not overwriting, since the failure it guards against runs in both directions.

## Open findings (all non-blocking; `/debrief` PASSED with these named)
- **C8 (canaries) is `inconclusive` on the `/debrief` side, four passes running, and correctly so.** It is a claim about
  executed outcomes no read-only checker can discharge; it belongs to `/vet`, which executes and reported pass. Do not
  read it as a defect and do not try to "fix" it.
- **`is_change_event` is NARROWED, not closed.** Host `/vet` judges the predicate; it never judges the watch loop's
  *use* of it. Say narrowed.
- **A whole class of watcher behavior is WSL-only** — `ReadDirectoryChangesW` produces no read events (Windows 0, WSL 1),
  so the Decision-5 defect and any successor are structurally invisible to host `/vet`.
- **Two guards are test-after-implementation**, disclosed in-file: the `startup_facts` drain and the C7 sibling leg's
  plumbing.
- **OWED, outside this slice's diff:** `crates/rezidnt-mcp/tests/gate_explain.rs:10-15` claims no ratified v1 baseline
  for the gate subjects. False. The correction to write is **"all FIVE `gate.*` subjects are ratified — four in the S3
  set, `gate.passed` in the S4 set"**. The S3 note defers `gate.passed`; the S4 set then ratifies it outright. Reading
  only the S3 deferral is how an earlier draft of this bullet seeded a false caveat *inside* a correction.
- **Three prose residues** the closing `/debrief` named, one line each next time those comments are touched: "refs" in
  the private-gitdir sentence (only HEAD/bisect/worktree/rewritten are per-worktree); "only WSL boards go red"
  understates host coverage (a `dead_code` lint trips); and one bullet above rules a two-clause caveat "confirmed false"
  while substantiating one clause.
- **`crates/rezidnt-tui/examples/board_rich.rs:323`** still fails WSL clippy (`unnecessary_sort_by`), pre-existing,
  invisible to host clippy. Same class as [[golden-txt-crlf-host-vet]].

## Decisions still needing a /dr
None outstanding. DR-047 §Decision 6 already **fixes the brief** for the next slice — but read the arc's lesson before
building to it: a fixed brief is a hypothesis.

## What's next (owner's steer — nothing forced)
**DR-047 §Decision 6 — the worktree RELEASE lifecycle slice.** `release_worktree` has **no production caller**, so
DR-007's ratified allocate→use→release never completes: every allocation leaks a `notify` watcher plus a debounce task
for the daemon's lifetime, trees stay on disk, registry entries are never closed. It is genuinely **undecided**, not
merely unbuilt — releasing at merge emits `worktree.released`, which would fold `status = "released"` over `"merged"`,
trading one derived-state regression for another. That slice must settle: is a merged worktree retained? what does
`worktree.released` do to a `"merged"` fold? does a failed run's tree survive for triage? and it owes the `WorktreeId`
threading `RunTaskContext` lacks. Also queued: **one warden `/subject`** (DR-047 §Decision 4) for the `diff.ready`
emitter cell and the `source: "rezidnt-adapter-git"` attribution the daemon's own fact carries — wire-visible and in
golden fixtures, so it moves *with* the ontology, never ahead of it.
Alternative arc: the **owned terminal substrate**, the other Phase-3 line and the actual herdr-removal endgame.

## Environment (essentials)
`/gauntlet` is the done gate now (vet ∥ debrief, concurrent). Host `/vet` = `bash .claude/hooks/vet.sh`; `--fast` for
inner loops. `bins/rezidentd/src/main.rs` gates `mod runs|mcp|gates` on `#[cfg(unix)]`, so **no daemon unit or e2e test
is ever visible to host `/vet`** — run WSL too, always **sequentially** ([[vet-concurrency-flake]]; it reproduced
host-vs-host this session, not just host-vs-WSL). **Check a record's premises against the tree before building to it** —
this arc has produced **eleven** defects of that one class, and **three of the eleven were introduced by remediation
commits**, including one inside the sentence its author reported catching ([[fanout-silent-wrong-pattern]]).

---
**NEXT ACTION → `registry-convergence` COMPLETE and closed. DR-046 §Decision 8 discharged in full; DR-047 ACCEPTED;
DR-044 §Decision 3 reached for the first time. Host /vet 638/0 pass + /debrief PASS + WSL 806/0, pushed at `8e1ef5c`.
NO forced next — owner's steer; strongest candidate is the DR-047 §Decision 6 worktree-release-lifecycle slice, which
is genuinely UNDECIDED (releasing at merge trades one derived-state regression for another), plus one queued warden
/subject. High autonomy ON.**
