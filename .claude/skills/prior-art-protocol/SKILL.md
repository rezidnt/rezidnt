---
name: prior-art-protocol
description: >-
  The DR-002 rules for handling competitor and third-party sources — what may be read, what
  is firewalled, and how influence stays auditable. This skill should be used whenever
  competitor products, docs, or external source come up, and by the analyst agent for every
  intel read. Encodes the clean-room boundary and the memo-then-DR discipline.
user-invocable: false
version: 0.1.0
---

# Prior-art protocol (DR-001 / DR-002)

The rule is auditability, not abstinence. Full text: architecture doc DR-001 and DR-002.

## What is firewalled (never read for implementation)
herdr and any AGPL/copyleft source. Enforced by `.claude/hooks/firewall.sh`. Do not attempt to reason around the hook. Black-box behavior — installing and running the binary, observing outputs — is permitted (rule 6); source is not.

## What is permitted read-only
Competitor docs, blogs, issues, and permissively-licensed repos (Omnigent is Apache-2.0). Reading buys no legal contamination absent copying; independent invention is not a patent defense, so purity provides no legal cover and is not the point. The point is that influence stays traceable.

## The mechanical protocol
1. **Design-first** — no competitor source opens until the corresponding rezidnt design is committed in writing. (Fabric, run substrate, traits, gate model are already committed as of v0.2 + DR-001.)
2. **Extraction-scoped reads** — write the questions the read must answer first; end with a memo in `intel/` (use `intel/TEMPLATE.md`). No question, no read.
3. **Traceable influence** — any design change motivated by an intel memo requires its OWN decision record citing that memo. Anchoring becomes an audit trail, not a superstition.
4. **Primary oracles** — implementation guidance comes from the harness vendor's own docs plus recorded-transcript contract tests. Competitor code is never an implementation reference.
5. **Nothing ported** — permissive prior art informs; it is not transplanted. NOTICE obligations never arise because nothing is copied.
6. **Benchmark exception** — running and scoring competitor binaries is unrestricted.
7. **Post-freeze gap-diff** — after ontology v1 freezes, one scoped read of a competitor's event/policy taxonomy to find coverage gaps, memo'd per rule 2, additions DR'd per rule 3.

## Memo shape
Questions → findings (each with a URL, a date, and a confidence level) → implications → mandatory footer: "Design changes motivated by this memo require a DR citing it." Verbatim-quote budget near zero. Describe behavior and gaps, never transplantable design structure.
