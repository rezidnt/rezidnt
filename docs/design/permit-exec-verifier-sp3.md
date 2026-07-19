# Design sketch — SP3 policy-as-exec-verifier (bring-your-own policy DSL on the permit axis)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** a DR (owner sign-off) + optional scoped `/intel` on OPA/Cedar (design §12) · **Builds on:** [permit-engine](permit-engine.md) §6/§10.2/§11-SP3, [DR-008](../decisions/DR-008-permit-engine-pivot.md), [DR-011](../decisions/DR-011-permit-pdp-config-seam.md) (config seam), SP2 (`decide_permit`, `permit::aggregate`) · **Owner:** TwofoldTech LLC

> Not BINDING. Committed in writing before any `/oracle` and before any scoped `/intel` read of OPA/Cedar (DR-002 rule 1). Nothing built until the DR is ACCEPTED. Sequence: this sketch → `/dr` (owner) → optional `/intel` → `/oracle` → implementer → `/vet` → `/debrief`.

## 1. Scope — let an external policy file decide a permit

permit-engine §11-SP3 acceptance: *"an external policy file decides a permit."* The native permit pack (tool-allowlist, path-scope, spend/rate cap, intent-lock) covers the common cases; SP3 opens the **bring-your-own-DSL** door (design §6): an **OPA/Rego or Cedar** policy — or any argv program — plugs in as an exec permit-verifier speaking the §8 JSON contract. rezidnt does not build a policy language; it dispatches a mature one.

## 2. The seam already exists on two sides — SP3 joins them

| Half | Where | State |
|---|---|---|
| The exec-verifier contract | `crates/rezidnt-gate/src/lib.rs:867` `ExecVerifier` | **built** — argv program, §8 `VerifierInput` stdin → `VerifierOutput` stdout, wall-clock `timeout_ms`, nonzero-exit→`inconclusive{nonzero_exit}`, unparseable→`inconclusive{malformed_output}`, network-off + scrubbed env (doc §12). Used today on `vet`/`pre_merge`. |
| The spec already parses exec permit entries | `crates/rezidnt-run/src/spec.rs:74` `VerifierSpec.exec: Option<PathBuf>` | **built** — a `[gates.permit]` block can already declare `{ exec = "policies/agent.rego-run", name = "cedar", params = {…} }`. |
| The permit axis dispatches them | `crates/rezidnt-mcp/src/mcp.rs:157`, `crates/rezidnt-gate/src/permit.rs:293` | **the gap** — `permit_config_for` FILTERS to `native` only ("an exec entry on the permit axis is skipped, never silently run"); `permit::aggregate` resolves natives by name and cannot carry an exec entry. |

**SP3 = close the gap.** Un-filter exec entries in the resolver; extend `PermitVerifierSpec` to carry an exec kind (argv + display name + params); extend `permit::aggregate` to dispatch exec entries through `ExecVerifier`, preserving the aggregate ordering (first-`Fail`→Deny short-circuit; any `Inconclusive`→Escalate; else Grant) and the verdict→decision map (I6). No new verifier machinery, no new vocabulary — the verdict contract already maps `pass|fail|inconclusive → allow|deny|ask`.

## 3. The load-bearing decision — the sync/async seam

`ExecVerifier::run` is **async** (`tokio::process`, `lib.rs:877`); `permit::aggregate` is **sync** and runs today inside the `spawn_blocking` that `decide_permit` uses for the native pack + CAS. An exec verifier cannot be `await`ed from inside `spawn_blocking`. This is SP3's real architectural choice:

- **(A) — async permit dispatch (recommended).** Lift permit aggregation to the async layer of `decide_permit`: natives still run in `spawn_blocking` (they're CPU/CAS, sync by design), exec verifiers run via `ExecVerifier::run().await`, and the aggregator interleaves them **in configured order** so a first-`Fail` still short-circuits (an exec `Deny` earlier in the list must be able to stop a later native, and vice-versa). The aggregate becomes an async orchestration over a heterogeneous set; the pure per-verdict combine logic stays where it is.
- **(B) — `block_on` the exec runner inside the sync aggregate.** Cheapest diff, but blocking a runtime thread on a subprocess inside `spawn_blocking` is exactly the "no blocking in async"/thread-starvation smell rust-conventions warns against, and it hides the subprocess from the async scheduler. **Rejected** unless (A) proves infeasible.

Recommend **(A)**. It keeps the exec subprocess visible to the scheduler (and to the hot-path timeout), and preserves ordered short-circuit across native+exec.

## 4. Determinism & replay (BINDING — I6, the compliance sentence)

Exec permit-verifiers must be **deterministic and replayable** like every verifier (I6; the `debrief` replay re-executes recorded verdicts). Two obligations:

1. **The policy artifact is content-pinned.** The policy file (`agent.rego` / `agent.cedar`) is pinned to CAS and its ref rides the decision fact's `policy_ref` — exactly as SP-wire pins the deciding verifier's params. Replay re-executes the *same* policy bytes; a changed policy is a different `policy_ref`, not a silent drift. The `§8` contract already records the exact stdin (`VerifierInput`) and stdout (`VerifierOutput`) per run, so `debrief` can re-run and compare (the DR-006 integrity-alarm path already exists for divergence).
2. **The exec runs sealed.** `ExecVerifier` already runs network-off + scrubbed env (doc §12) — a policy that reaches the network for a decision is non-deterministic and outside the contract. State this: an OPA/Cedar policy that consults an external data source at decision time is not a conforming permit-verifier in SP3 (bundled data must be a pinned input, not a live fetch).

## 5. I7 — rezidnt does NOT bundle a policy engine

**Load-bearing for the DR.** I7 is one static binary, no bundled subsystems. rezidnt ships the **dispatch**, not OPA/Cedar. The policy engine is a **local argv the operator provides** — precisely the exec-verifier model (any argv speaking §8). SP3's acceptance is proven with a **tiny reference policy program** (a few-line argv that reads the §8 stdin and emits a §8 verdict — the oracle's deterministic judge), NOT by vendoring `opa`/`cedar` binaries. A concrete OPA-`eval` or Cedar wrapper is an *example/adapter*, demand-gated, never a build dependency. Bundling either would violate I7 and is explicitly out of scope.

