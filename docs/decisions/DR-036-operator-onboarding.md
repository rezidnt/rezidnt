> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §9 (CLI — `spec init`, `doctor`, `init` verbs), §13 (project spec + `rezidnt spec init` generator contract), §1/§18 (the BINDING golden path), §16 (roadmap — Phase-1 golden-path exit, S1/S3) · invariants I1, I3, I5, I7 · operationalizes an ALREADY-BINDING contract, mints no new product surface

# Decision Record DR-036 — Operator onboarding arc (golden-path first-run: `spec init` + `doctor` + `init` wrapper + quickstart)

**Date:** 2026-07-22
**Status:** ACCEPTED
**Amends:** §9 and §13 (implements the already-SPECIFIED but unimplemented `rezidnt spec init` generator and the listed-but-unimplemented `doctor` verb, and adds a `rezidnt init` wrapper) — closes the gap between the BINDING golden path and a CLI that has no `init`/`spec`/`doctor` verb; §16 (attaches an onboarding arc supporting the Phase-1 golden-path exit, S1/S3). Adds an operator quickstart doc under `docs/` as the narrated one-take demo. Mints NO new invariant, subject, dependency, or product-facing lore term.

## Context

The golden path is ALREADY the BINDING product contract (§1/§18, ~line 18): "cold machine → `curl` install → `rezidnt open <repo>` → worktrees allocated, agents spawned under gates, fleet state visible, first verified diff merged — one take, zero config edits, single-digit minutes. Every phase exit is judged against this demo." Onboarding is therefore NOT new product surface to invent — it is OPERATIONALIZING a contract the plan already binds us to. This record scopes ONE audience (owner-fixed): an operator ADOPTING rezidnt for their own project — the golden-path first-run (install → point at a repo → first run → first gate/permit decision). NOT contributor/dev onboarding; NOT the stakeholder reading dossiers.

The load-bearing gap, read not assumed: §13 (~line 290) already SPECIFIES the scaffolding command — "`rezidnt spec init` generates this [the project spec] interactively; the golden path must work with the generated file untouched" — and §9 (~line 242) lists both `doctor` and `spec init` in the CLI contract. But the shipped `rezidnt` CLI (`bins/rezidnt/src/main.rs`) has verbs Rebuild/Tail/Board/Open/Attach/Vet/Debrief/Gate/Operator/PermitHook ONLY — there is no `init`, `spec`, or `doctor` subcommand. So the BINDING golden path assumes an interactive spec generator that DOES NOT EXIST: an operator on a cold machine today cannot get to `rezidnt open` with zero config edits, because they must hand-author the §13 `rezidnt.toml` first.

The deliverable (owner-fixed) is BOTH a command surface AND docs, and the owner ratified the FULLEST command scope: the `spec init` generator, a `doctor` environment preflight, and a `rezidnt init` wrapper that chains `doctor → spec init → open` into one smooth first-run command — plus a quickstart/walkthrough (under `docs/`) that narrates that flow. A multi-slice arc, not a single slice.

**Strongest counterargument (recorded, not just the outcome):** "onboarding is polish; ship product depth (the C3 chokepoint work, richer operator actions) and let adopters read §13 and write the TOML by hand." Rejected: the golden path is BINDING and its "zero config edits" clause is currently UNMET — an operator literally cannot complete the recorded demo without the missing generator. This is not gilding; it is closing a hole between a BINDING contract and the code. The counterargument's real force — don't let onboarding balloon into a product of its own — is honored by SCOPE (Decision 1 fences the arc to first-run and nothing beyond), not by skipping the work.

Two invariant tensions were engaged head-on, not waved past:
- **I1 (zero pixels in core) vs an "interactive" generator.** "Interactive `spec init`" (and `doctor`, and the `init` wrapper) means plain-CLI stdin/stdout prompts (read a line, print a prompt), NOT a TUI/curses/pixel surface. All three run in the `rezidnt` CLI binary, which is already a client — `rezidentd` renders nothing and gains nothing here. The boundary is stated so `init` cannot smuggle a UI into core.
- **I5 (MCP-first) vs a bootstrap CLI.** `init`/`spec init`/`doctor` necessarily run BEFORE any MCP surface exists (the daemon may not be running; the operator is on a cold machine with no spec yet). They are a legitimate pre-MCP bootstrap exception — they generate a local file / check the environment so that the daemon can later be pointed at it — not capabilities that should have been MCP tools first. Stated so the exception is deliberate, not an erosion of I5.

## Decision

1. **Scope = operationalize the BINDING golden path for a new operator; first-run only.** The onboarding arc's scope is the first-run experience — install → (`doctor` →) `spec init` → `open` → first gated/permitted run — and NOTHING beyond it. It adds no new product capability; it makes an already-BINDING contract reachable. Out of scope by construction: contributor/dev setup, stakeholder/reviewer docs, any post-first-run "day 2" operator ergonomics.

