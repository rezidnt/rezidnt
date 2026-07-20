[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-023 — Extract a shared daemon-driving client (`rezidnt-client`) so `bench/harness`'s `DaemonDriver` can drive the real golden path; keep the test-fixture builders as test-support

**Date:** 2026-07-20 · **Status:** ACCEPTED (owner-delegated autonomy, 2026-07-20) · **Amends:** §4 (workspace layout — adds one internal library crate, `crates/rezidnt-client`, and moves the `rezidnt` CLI's private socket-driving into it; no new external dependency) and DR-022 (unblocks its Part-3 FLAGGED boundary and its exit demo — "the benchmark harness runs end-to-end against rezidnt itself"). No invariant text is rewritten; this is plumbing that SERVES the invariants (see Invariant-fit). **No downstream warden `/subject`** — this mints no event, no subject, no payload field; it only relocates code that already speaks the existing wire protocol. · **Cites:** no intel memo (this is internal plumbing, not a competitor-motivated capability). · **Builds on:** DR-022 (the benchmark slice that hit this boundary — `bench/harness/src/lib.rs:234-275`, the `DaemonDriver::drive` FLAGGED `todo!()`); the S4 golden-path reference `bins/rezidentd/tests/golden_path.rs`; `rezidnt-proto` (already a lib carrying `socket_path`/`check_hello`/`decode_hello`/`encode_request`, `crates/rezidnt-proto`).

## Context

DR-022's Part-3 exit — the harness runs end-to-end against rezidnt itself — is blocked on `DaemonDriver::drive`, which cannot be built as production code because **every golden-path-driving primitive is bin-private or test-scoped**. Re-confirmed against the tree while drafting:

- `rezidentd` is a **BIN with no `[lib]` target** (`bins/rezidentd/Cargo.toml` — `[package]` + `[dependencies]`, no `[lib]`), so there is nothing to depend on.
- The daemon-driving scaffolding lives in `bins/rezidentd/tests/common/mod.rs` — `#![cfg(unix)]`, `#[allow(dead_code)]`, test-only, exported nowhere: `start_daemon`/`start_daemon_prepared`, `connect`, `send_line`, `read_until`, `run_cli`, `seed_db_from_fixture`, plus the fixture builders `make_gated_project`/`gated_stub_harness`/`exec_pass_verifier`.
- The CLI's socket client (`connect_and_request`, `bins/rezidnt/src/main.rs:207-233`, and the `open` orchestration at `:465`) is **private to the CLI bin**. Notably it is already a thin layer over `rezidnt-proto`'s public `socket_path`/`decode_hello`/`check_hello`/`encode_request` — the wire types are shared; only the connect-tail-read *orchestration* is duplicated and bin-locked.

The primitives split cleanly into **two kinds**: a genuine **socket client** (connect over UDS, consume+check the hello, send a `Request`, tail/read facts back) that production code legitimately needs, and **test-fixture construction** (`make_gated_project`, `gated_stub_harness`, `exec_pass_verifier`, `seed_db_from_fixture`) that stages git repos, stub harness scripts, and seeded logs — scaffolding a production `DaemonDriver` should NOT carry into the dep graph.

**The decision: WHERE the shared driving capability lives, and WHAT moves into it.** Three options weighed:

- **(A) A new client library crate** (`crates/rezidnt-client`) holding the socket client, consumed by BOTH the `rezidnt` CLI and the harness's `DaemonDriver`. Most I5-aligned (the driver speaks the existing socket protocol, not a parallel one) and reuse-positive: the CLI stops hand-rolling its own client.
- **(B) Give `rezidentd` a `[lib]` target** exposing the daemon-spawn/driving surface. Simpler to reach for, but couples the harness to the daemon bin's internals and drags daemon guts (`rezidnt-run`, `rezidnt-gate`, `rezidnt-mcp`, `tokio`) into the harness dep graph for a client that only needs a socket and the wire types. Rejected against I4 (the harness should depend on a narrow client seam, not the daemon's implementation) and I7 (needless dep-graph weight).
- **(C) A shared test-support crate** for the fixture builders. Correct for the fixtures, but insufficient alone: the *client* is not test-support — it is the thing prod `DaemonDriver` runs on.

**Chosen: (A) + (C) — the honest split.** The socket CLIENT becomes a real library (`crates/rezidnt-client`); the TEST-FIXTURE builders become test-support the harness consumes as a **dev-dependency**, not production surface. `DaemonDriver` (production) depends on the client lib; `tests/real_driver.rs` (dev) depends on the fixture builders to stage its gated project.

**Strongest counterargument (dissent, recorded verbatim per house style):** *"Extracting a client lib to serve a benchmark that measures three metrics is a lot of refactor for a dogfood harness. It touches shipped code — the `rezidnt` CLI now consumes a new crate — to unblock one `#[cfg(unix)]` test. Keep the driving in a test-support crate and let `bench/harness` be a dev-dependency consumer of it, instead of minting a public client crate and refactoring the CLI's working socket code for no user-visible gain."* **Counter to the counter:** the harness's `DaemonDriver` is **production code** (`bench/harness/src/lib.rs`, not a `tests/` module) — DR-022 pins `run_cases_default` as the CLI entry point the exit demo invokes. Production code cannot depend on another crate's `tests/` module; a "test-support crate" the *production* `DaemonDriver` consumes is a contradiction in terms and would drag the whole fixture-staging surface (git init, stub scripts, chmod) into the shipped harness. The client extraction is *also* small: `connect_and_request` is ~25 lines already sitting on `rezidnt-proto`'s public API, so the lib is a relocation, not a rewrite — and it DE-DUPLICATES (the CLI stops hand-rolling; I5 says every capability is a socket/MCP client before a keybinding, so a shared client is the I5-native home). The dissent's instinct is right about the FIXTURES — those stay test-support (that is the (C) half) — but wrong to lump the client in with them. **The owner accepts or rejects the (A)+(C) split at ratification.**

## Decision

- **Mint `crates/rezidnt-client`** (internal library, no new external dependency): the socket-driving client — connect over UDS, consume + check the hello (via `rezidnt-proto`), send a `Request`, and tail/read facts back. This is the seam BOTH the `rezidnt` CLI and the harness's `DaemonDriver` consume.
- **Refactor the `rezidnt` CLI to consume `rezidnt-client`.** `connect_and_request` (`bins/rezidnt/src/main.rs:207-233`) and its `open`/`tail`/`attach` call sites move to invoking the shared client. **Pure move — no CLI behavior changes** (same wire, same hello check, same requests); the CLI's existing test suite must stay green unchanged.
- **Driving stays behind the existing `Driver` trait (I4).** DR-022 already put `DaemonDriver` behind `trait Driver` (`bench/harness/src/lib.rs:150`); this DR does not add a trait — it fills in the production impl. A future non-daemon or remote driver remains additive (another `Driver`), and the client lib itself is the swappable transport seam.
- **Keep the fixture builders as TEST-SUPPORT, consumed by the harness as a dev-dependency (C).** `make_gated_project`, `gated_stub_harness`, `exec_pass_verifier`, `seed_db_from_fixture` are extracted from `bins/rezidentd/tests/common/mod.rs` into a **dev-only** shared test-support location (a `crates/rezidnt-testkit` dev-crate, or a shared test module) that both `bins/rezidentd`'s integration tests and `bench/harness/tests/real_driver.rs` depend on **as a `[dev-dependency]`**. Production `DaemonDriver` does NOT depend on these — a benchmark run against a real target repo stages nothing; only the `#[cfg(unix)]` integration proof stages a fixture.
- **What stays `#[cfg(unix)]`:** the UnixStream-based driving is unix-only (as `golden_path.rs` and the CLI's `connect_and_request` already are). `rezidnt-client` compiles its UDS path under `#[cfg(unix)]`; `bench/harness/tests/real_driver.rs` stays `#![cfg(unix)]`. Host `/vet` (Windows) compile-skips the unix path and the real-driving test — honestly reported, not a green-washed pass ([[vet-is-host-side-wsl-insufficient]]). The proof turns green only on the WSL run.
- **No new event, subject, or payload field.** This relocates code that speaks the existing protocol. No warden `/subject` is implied.

## Invariant-fit

| Inv | Fit |
|---|---|
| **I5 (load-bearing here)** | The driver speaks the EXISTING socket/MCP wire (`rezidnt-proto` `Request`/`Hello`), never a parallel protocol. A shared client crate is the I5-native home — every capability is a socket client first; the CLI stops hand-rolling its own. |
| **I4** | Driving stays behind the DR-022 `Driver` trait; the transport (the client lib) is a swappable seam. A future remote/non-daemon driver is additive. Option (B) — depending on daemon guts — was rejected precisely to keep the seam narrow. |
| **I7** | NO new external dependency. One internal crate (`rezidnt-client`) + a dev-only test-support crate; the client sits on `rezidnt-proto` + std UDS. Option (B)'s daemon-guts dep-graph weight is avoided. One static binary posture is untouched. |
| **I1/I2/I3** | Untouched. The client carries wire frames (facts/refs, ≤32 KiB, I2); it renders nothing (I1); it reads facts off the log as truth (I3). |
| **I8** | No clean-room issue — no AGPL/Omnigent source read; pure internal relocation. |

## Consequences

- **§4 delta:** the workspace gains `crates/rezidnt-client` (internal lib) and a dev-only test-support crate; the `rezidnt` CLI's dependency list gains `rezidnt-client`. The layout table in §4 gains one row.
- **DR-022 delta:** the Part-3 FLAGGED boundary (`bench/harness/src/lib.rs:236-274`) is resolved — `DaemonDriver::drive` and `run_cases_default` can now be filled in as production code. DR-022's exit demo becomes reachable.
- **Blast radius on the `rezidnt` CLI (shipped code):** the CLI is refactored to consume `rezidnt-client` — a **pure move, zero behavior change**. Same socket path, same hello check, same `Request` frames, same output. The risk is a mechanical-refactor regression, bounded by the CLI's existing test suite: if it stays green unchanged, the move is proven behavior-neutral. No other shipped crate changes.
- **Test-support relocation:** `bins/rezidentd/tests/common/mod.rs`'s fixture builders move to the shared dev-crate; the daemon's own integration tests re-point their imports. This is a dev-dependency move — **no shipped code depends on it**, so it cannot bloat the binary.
- **Risk-register (§18):** *refactor-of-shipped-code risk (new, low)* — moving the CLI's working client could regress it; mitigated by the pure-move constraint + the CLI suite gating it green. *host-vet blind-spot (carried, [[vet-is-host-side-wsl-insufficient]])* — the real-driving proof is unix-only and host-vet compile-skips it; stated plainly, the WSL run is the real gate.
- **No shipped test or acceptance criterion is weakened.** In plain words: this DR relaxes NO gate, verifier, golden, or criterion. It ADDS a client lib + a dev test-support crate and RELOCATES existing code; the CLI's behavior and its test suite are unchanged (and are the guardrail). The unix-only driving compiling to nothing on host is not a weakened criterion — it is the honest platform boundary already true of `golden_path.rs`.

## Acceptance-criteria sketch (what `/oracle` encodes once the extraction build slice starts)

1. `rezidnt-client` exists as an internal lib exposing the socket-driving client (connect → hello check → send `Request` → tail/read facts), on `rezidnt-proto` + std UDS, `#[cfg(unix)]` for the UDS path, **no new external dependency**.
2. The `rezidnt` CLI consumes `rezidnt-client` and its **existing test suite passes unchanged** — no behavior change (same wire, same output). The pure-move property is the pinned assertion.
3. `DaemonDriver::drive` is real production code: it drives ONE case's golden path (open → vet → spawn → diff.ready → pre_merge → merge → diff.merged → debrief) via `rezidnt-client` and reads `reached_verified_merge` off the log's terminal facts (`gate.passed`(pre_merge) → `diff.merged`) — never a `Case::expect_merge` echo. **`bench/harness/tests/real_driver.rs` (`#[cfg(unix)]`, WSL) goes GREEN.**
4. The fixture builders (`make_gated_project` et al.) live in a **dev-only** test-support crate; production `DaemonDriver` does NOT depend on them; both the daemon integration tests and `real_driver.rs` consume them as a `[dev-dependency]`.
5. **Host `/vet` stays green** — the unix-only driving compiles to nothing on the Windows host and is honestly reported as such (not misread as a green real-driving proof); the WSL workspace-test run is the real gate ([[vet-is-host-side-wsl-insufficient]], [[vet-concurrency-flake]]).

## What this does NOT decide

- The metrics-report surface (out-of-band vs `bench.completed` subject) — still DR-022's deferred warden `/subject`.
- Any change to the wire protocol, the `Request`/`Hello` shape, or daemon behavior — none; this is a relocation.
- The exact name/shape of the dev test-support crate (`rezidnt-testkit` vs a shared module) — an implementation call the build slice settles; this DR fixes only that the fixtures are dev-only and NOT in the production client.

*Amendments to this record require DR-024.*
