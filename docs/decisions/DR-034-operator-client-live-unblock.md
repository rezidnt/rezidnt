> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §16 (permit engine), §19 (operator write client) · invariants I1, I2, I3, I5, I6, I7 · pulls forward DR-033's deferred live-unblock; extends DR-014 (PEP hook + fail-closed timeout) · possible paired warden /subject (see §Design 3)

# Decision Record DR-034 — Operator client: live-unblock (resume the currently-stalled agent on resolve)

**Date:** 2026-07-22
**Status:** ACCEPTED
**Amends:** DR-033 §Decision 1 (which states resolve "adds NO live-unblock / long-poll to the PEP") — this record pulls forward the option DR-033 deferred and adds a bounded live path; §16 (permit PDP decision path — the applied decision is unchanged from DR-033; only the *moment the PEP learns it* changes); §19 (operator write client — no new verb; `resolve-permit` is unchanged). Extends DR-014 (PEP hook, fail-closed timeout discipline). Reaffirms I1's board proof untouched.

## Context

The operator-client arc is COMPLETE and shipped: DR-031 (seam) → DR-032 (kill-run) → DR-033 (resolve-escalation). DR-033 deployed "honored on next ask": `resolve_permit` records a durable `permit.resolved` fact, and `decide_permit` consults the folded ledger BEFORE verifiers, applying a matching prior resolution as `permit.granted`/`permit.denied`. Its honest, by-design limit: it does NOT resume the *currently-stalled* tool call — the agent must ask again. DR-033 explicitly REJECTED live-unblock for slice 2 and named it a POSSIBLE future slice, "demand-gated, not scheduled." The owner has now pulled it forward (2026-07-22). This record designs it.

**The rejection DR-033 recorded — engaged head-on, not pretended away.** DR-033 rejected live-unblock (a new `Reply::PermitUpdate` plus a PEP that holds its connection open and long-polls) on three grounds: (1) it "inverts the one-shot PEP contract (a held-open connection reintroduces the exact control/data-plane liveness coupling I2 and the event-sourcing discipline push out)"; (2) it "balloons scope across proto + PEP + daemon socket lifecycle"; (3) it "trades log-truth simplicity for a live mechanism the PDP-replays-the-log design makes unnecessary for correctness." Those objections have not vanished, and this record does not claim they have.

**What changed to justify accepting them now** is exactly the trigger DR-033 itself named: owner-directed demand for the operator-friction fix. Nothing about correctness changed — DR-033's ledger-check remains the source of truth and the fallback. Live-unblock is a **latency/UX layer ON TOP of the correct "honored on next ask" substrate**, not a replacement for it. Objection (3) is therefore answered by scope, not denial: correctness still comes from the log; live-unblock only shortens *when* a live-held agent learns the decision. Objection (2) is bounded by the design below (one long-poll deadline, no push variant, no held-open-forever socket). Objection (1) — the I2 coupling — is the real cost and is treated as bounded-and-honest below, with the dissent recorded, not dissolved.

The mechanics were read this session, not assumed:
- **The PEP is one-shot and fail-closed.** `bins/rezidnt/src/permit_hook.rs` `ask_daemon()` (~line 139) connects the UDS, discards the hello, writes ONE `Request::RequestPermission`, reads exactly ONE reply line, matches `Reply::PermitDecision`/`Reply::Error`, returns. Entirely `#[cfg(unix)]` (the `#[cfg(not(unix))]` stub bails). Every blocking op is bounded by `REZIDNT_PERMIT_TIMEOUT_MS` (250ms default, DR-014 §Decision 3) via `set_read_timeout`/`set_write_timeout`; `decide()` funnels every failure to a fail-closed `ask`.
- **`Reply` has no waiting variant.** `crates/rezidnt-proto/src/lib.rs:178-204` — `OpenOk`, `AlarmsRecorded`, `PermitDecision { request_id, decision, reason? }`, `Error`. Additive-evolution discipline: absent = OMITTED never null, older peers still parse.
- **`Request::RequestPermission`** — `crates/rezidnt-proto/src/lib.rs:98-115`.
- **Daemon socket handler** — `bins/rezidentd/src/main.rs` services `RequestPermission`, answers `PermitDecision`.
- **PDP** — `crates/rezidnt-mcp/src/lib.rs` `decide_permit` folds run state every call; DR-033 added the pre-verifier ledger-check.
- **request_id is not stable across asks** (DR-033) — a re-ask carries a fresh id, so DR-033 keys next-ask matching on ACTION identity `(run, tool, action/target)`. A live-held PEP, by contrast, still holds the ORIGINAL escalated request_id on its open connection.

## Decision

