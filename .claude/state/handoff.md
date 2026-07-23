# Handoff — 2026-07-23 (session 22: two DR-036 follow-ups + the WHOLE DR-037 installer arc — golden-path `curl` install is now REAL)

## State of play
Cold-started from session 21's handoff (DR-036 onboarding arc complete). Owner steered toward the installer.
Delivered: two non-blocking DR-036 follow-ups, then the full **DR-037 installer arc drafted → ratified → 3 slices
shipped**, culminating in a published **`v0.0.1` pre-release** and a `curl | sh` install PROVEN end-to-end. Everything
/vet + /debrief PASS, pushed to `origin/main` (synced, `a50e576`). High autonomy ON ([[autonomy-high-trust]]).
`current-slice` = `quickstart-real` (**done** — last slice of the DR-037 arc; the arc is COMPLETE).

The §1/§18 BINDING golden path's FIRST step — `curl` install — is now real (it was aspirational after DR-036 shipped
every OTHER step). DR-036 closed "zero config edits"; DR-037 closes the install step.

## What shipped this session (each through the full loop)
1. **Two DR-036 follow-ups** (`724068b`) — nested-verb lockstep coverage (`quickstart_lockstep.rs` now catches drift
   in `gate why`, not just `gate`) + the `check_socket_writable` REZIDNT_LOCKFILE-first divergent-parent pin
   (`doctor_socket_unix.rs`). /vet + /debrief PASS.
2. **DR-037 ACCEPTED** (`0aefd51`) — distribution/release posture; plan wired (§1/§16/§18 amended-by pointers +
   §18 risk row + §20 index, next-record → DR-038). Owner-settled: Linux/WSL-first; `x86_64-unknown-linux-musl` only
   (aarch64 deferred); TWO static binaries this arc (the pre-existing reality — the combined multi-call binary is a
   NAMED follow-up, NOT an I7 per-artifact re-read); raw GitHub-asset endpoint; DR-007 is a naming collision (its
   `release_worktree` is a runtime trait, unrelated).
