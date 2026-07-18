# Design sketch — SP2 harness PEP integration (make the "may" axis enforce)

**Status:** PROPOSED (design-first per [DR-002](../decisions/DR-002-prior-art-protocol.md) rule 1) · **Feeds:** a new DR (owner sign-off) amending §16/roadmap and recording the fail-posture stance · **Builds on:** [permit-engine](permit-engine.md) §3/§5/§10/§11, [DR-008](../decisions/DR-008-permit-engine-pivot.md), [DR-009](../decisions/DR-009-match-omnigent-scope.md), [DR-011](../decisions/DR-011-permit-pdp-config-seam.md), [DR-012](../decisions/DR-012-empty-vs-absent-intent.md) · **Owner:** TwofoldTech LLC

> Not BINDING. This exists so SP2's external-boundary crossing (a real harness hook that BLOCKS a live tool call) is committed in writing before any `/oracle`. Nothing here is built until the DR is ACCEPTED. Sequence: this sketch → `/dr` (owner) → `/subject` if needed → `/oracle` → implementer → `/vet` → `/debrief`.

## 1. Scope — SP2 is the slice that makes the engine *enforce*

Everything shipped through SP-empty **decides**: SP1 pinned the wire shape and native permit pack, SP-intent added intent-lock, SP-wire made the live PDP dispatch the configured `[gates.permit]` set, SP-empty distinguished declared-empty from absent intent. All of it answers *"may this action proceed?"* — but only when something bothers to ask, and today only the MCP surface answers. SP2 is the slice where a **real mid-run tool call gets stopped by policy** (permit-engine §11 SP2 acceptance: *"a real mid-run tool call is blocked by policy, one take"*).

Two concrete gaps close here:

1. **The socket PDP path.** `bins/rezidentd/src/main.rs:355` currently answers `Request::RequestPermission` with a single honest `op.not_served` error frame — SP1 pinned only the wire shape and routed the decision through MCP (I5). SP2 un-stubs it: the socket must service a real permit decision so a harness hook (which speaks the socket, not loopback-HTTP) can ask.
2. **The PEP itself.** A claude-code `PreToolUse` hook that intercepts the agent's tool call, asks the daemon over the socket, and enforces the answer (block on `deny`, route on `ask`, proceed on `allow`).

Non-goal for SP2: the sole-execution-chokepoint posture (C3 — OS sandbox / egress / credential brokering). That stays fenced behind its own design + DR (DR-009). SP2 is enforcement *as strong as the PEP allows* and no stronger (design §3, §10.1) — we say so, we do not overclaim.

## 2. The decision seam already exists — do not fork it (I3)

`McpCore::call_request_permission` (`crates/rezidnt-mcp/src/lib.rs:461`) already performs the entire PDP flow:

1. badge check (§12 door discipline) → 2. emit `permit.requested` → 3. resolve the configured verifier set (`permit_config_for`, DR-011 §1) → 4. fold the run's per-run state (intent allowlist + spend accumulator, injected as content-pinned params, DR-011 §2 / DR-012 option B) → 5. `permit::aggregate` (first-Fail short-circuit → Deny; any Inconclusive → Escalate; else Grant) → 6. map via the verdict→decision table (I6, `inconclusive → ask`, never coerced) → 7. emit ONE decision fact carrying the deciding verifier's `policy_ref` + honest `evidence_ref`.

**The load-bearing SP2 decision: the socket handler must not reimplement any of that.** Two live facts on the log per permission (I3, first-class permission stream in `tail`) come from exactly one code path, or the two transports drift and one of them lies. So:

**Recommendation (B): extract a transport-neutral PDP entrypoint.** Lift the body of `call_request_permission` into a typed method — sketch signature:

```rust
// on McpCore (it already holds fabric, cas, substrate, permit_config)
pub async fn decide_permit(&self, req: PermitRequest) -> Result<PermitOutcome, PdpError>;

struct PermitRequest {          // transport-neutral; both callers build this
    run: String,
    request_id: Option<String>, // socket supplies the PEP's token; MCP mints one
    action: String,
    tool: String,
    badge: Option<String>,
    context_ref: Option<String>,
    paths: Option<Value>,       // the request axis the natives read
}
struct PermitOutcome { request_id: String, decision: Decision /* Allow|Deny|Ask */, reason: Option<String> }
```

