# rezidnt quickstart

rezidnt is a local-first resident daemon that runs, verifies, and audits a fleet of coding agents across git worktrees — one static binary that owns a typed event fabric, a supervisor, and a deterministic verifier gate engine. This walkthrough takes you from a cold machine to your first gated run with a single command, `rezidnt init`, editing zero config files. It is the narrated one-take golden-path demo; if you can read, you can run it.

The bar we hold ourselves to: one take, zero config edits, single-digit minutes.

## What you need

- `git` on your `PATH` (rezidnt allocates git worktrees).
- Your agent harness resolvable on `PATH` (the generated spec defaults to `claude-code`).
- On Windows: WSL2, where the daemon and substrates run.

`rezidnt init` runs an environment preflight for all of this before it does anything else, so you don't have to check by hand — but if a piece is missing it will tell you plainly.

## 1. Install

The golden path is a one-line install of two static binaries — no runtime dependencies, no telemetry. On Linux or Windows+WSL2:

```bash
curl -fsSL https://raw.githubusercontent.com/rezidnt/rezidnt/main/install.sh | sh
```

This fetches the newest release's static `x86_64-unknown-linux-musl` binaries, verifies them against their published `SHA256SUMS` **before** installing (it fails closed and installs nothing on a mismatch), and drops `rezidnt` and `rezidentd` in `~/.local/bin` (override with `REZIDNT_INSTALL_DIR`). It installs on Linux/WSL only — macOS and native Windows are not yet supported, and the installer refuses any other platform rather than half-installing. (A friendlier `rezidnt.dev/install.sh` vanity URL is planned; the raw GitHub URL above is the one that works today.)

rezidnt is pre-1.0: releases are marked **pre-release** (early access), and the installer resolves the newest one. The install step itself is real; the full one-take demo lands at the Phase-1 exit as the rest of the golden path is completed.

Prefer to build from source, or on a platform the installer doesn't cover? With a Rust toolchain:

```bash
cargo install --path bins/rezidnt
cargo install --path bins/rezidentd
```

Either way you end up with two binaries: `rezidnt` (the CLI you drive) and `rezidentd` (the daemon it talks to).

## 2. Start the daemon

`rezidnt init` will point the daemon at your repo, but it does not start the daemon for you. Bring it up first, in its own terminal, and leave it running:

```bash
rezidentd
```

This is the resident daemon — it owns the event fabric, supervises substrates and agents, and runs the gates. It renders nothing; every UI, including this CLI, is just a client of its socket.

## 3. Run it: `rezidnt init`

From the root of the repository you want rezidnt to work on, run the one command this whole flow is built around:

```bash
rezidnt init
```

`rezidnt init` chains three steps in-process:

1. **Preflight** — the same checks as `rezidnt doctor`: `git` present, your harness resolvable, the daemon socket/lockfile path writable, WSL2 reachable. A hard failure stops here and tells you what to fix; an inconclusive check is surfaced as a warning and the run proceeds (it is never silently coerced to a pass).
2. **Spec generation** — the same generator as `rezidnt spec init`: it prompts you for a project name, repo path, and one agent (name + harness), then writes a `rezidnt.toml` next to you. The golden path opens this file **untouched** — there is no config to edit. If a `rezidnt.toml` is already present it is left byte-for-byte unchanged and simply opened.
3. **Open** — it hands that spec to the running daemon, which allocates a git worktree and spawns the agent under its gates.

Want it fully non-interactive? Skip the prompts and write a minimal valid spec:

```bash
rezidnt init --defaults
```

To regenerate a spec that's already there, add `--force`.

On success `init` prints one line identifying the opened workspace and its run id (a ULID) — that run id is what the commands below take.

## 4. See the fleet

The daemon is now supervising a worktree with an agent running in it under gates. Watch the live event stream:

```bash
rezidnt tail
```

Or open the read-only fleet board for a rendered view of workspaces, agents, and runs (`q` or Ctrl-C quits):

```bash
rezidnt board
```

Both are pure clients — they only read the daemon's stream, they never write to it.

## 5. Your first gated run

As the agent works, its actions pass through the gate engine, and permit decisions and verifier verdicts are recorded to the log. To interrogate the gate blocking a run — the failing verifier and the exact recorded inputs behind it:

```bash
rezidnt gate why <run-ulid>
```

And to replay a run's recorded verdicts from the log and get the compliance verdict:

```bash
rezidnt debrief <run-ulid>
```

Add `--json` to either for a machine-readable answer. A verdict is `pass`, `fail`, or `inconclusive`, and an inconclusive is never reported as a pass — verifiers are deterministic and interrogable, so the same inputs always replay to the same answer.

That gate/permit decision is the finish line of this walkthrough: a first verified, gated run.

## What you just saw

From a cold machine you ran `curl`-style install → started the daemon → `rezidnt init` → and reached a first gated run. The `rezidnt.toml` the daemon opened was the one `init` generated, used untouched — you edited no config. That is the golden path, and the bar it holds is exactly this: one take, zero config edits, single-digit minutes.

From here, `rezidnt tail` and `rezidnt board` keep you watching the fleet, and `rezidnt gate why` and `rezidnt debrief` are how you hold every run to account.
