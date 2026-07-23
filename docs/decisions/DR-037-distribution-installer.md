> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §1/§18 (the BINDING golden path — its `curl` install step) · §16 (roadmap — Phase-1 golden-path exit, S1/S3; the recorded one-take demo) · §11 (topology — Linux/WSL2 today, native-Windows deferred to Phase 3) · reconciles with [DR-007](DR-007-release-worktree.md) (no collision: DR-007's `release_worktree` is the runtime RepoSubstrate trait, NOT a release-build worktree) and [DR-036](DR-036-operator-onboarding.md) (which shipped the arc's `spec init`/`doctor`/`init`/quickstart but left step ONE, `curl` install, aspirational) · invariants I6, I7, I8 · operationalizes an ALREADY-BINDING contract, mints no new product surface

# Decision Record DR-037 — Distribution / release posture: a real `curl | sh` installer for the golden-path install step

**Date:** 2026-07-23
**Status:** ACCEPTED
**Amends:** §1/§18 (the BINDING golden path's FIRST step — `curl` install — becomes honestly demonstrable, not aspirational); §16 (attaches a distribution/release arc supporting the Phase-1 golden-path exit, S1/S3 — the recorded one-take demo opens with `curl`); §20 (decision-index row). Reconciles with — does not contradict — DR-007 (runtime RepoSubstrate trait, unrelated) and DR-036 (which shipped every golden-path step EXCEPT install). Mints NO new invariant, subject, dependency, or product-facing lore term.

## Context

The golden path is the BINDING product contract (§1/§18, line 18): "cold machine → `curl` install → `rezidnt open <repo>` → … one take, zero config edits, single-digit minutes. Every phase exit is judged against this demo." DR-036 shipped the onboarding arc (`spec init` / `doctor` / `init` / quickstart) and closed the "zero config edits" clause — but the FIRST step is still not real. `docs/quickstart.md` (lines ~17–30) frames `curl -fsSL https://rezidnt.dev/install.sh | sh` as "the intended distribution channel … not yet live from this repository," and directs the works-today reader to `cargo install --path bins/rezidnt` + `cargo install --path bins/rezidentd`. So the recorded one-take demo opens with a command that does not run. This record OPERATIONALIZES an already-BINDING contract — it mints no new product surface; it makes step one honest.

**Owner-settled facts (fixed, not open):**
- **Scope = Linux/WSL-first.** The demo's "cold machine" is Linux or Windows+WSL2, matching substrate reality: every sandbox/egress backend is Linux/WSL today (§11; native-Windows daemon is Phase-3-deferred, DR-025/026 backends are bwrap/netns Linux). The installer MUST cover only platforms whose substrate actually runs, so it never ships a binary that half-works. A cross-platform (macOS / native-Windows) installer is explicitly OUT of scope until those backends land — a later DR. This scope fence is load-bearing and is an **I6-honesty move**: an honest installer never overclaims what works.
- **Sequencing (recorded 2026-07-23):** the owner chose this follow-up over the benchmark harness (DR-022) and over the macOS / native-Windows backends.

**Strongest counterargument (recorded, not just the outcome):** "Skip the installer — `cargo install` is fine for adopters who already have Rust; spend the effort on product depth (the C3 chokepoint) or the missing backends." Rejected: the golden path is BINDING and its install step is literally `curl`, not `cargo`. An adopter on a cold machine may have NO Rust toolchain, so `cargo install` is not an install path for the demo's stated audience. The phase-exit demo is judged against the recorded one-take, and that take starts with `curl` — while it does, the recorded demo is unrunnable as written. The counterargument's real force — don't let release engineering balloon — is honored by the Linux/WSL-first scope fence and a three-slice arc, not by skipping the work.

## Invariant tensions engaged (not waved past)

- **I7 (one static binary, no telemetry) — the whole point, AND a wording tension, honestly framed.** The arc delivers genuinely static binaries with zero telemetry in the installer. But I7 says "one static binary" while the quickstart installs TWO (`rezidnt` CLI + `rezidentd` daemon). **This two-binary state is a PRE-EXISTING condition, not something this DR invents:** the workspace has shipped `bins/rezidnt` + `bins/rezidentd` all along, and DR-036's ACCEPTED quickstart already installs both. The installer packages what exists. This record therefore does NOT resolve the I7 tension by re-reading "one static binary" as a permanent "per-artifact" property — that would be the quiet erosion the warden/auditor ceremony exists to catch. Instead it names the tension and its resolution PATH: the literal-I7 form is a combined multi-call binary (busybox-style — one static image dispatching to `rezidnt` / `rezidentd` by argv[0] or a `rezidnt daemon` subcommand), which requires extracting the ~714-line daemon out of the `rezidentd` bin crate into a library the CLI can call (the daemon's entry is already clean — `main()` is a 12-line shim over a `pub fn run()`). That extraction is an ARCHITECTURE change, out of scope for a DISTRIBUTION arc; it is filed as a distinct near-term follow-up (its own slice/DR), so I7 has a real path back to literal satisfaction rather than a reinterpretation left standing in the record. Recorded plainly as the temporary state, with the fix named.
- **I8 (clean-room).** `install.sh` is written from scratch. Do NOT read or port any copyleft installer (rustup's `sh.rs`, or others). Multi-call dispatch and checksum-verified install are generic Unix prior art (busybox, ca. 1996), not copyleft-tainted knowledge. No intel memo is cited by this record.
- **I6 (never coerce / never overclaim).** The Linux/WSL-first scope fence is the I6 honesty move above; additionally the installer verifies a published sha256 before install (no blind pipe-to-shell), and refuses in plain language on an unsupported platform rather than half-installing — trust, made interrogable.

## Decision

1. **Operationalize the golden path's `curl` install step, Linux/WSL-first.** Build the release machinery and installer so the demo's step one runs. Out of scope by construction: macOS and native-Windows installers (deferred to when those backends land — a later DR).

2. **Static-binary target (I7): `x86_64-unknown-linux-musl` only, in the first cut.** A genuinely static binary that honors I7 and needs no host libc. `aarch64-unknown-linux-musl` is DEFERRED (owner-settled 2026-07-23) — added later only on demand. Tradeoff recorded: musl build friction and any future C-dep (none today — deps are portable-pty/tokio/rusqlite; rusqlite bundles SQLite) would need musl-clean handling.

3. **Two static binaries shipped together this arc** (`rezidnt` + `rezidentd`), both placed on PATH by the installer — packaging the pre-existing two-binary reality, NOT a new I7 reinterpretation. The combined multi-call single binary is named as the I7-honoring target and filed as a distinct follow-up (daemon-crate extraction; see the I7 tension above). Owner-settled 2026-07-23.

4. **Release channel = GitHub Releases off semver tags**, CI-built artifacts with published sha256 checksums. **Endpoint (owner-settled 2026-07-23): the real-today path** — `install.sh` served as a raw asset from the GitHub repo/release (github.com/rezidnt/rezidnt), fetching the tagged release artifacts. The `rezidnt.dev/install.sh` vanity domain is a later nicety (only if the domain is owned/committed); it is NOT a blocker, so the quickstart can stop saying "not yet live" and cite the real GitHub asset endpoint now.

5. **Integrity + trust posture.** `install.sh` verifies the downloaded binary against its published sha256 BEFORE install — no blind pipe-to-shell without a checksum gate (fail-closed on mismatch). NO telemetry / no phone-home in the script (I7). Clean-room original (I8).

6. **Reconcile with DR-007 (naming collision defused).** DR-007's `release_worktree` is the runtime RepoSubstrate trait method (allocate→use→release of git worktrees at run time); it has NOTHING to do with a release-build worktree. The release cross-compile runs in ordinary CI (a tagged-ref job), NOT inside DR-007's runtime worktree mechanism. This record does not touch DR-007's trait or §7. Owner-confirmed 2026-07-23.

## Slicing (a multi-step arc; done = criteria pass /vet + /debrief)

- **Sub-slice `release-ci` (first — the artifact source of truth).**
  1. A tagged-ref CI workflow cross-compiles the `x86_64-unknown-linux-musl` `rezidnt` + `rezidentd` static binaries and publishes them as GitHub Release assets with per-artifact sha256 checksums.
  2. The artifacts are genuinely static (no host libc dependency) and carry no telemetry (I7).
- **Sub-slice `install-script` (second — consumes the artifacts).**
  1. A clean-room `install.sh` (I8, written fresh) fetches the tagged release artifacts, verifies each against its published sha256 BEFORE install (fail-closed on mismatch), and places both binaries on PATH.
  2. No network beacon / no telemetry (I7); Linux/WSL detection with a plain-language refusal on an unsupported platform (I6 — never half-installs).
- **Sub-slice `quickstart-real` (third — flip the doc from aspirational to live).**
  1. `docs/quickstart.md`'s install section (lines ~17–30) stops saying "not yet live": the `curl | sh` line becomes the real, working install, citing the ratified GitHub-asset endpoint (Decision 4).
  2. The `quickstart_lockstep.rs` judge only asserts `rezidnt <verb>` commands, so the install block is PROSE — the slice must keep that prose honest by hand (state this plainly): the doc must not claim an endpoint the release machinery does not serve.

## Consequences

- **Roadmap (§16).** Attaches a distribution/release arc supporting the Phase-1 golden-path exit (S1 open/spawn, S3 gated run — the recorded one-take demo opens with `curl`). No new phase. Advances `current-slice` to `release-ci` on ratification. Sequence: `release-ci` → `install-script` → `quickstart-real`, ordered so each slice's consumer exists first (artifacts before the script; a working script before the doc flips to live).
- **Risk register.** CLOSES a standing honesty gap the plan carried since DR-036: the BINDING golden path's FIRST step was aspirational — the recorded one-take demo opened with a `curl` command that did not run. ADDS one honest risk in plain words: an `install.sh` that drifts from what CI actually publishes (wrong artifact name, stale checksum, moved endpoint) would silently break step one — mitigated by making `install-script` verify the sha256 it fetches against the published one (fail-closed) and by keeping the quickstart prose pinned to the ratified endpoint.
- **Test/criterion honesty (plain words).** This record WEAKENS no test and lowers no bar. Note plainly: the `quickstart_lockstep.rs` judge does NOT cover the install prose (it only checks `rezidnt <verb>` lines), so the `quickstart-real` slice's honesty is NOT machine-enforced — it is a prose obligation the arc must uphold by hand. This is stated so no one mistakes a green lockstep for a verified install claim.
- **Deferred, named (not silently dropped).** (a) The combined multi-call single binary (literal-I7 form) — a daemon-crate extraction, its own follow-up. (b) `aarch64-unknown-linux-musl`. (c) The `rezidnt.dev/install.sh` vanity domain. (d) macOS / native-Windows installers — gated on those substrates existing (a later DR).

Amendments to this record require DR-038.
