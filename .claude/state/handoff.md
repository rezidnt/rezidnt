# Handoff — 2026-07-21 (session 15: C3 sole-chokepoint mechanism complete; next = run-loop wiring)

## State of play
The **C3 sole-chokepoint MECHANISM is done and proven** — sandbox confinement + inescapable netns egress +
credential brokering all work. Pushed to `origin/main` at **`f70dd56`**. Tree clean, synced. `current-slice`
= `c3bc-enforce` (**done** — all its criteria pass `/vet` + `/debrief`). High autonomy ON
([[autonomy-high-trust]]).

## Current slice & criteria
`c3bc-enforce` — DONE. DR-026 crit 3 (inescapability), crit 4 (credential non-exposure), and the real-traffic
arms of 1/7 all pass: host `/vet` PASS, the 4 `#[cfg(unix)]` WSL mediation tests 4/4 (I re-ran them
independently — 0 ignored), `/debrief` PASS (auditor cleared the "manufactured green" risk with cited
byte-path evidence: real route-table seal, independent capture, non-vacuous proxy-round-trip guard).

## What changed this session (git log since C3a)
- `924c86b` **C3a Linux sandbox** (DR-025) · `5c8be98` **c3bc-decide** (DR-026+DR-027; egress governance/type
  layer, enforcement-inert) · `68d0c4c` **c3bc-enforce** (live `pasta`-netns → `rustls`-MITM → upstream
  dataplane; injection at the ONE `.expose()` egress.rs:1424; independent capture server; `f70dd56` handoff).
- New: `rezidnt-run/src/egress.rs` (decide+enforce), `examples/egress_c3bc_probe.rs` (dev-only probe+capture).
  Linked deps added: `rustls`+`rcgen` only (dev-only: `anyhow`, `tempfile`). No ontology minted.

## THE OPEN GAP (why run-loop wiring is next)
The C3 substrates (C3a sandbox + c3bc egress dataplane) are proven behind their seams but **NOT wired into a
live daemon run loop** — `EgressDataplane::start` with unset wiring returns an honest error; the live spawner
doesn't invoke the sandbox/egress. So an actual `rezidnt open` run is **not yet confined or egress-mediated**;
"enforced in a shipped run" is not claimable (module docs say so; DR-027 honesty guard, narrowed).

## Next action — C3 run-loop wiring
Wire the C3a `SandboxSubstrate` + the c3bc `EgressDataplane` into the daemon `rezidnt open`/spawn path so a
real governed run is confined AND egress-mediated. Open questions the wiring must settle (likely a **light DR**
first, then oracle→impl→/vet→/debrief): where the folded egress `EgressPolicy` + brokered secrets come from in
a real run; how sandbox + egress compose at spawn (the netns is shared — the egress connector rides the
sandbox's sealed netns); the degrade path in a live run. Rides DR-025/DR-026 ratified design; the DR is for
the wiring/composition decisions, not new posture.

## Open /debrief findings
ONE non-blocking (carried, optional): crit-3's `unset_proxy_env` probe vector collapses into the raw-socket
mechanism (both raw-connect to `1.1.1.1`, diff port) — no in-ns resolver. Property genuinely proven +
non-vacuous; a fidelity nit vs DR-026's three-distinct-vector text. Fix = a real getaddrinfo-by-name attempt
(also exercises DNS-sealing distinctly). No carried defects otherwise.

## Decisions still needing a /dr or /subject
- **C3 run-loop wiring** (light DR for the composition decisions above) · `egress.*`/`credential.*` +
  `sandbox.*` warden `/subject` (facts are placeholders now) · macOS/Windows egress+sandbox backends (each own
  DR; Windows coupled to the deferred Platform phase) · smaller carried: bench.completed, holder-offline
  (DR-018 §b), fast-path cache, OPA/Cedar. **Platform/Windows transport** = Phase-3, demand-gated.

## Environment (essentials)
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`, **quote
the PATH export** ([[wsl-dev-environment]]). Vet host-side; **host+WSL SEQUENTIAL** ([[vet-concurrency-flake]]);
**WSL-green NOT sufficient, /vet is host-side** ([[vet-is-host-side-wsl-insufficient]]). `#[cfg(unix)]` egress
suite: `cargo test -p rezidnt-run --test egress_mediation_c3bc -- --test-threads=1` on WSL (needs `pasta` +
netns — both present; host → 0 tests). **For WSL-only enforce evidence, re-run it yourself — the auditor
can't.** `bwrap` 0.9.0 + `pasta` at `/usr/bin/`, unprivileged user+net namespaces work.
[[clippy-doc-lazy-continuation-trap]] still bites doc/test headers.

---
**NEXT ACTION → C3 run-loop wiring: make a real `rezidnt open` run confined + egress-mediated by wiring the
C3a sandbox + c3bc egress dataplane into the live spawn path. Draft a light DR for the composition decisions
(folded egress policy + secret source in a real run; sandbox+egress share the sealed netns; live degrade),
then oracle→impl→/vet→/debrief. `current-slice`=c3bc-enforce (done). High autonomy ON. For WSL-only evidence,
re-run it yourself.**
