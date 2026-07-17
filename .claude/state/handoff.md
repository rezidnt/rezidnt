# Handoff — 2026-07-17 (session 4 close, part 4: /dr + warden + hardening cleanup DONE)

## State of play
**Current slice: S5** (ratatui read-only fleet board) — STILL not started; this was a cleanup pass, no S5 work. The golden path (S4) remains complete. This part cleared the accumulated /dr + warden + hardening debt in three loops, each vet+debrief'd:
1. **DRs 005/006/007 owner-ratified** (badge model Option A; replay-divergence→durable fact; RepoSubstrate release_worktree as-built). DR-005/007 doc-only; DR-006 became real work.
2. **Hardening batch** (6 low debt items) + 4 ontology cleanups — vet+debrief PASS.
3. **DR-006 integrity.alarm** — oracle→warden /subject→impl→**first debrief FAILED** (daemon-unreachable path)→oracle-first remediation→**re-debrief PASS**.

## Session log (part 4)
`6ab33fa` DR-005/006/007 → `9799689` hardening board+ontology cleanup → `313515c` hardening impl → `07a4aa3` gitignore junk guard → `65db6cc` DR-006 board+subject → `28fd41c` DR-006 impl. **PUSHED through 9b24ed6 earlier; parts-4 commits (6ab33fa..28fd41c, 6 commits) are LOCAL — push on owner order** (owner said "push. then spend a session on..." — the push covered through S4 close; these cleanup commits are unpushed).

## Next action
**Push the 6 cleanup commits, then S5 planning → `/oracle tui`.** S5 = ratatui read-only fleet board consuming ONLY watch channels (proof I1 held; pure client, no daemon change). Confirm with owner S5-vs-bank-more-hardening (golden path is done, pressure off). NOTE `/slice` first to re-read S5 criteria.

## What got fixed/ratified this part (off the queue now)
- **DR-005 badge model:** §12 rule narrowed to "state-mutating call"; gate_explain/tail read-class; badge.issued/revoked annotated no-emitter; operator badge = daemon-lifetime class. Zero code.
- **DR-006 integrity.alarm:** new subject + reducer (agent_runs[run].integrity_alarms, dedup (run,gate,verifier)) + daemon-routed append via new RecordAlarms socket op (CLI reads locally, daemon owns the write — I3). Divergence debrief now lands a durable, rebuild-visible fact. Daemon-down degrades loudly (report + exit 3 + stderr warning), never exit 1.
- **DR-007:** RepoSubstrate 3-method trait (incl release_worktree) ratified BINDING as-built.
- **Hardening:** HTTP 64KiB body cap; lockfile create_new/O_EXCL (0600 always); daemon.warning carries open correlation; InconclusiveReason::CouldNotRun (spawn/io-fail, distinct from MalformedOutput); agent_spec_toml extracted to rezidnt-run (byte-identical, dedup); environment_is_scrubbed rewritten (no process-global set_var); git-adapter doc aligned to at-least-once.
- **Ontology:** could_not_run reason, badge no-emitter, daemon.warning v1 {what,error}, worktree.conflict honest at-least-once. integrity.alarm v1 minted (35 subjects; SUBJECTS_V0 companion landed).
- **Infra:** cleaned a junk `C:Usersdakot/.cache` dir that a stray `git add -A` swept in (agent ran host cargo with the WSL CARGO_TARGET_DIR idiom; Git-Bash $HOME=C:\Users\...); added .gitignore guard `/C*Usersdakot*` + `.cache/`. [[vet-concurrency-flake]] memory also saved.

## Open /debrief residues (all in-latitude, tracked)
- **DR-006 (this part):** daemon-down test pins exit 3 + report but NOT the stderr-warning substring — loud-degradation guarded by inspection only; a future edit could drop the eprintln and stay green. Add a substring-tolerant stderr assert. (auditor-recommended, low)
- **Hardening:** HTTP cap can overshoot ~4KiB (one read) before the 413 (bounded, not unbounded); daemon.warning ontology note phrasing slightly understated now that failed opens deliberately share correlation.
- **S4 carried:** exec spawn EAGAIN now surfaces as CouldNotRun under load (the flake, now honestly labeled); daemon diff-summary still duplicates the S2 adapter parser (route through RepoSubstrate when wired — ties to DR-007's deferred GitError item); run_native cost floor .max(1) not replay-stable (replay compares verdicts only, no risk).

## /dr and warden queue (REMAINING)
- **Deferred (not blocking, no owner decision pending):** DR-007's GitError→associated-type I4 fix (when a 2nd RepoSubstrate impl lands); badge_id on other mutation facts + badge.issued emitter (if a delegation use case appears) — both DEFAULT-deferred by DR-005, foreclose nothing.
- **Carried non-slice:** capture-chunk subject (Phase-3 demand-gated, scribe assessed "premature" — leave tracked); scribe note hand-rolled-over-rmcp DEFAULT + the T8 silent DEFAULTs (protocol version, tail limit, 202, dedicated runtime, schemars runtime dep); S1 hardening list; root README; crates.io placeholder (owner `cargo login`); `rezident` fallback doc note; S2-T4 ingest helper (next git-adapter touch); S2-T5 prune verb + T1 (Phase-2/S5 hardening); demo recording (S3) location not noted in-repo (docs/demo/?).

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target` **(WSL-ONLY — never export on host/Git-Bash cargo: creates the junk dir)**. Vet hook host-side; daemon/gate tests WSL. **Run host vet.sh and WSL workspace SEQUENTIALLY, never concurrent (spawn flake).** `cargo test -p rezidentd` alone doesn't rebuild the sibling `rezidnt` bin — use `--workspace` or `cargo build -p rezidnt` first for daemon tests that shell the CLI. Fable 5 hit its weekly credit limit — all agents ran on Opus 4.8 this part (owner switched default). Demo daemon may still be up (port 40173, ~/rezidnt-demo).
