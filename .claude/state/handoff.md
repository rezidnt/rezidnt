# Handoff — 2026-07-22 (session 21: --scope CLI + live-op demo + DR-036 onboarding arc ALL shipped)

## State of play
Cold-started from session 20's handoff (DR-034 live-unblock + DR-035 TTL/grant-all complete). Owner steered
"live op, then we will focus on onboarding." Delivered both and the whole onboarding arc. Everything /vet + /debrief
PASS, pushed to `origin/main` (synced, `5b36157`). High autonomy ON ([[autonomy-high-trust]]). `current-slice` =
`quickstart` (**done** — the last slice of the DR-036 arc; the arc is COMPLETE).

Five units shipped this session, each through the full loop:
1. **`--scope` CLI flag** (`e295887`) — DR-035 §Decision 2 parity: `rezidnt operator resolve-permit --scope run_tool`
   (the MCP arg existed; the CLI only had `--ttl-ms`). Thin verbatim pass-through; daemon owns the semantics.
2. **Live-op e2e demo** (`5ff5941`) — `bins/rezidentd/tests/operator_liveops_e2e.rs`: every operator action driven
   against a REAL daemon through the REAL `rezidnt` CLI (resolve allow/deny, live-unblock, TTL-boxed, `--scope`
   broad grant, kill-run, coupling-guard refusal). First live-daemon exercise of the `--scope`/`--ttl-ms` flags.
3. **DR-036 ACCEPTED** (`6596395`) — operator onboarding arc (audience + scope owner-settled: operator adopting
   rezidnt; command AND docs; fullest command scope = doctor + init wrapper; docs under `docs/`).
4. **The 4-slice arc** — `spec-init` (`e48a01a`), `onboarding-doctor` (`b848e4d`), `init-wrapper` (`6735ffa`),
   `quickstart` (`a18e353`).
5. **DR-036 §9/§13 amendments** (`5b36157`) — CLI verb list + spec-init sentence updated now the verbs are real.

## What the onboarding arc built (the golden path's "zero config edits" clause is now MET)
Before this, an operator on a cold machine could not reach `rezidnt open` with zero config — the §9/§13-specified
`rezidnt spec init` generator did not exist. Now, all in the ONE `rezidnt` binary (I7), plain-CLI (I1, no TUI),
fact-free/daemon-free except the `open` step (I3), no telemetry (I7):
- **`rezidnt spec init [DIR] [--defaults] [--force]`** — interactive generator writing a §13 `rezidnt.toml` the
  golden path opens UNTOUCHED. Anti-drift: re-parses its own output through the real `rezidnt_run::spec::ProjectSpec`
  before writing. Bare-verb clobber guard = exit 2 without `--force`.
- **`rezidnt doctor [--json]`** — read-only §11 preflight (`git`/`harness`/`socket-lockfile-writable`/`wsl`).
  I6 never-coerce: undeterminable → `inconclusive`, NEVER `pass`. Exit 0 all-pass / 3 any non-pass / never 5.
- **`rezidnt init [DIR] [--defaults] [--force]`** — the entry: chains `doctor → spec init → open` in-process. Gate on
  fail (abort 3), warn on inconclusive+proceed (owner-settled). Wrapper clobber nuance: existing spec w/o `--force`
  → SKIP + open (byte-unchanged), NOT bare-verb's exit 2.
- **`docs/quickstart.md`** — the narrated one-take demo, kept honest by a lockstep test
  (`bins/rezidnt/tests/quickstart_lockstep.rs`) that mines every `rezidnt <verb>` and drives the shipped binary so
  the doc can't drift from the CLI.

## Reusable seams worth knowing (for the next slice/test author)
- `rezidnt_testkit::cli_bin()` locates the real `rezidnt` binary (sibling of `daemon_bin()`), so a cross-crate test
  can drive the actual operator/onboarding CLI, not just the socket/MCP doors. Used by the live-op + init e2e.
- The daemon resolves `spec.repo` against ITS OWN cwd (`bins/rezidentd/src/runs.rs:~565`). A default spec's
  `repo="."` therefore materializes the DAEMON's cwd, not the spec dir — so `spec_init_open_e2e.rs`/`init_wrapper_e2e.rs`
  either splice the tempdir's ABSOLUTE path into the generated `repo` (spec-init e2e) or start the daemon with
  `current_dir` = the scaffolded repo + a `claude` stub on its PATH (init e2e). New e2e tests: reuse one of these.