## 6. Hot-path latency — sharper than natives, stated not hidden

An exec subprocess per governed tool call (fork+exec+interpreter startup) is far heavier than an in-binary native, and it sits on the agent's critical path against a fabric sized for ≤~10³ events/min (permit-engine §10.2). SP3's posture:

- **Correctness first, one-shot argv.** SP3 ships the one-shot dispatch and **states the per-call latency ceiling** (an OPA/Cedar cold `eval` is 10s–100s of ms — comparable to, or above, the SP2 250 ms hot-path budget). A run that puts an exec verifier on `[gates.permit]` opts into that cost knowingly.
- **The cache is the answer, not required in SP3.** The decision fast-path cache (permit-engine §10.2, keyed by policy-hash + request-shape) is the latency fix and applies to exec verdicts especially well (same policy + same request-shape → cached decision). Whether SP3 must ship the cache, or state the ceiling and defer it, is a DR question (recommend: defer, state the ceiling).
- **Long-lived policy sidecar (OPA server mode) is out of SP3 scope** — it reintroduces a network hop and a stateful dependency (I7/determinism tension); note it as a possible later phase, not built here.

## 7. Verdict → decision (no new vocabulary)

An exec permit-verifier returns a §8 `VerifierOutput` (`pass|fail|inconclusive` + evidence). The existing `decide_permit`/`aggregate` path maps it verbatim: `pass→allow`, `fail→deny`, `inconclusive→ask` (I6, never coerced — a policy that errors or times out is `inconclusive{nonzero_exit|malformed_output|timeout}` → **escalate**, never a synthesized allow). The decision fact carries the exec verifier's `policy_ref` (the pinned policy) + `evidence_ref` (its §8 evidence) — `gate_explain` then surfaces the deciding *external* policy, not a hardcoded native.

## 8. Acceptance criteria (for the oracle, once the DR lands)

1. **Headline (SP3):** a `[gates.permit]` block with an `exec` policy that DENIES a forced-breach request yields `deny` (the external policy decided); an allowing policy yields `allow`. Proven with the reference policy program (§5).
2. **Un-filtered + dispatched:** an exec entry on `[gates.permit]` is no longer dropped by `permit_config_for` and is actually executed by `aggregate` (not silently skipped).
3. **Ordered short-circuit across kinds:** a native `Fail` before an exec entry short-circuits to Deny without running the exec; an exec `Fail` short-circuits a later native — order is honored across native+exec (the §3(A) interleave).
4. **Never-coerce (I6):** an exec policy that exits nonzero / emits malformed stdout / overruns `timeout_ms` → `inconclusive` → `ask`, never allow.
5. **Determinism/replay (I6):** the policy is pinned (`policy_ref`); `debrief` re-executes the recorded §8 stdin against the same policy and the verdict matches (or raises a DR-006 integrity alarm on divergence).
6. **I7:** no vendored policy-engine binary enters the build; the reference judge is a local argv.

## 9. Decisions the DR ratifies (two owner-ratified 2026-07-19; two recommended)

1. **Sync/async dispatch** — async permit aggregation (§3 A, **recommended**) vs `block_on` (B). Architectural; shapes `decide_permit`. The DR ratifies A.
2. **Latency posture** — one-shot argv + stated ceiling, cache deferred (**recommended**) vs require the fast-path cache in SP3. The DR ratifies defer-with-stated-ceiling.
3. **Scope of SP3** — **RATIFIED (owner 2026-07-19): generic exec-permit dispatch + a reference policy program.** No vendored OPA/Cedar binary enters the build (I7); a concrete OPA-`eval`/Cedar adapter is a demand-gated follow-on, not SP3.
4. **`/intel`** — **RATIFIED (owner 2026-07-19): skip for now.** The §8 exec-verifier contract is already fixed and engine-agnostic, so the generic dispatch needs no OPA/Cedar-specific read; defer any scoped `/intel` to whenever a concrete adapter is actually built (its own DR then, DR-002 rules 1–3).

## 10. Honest risks

- **Latency** (§6) — the sharpest tension; exec on the hot path is heavy. Mitigation is the cache, not fail-open. If SP3 ships without the cache the ceiling is stated, not hidden.
- **Determinism drift** (§4) — a policy that reaches the network or reads unpinned data breaks replay. The sealed-env contract forbids it; the DR must state that a non-sealed policy is non-conforming.
- **Strategic dilution** (permit-engine §10.3, restated) — SP3 is table-stakes DSL parity, not the evidence wedge. It's bounded because the exec-verifier machinery already exists; the marginal cost is the permit-axis dispatch + the sync/async seam, nothing more.
