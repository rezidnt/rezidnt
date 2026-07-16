# Contributing to rezidnt

## Ground rules

- **Licensing in**: contributions to `crates/*` are accepted under
  `MIT OR Apache-2.0`; contributions to `bins/*` and everything else under
  `Apache-2.0`. By submitting, you license your contribution accordingly.
- **DCO**: every commit must carry a `Signed-off-by` line
  (`git commit -s`) certifying the [Developer Certificate of
  Origin](https://developercertificate.org/). Unsigned commits are not
  merged.
- **Clean room (BINDING — see DR-001/DR-002 in `docs/rezidnt-architecture.md`)**:
  do not submit code derived from copyleft (AGPL) codebases, and do not
  port code from competitor products regardless of license. If prior art
  informed your design, say so in the PR so the influence is traceable.

## How changes land

The canonical design is `docs/rezidnt-architecture.md`; the eight
invariants (I1–I8) are non-negotiable and BINDING items change only via a
decision record. The development loop is oracle-first: acceptance criteria
become failing tests before implementation, and done means the verifier
gauntlet (`bash .claude/hooks/vet.sh`: rustfmt, clippy `-D warnings`, the
full test suite, golden-fixture replay) passes.

Practical checklist for a PR:

1. Tests first, or alongside — a change without a failing-test justification
   will be asked for one.
2. `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace` all clean.
3. No new dependencies without a written note in the manifest explaining why
   (the approved set is in the rust-conventions skill; every new dependency
   is attack surface).
4. `spec/ontology.md` is warden-gated: subject/payload changes go through
   the `/subject` flow, never direct edits.

## Style

Rust edition 2024. thiserror in libraries, anyhow in binaries, no
`unwrap`/`expect` outside tests, no blocking calls in async contexts,
newtype every id. Match the surrounding code.