- `call_request_permission` becomes a thin JSON-RPC adapter over `decide_permit` (build `PermitRequest` from `args`, map `PermitOutcome` back to `tool_ok`).
- The socket handler builds `PermitRequest` from `Request::RequestPermission` and maps `PermitOutcome` → `Reply::PermitDecision { request_id, decision, reason }`.
- **`request_id`:** the socket `Request` already carries one (the PEP's correlation token, proto `lib.rs:98`). `decide_permit` uses the supplied id when present, else mints — so the PEP's ask and the on-log decision fact share one id. MCP callers don't supply one, so they keep minting. (Alternatives A — socket re-encodes to `args` and calls the JSON-RPC method — is rejected: it mints a fresh request_id internally, discarding the PEP's token, and drags JSON-RPC envelope shapes onto the socket.)

**Availability wrinkle (must be in the DR/impl):** `McpCore` is currently constructed only when `REZIDNT_MCP_LOCKFILE` is set (`main.rs:197`), because it only ever backed the HTTP transport. The socket PDP must not depend on the HTTP transport being enabled. So the daemon constructs the PDP core (or at least its permit component — fabric + cas + `McpBridge` substrate, all already owned by `Daemon`) unconditionally at startup, and both the socket handler and the optional HTTP transport share the one `Arc`. This is a small startup-wiring change, called out so the auditor expects it.

## 3. The PEP contract — claude-code `PreToolUse` hook

The PEP is a hook script (the repo already runs hook scripts — `.claude/hooks/vet.sh`, `firewall.sh`, `ontology-gate.sh` — this is the same shape, PreToolUse instead of Pre/PostToolUse on our own tooling). Contract:

- **Input (stdin JSON):** the harness passes the tool name + tool input. The hook maps `tool_name`→`tool`, extracts the run id and any path arguments, and (per I2) if the tool input is bulky it pins it to CAS and carries `context_ref`, never inline bytes over the socket.
- **Transport:** connect to the daemon UDS (`REZIDNT_SOCKET` / `socket_path()`), read the hello, send one `Request::RequestPermission` line, read one `Reply::PermitDecision` (or a `Reply::Error`).
- **Enforcement (stdout / exit code, per the claude-code hook protocol):** `allow` → let the tool proceed; `deny` → block with the `reason` surfaced to the agent; `ask` → route to the human-decision surface (the escalate path is a *client*, I1 — the daemon renders nothing). The mapping from these three to the harness's own PreToolUse block/allow/ask output is the hook's job.
- **Addressing / discovery:** the hook finds the daemon via `REZIDNT_SOCKET`, falling back to the documented `socket_path()` location; a run without a reachable daemon takes the fail-posture below.

**Fail-posture on the hot path (RATIFIED by owner 2026-07-18 → the DR records it):**

