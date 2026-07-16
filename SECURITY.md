# Security policy

## Reporting a vulnerability

Report suspected vulnerabilities privately via
[GitHub Security Advisories](https://github.com/rezidnt/rezidnt/security/advisories/new)
— do not open a public issue for an unpatched vulnerability.

You should receive an acknowledgment within a week. Please include a
reproduction path; the event log's replayability usually makes a minimal
fixture the fastest form of proof.

## Scope and threat model

The daemon's trust boundary is one user on one machine (architecture doc
§12): the socket is owner-only (0600), agent runs carry per-run badge
tokens, the fabric runs an ingress redaction pass, and the event log is
hash-chained for tamper evidence. Explicitly out of scope for now, as
documented: hostile local root, malicious verifier binaries installed by
the operator themselves, and multi-tenant isolation.

## Supported versions

Pre-1.0: only the latest release line receives fixes.
