[← Decision records index](../rezidnt-architecture.md#20-decision-records) · [Architecture plan](../rezidnt-architecture.md)

# Decision Record DR-003 — Retire the employer IP memo standing gate

**Date:** 2026-07-16 · **Status:** ACCEPTED (owner) · **Amends:** the preamble standing gates and §17; removes the pushgate guardrail from the development harness. Reverses nothing in DR-001/DR-002.

## Decision

The repository-stays-private-until-the-employer-IP-memo-is-executed standing gate is retired in full. `git push` is no longer blocked; the repository may be made public at the owner's discretion. The name-registry standing gate (`rezident` fallback) is unchanged.

## Basis

Owner determination, 2026-07-16: the employer-IP question the gate guarded against is not a concern for this project. The maintainers raised the risk profile of removing the gate without an executed memo (publication is the irreversible step; an IP dispute surfacing post-publication is the expensive form) and the owner accepted it and directed removal. This record exists so the decision is dated and attributable rather than implicit.

## Mechanical changes

Preamble standing-gates line reduced to the name gate. §17's "(post-memo)" qualifier and its memo-pending exclusion clause deleted. Harness: `.claude/hooks/pushgate.sh` deleted, its PreToolUse registration removed from `.claude/settings.json`, the session-start gate notice dropped from `slice-status.sh`, and the README/CLAUDE.md guardrail references updated. The `.claude/state/ip-memo-cleared` marker mechanism ceases to exist.

*Amendments to this record require a further decision record.*
