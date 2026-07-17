# Handoff — 2026-07-17 (session 5: S5 fleet board + hardening batch COMPLETE)

## State of play
**Current slice: S5 (ratatui read-only fleet board) — DONE**, then a **2-item hardening
batch banked** (owner chose "bank hardening" over Phase 3, which stays demand-gated). Both
passed `/vet` + `/debrief` (auditor **pass** on each). Golden path (S4) + S5 both complete;
no scheduled slice remains.

## What changed this session — 3 commits (`712ffc5` PUSHED; `9239b68` + handoff LOCAL)
**`712ffc5` (S5, pushed):**
- **New crate `crates/rezidnt-tui`** — pure testable core: `project(&Graph)->BoardView`
  (state carried verbatim, I3), `draw(&BoardView)` via ratatui (TestBackend golden),
  `ingest_into_watch` (fold onto a `watch::Sender<Graph>`). Runtime deps limited to
  rezidnt-state + rezidnt-types + ratatui/crossterm + tokio[sync]; proto/ulid dev-only —
  the structural read-only proof, guarded by `tests/read_only.rs` (real writer-dep tripwire).
- **`bins/rezidnt` `board` subcommand** — pure socket client (I1): rides the EXISTING
  `Request::Tail{None}` op (no daemon change, no new proto op), spanned ingest+render adapter
  tasks over the watch seam, crossterm raw-mode with unconditional teardown; non-unix stub.
- First render deps in the workspace (ratatui 0.29 + crossterm 0.28, MIT, doc-blessed S5).
- Daemon, `rezidnt-proto`, `spec/ontology.md` untouched. 8 assert-red tests + 3 guards, 11/11.
- Memory: [[windows-test-binary-update-uac]] (os error 740 on host test-bins named *update*).

**`9239b68` (hardening batch, LOCAL — ready to push):**
- **rezidnt-mcp HTTP body-cap tightened (I2):** new pure `next_read_len(accumulated,cap,buf)`
  clamps each read (`min(buf, cap-accumulated)`) + at-cap reject (`>=` not `>`), wired into
  `serve_http_conn` body loop. Body is now held `<= cap` at all times (was `cap + one 4KiB
  read`). Up-front Content-Length reject + 413 intact; 3 helper unit tests (2 formerly-red).
- **DR-006 daemon-down stderr guard closed:** `golden_path.rs` now asserts the loud
  degradation warning (substring `NOT durably recorded` + `unreachable`) — dropping the
  eprintln now goes red. Honest regression guard, WSL-only (`#![cfg(unix)]`).

## Next action
**Push `9239b68` (owner order), then confirm the next direction.** No scheduled slice remains:
(a) Phase 3 stays DEMAND-GATED (pull only if attach-fidelity friction is measured); (b) more
hardening — remaining residues below are gated/doc-only, thinner than this batch; (c) move
toward a release (root README, `rezident` fallback note, crates.io needs owner `cargo login`).

## Open /debrief findings (this session — all CLEARED / notes only)
- S5 auditor `pass` residues both fixed before commit: stale oracle-scaffold doc prose in
  `rezidnt-tui/src/lib.rs` deleted; `board()` panic-teardown doc claim softened (no Drop guard;
  wrap terminal in one if panic-safe restore is ever wanted — deferred, low).
- Hardening auditor `pass`, notes only: the body-cap defense is duplicated (up-front reject +
  in-loop clamp) — if a future refactor drops the up-front Content-Length reject, re-audit (the
  `None`/413 branch relies on it). `BODY_CAP_BYTES` is defined in both lib.rs:84 and the
  integration test (http_body_cap.rs:33) — intentional, but can silently drift. Both pre-existing.

## Carried residues / debt (non-slice, tracked — two items BANKED this session, struck below)
- **S4/DR-006 carried:** ~~DR-006 daemon-down stderr assert~~ (DONE `9239b68`); ~~HTTP body cap
  overshoot~~ (DONE `9239b68`); exec spawn EAGAIN surfaces as CouldNotRun under load (honestly
  labeled, no action); daemon diff-summary duplicates the S2 adapter parser (route through
  RepoSubstrate — **DR-007-gated**, fires when a 2nd impl lands); run_native cost floor `.max(1)`
  not replay-stable (verdict-only replay, no risk).
- **/dr + warden queue (deferred, no owner decision pending):** DR-007 GitError→associated-type
  I4 fix (when a 2nd RepoSubstrate impl lands); badge_id on other mutation facts + badge.issued
  emitter (if delegation use case appears); capture-chunk subject (Phase-3 demand-gated); the T8
  silent DEFAULTs; S1 hardening list; root README; crates.io placeholder (owner `cargo login`);
  `rezident` fallback doc note; S2-T4 ingest helper; S2-T5 prune verb; demo recording location
  (docs/demo/?).

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY — never export on host/Git-Bash cargo: creates the junk dir)**. Vet hook host-side;
daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent (spawn
flake — [[vet-concurrency-flake]]).** Host test/bin file names must avoid the substring `update`
(UAC os error 740 — [[windows-test-binary-update-uac]]). `rezidnt board` is `#[cfg(unix)]`; the
Windows named-pipe path is stubbed (bails). Demo daemon may still be up (port 40173, ~/rezidnt-demo).

---
**NEXT ACTION → Push `9239b68` on owner order, then confirm the next direction (`712ffc5` already
pushed; no scheduled slice remains: Phase 3 is demand-gated, thin gated/doc residues, or release).**
