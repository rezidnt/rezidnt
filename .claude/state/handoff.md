# Handoff — 2026-07-20 (session 13: direction fork resolved → C3a Linux OS-sandbox SHIPPED)

## State of play
The Phase-2-exit direction fork is resolved. Owner considered pulling **Platform** (native-Windows
named-pipe transport, the "nobody owns Windows" wedge) forward, then chose **finish the core arc first** —
and since Phase 2 was already EXITED (last session), the real next work was the fork itself. Owner picked
**C3 sole-chokepoint**, Linux-first. One full loop shipped: **C3a (Linux OS-sandbox) is DONE** — `/vet` host
PASS, WSL bwrap suite 3/3, `/debrief` PASS. Pushed to `origin/main` at **`924c86b`**. Tree clean, synced.
`current-slice` reads `c3a` (done). High autonomy ON ([[autonomy-high-trust]]).

## The Platform deferral (why it matters going forward)
Windows named-pipe transport is a **Phase-3** item, deliberately sequenced after gates (architecture §11,
§254). Owner deferred it. It is coupled to C3 at exactly one point: **full Windows enforcement parity
(sandbox via Job Objects/AppContainer) waits on the native-Windows daemon** — so C3 ships Linux/macOS first
and Windows degrades loudly (DR-025 §6). No reorder implied. When Platform is picked up later it is its own
direction fork (design a transport seam / `SandboxSubstrate` Windows tier), gated by the phase-exit-demo test.

## What shipped this session (all pushed)
- **`75af7d9` — C3 design sketch + DR-025.** `docs/design/permit-sole-chokepoint-c3.md` decomposes C3 into
  three independently-shippable primitives (**C3a** sandbox / **C3b** egress proxy / **C3c** credential
  brokering); recommends C3a first (containment is the foundation the other two require — a bypassable proxy
  is theater). **DR-025** ratifies C3a scope (owner sign-off — NOT self-ratified; C3 is a posture commitment,
  the DR-009 fence required its own design + impl DR). §16 sliced, §20 index + pointer, current-slice→c3a.
- **`924c86b` — C3a build.** `SandboxSubstrate` trait (I4) + Linux `bwrap` impl in `crates/rezidnt-run/src/
  sandbox.rs`; `PathConfinement` native permit-verifier in `crates/rezidnt-gate` (registered). bwrap exec'd
  via `std::process` like the git-CLI (zero new sandbox-binding crate, I7); loud `Unavailable{reason}` degrade
  (I6, never a silent allow); no ontology minted (sandbox.* is a deferred warden /subject). 10 files, +1395.

## The maker/checker discipline earned its keep AGAIN (all caught pre-done)
1. **Criterion-1 fixture under-bound the toolchain** — the WSL write-inside test bound only `/bin` ro; on
   usr-merged Ubuntu-24.04 (`/bin`→`/usr/bin`) bwrap couldn't resolve `/bin/sh`'s interp/libs, so the child
   never execed. The IMPLEMENTER flagged it back rather than patching the oracle's fixture (maker touching
   checker's artifact). Oracle repaired: added read-only toolchain binds `/usr,/bin,/lib,/lib64`.
2. **Criterion-2 was passing VACUOUSLY** — same under-bind meant the denial test's "no file at /etc" passed
   because the shell never ran, not because the write was denied. Oracle caught it while fixing #1: now writes
   an inside-worktree sentinel FIRST and asserts `ran_marker.exists()` before asserting the escape denied.
3. **No-widening enforced in the TYPE SYSTEM** — `SandboxPolicy.binds` private, `from_folded_authority` the
   sole constructor, `bwrap_argv` never reads `_plan`; adversarial tests smuggle `/`,`/etc`,`/root`,
   `--share-net` through the plan and prove none reach the argv. Auditor found no escape hatch.
Reusable rules distilled: **a sandbox confinement fixture must bind the toolchain the confined shell needs, or
the denial passes vacuously (shell never execs)** — always prove the shell RAN (inside-bind sentinel) before
asserting an escape was blocked; **the no-widening wall belongs on the policy type (`binds` private +
folded-only constructor), NOT on `SpawnPlan`** — the tripwire is `sandbox_no_widening_c3a.rs` if any future
change reads `plan.env`/`plan.args` inside `bwrap_argv`.

## Autonomy — [[autonomy-high-trust]] (owner granted 2026-07-20)
Proceed WITHOUT asking: full loop, commit+push green+debrief-PASS increments, draft+self-ratify routine
engineering DRs. **This session an owner sign-off WAS required** (DR-025 held PROPOSED until the owner said
"proceed") — C3 is a posture commitment fenced by DR-009, not self-ratifiable. Direction forks at milestones
get a light checkpoint (asked Platform-vs-core → owner picked core/C3, then Linux). **Still surface:**
irreversible/destructive git, a DR amending BINDING invariant TEXT or licensing/clean-room, firewalled sources,
publishing beyond a main push, one-way doors, and C3b/C3c/Windows scope (each its own DR).