2. **The command surface (owner ratified the fullest): `spec init`, `doctor`, and a chained `rezidnt init` wrapper.**
   - `rezidnt spec init` — an interactive, PLAIN-CLI (stdin/stdout prompts, no TUI, I1) generator that writes a §13-shape `rezidnt.toml` the golden path runs UNTOUCHED. A pure LOCAL file generator: the daemon need not be running, so it emits NO fabric fact (I3-neutral — it writes a file, nothing to fold).
   - `rezidnt doctor` — a read-only environment preflight that checks what the golden path assumes (§11 topology: WSL2 reachable, git present, the chosen harness resolvable, the daemon socket/lockfile path writable), printing pass/inconclusive findings and NEVER coercing an inconclusive to pass (I6 posture, even pre-daemon). No telemetry, no network beacon (I7).
   - `rezidnt init` — a thin wrapper chaining `doctor → spec init → open`, so a cold-machine operator reaches a first gated run with one command. It fabricates nothing the sub-commands don't already do; it is orchestration, judged against the single-digit-minutes golden-path bar.

3. **The docs: an operator quickstart under `docs/` that IS the narrated golden-path demo, kept in lockstep with it.** A quickstart/walkthrough takes a newcomer from zero to a first gated run using `rezidnt init`. Location (owner-ratified): `docs/quickstart.md` (grouped with the architecture doc and decision records, keeping the repo root clean). It is not free prose: it is the recorded ONE-TAKE golden-path demo, narrated, and it MUST stay in lockstep with §1/§18 — if the golden path changes, the quickstart changes with it, or it is stale. It cites `rezidnt init` as the zero-config-edits entry point and asserts the same "single-digit minutes, generated file untouched" bar.

4. **Slicing: a four-sub-slice arc — `spec-init` → `onboarding-doctor` → `init-wrapper` → `quickstart`.** Ordered so each slice's consumer exists before it: `spec-init` first (the one piece the BINDING path strictly requires); `onboarding-doctor` next (independent read-only preflight); `init-wrapper` third (chains the two prior commands + `open`); `quickstart` last (narrates the finished `rezidnt init` flow, so it documents the real end-state UX, not an interim one). Each slice's definition of done = its criteria pass `/vet` and `/debrief`; acceptance is tied to the BINDING constraint ("works with the generated file untouched, zero config edits, single-digit minutes"). Sub-slice criteria in Slicing below.

## Design

- **`spec init` shape.** A new `rezidnt spec init` verb (subcommand of the existing `rezidnt` CLI, I7 — one binary, not a new bin) prompting for the §13 fields (project name, repo, at least one agent with harness + worktree=auto + gates) and writing a §13-shape `rezidnt.toml`. Refuses to clobber an existing file without an explicit overwrite flag (local input safety, DR-004 exit 2 on usage error). Global `--json` is not meaningful for an interactive prompt flow; a `--defaults`/non-interactive path (write a minimal valid spec with no prompts) is pinned by the oracle so the generator is testable deterministically.
- **Purely local, no fact (I3).** `spec init` writes a file to the operator's cwd; the daemon may not exist yet. It emits nothing to the fabric — there is no log to append to and nothing to fold. This is the correct call: onboarding never fabricates a fact before the daemon is even running.
- **`doctor` shape.** A read-only preflight over §11 topology (WSL2, git, harness resolvable, socket/lockfile path writable), pass/inconclusive findings, inconclusive NEVER coerced to pass (I6). No telemetry, no network beacon (I7). Emits no fact (pre-daemon).
- **`init` wrapper.** Orchestrates `doctor` (advisory/gating per its findings) → `spec init` (skip if a spec already present, per the clobber guard) → `open`. Surfaces the same DR-004 exit classes as the sub-commands it drives; adds no new failure semantics.
- **Quickstart doc.** Prose + copy-pasteable commands mirroring the golden path via `rezidnt init` exactly; no screenshots of a TUI in the first-run path (the board is a later, optional read-only surface, not part of zero-to-first-run). Lives at `docs/quickstart.md`.

## Invariants

