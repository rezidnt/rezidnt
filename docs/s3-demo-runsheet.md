# Phase-1 exit demo — run-sheet (S3, one take, recorded)

The take must show, **via MCP only**: open a project → spawn an agent → read its
dossier → `gate_explain` a forced failure — plus the `attach` byte-proxy over the
socket. Everything below is copy-paste ready; do the pre-flight off camera, then
record the take section in one pass.

Terminals: **A** = WSL (`wsl.exe -d Ubuntu-24.04`), daemon. **B** = WSL, CLI attach.
**C** = Claude Code (Windows or WSL — WSL2 loopback forwarding covers both).
Repo path in WSL: `/mnt/d/github/rezidnt`.

---

## Pre-flight (off camera)

All of this in **A**:

```sh
cd /mnt/d/github/rezidnt
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/rezidnt-target"

# 1. Build everything the take touches (so nothing compiles on camera).
cargo build -p rezidentd -p rezidnt --example seed_fixture

# 2. Fresh demo state dir — db, socket, lockfile all live here.
DEMO="$HOME/rezidnt-demo"; rm -rf "$DEMO"; mkdir -p "$DEMO"

# 3. Seed the FORCED FAILURE onto the log before the daemon ever starts
#    (I3: the log is truth — the stub gate.failed verdict lives there and
#    nowhere else). The seeded run ULID is 01S3GATEFA1DED000000000R01.
"$CARGO_TARGET_DIR/debug/examples/seed_fixture" \
  "$DEMO/events.db" spec/fixtures/s3_gate_forced_failure.jsonl

# 4. A demo project: a real git repo + §13 spec.
mkdir -p "$DEMO/repo" && git -C "$DEMO/repo" init -q
cat > "$DEMO/rezidnt.toml" <<EOF
[project]
name = "phase1-exit"
repo = "$DEMO/repo"

[[agent]]
name = "impl"
harness = "claude-code"
worktree = "auto"
EOF
```

Harness choice: the spec above runs the **real** `claude` binary (golden path —
verify `claude --version` works in WSL first). If you need a bulletproof take,
add `bin_override = "$DEMO/harness.sh"` to the `[[agent]]` entry with the stub
script from `bins/rezidentd/tests/common/mod.rs:132` (three stream-json lines,
`chmod 755`) — the S1 exit demo precedent.

```sh
# 5. Start the daemon.
REZIDNT_DB="$DEMO/events.db" \
REZIDNT_SOCKET="$DEMO/rezidnt.sock" \
REZIDNT_MCP_LOCKFILE="$DEMO/mcp.lock" \
"$CARGO_TARGET_DIR/debug/rezidentd" &

# 6. Lockfile discovery — port + operator badge (0600, doc §12).
cat "$DEMO/mcp.lock"       # {"pid":…,"port":P,"url":"http://127.0.0.1:P/","badge":"<64-hex>"}
```

7. Register the endpoint with Claude Code (in **C**'s shell; substitute `P`):

```sh
claude mcp add --transport http rezidnt http://127.0.0.1:P/
```

---

## The take (on camera, one pass)

**Beat 1 — the surface is discovered, not configured.** Show `cat "$DEMO/mcp.lock"`
in A: port 0 was bound, the real port + badge announced here. In C, `/mcp` shows
the `rezidnt` server with four tools whose schemas are generated from
`rezidnt-types` (the no-drift rule).

**Beat 2 — MCP-only golden path.** Prompt Claude Code (one prompt is fine):

> Using only the rezidnt MCP tools: (1) call `open_project` with badge `<badge>`
> and the spec_toml from this file: `<paste $DEMO/rezidnt.toml contents>`.
> (2) `spawn_agent` in the returned workspace: agent `impl`, badge `<badge>`,
> idempotency_key `demo-take-1`. (3) `tail_events` until the run completes, then
> read the resource `rezidnt://run/<run-ulid>/dossier` and summarize it.
> (4) Call `gate_explain` for run `01S3GATEFA1DED000000000R01` and tell me which
> verifier failed, its evidence refs, and the exact inputs it was judged on.

What the viewer sees, mapped to the exit criteria:
- (a) `open_project` → **request-scoped ack**: workspace ULID + correlation, and
  `workspace.opened` on the log carries exactly that correlation.
- (b) `spawn_agent` → one `agent.spawned`; badge-gated (optionally show a call
  *without* the badge first: `badge.required`, log untouched).
- (c) dossier resource → the folded run state, derived from the log (I3).
- (d) `gate_explain` → verifier `tests-pass`, CAS evidence refs, the verbatim
  §8 stdin document — interrogability, not a refusal string. A `gate.explained`
  fact lands on the log (show it via `tail_events` or terminal B).

**Beat 3 — attach byte-proxy.** In **B**:

```sh
REZIDNT_SOCKET="$HOME/rezidnt-demo/rezidnt.sock" \
  "$HOME/.cache/rezidnt-target/debug/rezidnt" attach <run-ulid-from-beat-2>
```

Raw bytes replayed over the socket — the dtach model, no re-render. Optionally
show the error frame: `… attach 01ZZZZZZZZZZZZZZZZZZZZZZZZZZ` → one
machine-readable `run.unknown` frame, then close.

**Close.** `kill %1` is NOT part of the take; leave the daemon up. End on the
dossier or the gate_explain output.

## After the take
Save the recording (suggested: `docs/demo/phase1-exit.mp4` or a link note),
advance `.claude/state/current-slice` to S4, and record both in the handoff.
