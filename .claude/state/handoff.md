# Handoff — 2026-07-21 (session 15: c3bc-enforce — the live egress dataplane is REAL)

## State of play
The **C3 sole-chokepoint MECHANISM is complete and proven.** This session built **c3bc-enforce** — the live
`pasta`-netns → `rustls`-terminating-proxy → upstream byte-path with real inescapability and real credential
injection. All 4 `#[cfg(unix)]` WSL mediation tests pass with real netns + real TLS (I re-ran them
independently: 4/4, 0 ignored), host `/vet` PASS, `/debrief` PASS (auditor cleared the "manufactured green"
risk with cited byte-path evidence). Pushed to `origin/main` at **`68d0c4c`**. Tree clean, synced.
`current-slice` = `c3bc-enforce` (done). High autonomy ON ([[autonomy-high-trust]]).

## The C3 arc so far (what's DONE vs what remains)
- **C3a Linux sandbox (DR-025, `924c86b`)** — `bwrap` `SandboxSubstrate`, confinement, loud-open degrade. DONE.
- **c3bc-decide (DR-026 + DR-027, `5c8be98`)** — egress decision/governance + type-safety + CA/TLS scaffold,
  landed enforcement-inert. DONE.
- **c3bc-enforce (`68d0c4c`)** — the live dataplane: netns route sealed to a proxy-only /32 (direct egress →
  ENETUNREACH), rustls SNI-mediated termination, upstream TLS, credential injection at the ONE `.expose()`
  (egress.rs:1424), independent two-sided capture. DONE (mechanism proven).
- **NOT DONE — the C3 substrates are NOT wired into a live daemon run loop.** Both the C3a sandbox and the
  c3bc egress dataplane exist + are test-proven behind their substrate seams, but nothing in the daemon /
  `rezidnt open` spawn path calls them yet: `EgressDataplane::start` with unset wiring returns an honest
  error; the sandbox substrate isn't invoked by the live spawner. **The mechanisms are real; making an actual
  governed run confined + egress-mediated is a distinct integration step** (the honest current limit —
  product copy must not claim egress is enforced in a shipped run yet; DR-027 honesty guard, narrowed).

## What shipped this session (`68d0c4c`, pushed)
- `crates/rezidnt-run/src/egress.rs` — `EgressDataplane::start` for `PastaProxy` (`unix_dataplane`): real
  `pasta` netns (no route but the proxy), `rustls` terminating listener reading SNI + mediating the folded
  allowlist, per-SNI leaf from `RezidntCa`, own upstream TLS, injection at the sole `.expose()` upstream-write.
  Honest errors for non-unix + unset wiring. Module header updated (decide+enforce landed, not run-loop-wired).
- `crates/rezidnt-run/examples/egress_c3bc_probe.rs` — NEW dev-only test-support: the confined route-sealing
  probe + an INDEPENDENT capturing upstream TLS server (so crit-4 non-exposure is observed, not re-derived).
- `egress_mediation_c3bc.rs` — `handle()` stands up the capture server; 4 `#[ignore]` gates removed;
  assertion bodies unchanged. `anyhow` added dev-only.

## Honesty discipline this session (the theme of the whole C3 arc)
- The auditor specifically hunted the **"manufactured green"** fake (a canned capture / a probe that declines
  rather than a sealed netns / a circular crit-4) and cleared each with cited source: the netns route table is
  really sealed, the upstream capture is an independent process's file (not re-derived from the injection
  code), and crit-3 is non-vacuous (direct egress ENETUNREACH + a MANDATORY live proxy round-trip guard, so a
  dead-network box FAILS rather than passes).
- **I independently re-ran the WSL suite** rather than trust the implementer's green — the same reason the
  decide/enforce split happened last session (a "green" once hid the missing dataplane). Rule reinforced:
  **for WSL-only enforce evidence the read-only auditor can't run, the coordinator re-runs it.**
- **One non-blocking auditor nit (carried):** the crit-3 `unset_proxy_env` vector collapses into the
  raw-socket mechanism (both raw-connect to `1.1.1.1`, different port) because there's no in-ns resolver — so 2
  of 3 direct vectors share a mechanism vs DR-026 crit-3's three named-distinct vectors. Property is genuinely
  proven + non-vacuous (not a fake); a fidelity refinement, not a bug. Fix = a real getaddrinfo-by-name
  attempt for the unset-proxy-env vector (also exercises DNS-sealing distinctly). Optional; whoever hardens
  the exit demo.

## Autonomy — [[autonomy-high-trust]] (owner granted 2026-07-20)
Full loop without asking; commit+push green+debrief-PASS increments; self-ratify routine engineering DRs.
Owner sign-off required + given this arc for the posture-commitment DRs (DR-025, DR-026). Scope forks got
checkpoints (Platform-vs-core, C3a-first, C3b+c TLS scope, the dataplane gap → decide/enforce split). **Still
surface:** irreversible/destructive git, a DR amending BINDING invariant TEXT or licensing/clean-room,
firewalled sources, publishing beyond a main push, one-way doors, each new posture commitment / linked-dep
slice. c3bc-enforce rode DR-026's ratified design (no new DR needed; DR-027 had sequenced it).

