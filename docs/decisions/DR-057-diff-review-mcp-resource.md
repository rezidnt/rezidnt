> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §9 (MCP command surface — tool register + BINDING no-drift) · §19 (operator-client register) · fulfils/extends the operator-client seam of [DR-038](DR-038-operator-client-gui-tauri.md) §Decision 4/5 (existing-surfaces-only, no new backchannel), builds on [DR-039](DR-039-board-view-mcp-resource.md)/[DR-040](DR-040-get-escalations-mcp-resource.md) (read-class-unbadged precedent, view types in `rezidnt-state`) · keying precedent [DR-049](DR-049-worktree-release-lifecycle.md) (correlation join ruled UNSOUND; `gate.failed.worktree?` attribution) · voluntarily follows the shape/cap of [DR-056](DR-056-prose-maintenance-tax.md) (PROPOSED) · invariants I1, I2, I5, I6

# Decision Record DR-057 — `diff_view` + `cas_read`: the read-only MCP surface that closes Review

**Date:** 2026-07-26
**Status:** PROPOSED.
**Amends:** §9 (adds two READ-class, unbadged MCP tools, no-drift generated like `board_view`/`get_escalations`). Reaffirms — does not touch — I1, I5, I6. **Rules NOTHING about I2's scope** (see Context; an earlier draft did, and was narrowed). Mints NO ontology subject or field. Adds a §20 index row.

## Context

The operator cockpit (`rezidnt-operator`, DR-038, 8 commits) wires Watch (`board_view`/`tail_events`/`orchestration_graph`) and Steer (`kill_run`/`resolve_permit`/`get_escalations`). Compare is `open_trial` (DR-055). **Review has no surface at all** — an operator cannot see what an agent changed without opening a separate IDE, exactly the gap DR-038 §Decision 4/I5 require closed as a governed MCP capability before any button exists.

Diff bytes are already CAS-pinned: `runs.rs:2136/2148` puts `gates::summarize_worktree`'s `CasRef` on `diff.ready.diff`; `git/src/lib.rs:1617` writes it `cas.put(text, "text/x-diff")`; `diff.ready` v1 pins the contract ("never inline diff bytes"). **No MCP path resolves those bytes** — `rezidnt-mcp/src/lib.rs`'s only CAS access is `permit_cas`/`ephemeral_cas`, internal to the permit gate.

**Correction to a cited premise:** `bins/rezidnt/src/main.rs` has no `diff` verb. Its `Cmd` enum has no such case; every `"Diff"` token there is the `fmt-check` verifier parsing rustfmt's `Diff in <path>` output — unrelated. Review is missing on CLI and MCP alike.

