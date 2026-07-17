[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-002 — Prior-art protocol for competitor sources

**Date:** 2026-07-04 · **Status:** ACCEPTED 2026-07-16 (owner) · **Amends:** DR-001 clean-room rule — tightens it; reverses nothing.

## Context

Concern raised: using Omnigent as a "donor" — even read-only — anchors rezidnt's design on Databricks' frame ("plan poisoning"). Assessment: **legal** contamination from Apache-2.0 source is near-zero absent copying (permissive license; no obligations attach to reading; patent exposure is orthogonal to reading, since independent invention is not a patent defense — confirm with counsel). **Cognitive** anchoring is a real mechanism, demonstrated by v0.1 inheriting herdr's terminal frame *without anyone reading herdr source* — the vector was the integration plan and product osmosis, not code access. The mitigation is sequencing and traceability, not ignorance.

## Rules

1. **Design-first.** No competitor source is opened until the corresponding rezidnt design is committed in writing. (Satisfied for fabric, traits, run substrate, and gate model as of v0.2 + DR-001.)
2. **Extraction-scoped reads.** Every competitor read begins with written questions and ends with a findings memo in `/intel/`. Memos feed the benchmark, the risk register, and positioning — never trait or ontology definitions directly.
3. **Traceable influence.** Any design change motivated by an intel memo requires its own DR citing that memo. Anchoring is made *auditable*, not pretended away.
4. **Copyleft unchanged.** herdr and all AGPL sources are never read for implementation purposes (DR-001 stands in full).
5. **Implementation oracles are primary sources.** Harness vendors' own documentation plus recorded-transcript contract tests. Omnigent code is never an implementation reference for any rezidnt component.
6. **Benchmark exception.** Installing, running, and scoring competitor binaries is unrestricted — a benchmark cannot be built blind, and black-box behavior is not source.
7. **Post-freeze gap-diff.** The single sanctioned design-adjacent use: after ontology v1 freezes, one scoped read of Omnigent's event/policy taxonomy to diff for *coverage gaps* ("do they handle a lifecycle fact we lack a subject for?"). Output is a memo per rule 2; additions require a DR per rule 3.

*Amendments to this record require DR-003.*
