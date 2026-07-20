# Handoff — 2026-07-20 (session 10: SP4c shipped end-to-end; SP4 COMPLETE)

## State of play
Pointer = **SP4**. **SP4 is now COMPLETE** — SP4a (roles) + SP4b (macaroon delegation) + **SP4c
(C8 layered precedence) all DONE**. SP4c closed the full loop this session: `/dr` (DR-019 ratified
by owner) → `/oracle` → implementer → `/vet` **pass** → `/debrief` **PASS**. **Committed + pushed to
origin/main as `4b97d4f`; tree clean.** The `current-slice` file still reads SP4 — **advancing it is an
explicit owner action** (SP4 has no more sub-slices; next slice is owner's call — see candidates below).

## What shipped this session (commit `4b97d4f`, pushed)
- **DR-019 (ACCEPTED 2026-07-20):** C8 layered precedence composes admin/dev/session by **concatenate-
  ordered merge** (admin→dev→session) in `permit_config_for`. **Stricter-wins is INHERITED from the
  unchanged monotone aggregate** — no allow-override primitive exists, so a later layer can't un-Fail an
  earlier Fail. Lattice rejected as premature. Settles the sketch §5 deferred composition question.
- **SP4c core (`crates/rezidnt-gate/src/permit.rs`, +165/−22):** `PermitLayer{Admin,Dev,Session}`,
  `compose_layers(admin,dev,session)`, per-spec `layer` provenance (`native_in_layer`/`exec_in_layer`;
  existing `native`/`exec` default to **Session** = least-authority), and `deciding_layer:
  Option<PermitLayer>` threaded through all 8 `PermitOutcome` sites (`None` only for the empty-set
  escalate). **Aggregate + verdict→decision table UNCHANGED** (DR-019 Decision 1 — auditor confirmed).
- **Oracle tests (`tests/permit_layered_precedence.rs`, 8 green):** admin-deny-non-overridable, layer
  provenance surfaced (×2), concat-order + `later_layer_cannot_un_fail`, monotonicity proptest
  (`prop_assume!(!base.is_empty())` — excludes only the empty-base sentinel, not a weakening), empty→
  escalate (×2). Docs: §20 index row + next-record=DR-020; sketch §5 "settled by DR-019" pointer.

## Next action — SP4 done; owner picks the next slice (candidates, most-leverage first)
1. **SP-wire live config-dispatch `/dr`** (HIGH leverage) — the daemon three-source wiring
   (`McpCore::with_layered_permit_config` / live `permit_config_for`) is a pre-existing `/dr`-gated seam.
   Ratifying it unblocks **both** `permit_wire_dispatch.rs` (SP-intent live residual) **and** the deferred
   `permit_layered_live.rs` (C8 live e2e), and lets the emit path pin `deciding_layer` on the decision
   fact so `gate_explain` surfaces the layer on the wire (auditor's forward note; latent until then).
2. **SP5** — warden `/subject` completion for `permit.*` + folding reducers (roadmap `permit-engine.md:143`).
3. Advance the `current-slice` pointer if starting a new numbered slice.

## Open /debrief residuals (none blocking; from DR-019 PASS)
- **`deciding_layer` not yet on the wire** — the type carries it, but the MCP emit path (`lib.rs:846`)
  pins only `deciding_verifier`. Surfacing the layer through `request_permission`/`gate_explain` is the
  deferred live-wiring slice (candidate 1 above), not a defect. Tracked in the gate test-file header note.
- **Live C8 e2e deferred** — `permit_layered_live.rs` removed this session (its `with_layered_permit_config`
  builder is the gated seam); intent preserved verbatim as a DEFERRED block in the gate test header.

## Decisions still needing a /dr
- **SP-wire live config-dispatch seam** (candidate 1) · **Holder-offline attenuation sub-slice** (DR-018 §b)
  · **decision fast-path cache** (permit §10.2, I3 pressure) · **concrete OPA/Cedar adapter** · **C3 —
  sole-chokepoint enforcement** (DR-009 fenced; own sketch + DR).
- Carried pre-permit debt: DR-007 GitError→associated-type; `badge.issued` emitter / `badge_id` on other
  mutations; release items (root README, crates.io `cargo login`); Phase 3 demand-gated.

## Environment
WSL = `wsl.exe -d Ubuntu-24.04`, cargo `~/.cargo/bin`, `CARGO_TARGET_DIR=$HOME/.cache/rezidnt-target`
**(WSL-ONLY)**. Vet hook host-side (`bash .claude/hooks/vet.sh`). **Host vet.sh + WSL workspace SEQUENTIAL,
never concurrent** ([[vet-concurrency-flake]] — hit once this session: a transient `test;` fail on the
exec-verifier spawn tests, cleared on sequential re-run). **`/vet` is host-side; WSL-green is NOT
sufficient** ([[vet-is-host-side-wsl-insufficient]] — hit this session: clippy `doc-lazy-continuation` in a
test header passed WSL but failed host `-D warnings`; fixed). Host test/bin names avoid substring `update`
(UAC 740). Auto-push to `main` classifier-gated — ask first (owner approved this session's push).

---
**NEXT ACTION → SP4 is COMPLETE and pushed (`4b97d4f`). No unblocked build work remains in SP4. Owner
picks the next slice: most-leverage is a `/dr` for the SP-wire live config-dispatch seam (unblocks the two
`#[ignore]`d live suites + puts `deciding_layer` on the wire); alternative is SP5 (`permit.*` /subject +
reducers). Advancing `current-slice` past SP4 is an explicit owner action.**