**Keying, settled against the tree, not assumed.** `RunRow` carries no worktree reference; `WorktreeState.allocator` on the ordinary (non-fan-out) path is the bare string `"rezidnt"` — no run named at all. A run-keyed resource has nothing sound to join on, and DR-049 already ruled the obvious alternative (a correlation join) UNSOUND — one correlation spans N runs and N trees. The worktree path is the one entity both `diff.ready` and the existing fold already key on (`WorktreeState.last_diff`; `gate.failed.worktree?`'s attribution precedent, `spec/ontology.md` ~349).

**Does I2 govern this response? DELIBERATELY NOT RULED HERE.** The question is real: I2's text names "the event fabric," its rationale is bus throughput ("≤ ~10³ events/min"), and its only enforcement is on event payloads (`rezidnt-types/src/lib.rs:221`) — all of which point fabric-scoped. But [DR-034](DR-034-operator-client-live-unblock.md) already invoked I2 against an MCP *transport* choice ("unbounded I2 coupling"), so the house has not read it as fabric-only. **This record does not need the answer:** DR-034's concern was an unbounded held-open channel, whereas `cas_read` is one-shot and REFUSES over-bound content rather than streaming it — I2-consistent under either scope. The scope question is left OPEN and named, not settled in passing by a record that can proceed without it.

**The open question this surfaced, owed its own record:** `board_view` and `orchestration_graph` return whole-log JSON with no size bound at all (verified: no cap, limit, or truncation in either call path). If I2 does reach MCP responses, those predate and exceed anything here — `cas_read` would be the only bounded read tool on the surface. That is a question about shipped tools ([DR-039](DR-039-board-view-mcp-resource.md) onward), not about this one.

**Strongest counterargument, recorded.** A bounded byte-reader generalizes past "diff review" into a generic CAS-read capability, inviting scope creep toward paging arbitrary evidence blobs. Accepted anyway: a diff without its bytes isn't review, DR-038 §Decision 4 already forecloses the local-disk-read shortcut (no new backchannel; the client is not privileged to know `~/…/cas/<hash>`, wherever the daemon actually roots it — `cas_path()` is `<log-dir>/cas`, not a fixed path), and a single-shot bounded reader is the narrowest thing that satisfies both diff review and I2's spirit.

## Decision

1. **`diff_view`** — read-only, unbadged (DR-005/DR-039 read-class). Args `{worktree: string}` (the canonicalized key `WorktreeRow`/`gate.failed.worktree?` already use). Returns `{worktree, lifecycle, outcome, diff: CasRef | null}` — null when no `diff.ready`/`diff.merged` has folded for that tree, never a fabricated empty diff. Requires retaining the FULL `CasRef` (today `WorktreeState.last_diff` keeps only the hash string, discarding bytes/mime already on the wire) — an ADDITIVE internal widening of a `rezidnt-state` Rust struct, not an ontology mint, and it does not touch `board_view`'s shape (DR-039 untouched).
2. **`cas_read`** — read-only, unbadged. Args = a `CasRef` (hash, bytes, mime — the shape already on `diff.ready`/`gate.failed.evidence`), not a bare hash: it echoes and verifies the caller's own ref rather than inventing metadata the CAS layer never persists (mime lives only in event payloads). Returns `{content: string, bytes_returned: u64, truncated: bool}`. DEFAULT bound 256 KiB (cheap to revisit, not BINDING): over-bound content is REFUSED, never silently chopped — `truncated`/`bytes_returned` let a client never mistake a partial diff for a whole one. v1 serves text mimes only; a non-text mime is refused with a plain code, never mangled.
3. **Keyed by worktree**, never by run or by a fact's own id, per the finding above.
4. **No badge** — same trust tier as `board_view`/`tail_events`.

## Ledger

Invariant: NONE (I2's scope left OPEN and named, not ruled — see Context). Subject: NONE. Field: NONE (ontology unchanged; only an internal `rezidnt-state` struct widens). Trait method: NONE (pure read via existing `replay`/`fold` + `Cas::get`). Dep: NONE.

## Consequences

**Roadmap.** §9 gains two rows; §19 gains Review as a closed cockpit verb, unblocking `rezidnt-operator`'s diff panel per DR-038 §Decision 4/5. §20 owes TWO index bumps on acceptance — DR-056's withheld "next record is DR-057" and this record's own "next record is DR-058" — land both together.

**Risk register.** ADDS: `cas_read` generalizes past diff bytes; a second consumer (e.g., gate evidence) should get its own DR, not be assumed free. RETIRES the "Review has no surface" gap.

**Test/criterion honesty, plain words.** Weakens no test; adds two schemars no-drift cases (`jsonrpc_surface.rs`, DR-039's pattern) as new obligations.

**No intel memo cited.**

**Evidence against DR-056's cap, recorded while DR-056 is still PROPOSED.** This record adopted DR-056's four-section SHAPE voluntarily and the shape held. Its ~800-word CAP did not: this record ran 866 words as drafted and 986 after being narrowed to stop ruling on I2 — and DR-056 itself came in at 851. **Neither the record proposing the cap nor the first record following it met it.** Two data points, both over, one of them the proposal itself. The shape is worth keeping; the number wants revisiting before DR-056 is ratified — plausibly ~1000, or an explicit exemption for design-bearing records as against the ledger-and-correction records DR-056 was actually reacting to.

Amendments to this record require DR-058.
