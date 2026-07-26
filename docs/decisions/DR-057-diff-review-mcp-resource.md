> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §9 (MCP command surface — tool register + BINDING no-drift) · §19 (operator-client register) · fulfils/extends the operator-client seam of [DR-038](DR-038-operator-client-gui-tauri.md) §Decision 4/5 (existing-surfaces-only, no new backchannel), builds on [DR-039](DR-039-board-view-mcp-resource.md)/[DR-040](DR-040-get-escalations-mcp-resource.md) (read-class-unbadged precedent, view types in `rezidnt-state`) · keying precedent [DR-049](DR-049-worktree-release-lifecycle.md) (correlation join ruled UNSOUND; `gate.failed.worktree?` attribution) · voluntarily follows the shape/cap of [DR-056](DR-056-prose-maintenance-tax.md) (PROPOSED) · invariants I1, I2, I5, I6

# Decision Record DR-057 — `diff_view` + `cas_read`: the read-only MCP surface that closes Review

**Date:** 2026-07-26
**Status:** PROPOSED.
**Amends:** §9 (adds two READ-class, unbadged MCP tools, no-drift generated like `board_view`/`get_escalations`). Reaffirms — does not touch — I1, I5, I6. Clarifies, does not amend, I2 (see Context). Mints NO ontology subject or field. Adds a §20 index row.

## Context

The operator cockpit (`rezidnt-operator`, DR-038, 8 commits) wires Watch (`board_view`/`tail_events`/`orchestration_graph`) and Steer (`kill_run`/`resolve_permit`/`get_escalations`). Compare is `open_trial` (DR-055). **Review has no surface at all** — an operator cannot see what an agent changed without opening a separate IDE, exactly the gap DR-038 §Decision 4/I5 require closed as a governed MCP capability before any button exists.

Diff bytes are already CAS-pinned: `runs.rs:2136/2148` puts `gates::summarize_worktree`'s `CasRef` on `diff.ready.diff`; `git/src/lib.rs:1617` writes it `cas.put(text, "text/x-diff")`; `diff.ready` v1 pins the contract ("never inline diff bytes"). **No MCP path resolves those bytes** — `rezidnt-mcp/src/lib.rs`'s only CAS access is `permit_cas`/`ephemeral_cas`, internal to the permit gate.

**Correction to a cited premise:** `bins/rezidnt/src/main.rs` has no `diff` verb. Its `Cmd` enum has no such case; every `"Diff"` token there is the `fmt-check` verifier parsing rustfmt's `Diff in <path>` output — unrelated. Review is missing on CLI and MCP alike.

**Keying, settled against the tree, not assumed.** `RunRow` carries no worktree reference; `WorktreeState.allocator` on the ordinary (non-fan-out) path is the bare string `"rezidnt"` — no run named at all. A run-keyed resource has nothing sound to join on, and DR-049 already ruled the obvious alternative (a correlation join) UNSOUND — one correlation spans N runs and N trees. The worktree path is the one entity both `diff.ready` and the existing fold already key on (`WorktreeState.last_diff`; `gate.failed.worktree?`'s attribution precedent, `spec/ontology.md` ~349).

**The crux: does I2 govern this response?** Plan text scopes I2 explicitly to "the event fabric" (32 KiB cap, CAS-ref rule) — a bus-design constraint, not a general MCP-response rule; `board_view`/`orchestration_graph` already return uncapped whole-log JSON. I2 does not literally bind this surface. The ref-not-bytes discipline is adopted anyway as existing HOUSE PRACTICE (`gate.failed.evidence`/`inputs.refs` are refs-only by convention, not by I2's letter) — the crux is settled as: I2 is fabric-scoped; refs-over-bytes survives here by precedent.

**Strongest counterargument, recorded.** A bounded byte-reader generalizes past "diff review" into a generic CAS-read capability, inviting scope creep toward paging arbitrary evidence blobs. Accepted anyway: a diff without its bytes isn't review, DR-038 §Decision 4 already forecloses the local-disk-read shortcut (no new backchannel; the client is not privileged to know `~/…/cas/<hash>`, wherever the daemon actually roots it — `cas_path()` is `<log-dir>/cas`, not a fixed path), and a single-shot bounded reader is the narrowest thing that satisfies both diff review and I2's spirit.

## Decision

1. **`diff_view`** — read-only, unbadged (DR-005/DR-039 read-class). Args `{worktree: string}` (the canonicalized key `WorktreeRow`/`gate.failed.worktree?` already use). Returns `{worktree, lifecycle, outcome, diff: CasRef | null}` — null when no `diff.ready`/`diff.merged` has folded for that tree, never a fabricated empty diff. Requires retaining the FULL `CasRef` (today `WorktreeState.last_diff` keeps only the hash string, discarding bytes/mime already on the wire) — an ADDITIVE internal widening of a `rezidnt-state` Rust struct, not an ontology mint, and it does not touch `board_view`'s shape (DR-039 untouched).
2. **`cas_read`** — read-only, unbadged. Args = a `CasRef` (hash, bytes, mime — the shape already on `diff.ready`/`gate.failed.evidence`), not a bare hash: it echoes and verifies the caller's own ref rather than inventing metadata the CAS layer never persists (mime lives only in event payloads). Returns `{content: string, bytes_returned: u64, truncated: bool}`. DEFAULT bound 256 KiB (cheap to revisit, not BINDING): over-bound content is REFUSED, never silently chopped — `truncated`/`bytes_returned` let a client never mistake a partial diff for a whole one. v1 serves text mimes only; a non-text mime is refused with a plain code, never mangled.
3. **Keyed by worktree**, never by run or by a fact's own id, per the finding above.
4. **No badge** — same trust tier as `board_view`/`tail_events`.

## Ledger

Invariant: NONE (I2 clarified fabric-scoped, not amended). Subject: NONE. Field: NONE (ontology unchanged; only an internal `rezidnt-state` struct widens). Trait method: NONE (pure read via existing `replay`/`fold` + `Cas::get`). Dep: NONE.

## Consequences

**Roadmap.** §9 gains two rows; §19 gains Review as a closed cockpit verb, unblocking `rezidnt-operator`'s diff panel per DR-038 §Decision 4/5. §20 owes TWO index bumps on acceptance — DR-056's withheld "next record is DR-057" and this record's own "next record is DR-058" — land both together.

**Risk register.** ADDS: `cas_read` generalizes past diff bytes; a second consumer (e.g., gate evidence) should get its own DR, not be assumed free. RETIRES the "Review has no surface" gap.

**Test/criterion honesty, plain words.** Weakens no test; adds two schemars no-drift cases (`jsonrpc_surface.rs`, DR-039's pattern) as new obligations.

**No intel memo cited.**

**Following DR-056's cap voluntarily** (itself PROPOSED): second proof the Context/Decision/Ledger/Consequences shape holds for a design-bearing record.

Amendments to this record require DR-058.
