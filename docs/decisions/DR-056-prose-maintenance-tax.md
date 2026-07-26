> Index: [§20 of the plan](../rezidnt-architecture.md#20-decision-records) · plan §20 (decision-record practice) · cites `intel/003-omnigent-comparison-2026-07-26.md` (DR-002 rule 3) · retires the errata-record genre [DR-052](DR-052-worktree-release-ledger-correction.md)/[DR-053](DR-053-agent-completed-error-mint-disclosure.md) exemplify · precedent: [DR-049](DR-049-worktree-release-lifecycle.md) §Status's in-place strikethrough · invariant boundary: I6 untouched

# Decision Record DR-056 — The prose-maintenance tax: ledgers amend in place, doc claims get pinned, records cap at 800 words

**Date:** 2026-07-26
**Status:** PROPOSED.

**Amends:** §20 of the plan (decision-record correction practice) — supersedes the informal precedent [DR-049](DR-049-worktree-release-lifecycle.md) §Status set and that [DR-052](DR-052-worktree-release-ledger-correction.md)/[DR-053](DR-053-agent-completed-error-mint-disclosure.md) formalized into a genre. [DR-053](DR-053-agent-completed-error-mint-disclosure.md) §Decision 5's five-limb ledger form is UNCHANGED and keeps governing every ledger sentence; it now ships as an in-place amendment, not a new numbered record. No invariant text is rewritten. Owed on acceptance, withheld while PROPOSED (DR-052/DR-053's own precedent): a one-line pointer at DR-052 and DR-053 naming them the closing instances of the retired genre, and the §20 index bump to "next record is DR-057."

## Context

The build loop is fast — 44–48 commits/day — and the tax is not in building. Code claims are pinned by the gauntlet; that is why they are accurate, and it is the moat. Prose claims (DR ledgers, doc-comment rationale, `file:line` citations) have nothing pinning them, so they drift, and the correction instrument is itself prose that drifts too.

Commit history since 2026-07-24 carries a steady run of prose-only corrections, verified against the log: "correct an overstated rationale in the guard's doc comment," "strike the registry premise from the allocator comment," "DR-044: refresh three stale line citations," and, third-order, "the eleventh defect, and a false caveat inside a correction." [DR-052](DR-052-worktree-release-ledger-correction.md) (2,136 words, counted) exists only to correct [DR-049](DR-049-worktree-release-lifecycle.md)'s ledger; [DR-053](DR-053-agent-completed-error-mint-disclosure.md) (3,121 words, counted) only to correct DR-050's and DR-051's mint disclosures — its own Status field spends ~500 words adjudicating whether its own back-pointers may apply. Records already run ~80,399 words against ~412,740 words of Rust (tests and doc comments included); DR-044 through DR-055 average ~1,900 words. `intel/003-omnigent-comparison-2026-07-26.md` compounds the proof (DR-002 rule 3): it asserted a worktree-lifecycle gap DR-049 had closed the day before, and now carries its own same-day correction. The same memo names the cost concretely: rezidnt's widest gap against Omnigent is harness breadth — one production harness (`bins/rezidentd/src/runs.rs:771`) against ~eleven — and the I4 `AgentSubstrate` seam already makes an adapter cheap to build; yet adapter #2 alone has consumed five records and ~10,000 words (DR-048/050/051/053/055) without reaching `SUPPORTED_HARNESSES`. Bookkeeping, not engineering, is what makes that gap unfillable.

**Strongest counterargument, recorded.** In-place correction trades a citable, standalone errata record for one increasingly scarred file, and an 800-word cap risks cutting the very recorded dissent — like this paragraph — that makes these records worth more than their outcomes. Both costs are real. They are accepted because the alternative was tried, for two records totaling ~5,250 words, without stopping the drift it existed to fix: DR-052 corrected DR-049, and one record later DR-053 disclosed a defect of the identical undisclosed-mint class.

## Decision

1. **Ledger and citation corrections amend the original record in place** — dated strikethrough, verbatim preserved — the pattern DR-049's own Status field already used. Only a changed DECISION mints a new numbered record. This retires the errata-record genre, not DR-052 or DR-053 themselves: both stand as accurate history of their moment; they are the last built the old way.
2. **A doc comment that asserts another module's behavior, or cites `file:line`, must be pinned by a source-text guard — proven red by deletion, per the standing mutation-proof rule — or it must not be written.** An unpinned claim is a liability with no verifier; this arc paid for that at least ten times in three days.
3. **Decision records cap at ~800 words: Context / Decision / Ledger / Consequences.** Anything longer is design work and belongs in the slice brief, not the record. This record obeys its own cap.

**This cuts UNPINNED prose only.** Test-pinned claims — oracle boards, source-text guards, the gauntlet, the auditor's verdict contract — are the moat and stay exactly as rigorous; [DR-051](DR-051-codex-failure-recording-and-fallback-fidelity.md) is the proof the discipline is load-bearing, catching a near-I6 miscoercion before it shipped. Nothing here is a license to test less.

## Ledger

Invariant: NONE. Subject: NONE. Field: NONE. Trait method: NONE. Dep: NONE.

## Consequences

**Roadmap.** §20 gains this row on acceptance, with the index bump to DR-057. DR-052 and DR-053 each gain a one-line pointer, applied only on acceptance, naming them the closing instances of the retired genre — no other text in either record changes. Owed, same acceptance: a corollary line in the rust-conventions skill naming Decision 2 the standing rule. No slice, no arc reordering.

**Risk register.** RETIRES open-ended errata-record growth as a standing risk. ADDS, named not fixed: an in-place record could itself scar unreadably across many corrections — accepted, matching DR-049's own two-corrections-deep precedent.

**Test/criterion honesty, plain words.** Weakens no test, oracle board, or gauntlet gate. Decision 2 is a new obligation on future doc comments, not a relaxation of an existing one.

**Deferred, named.** An audit of existing doc comments against Decision 2 — not done here; owed to a future slice.

Amendments to this record require DR-057.