## Next action — pick one (all ride ratified design or are light)
- **C3 run-loop wiring (RECOMMENDED — turns the proven mechanisms into live enforcement).** Wire the C3a
  sandbox + the c3bc egress dataplane into the actual daemon `rezidnt open`/spawn path so a real governed run
  is confined AND egress-mediated. This is what makes "sole chokepoint" true in a shipped run, not just in
  tests. Likely needs a light DR for the wiring decisions (where the folded egress policy + brokered secrets
  come from in a real run; how the sandbox+egress compose at spawn), then oracle→impl→/vet→/debrief. The
  honest current limit (§State-of-play) closes here.
- **Warden `/subject` for `egress.*`/`credential.*` + `sandbox.*`** — mint the taxonomy so the C3 facts
  (currently placeholders + `TODO(warden,/subject)` in `injected_fact` / `sandbox_unavailable_fold`) become
  first-class in tail/board/rebuild. Light; closes real loops from C3a + c3bc.
- **Minor crit-3 vector-fidelity refinement** (the auditor nit above) — tiny, optional.
- **Omnigent-baseline egress benchmark** — now that the chokepoint is real, the memo's #7/#9/#17 scenarios can
  run black-box (DR-002 rule 6). A separate activity, not a gate.
- **macOS/Windows egress+sandbox backends** — each own DR; Windows coupled to the deferred Platform phase.
- Smaller carried /dr items: bench.completed subject · holder-offline attenuation (DR-018 §b) · fast-path
  cache · OPA/Cedar adapter.

## Open /debrief residuals — ONE non-blocking (the crit-3 vector nit above). No carried defects.

## Platform deferral (unchanged)
Windows named-pipe transport = Phase-3, owner-deferred. Full Windows egress+sandbox parity waits on the
native-Windows daemon. C3 ships Linux-first, degrades loudly/closed elsewhere. No reorder implied.

## Environment (+ C3/egress additions)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`. **Quote
the PATH export** ([[wsl-dev-environment]]). Vet host-side (`bash .claude/hooks/vet.sh`); **host + WSL
SEQUENTIAL** ([[vet-concurrency-flake]]). **`/vet` host-side; WSL-green NOT sufficient**
([[vet-is-host-side-wsl-insufficient]]) — `#[cfg(unix)]` suites run WSL-only, host → 0 tests: now includes
`egress_mediation_c3bc.rs` (needs `pasta` + netns), `sandbox_bwrap_confinement_c3a.rs`, daemon golden_path,
bench real_driver, permit_role_live. **To re-run the egress dataplane suite:** `cargo test -p rezidnt-run
--test egress_mediation_c3bc -- --test-threads=1` on WSL (serial — netns). **WSL tooling present:** `bwrap`
0.9.0, `pasta` (`/usr/bin/pasta`), unprivileged user+net namespaces work — the confined-egress dataplane
runs there. **C3a sandbox-fixture gotcha:** bind the toolchain (`/usr,/bin,/lib,/lib64` ro) or `/bin/sh`
won't exec on usr-merged Ubuntu. **clippy::doc_lazy_continuation** still bites doc/test headers
([[clippy-doc-lazy-continuation-trap]]). **Linked deps (C3b+c):** `rustls` + `rcgen` only (dev-only:
`anyhow`, `tempfile`). New src: `rezidnt-run/src/egress.rs` (decide+enforce), `examples/egress_c3bc_probe.rs`.

## Decisions still needing a /dr or /subject
- **C3 run-loop wiring** (likely a light DR) · `egress.*`/`credential.*` + `sandbox.*` warden /subject ·
  macOS/Windows backends (each own DR) · smaller carried (bench.completed, holder-offline DR-018 §b, fast-path
  cache, OPA/Cedar). Carried debt: DR-007 GitError→associated type; `badge.issued` emitter; release items.
  **Platform / Windows transport** = Phase-3 direction fork, demand-gated.

---
**NEXT ACTION → c3bc-enforce done (`68d0c4c`); the C3 sole-chokepoint MECHANISM is real + proven (sandbox +
inescapable egress + credential brokering all work in WSL tests). The honest gap: none of it is wired into a
live daemon run loop yet. RECOMMENDED next = **C3 run-loop wiring** (make a real `rezidnt open` run confined +
egress-mediated — likely a light DR then oracle→impl→/vet→/debrief), or the light `egress.*`/`sandbox.*`
warden /subject. `current-slice`=c3bc-enforce (done). High autonomy ON: proceed without asking; surface only
irreversible/constitution-level/outward-facing calls. For WSL-only enforce evidence, RE-RUN it yourself — the
auditor can't.**