## Next action — C3 continues, or a smaller item (all DR-gated)
C3a is the containment foundation; the roadmap MAY stop here (DR-025 makes C3a not a down-payment on b/c).
Options:
- **C3b egress proxy** — the next C3 primitive (L7 proxy the sandboxed netns is forced through). Needs its
  OWN design sketch + DR (proxy crate choice + TLS-CA threat model, evaluated against I7 like SP4b's macaroon
  crate eval). The differentiated primitive, but only trustworthy given C3a's containment.
- **C3c credential brokering** — rides C3b (inject-on-approved-egress; secret never reaches the agent, memo
  #7/#17). After C3b.
- **The deferred warden /subject** — mint the `sandbox.*` taxonomy (`sandbox.spawned`/`.denied`/`.unavailable`
  vs. riding `permit.denied`) + folding reducers, so C3a's degrade/denial facts become first-class (currently
  a placeholder + TODO in `sandbox_unavailable_fold_c3a.rs`). Light, closes a real loop from this slice.
- **Smaller deferred /dr items** (carried): `bench.completed` subject · holder-offline attenuation (DR-018 §b)
  · decision fast-path cache · OPA/Cedar adapter.
- **Platform / Phase 3** — demand-gated, NOT scheduled (see deferral note above).
Pattern for any C3 primitive: **design sketch → owner-ratified DR → oracle → impl → /vet → /debrief** (the
DR-009 fence applies to each). The /subject item skips the sketch (warden-gated session).

## Open /debrief residuals — ONE non-blocking, no code change
Auditor PASS with a single INCONCLUSIVE sub-finding: the read-only auditor cannot execute WSL, so the
real-bwrap fs-enforcement (write-inside / out-of-bounds-denial) replay arm of the DR-025 exit demo was
verified as SOURCE-HONEST but not first-hand-witnessed by the auditor — taken as attested from the /vet +
oracle WSL 3/3 green. Belongs to whoever records the exit demo, not a code fix. Not carried as debt.

## Environment (unchanged + one C3 addition)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`.
**Quote the PATH export** (parens break it unquoted). Vet host-side (`bash .claude/hooks/vet.sh`); **host +
WSL SEQUENTIAL** ([[vet-concurrency-flake]]). **`/vet` host-side; WSL-green NOT sufficient**
([[vet-is-host-side-wsl-insufficient]]) — `#[cfg(unix)]` suites (daemon golden_path.rs, bench real_driver.rs,
permit_role_live.rs, **and now `sandbox_bwrap_confinement_c3a.rs`**) run WSL-only; host compiles them to 0
tests. C3a host suites (PathConfinement, no-widening, degrade/dep-scan, fabric fold) ARE host-runnable.
**bwrap present on WSL** at `/usr/bin/bwrap` (0.9.0). **C3 SANDBOX TEST GOTCHA:** a bwrap confinement fixture
must bind the toolchain (`/usr,/bin,/lib,/lib64` ro) or `/bin/sh` won't exec on usr-merged Ubuntu and denial
tests pass vacuously — documented in `sandbox_bwrap_confinement_c3a.rs` header. **clippy::doc_lazy_continuation**
still bites `//!`/test-doc headers ([[clippy-doc-lazy-continuation-trap]]) — hit + fixed again this session.
New this session: `crates/rezidnt-run/src/sandbox.rs`, `PathConfinement` in rezidnt-gate.

## Decisions still needing a /dr
- **C3b egress proxy** (design sketch + DR) · **C3c credential brokering** (after C3b) · `sandbox.*` warden
  /subject · holder-offline attenuation (DR-018 §b) · decision fast-path cache · OPA/Cedar adapter ·
  `bench.completed` subject. Carried debt: DR-007 GitError→associated type; `badge.issued` emitter; release
  items. **Platform / Windows transport** is a Phase-3 direction fork, demand-gated.

---
**NEXT ACTION → C3a shipped (`924c86b`); the C3 sole-chokepoint phase has its containment foundation. Next is a
CHOICE: continue C3 (C3b egress proxy — needs its own sketch+DR) vs. the light `sandbox.*` warden /subject that
closes C3a's fact loop vs. a smaller deferred /dr item vs. demand-gated Platform. `current-slice`=c3a (done).
High autonomy ON ([[autonomy-high-trust]]): design-sketch→owner-ratified-DR→oracle→impl→/vet→/debrief for any
C3 primitive (DR-009 fence); surface owner sign-off for each posture commitment.**