- `run_doctor_checks()` and `generate_spec()` are factored out of `doctor()`/`spec_init()` for reuse — `init` calls
  them directly. `check_socket_writable` now prefers `REZIDNT_LOCKFILE` over `REZIDNT_SOCKET` (the path this CLI is
  authoritative about; a dead socket is `open`'s exit-4 concern, not a doctor gate).

## Next action (owner's steer — DR-036 arc COMPLETE, nothing gated)
No forced next. `current-slice` sits at `quickstart` (done); the onboarding arc is finished. Natural options:
1. **Non-blocking follow-ups from this arc's debriefs** (both auditor-noted, low priority, no DR):
   - `quickstart_lockstep.rs` extractor checks only the FIRST verb token, so a nested sub-verb (`gate why`) is
     verified only as `gate`. Extend to the two-token form to catch nested drift. (`bins/rezidnt/tests/quickstart_lockstep.rs:~136`)
   - The `check_socket_writable` REZIDNT_LOCKFILE-first precedence has no test pinning the divergent-parent case
     (socket parent ≠ lockfile parent). Add one small pin.
2. **A real `curl | sh` installer** — `docs/quickstart.md` frames it as "intended, not yet live"; building the actual
   install script + release would make the golden path's install step real (currently `cargo install --path` is the
   works-today path). Likely needs a DR (release/distribution posture).
3. Other roadmap phase: benchmark harness (DR-022), macOS/Windows sandbox+egress backends, the operator `--scope`
   ergonomics elsewhere.

## Open /debrief findings (NON-BLOCKING, none blocks done)
- spec-init: the e2e took 4 debrief rounds purely on test-doc honesty (stale RED-first framing leaking into a live
  assertion message + an e2e opening the daemon's cwd repo instead of the scaffolded tempdir). All remediated. The
  host `spec_init_cli.rs` still carries oracle "RED today" DOCSTRING framing (auditor ruled non-blocking, matches the
  committed `operator_resolve_permit_cli.rs` idiom); reword to past tense if that file is next touched.
- init-wrapper: `check_socket_writable` REZIDNT_SOCKET→REZIDNT_LOCKFILE swap adjudicated SOUND (not a mask); the
  divergent-parent path of the retired socket-first probe is now untested (see follow-up 1 above).

## Decisions still needing a /dr
- None outstanding from this arc. A real installer (follow-up 2) would want a distribution/release DR (DR-037).
- Prior carried (unrelated): macOS/Windows sandbox+egress backends; MCP-based 1Password egress backend.

## Environment (essentials)
Host `/vet` = `bash .claude/hooks/vet.sh` (definition-of-done). The onboarding generator/doctor/init CORE is in the
platform-neutral `rezidnt` bin and is host-lintable; the e2e tests (`spec_init_open_e2e.rs`, `init_wrapper_e2e.rs`,
`operator_liveops_e2e.rs`) are `#[cfg(unix)]` and need WSL clippy+test ([[vet-is-host-side-wsl-insufficient]]).
WSL: `wsl.exe -d Ubuntu-24.04 -e bash -lc 'cd /mnt/d/github/rezidnt && export CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target
PATH=$HOME/.cargo/bin:$PATH && cargo …'` ([[wsl-dev-environment]]). Build BOTH bins on WSL before an e2e run (the
e2e's `cli_bin()` locates the sibling `rezidnt`). Host+WSL SEQUENTIAL ([[vet-concurrency-flake]]). Watch
[[clippy-doc-lazy-continuation-trap]] — it bit THREE times this session (oracle test headers + a wrapped list item);
a `//!`/`///` continuation line must not start with `+`/`-` or lazily continue a list without a blank line/indent.
Added `.rezidnt/` to `.gitignore` (daemon runtime worktree state, never tracked). Untracked `.playwright-mcp/` +
`docs/site/` are stray, not part of the project — leave them.

---
**NEXT ACTION → --scope CLI + live-op demo + the entire DR-036 onboarding arc (spec-init → doctor → init-wrapper →
quickstart) all shipped, every slice /vet + /debrief PASS, pushed to origin/main (`5b36157`). `current-slice` =
quickstart (done); onboarding arc COMPLETE, DR-036 amendments applied (§9/§13/§16/§20). NO forced next — owner's
steer. Strongest candidates: (1) two small non-blocking follow-ups (nested-verb lockstep coverage; divergent-parent
socket-check pin), (2) a real `curl | sh` installer to make the quickstart's install step real (likely DR-037,
distribution posture), (3) a different roadmap phase (benchmark DR-022; macOS/Windows backends). High autonomy ON.
Onboarding CORE is host-lintable; the three e2e suites are #[cfg(unix)] → WSL.**