3. **`release-ci`** (`0137bcf` + `21afaef` SHA-pin) — `.github/workflows/release.yml` (repo's FIRST CI): on a semver
   tag, cross-compiles both static musl binaries, self-gates on I7 (fails if `file`/`ldd` show non-static), strips,
   emits `SHA256SUMS`, publishes as Release assets (`v0.*` → `--prerelease`). Observed GREEN on GitHub.
4. **`install-script`** (`90d11f9`) — clean-room POSIX-sh `install.sh` (repo root) + `bins/rezidnt/tests/curl_sh_unix.rs`
   (`#[cfg(unix)]`, fixture-driven via `file://`). Verifies sha256 BEFORE install (fail-closed, no partial install),
   Linux/WSL+x86_64 gate with plain refusal, no telemetry.
5. **Pre-release prep** (`4380c77`) — `v0.*`→prerelease in the workflow; `install.sh` resolves newest via `/releases`
   (NOT `/releases/latest`, which excludes pre-releases).
6. **`v0.0.1` cut** (tag pushed) — workflow ran green INCLUDING publish; **`v0.0.1` is a live pre-release** (assets:
   `rezidnt-x86_64-unknown-linux-musl`, `rezidentd-x86_64-unknown-linux-musl`, `SHA256SUMS`). Real `curl | sh` PROVEN
   end-to-end (resolves v0.0.1 via /releases, https fetch, sha256 verify, both bins install, `rezidnt --version`→0.0.1).
7. **`quickstart-real`** (`a50e576`) — flipped `docs/quickstart.md`'s install block from "not yet live" to the live
   `curl -fsSL https://raw.githubusercontent.com/rezidnt/rezidnt/main/install.sh | sh`, with honest pre-release/Phase-1
   framing. /debrief PASS (prose honesty is a hand-held obligation — the lockstep judge doesn't cover install prose).

## Reusable knowledge (for the next author)
- **musl toolchain is now installed on WSL** (Ubuntu-24.04): `musl-tools` + the `x86_64-unknown-linux-musl` target.
  Build recipe (proven): from repo root, `export CC_x86_64_unknown_linux_musl=musl-gcc` then
  `cargo build --release --target x86_64-unknown-linux-musl -p rezidnt -p rezidentd`. Static works because no OpenSSL
  anywhere (rustls/rcgen use `ring`), `portable-pty` is unlinked, `rusqlite` bundles SQLite (compiles under musl-gcc).
  Full recipe + release-workflow details in [[installer-arc-progress]].
- **Windows UAC install-name trap**: a host test file `install_script_unix.rs` failed with os error 740 (Windows flags
  `*install*`/`*setup*`/`*update*`/`*patch*` exes for elevation). Renamed → `curl_sh_unix.rs`. Heed PROACTIVELY when
  naming any installer/updater test ([[windows-test-binary-update-uac]], description broadened this session).
- `rezidentd` is a bare daemon with NO clap/`--version` (bare `unix_daemon::run()` shim) — never execute it as a smoke
  test (it binds a UDS and hangs); prove its staticness via `file`/`ldd` only. The release workflow's run-smoke is
  guarded to `rezidnt` only.
- `gh` is authed (account `smithdak`); `gh workflow run release.yml --ref main` runs a build+verify dispatch WITHOUT
  publishing (publish gates on `refs/tags/`).

## Open /debrief findings (NON-BLOCKING, none blocks done)
- **quickstart internal-consistency** (auditor-flagged, deferred): `docs/quickstart.md`'s "What you just saw" section
  (~lines 104-108) + the "one take … single-digit minutes" bar still narrate steps 2-5 as fully working today; only the
  new line-25 hedge admits the full path isn't live until the Phase-1 exit. NOT a violation (the install block itself is
  honest; steps 2-5 are real DR-036 verbs) — but worth a scribe pass to soften the demo narration when S1/S3 close.
- **install.sh coverage**: the real https/curl path + `/releases` resolution are now PROVEN live (no longer the
  auditor's gap 1/2), but are still not UNIT-tested (only the `file://` fixture is). Same-origin checksum trust (gap 3)
  is by-design (DR-037-accepted `curl|sh` model). A unit test of the API-parse branch is a nice-to-have.
- **checkout Node-20 deprecation**: the SHA-pinned `actions/checkout@v4.2.2` targets Node 20 (force-run on Node 24); a
  future bump to a checkout v5 SHA clears the annotation. Cosmetic.

## Decisions still needing a /dr
- **Combined multi-call single binary** (literal-I7 form) — a daemon-crate extraction (pull `bins/rezidentd/src`'s
  ~714-line `unix_daemon` into a lib so `rezidnt daemon` can dispatch). DR-037 NAMED this as the I7-honoring follow-up;
  it wants its own slice/DR. Not urgent.
- Prior carried (unrelated): macOS/Windows sandbox+egress backends; MCP-based 1Password egress backend.

## Next action (owner's steer — DR-037 arc COMPLETE, nothing gated)
No forced next. `current-slice` = `quickstart-real` (done); the installer arc is finished; `v0.0.1` pre-release is live.
Natural candidates:
1. **Phase-1 core to make the FULL golden path real** — §16 S1 (herdr adapter + `rezidnt open` materialization) and S3
   (MCP surface). This is the highest-leverage next: it's what makes the demo the doc narrates ACTUALLY complete
   end-to-end (and lets the "What you just saw" prose become fully honest). The install step is now real; the *rest* of
   the one-take is the remaining gap to the Phase-1 exit demo (the only definition of done).
2. **A different roadmap phase** — benchmark harness (DR-022); macOS/Windows sandbox+egress backends (would un-gate a
   cross-platform installer later).
3. **The combined multi-call binary DR** (literal-I7 installer form) or the small non-blocking follow-ups above.

## Environment (essentials)
Host `/vet` = `bash .claude/hooks/vet.sh` (definition-of-done). The install.sh CORE is platform-neutral prose/shell; its
test (`curl_sh_unix.rs`) is `#[cfg(unix)]` → needs WSL clippy+test ([[vet-is-host-side-wsl-insufficient]]). WSL:
`wsl.exe -d Ubuntu-24.04 -e bash -lc 'cd /mnt/d/github/rezidnt && export CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target
PATH=$HOME/.cargo/bin:$PATH && cargo …'` ([[wsl-dev-environment]]). Host+WSL SEQUENTIAL ([[vet-concurrency-flake]]).
The `clippy doc_lazy_continuation` trap bit once more this session (a doc paragraph after a `//!` bullet list needs a
blank `//!` separator) ([[clippy-doc-lazy-continuation-trap]]). Untracked `.playwright-mcp/` + `docs/site/` are stray,
not part of the project — leave them.

---
**NEXT ACTION → DR-037 installer arc COMPLETE: two DR-036 follow-ups + release-ci → install-script → quickstart-real all
shipped, every slice /vet + /debrief PASS, pushed to origin/main (`a50e576`). `v0.0.1` published as a GitHub pre-release;
the golden path's `curl | sh` install step is REAL and proven end-to-end. `current-slice` = quickstart-real (done). NO
forced next — owner's steer. Strongest candidate: Phase-1 core (§16 S1 herdr adapter / S3 MCP) to make the FULL one-take
golden path actually complete (the install step is done; the rest of the demo is the remaining Phase-1-exit gap).
Alternatives: benchmark DR-022; macOS/Windows backends; the combined-binary literal-I7 DR. High autonomy ON.**
