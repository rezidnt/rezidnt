# Handoff — 2026-07-21 (session 16: c3-wire DONE — sandbox+egress wired into the live spawn; next = real egress fold)

## State of play
**`c3-wire` is DONE.** A real `rezidnt open` governed run now spawns through the COMPOSED path —
`pasta → bwrap → agent` over ONE shared netns — so it is filesystem-confined AND (when the fold carries
an allowlist) egress-mediated, with the daemon owning + reaping the composed child (S1). DR-028 ratified
and landed, implementation landed and audited. Host `/vet` PASS, the WSL `#[cfg(unix)]` composed suites
green (I re-ran them myself — the auditor can't reach the box), `/debrief` PASS. High autonomy ON
([[autonomy-high-trust]]). Pushed to `origin/main` at **`e6cb3fc`** (see push note at bottom). `current-slice`
= `c3-wire` (**done**).

## Current slice & criteria
`c3-wire` — DONE. DR-028's five criteria: (1) spawn goes through confinement not the raw
`tokio::process::Command` — the `runs.rs` bypass is gone; (3) binds/allowlist/secrets fold ONLY via
`from_folded_authority` (C6/DR-024 preserved end-to-end, no-widening test fails-first on a plan door);
(5) daemon owns the composed `tokio::process::Child`. All host-provable → GREEN (11 host tests + full
gauntlet `{"verdict":"pass"}`). (2) shared-netns inescapability under composition + (4) live degrade arm →
WSL `#[cfg(unix)]`: `compose_shared_netns_c3_wire` 2/2, `spawn_composed_c3_wire` 2/2, `egress_mediation_c3bc`
4/4 (no enforce regression), C3a + golden_path green. `/debrief` = PASS (auditor cleared the posture
relaxations + the manufactured-green risk with cited evidence; 3 non-blocking advisory nits, below).

## What changed this session (git log since the C3-mechanism handoff `7741785`)
- `a4b3265` **DR-028** (c3-wire): the composition/wiring DR — pasta-outer shared netns, daemon-owned composed
  child (S1), folded-from-spec first source (C6), the product of the two asymmetric degrades. Rides ratified
  DR-025/026/027; no invariant/posture/dep/ontology change. Ratified under the standing high-autonomy grant.
- `e6cb3fc` **c3-wire impl**: NEW `crates/rezidnt-run/src/compose.rs` (`composed_argv`, `ComposedDegrade`/
  `compose_degrade`/`degrade_fact`, `ComposedChild`, `start_composed_dataplane`, `confined_program_binds`);
  `sandbox.rs` (`bwrap_argv_shared_netns` — shared-netns posture drops `--unshare-net`+`--unshare-user` so
  pasta's netns owner keeps CAP_NET_ADMIN; `--dev`/`--proc` added, both arms); `egress.rs` (`start_composed`/
  `run_confined` splice + CA-pem ro-bind into the confined mount-ns); `runs.rs` (`fold_c3_policies` +
  `compose_spawn` replacing the raw spawn). 5 new test files (2 WSL-only). No new linked dep, no ontology minted.

## THE OPEN GAP (why "real egress fold" is next)
This slice is **honestly-minimal**: `fold_c3_policies` folds sandbox binds (worktree + toolchain + declared
harness dir) but an **EMPTY egress allowlist (deny-all)** — there is no `[gates.permit]`/role egress-config
fold field yet (DR-028 §"What this does NOT decide" #1). Consequence (auditor NON-BLOCKING nit, disclosed +
DR-scoped): the run-loop **Mediated spawn arm is dead in production** — a real governed run this slice is
**confined + CLOSED** (sealed netns, no network), NOT mediated. The Mediated shared-netns path is proven only
by the SUBSTRATE suite (`start_composed_dataplane`), not by a live governed run. This is truthfully disclosed
(the `ConfinedClosed` fact carries `network=sealed`/`egress_enforceable=false`) — **but no product copy may
claim run-loop egress mediation until the real egress fold lands.**

## Next action — the real egress fold (make a live governed run actually MEDIATED)
Source a **non-empty egress allowlist + brokered secrets** from the folded `[gates.permit]`/role layer so a
real `rezidnt open` run routes through a live proxy end-to-end at the RUN-LOOP level (activating the currently-
dead Mediated arm, `runs.rs` ~1093-1107). Likely a **light DR or just a slice under DR-028's deferral** (the
posture is already ratified; this is the fold-field + wiring). Then oracle→impl→/vet→/debrief. Pairs naturally
with the deferred warden `/subject` (below) since the live Mediated run emits `egress.*`/`credential.*` facts
that currently ride PLACEHOLDER subjects.

## Open /debrief findings (all NON-BLOCKING, advisory — carried, none blocks done)
1. **Dead run-loop Mediated arm** (`runs.rs` ~1093-1107) + placeholder `proxy_addr="127.0.0.1:9"` — cleared by
   the real egress fold above; keep the honesty comment until then.
2. **`insert_bwrap_chdir` targets `a.ends_with("bwrap")`** (`runs.rs` ~1183) — unambiguous today (no pasta token
   ends in "bwrap"), but a string-suffix heuristic; pin the index from the known handoff structure if a future
   proxy_addr/bind could ever end in "bwrap".
3. **`argv_to_command` indexes `argv[0]`** (`runs.rs` ~1170) unchecked — safe (every non-Unsandboxed arm pushes
   a program first); add an emptiness guard if refactored.

## Decisions still needing a /dr or /subject
- **Real egress fold** (the next action — light DR/slice under DR-028's deferral) · **warden `/subject` for
  `sandbox.*`/`egress.*`/`credential.*`** facts (STILL deferred; the wiring emits under placeholders like
  `sandbox.mediated`/`egress.unavailable`/`sandbox.unavailable` now) · macOS/Windows sandbox+egress backends
  (each own DR; Windows coupled to the deferred Platform phase) · smaller carried: bench.completed,
  holder-offline (DR-018 §b), fast-path cache, OPA/Cedar.

## Environment (essentials)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`, **quote
the PATH export** ([[wsl-dev-environment]]). Vet host-side; **host+WSL SEQUENTIAL** ([[vet-concurrency-flake]]);
**WSL-green NOT sufficient, /vet is host-side** ([[vet-is-host-side-wsl-insufficient]]). The composed WSL suites:
`cargo test -p rezidnt-run --test compose_shared_netns_c3_wire -- --test-threads=1` and
`cargo test -p rezidentd --test spawn_composed_c3_wire -- --test-threads=1` (need `pasta` + `bwrap` + netns —
all present; host → honest early-return). **The dev probe example must be built** (`cargo build -p rezidnt-run
--example egress_c3bc_probe`) for the WSL composed/enforce suites — bwrap binds it via the confined-program
fold. **For WSL-only evidence, re-run it yourself — the auditor can't.** [[clippy-doc-lazy-continuation-trap]]
still bites doc/test headers.

---
**NEXT ACTION → the real egress fold: source a non-empty egress allowlist + brokered secrets from the folded
`[gates.permit]`/role layer so a live governed `rezidnt open` run is actually MEDIATED end-to-end (activate the
dead run-loop Mediated arm), pairing with the deferred warden `/subject` for the `egress.*`/`credential.*` facts.
Light DR/slice under DR-028's deferral (posture already ratified), then oracle→impl→/vet→/debrief. `current-slice`
= c3-wire (done). High autonomy ON. For WSL-only evidence, re-run it yourself.**