- **I1 — no pixels in core, boundary made explicit.** `spec init`/`doctor`/`init` are plain-CLI stdin/stdout in the `rezidnt` CLI client; `rezidentd` renders nothing and is unchanged. "Interactive" = line prompts, NOT a TUI. The quickstart's first-run path shows no rendered UI. Onboarding cannot smuggle a UI into core.
- **I5 — bootstrap pre-MCP exception, deliberate.** The onboarding verbs run BEFORE any MCP surface exists (cold machine, no spec, daemon maybe down); they are legitimate bootstrap CLI, not capabilities that should have been MCP tools first. Stated so the exception does not erode the MCP-first default for everything post-bootstrap.
- **I3 — log is truth, unbothered.** `spec init` is a pure local file generator, emits no fact, folds nothing; `doctor` is read-only and emits no fact; `init` only drives sub-commands. Determinism of the log is untouched.
- **I7 — one binary, no telemetry.** All three verbs are subcommands of the existing `rezidnt` binary (no new bin, no new runtime dep). Onboarding NEVER phones home — no first-run analytics, no beacon, no "improve the product" ping. Explicit, because first-run flows are exactly where telemetry usually creeps in.
- **I2/I4/I6/I8 — untouched.** No new control/data-plane coupling, no new substrate/trait, no verifier semantics changed (`doctor`'s pass/inconclusive honesty mirrors I6 but adds no verifier), no competitor source read (this record is clean-room; it cites no intel memo).

## Consequences

- **Roadmap.** Opens the onboarding arc and advances `current-slice` to `spec-init` on ratification. The arc attaches to §16 supporting the Phase-1 golden-path exit (S1 spawn/open, S3 gated run) — it is the missing "zero config edits" enabler for that exit demo, not a new phase. Sequence (Decision 4): `spec-init` → `onboarding-doctor` → `init-wrapper` → `quickstart`. Standard loop each (`/oracle` → implement → `/vet` → `/debrief`). No paired `/subject` is required (the onboarding verbs are fact-free by Design); a `/subject` would be needed ONLY if a later slice decides to fact a first-run event, which this record does not.
- **Risk register.** CLOSES a standing gap risk the plan carried silently: the BINDING golden path's "zero config edits" clause was UNMET because the `spec init` generator §13 promises did not exist — an adopter could not complete the recorded demo without hand-authoring TOML. ADDS one honest risk in plain words: a generated spec that DRIFTS from what `rezidnt open` actually accepts would break "generated file untouched" — mitigated by making the `spec-init` slice's acceptance criterion an end-to-end `init → open` run (see Slicing), so the generator is pinned to the consumer, not to a snapshot of §13 prose.
- **Test/criterion honesty.** This record WEAKENS no existing test and lowers no bar. It ADDS criteria (below) and, notably, RAISES the effective bar on the golden path by making its "zero config edits" clause independently testable for the first time (an `init`-then-`open` gauntlet), rather than assumed.

## Amendments to the architecture doc

- §9: strike the "listed but unimplemented" status from `spec init` and `doctor`, and add the `init` wrapper, once each ships (applied per slice as it lands).
- §13 (~line 290): the `rezidnt spec init` sentence gains an "implemented by DR-036" pointer once `spec-init` ships.
- §16: the roadmap pointer note (applied on this ACCEPT) attaches the onboarding arc at the Phase-1 golden-path exit.
- §20: the decision-index row (applied on this ACCEPT).

## Risk deltas against invariants

- **I1:** LOWERED — the "interactive means plain-CLI, not TUI" boundary is now recorded, closing the risk that a future `init` slice reaches for a curses UI and drags a renderer toward core.
- **I5:** NEUTRAL — a scoped, recorded pre-MCP bootstrap exception; the MCP-first default is reaffirmed for everything after bootstrap.
- **I3/I7:** NEUTRAL-to-LOWERED — the onboarding verbs are deliberately fact-free and telemetry-free; recording this forecloses the two most common first-run erosions (a spurious first-run fact; a "usage" beacon).

## Slicing (acceptance criteria — house style; done = criteria pass /vet + /debrief)

- **Sub-slice `spec-init` (first — the strict golden-path requirement).**
  1. `rezidnt spec init` interactively (plain stdin/stdout) writes a §13-shape `rezidnt.toml`; a non-interactive/`--defaults` path writes a minimal valid spec for deterministic oracle pinning.
  2. END-TO-END: a spec produced by `spec init` (untouched) is accepted by `rezidnt open` and reaches a first `agent.spawned` — the "generated file untouched" BINDING clause, tested.
  3. Refuses to clobber an existing spec without an explicit overwrite flag (DR-004 exit 2 on the usage error); emits no fabric fact (I3).
- **Sub-slice `onboarding-doctor` (second — independent read-only preflight).**
  1. `rezidnt doctor` runs read-only environment checks (§11 assumptions) with pass/inconclusive findings, never coercing inconclusive to pass (I6), no telemetry (I7); emits no fact.
  2. Exit-code honesty per DR-004 (a failed check surfaces the right class; an inconclusive is never reported as pass).
- **Sub-slice `init-wrapper` (third — chains the two prior commands + open).**
  1. `rezidnt init` chains `doctor → spec init → open` and reaches a first gated run from a cold checkout with zero config edits, judged against the single-digit-minutes golden-path bar.
  2. It fabricates no new failure semantics — it surfaces the sub-commands' DR-004 exit classes; the clobber guard from `spec init` still holds when a spec already exists.
- **Sub-slice `quickstart` (fourth — narrate the finished flow).**
  1. `docs/quickstart.md` walks zero → first gated run using `rezidnt init`, copy-pasteable, no config edits.
  2. Its command sequence matches the §1/§18 golden path exactly (a lockstep check — the doc IS the recorded one-take demo, narrated); asserts the single-digit-minutes / generated-file-untouched bar.

Amendments to this record require DR-037.