1. **Live-unblock = a bounded server-assisted long-poll, NOT a held-open push.** On an "ask"/escalated decision, the PEP MAY re-issue its request to the daemon, which holds that request up to a bounded live-unblock deadline waiting for a matching `permit.resolved` to land, then either returns the applied `permit.granted`/`permit.denied` (the agent resumes without re-prompting) or returns `ask` on deadline expiry (DR-033's fallback — the agent re-asks). This layers on top of DR-033; it does NOT replace or weaken the ledger-check, which remains the correctness path and the fallback for any agent NOT holding a live connection.

2. **Bounded, fail-closed to `ask` — never a silent proceed, never an indefinite hang.** The live-unblock hold uses a SEPARATE, longer deadline than the 250ms hot-path budget (a stalled-agent wait is a different budget than a hot decision) — a new env knob, e.g. `REZIDNT_UNBLOCK_TIMEOUT_MS`, distinct from `REZIDNT_PERMIT_TIMEOUT_MS`. On expiry the design degrades to exactly today's behaviour: fail-closed to `ask` (DR-014, DR-033). A held request that errors or times out is an `ask`, never a proceed.

3. **Match on the ORIGINAL request_id (the live path's one advantage), fallback keeps DR-033's action-identity key.** Because the connection is still open, the live-held PEP still carries the ORIGINAL escalated request_id — so the daemon can wake it by that exact id (tighter than action-identity, no ambiguity). DR-033's `(run, tool, action/target)` key is unchanged and remains the matcher for the non-live "next ask" (a fresh id). Live-unblock adds a request_id-keyed wake path; it does not alter the ledger key.

## Design

- **Mechanism — chosen: (a) server-assisted long-poll.** The daemon holds the re-issued request up to the live-unblock deadline (a bounded server-side wait, not a busy spin), releasing on a matching resolution or on deadline.
  **Rejected: (b) held-open connection + `Reply::PermitUpdate` push** — the literal option DR-033 named. It is the sharpest I2 violation: an indefinitely-held socket the daemon pushes onto is a genuine liveness coupling with no natural bound, and it grows daemon socket-lifecycle state. Long-poll bounds the coupling to ONE deadline window and degrades to fail-closed `ask` — the same posture the PEP already has — so no new failure mode is introduced.
  **Rejected: (c) notify/wake subscription seam** (per-run resolution signal the PEP subscribes to) — more moving parts (a pub/sub seam on the socket) for the same effect long-poll gets from a bounded re-request; deferred as unnecessary.
- **Wire.** Prefer NO new `Reply` variant: the long-poll returns the existing `Reply::PermitDecision` (resolved) or an `ask` decision (expiry), so the PEP's existing match arms suffice and older peers are unaffected. IF a held-request needs a distinct on-the-wire "still waiting, re-poll" signal, that variant is added ADDITIVELY to `crates/rezidnt-proto/src/lib.rs:178-204` (absent = OMITTED). The socket-lifecycle change lands in the daemon handler (`bins/rezidentd/src/main.rs`) and in the `#[cfg(unix)]` body of `ask_daemon()`.
- **Subject — likely NONE, flagged not minted.** The applied decision is fully carried by DR-033's `permit.resolved` + the emitted `permit.granted`/`permit.denied`; live-unblock changes no fact. HOWEVER, if distinguishing "resumed the live-held call" from "honored on a fresh re-ask" has audit value in `gate why`/`debrief` (I6), that is a PAIRED warden `/subject` — e.g. a `resumed_live` marker on the applied grant/deny — flagged HERE, NOT minted here (mirrors DR-033's handling of `permit.resolved`). Default lean: no new subject; the request_id chain already tells the story.
- **Windows.** The PEP is `#[cfg(unix)]`-only; the named-pipe path is designed-not-built (DR-014). This design lands in `#[cfg(unix)]` bodies and the daemon UDS handler; the Windows named-pipe path remains designed-not-built and inherits this design when built. Scope is NOT expanded to build it.

## Invariants

- **I2 — bounded and honest, NOT untouched (record the dissent).** A long-polled connection IS a liveness coupling — DR-033 was right that the one-shot contract is the cleaner design. This record does not claim otherwise. The coupling is BOUNDED to one deadline window, degrades to the log-truth fallback (fail-closed `ask`) on expiry, and carries NO data-plane payload inline — evidence still rides by-ref. We accept a bounded, honest coupling as the price of the owner-directed UX fix; the held-open-push variant (b), which has no natural bound, stays rejected precisely to keep the coupling bounded.
- **I3 — untouched.** Live-unblock changes only WHEN the agent learns the decision, never WHAT is on the log. The applied decision is still DR-033's `permit.granted`/`permit.denied` via the single writer (DR-006). A replay reconstructs identically whether or not any agent was live-held — no hidden state.
- **I5 — no new operator tool.** `resolve_permit` is unchanged; this is a PEP/proto/daemon-socket change, not a new MCP verb. §19's register is unchanged.
- **I6 — interrogable.** `gate why`/`debrief` still explain "escalated → human-resolved → granted." Distinguishing "resumed live" from "re-asked" is the §Design 3 subject question, IF it carries audit value.
- **I1 — untouched.** No board/`rezidnt-tui` linkage; the PEP and operator client are separate from the read-only board. The `crate_has_no_writer_dependency` proof stays intact.
- **I7 — no heavy dep.** Reuses the UDS, the existing PDP fold, the single writer; adds one bounded timeout and a hold on the existing socket, no new runtime dependency.

## Consequences

- **Roadmap.** A new slice, `operator-live-unblock`, enters the loop ON RATIFY (standard sequence: DR → paired `/subject` IF §Design 3 lands one → `/oracle` → implement → `/vet` → `/debrief`). It does NOT auto-advance current-slice; that happens on acceptance.
- **Risk register.** CLOSES DR-033's carried-forward risk ("a resolved escalation does NOT auto-resume the stalled agent") for the live-held case. ADDS the honest new risk in plain words: a bounded live-hold keeps a longer-lived connection open on the socket (a resource/lifecycle cost), and there are now TWO timeout budgets to reason about (250ms hot-path vs the longer unblock deadline). The Windows named-pipe path stays behind.
- **Test/criterion honesty.** This LOWERS no bar and WEAKENS no DR-033 test. DR-033's ledger-check tests stay green — the fallback is unchanged and is still exercised on deadline expiry and for non-live agents. New criteria are ADDED for the live path (resolve-while-held resumes; deadline expiry degrades to `ask`; a held request never proceeds silently).
- Cross-references: DR-033 (parent — pulls forward its deferred option and amends its §Decision 1 "adds NO live-unblock" clause), DR-032/DR-031 (the arc), DR-014 (PEP hook + fail-closed timeout this extends), DR-008/DR-009 (the PDP), DR-006 (single-writer append).

Amendments to this record require DR-035.