**Fail *closed*, to `ask`.** If the PDP is unreachable or the request times out, the PEP must NOT silently let the action proceed — a governance product that permits-by-default when its brain is offline is dishonest, and it contradicts the already-ratified stance that an empty/unresolvable policy **escalates, never synthesizes an allow** (DR-011 §3). Failing closed *to `ask`* (escalate to a human) rather than hard-deny keeps a crashed daemon from freezing every agent into an unrecoverable wall — a human can unblock. Pair it with a **bounded timeout** (order 100–500ms; exact value is the DR's to set) before the PEP treats the PDP as unreachable, so the hot path stays hot (permit-engine §10.2 latency honesty).

**Dissent (record verbatim in the DR):** fail-open is friendlier and keeps agents moving when the daemon flaps, and the hot-path timeout adds latency to every governed tool call (permit-engine §10.2 — per-action checks sit on the agent's critical path against a fabric designed for ≤~10³ events/min). Counter: for the "may" axis, silent permit-on-failure is exactly the overclaim §10.1 forbids; the fast-path decision cache (permit-engine §10.2, keyed by policy-hash + request-shape) is the latency answer, not fail-open.

## 4. Degradation is a substrate capability (I4)

Enforcement is a **PEP capability**, not a daemon guarantee. Harnesses that expose a `PreToolUse` hook get true mid-run interception; harnesses that do not degrade **explicitly** to the shipped edges — pre-spawn `vet` + post-hoc `debrief` evidence — and the product copy says so (design §3, §10.1; DR-009 consequences). SP2 must not describe itself as universal interception. The degradation path is stated, tested (a harness-without-hook config still produces pre/post evidence), and surfaced in `gate_explain` so an operator can see whether a given run was permit-enforced mid-run or only edge-gated.

## 5. Acceptance criteria (for the oracle, once the DR lands)

1. **Socket PDP, deny path (the headline):** a `Request::RequestPermission` for a tool outside the run's `tool-allowlist` returns `Reply::PermitDecision { decision: "deny", reason: Some(_) }`, and the log carries `permit.requested` + `permit.denied` with `policy_ref`/`evidence_ref` (I3, I6). One take.
2. **Socket PDP, three-valued honesty:** an empty/unresolvable policy set returns `decision: "ask"` (escalate), never `allow` (I6, DR-011 §3); an allowlisted tool returns `allow`.
3. **request_id fidelity:** the `request_id` on the reply and on the decision fact equals the one the caller sent (the PEP's token is not discarded).
4. **PEP hook contract:** a hook-script contract test — stdin tool descriptor → a `deny` decision produces the harness's block output and a non-proceed exit; `allow` proceeds; `ask` routes to escalate.
5. **Fail-posture:** with the daemon unreachable / past timeout, the PEP fails closed to `ask` (per the DR's ratified stance) — not a silent proceed.
6. **Degradation (I4):** a harness config without a hook still yields pre-spawn `vet` + post-hoc evidence, and `gate_explain` distinguishes mid-run-enforced from edge-gated.
7. **Single decision path (I3):** MCP and socket permission requests with identical inputs produce a byte-identical **decision** fact (`permit.granted`/`denied`/`escalated`) — that byte-identity is the proof of §2's no-fork extraction, and it is scoped to the decision fact alone. The paired `permit.requested` fact is NOT claimed identical: it carries transport-local caller identity, `badge_id`, which is present on the MCP path (a badge resolves) and absent on the socket path (§3: the socket skips the badge door, `badge:None`), so the two `permit.requested` facts differ on exactly that one field by design — not a fork. Candidate home for the carried `sp_wire_aggregate_deny` fixture — wire it into this replay assertion rather than leave it a thin green-lock (handoff residual).

## 6. Open decisions the DR must ratify (owner sign-off)

1. **Fail-posture default** — RATIFIED (owner 2026-07-18): fail-closed-to-`ask` + bounded timeout (§3). Codebase-consistent with DR-011 §3. The DR records this and the fail-open dissent verbatim.
2. **Timeout budget** — the concrete hot-path deadline before the PEP calls the PDP unreachable (§3). Still open (a tunable number; the DR may set an initial value or defer to the impl slice).
3. **Badge over the socket** — RATIFIED (owner 2026-07-18): the socket's 0600 owner-only mode (`main.rs:222`) is sufficient identity for the local hook; `badge` stays optional on the socket `RequestPermission` (proto already optional, `lib.rs:104`), and `decide_permit` skips the §12 badge check on the socket transport. The MCP path keeps its badge-first door discipline unchanged.
4. **New subjects?** — SP2 likely needs no new taxonomy (it reuses `permit.requested`/`granted`/`denied`/`escalated`), but if the degradation-visibility signal (§4 / criterion 6) wants its own fact, that is a warden `/subject` pass, gated.
5. **Where the hook binary/script lives + how a spec opts a run into PEP enforcement** — a `[gates.permit]` flag vs harness-config convention.

## 7. Honest risks (must survive the DR)

- **Enforcement bounded by the PEP** (permit-engine §10.1) — unchanged and restated: SP2 does not make rezidnt the sole chokepoint; C3 does, later, behind its own DR.
- **Hot-path latency** (permit-engine §10.2) — every governed tool call now round-trips the socket. Mitigation is the decision fast-path cache, not fail-open. If SP2 ships without the cache, the latency ceiling is stated, not hidden.
- **Un-stub is a wire-behavior change** — the `op.not_served` branch and any test pinning it (`crates/rezidnt-proto/tests/permit_request.rs` and the socket-level tests) change; the oracle rewrites those expectations rather than the implementer quietly deleting a red test.
