//! `rezidnt` — the CLI.
//!
//! Verbs (doc §9):
//! - `rezidnt rebuild --db <path> --json` — refold from seq 0, print the
//!   `rezidnt_state::Graph` as JSON on stdout, exit 0. Pinned by
//!   `tests/rebuild_cli.rs`. Cross-platform.
//! - `rezidnt tail [--subject …]` — connect to the daemon socket, print the
//!   stream. Exercised by the S0 exit demo (two concurrent `rezidnt tail`
//!   clients), not by an automated oracle test — see the S0 work order.
//! - `rezidnt open <spec-path>` — materialize a workspace from a §13 spec
//!   file through the daemon; prints EXACTLY one stdout line
//!   `opened <workspace-name> run <run-ulid>` once `agent.spawned` is
//!   observed on the stream (the id is the fabric's, not decoration).
//!   Pinned by `bins/rezidentd/tests/cli_verbs.rs`.
//! - `rezidnt attach <run-ulid>` — replay the run's capture ring, then
//!   stream live bytes to stdout until EOF (dtach model). Pinned likewise.
//!
//! Stable exit codes (doc §9, ratified by DR-004): 0 ok, 1 unexpected
//! internal error, 2 local input/usage error, 3 substrate-fault (incl.
//! daemon-side refusals), 4 daemon-unreachable, 5 gate-fail (S4+; an
//! `inconclusive` verdict is 3, never coerced — I6). Mapping: `rebuild`
//! failures are substrate faults (the log store misbehaved) → 3;
//! `tail`/`attach` failures are daemon-side (unreachable socket, bad hello,
//! proto mismatch) → 4. `open`: a missing/unreadable/unparseable spec file
//! is a LOCAL input error → 2 (clap's usage-error convention, pinned by
//! cli_verbs.rs); a daemon-side open-failed refusal → 3; connection
//! failures → 4.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};

mod permit_hook;

#[derive(Parser)]
#[command(
    name = "rezidnt",
    version,
    about = "rezidnt CLI: rebuild, tail, open, attach"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Refold the event log from seq 0 and print the graph as JSON.
    Rebuild {
        /// Path to the event log (SQLite).
        #[arg(long)]
        db: PathBuf,
        /// Print compact JSON (default prints human-indented JSON).
        #[arg(long)]
        json: bool,
    },
    /// Connect to the daemon socket and print the event stream (JSONL).
    Tail {
        /// Only print events with exactly this subject.
        #[arg(long)]
        subject: Option<String>,
    },
    /// Read-only fleet board (ratatui). Connects to the daemon, tails the
    /// event stream (replay-from-seq-0 then live via the existing `tail` op),
    /// folds it into a `rezidnt_state::Graph`, and renders the fleet. Pure
    /// client — no daemon change, consumes only a watch channel (I1). `q` or
    /// Ctrl-C quits (read-only navigation).
    Board,
    /// Materialize a workspace from a project spec file (doc §13) and spawn
    /// its agents through the daemon.
    Open {
        /// Path to the project spec (rezidnt.toml shape).
        spec: PathBuf,
    },
    /// Replay a run's capture tail, then stream live bytes (dtach model).
    Attach {
        /// The run ULID, as printed by `rezidnt open`.
        run: String,
    },
    /// Run the `vet` gate over a project spec's governed agents (pre-spawn
    /// policy: bare-mode / pinned-version / allowed-tools). Exit 0 pass, 5
    /// fail, 3 inconclusive (DR-004).
    Vet {
        /// Path to the project spec (rezidnt.toml shape).
        spec: PathBuf,
        /// Emit a machine-readable verdict on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Replay a run's recorded verdicts from log + CAS and report the result
    /// (the compliance sentence, doc §8). Exit 0 all-pass, 5 gate-fail, 3
    /// inconclusive or integrity-alarm (DR-004; never coerced, I6).
    Debrief {
        /// The run ULID.
        run: String,
        /// Emit the machine-readable replay report on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Interrogate a run's gate verdicts (§9 interrogability).
    Gate {
        #[command(subcommand)]
        cmd: GateCmd,
    },
    /// Project-spec scaffolding (DR-036). Subcommands generate the §13
    /// `rezidnt.toml` the golden path opens. A pre-MCP bootstrap surface: the
    /// generator runs BEFORE the daemon exists (cold machine, no spec yet), so
    /// it is a legitimate plain-CLI exception to I5, not an eroded MCP-first
    /// default.
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// First-run wrapper (DR-036 sub-slice `init-wrapper`): chain
    /// `doctor -> spec init -> open` IN-PROCESS so a cold-machine operator reaches a
    /// first gated run with ONE command, zero config edits. Runs the environment
    /// preflight (gating on a `fail`, warning on an `inconclusive` — I6), generates
    /// `<DIR>/rezidnt.toml` (or leaves a present one byte-unchanged unless `--force`,
    /// the wrapper clobber nuance), then opens it. Invents NO new failure code — each
    /// step surfaces its own DR-004 class (doctor fail → 3, open refusal → 3, open
    /// unreachable → 4, clap usage → 2). NOT a new binary (I7).
    Init {
        /// Target directory; absent = the current working directory. The file
        /// written/opened is always `<DIR>/rezidnt.toml` (the §13 filename `open` reads).
        dir: Option<PathBuf>,
        /// Forwarded to the spec-init step: write the minimal valid spec with NO
        /// prompts (non-interactive).
        #[arg(long)]
        defaults: bool,
        /// Forwarded to the spec-init step: regenerate an existing `rezidnt.toml`.
        /// Without it, a present spec is left byte-unchanged and simply opened.
        #[arg(long)]
        force: bool,
    },
    /// Read-only environment preflight (DR-036 sub-slice `onboarding-doctor`).
    /// Checks the §11 golden-path substrate assumptions — `git` resolvable on
    /// PATH, the chosen agent harness resolvable on PATH, the daemon
    /// socket/lockfile path writable, WSL2 reachable — and prints per-check
    /// findings. A pure LOCAL, read-only preflight: it dials NO daemon, opens NO
    /// socket, makes NO network call, emits NO fabric fact, and sends NO
    /// telemetry (I3/I7). Each finding carries a status from the closed set
    /// {pass, inconclusive, fail}; an unprobeable/unsatisfiable check is NEVER
    /// coerced to pass (I6). DR-004 exits: 0 all-pass, 3 any non-pass (fail OR
    /// inconclusive — a substrate fault, the class `rebuild` uses; `doctor` is
    /// NOT a gate, so never 5).
    Doctor {
        /// Emit the machine-readable findings object on stdout
        /// (`{ "checks": [ { "name", "status" }, … ] }`).
        #[arg(long)]
        json: bool,
    },
    /// Operator-only actions (DR-031/DR-032): explicit operator authorization
    /// over the loopback-HTTP MCP surface, carrying the operator badge from the
    /// 0600 lockfile.
    Operator {
        #[command(subcommand)]
        cmd: OperatorCmd,
    },
    /// MCP over stdio for a local client (Claude Code) to spawn (§9 stdio
    /// transport; §16 S3 connection path). A thin stdio↔loopback-HTTP JSON-RPC
    /// PROXY to the resident daemon: it reads the 0600 lockfile (loopback port +
    /// operator badge), forwards each stdin JSON-RPC request to
    /// `127.0.0.1:<port>/mcp`, and relays the response to stdout. I3 forces this to
    /// be a PROXY — a Claude-spawned subprocess cannot own the daemon's single
    /// writer, so the surface itself stays resident in `rezidentd`. It injects the
    /// operator badge into MUTATING tool calls so the client never handles the
    /// token (§12; 0600 lockfile ⇒ possession = the local user). I7: reuses the
    /// hand-rolled `loopback_post` (no HTTP crate). Exits 4 (daemon-unreachable) if
    /// the lockfile is absent at startup; 0 on stdin EOF. Not a separate binary (I7).
    Mcp,
    /// The permit Policy Enforcement Point (DR-014 §Decision 1). claude-code's
    /// `PreToolUse` hook config invokes this: it reads the tool descriptor on
    /// stdin, asks the daemon PDP over `REZIDNT_SOCKET`, and writes the
    /// `hookSpecificOutput.permissionDecision` (`allow`/`deny`/`ask`) on stdout.
    /// Fails CLOSED to `ask` when the daemon is unreachable (never a silent
    /// proceed, I6). Not a separate binary (I7) — a subcommand of `rezidnt`.
    #[command(name = "permit-hook")]
    PermitHook,
}

#[derive(Subcommand)]
enum SpecCmd {
    /// Generate a §13 `rezidnt.toml` the golden path opens UNTOUCHED (DR-036
    /// sub-slice `spec-init`). Interactive by default (plain stdin/stdout line
    /// prompts, NOT a TUI — I1); `--defaults` writes a minimal single-agent spec
    /// with no prompts. A PURE local file writer: it dials no daemon and emits no
    /// fabric fact (I3). DR-004 exits: 0 written, 2 refused clobber (an existing
    /// `rezidnt.toml` without `--force` is a LOCAL input/usage error).
    Init {
        /// Target directory; absent = the current working directory. The file
        /// written is always `<DIR>/rezidnt.toml` (the §13 filename `open` reads).
        dir: Option<PathBuf>,
        /// Non-interactive: write a minimal valid single-agent spec, NO prompts.
        #[arg(long)]
        defaults: bool,
        /// Overwrite an existing `rezidnt.toml`. Without it, an existing file is
        /// left byte-unchanged and the command exits 2.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum OperatorCmd {
    /// Terminate a run (DR-032 §Decision 1): POST a `kill_run` `tools/call` over
    /// the loopback-HTTP MCP surface, carrying the operator badge from the 0600
    /// lockfile. DR-004 exits: 0 ok, 2 malformed run ULID (local input), 4
    /// daemon-unreachable, 5 tool-refused (the daemon refused the kill).
    #[command(name = "kill-run")]
    KillRun {
        /// The run ULID, as printed by `rezidnt open`.
        run: String,
    },
    /// Resolve a previously-escalated permit (DR-033 §Decision 1): POST a
    /// `resolve_permit` `tools/call` over the loopback-HTTP MCP surface, carrying
    /// the operator badge from the 0600 lockfile. The daemon records a
    /// `permit.resolved` fact the PDP applies on the agent's next ask. DR-004
    /// exits: 0 ok, 2 malformed run ULID / decision (local input), 4
    /// daemon-unreachable, 5 tool-refused (the daemon refused the resolve).
    #[command(name = "resolve-permit")]
    ResolvePermit {
        /// The run ULID the escalated permit belongs to.
        run: String,
        /// The escalated ask's request_id (the audit correlation this answers).
        request_id: String,
        /// The human decision: `allow` or `deny` (the closed two-value set).
        decision: String,
        /// Optional operator reason: rides the emitted permit.resolved fact.
        #[arg(long)]
        reason: Option<String>,
        /// Optional TTL (ms) time-boxing the resolution (DR-035 §Decision 1): the
        /// PDP applies it only while an incoming ask's envelope timestamp is at or
        /// before this resolution's own timestamp + `ttl_ms`; past that the ask
        /// re-escalates. Absent = permanent (today's behavior).
        #[arg(long = "ttl-ms")]
        ttl_ms: Option<u64>,
        /// Optional broad grant scope (DR-035 §Decision 2): `run_tool` makes the
        /// resolution match ANY action on its `(run, tool)` (tool stays exact),
        /// instead of the DR-033 exact request-scoped match. Absent = exact.
        /// Rides the tool call verbatim — the daemon owns the semantics
        /// (fail-closed on unknown values; a broad scope REQUIRES `--ttl-ms`, else
        /// the daemon refuses with `scope.requires_ttl` — broad OR permanent,
        /// never both).
        #[arg(long = "scope")]
        scope: Option<String>,
    },
}

#[derive(Subcommand)]
enum GateCmd {
    /// Return the failing verifier, evidence refs, and exact recorded inputs
    /// for a run's blocking gate. Exit 0 (the interrogation succeeded; the
    /// verdict rides the output, not the exit code).
    Why {
        /// The run ULID.
        run: String,
        /// Emit the machine-readable answer on stdout.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    // Per-verb stable failure class (doc §9); see module docs.
    let (failure_code, result) = match cli.cmd {
        Cmd::Rebuild { db, json } => (3, rebuild(&db, json)),
        Cmd::Tail { subject } => (4, tail(subject.as_deref())),
        Cmd::Board => (4, board()),
        Cmd::Open { spec } => {
            // Local input phase: exit 2 (see module docs for the /dr flag).
            let (name, spec_toml) = match read_spec(&spec) {
                Ok(parts) => parts,
                Err(e) => {
                    eprintln!("rezidnt: {e:#}");
                    std::process::exit(2);
                }
            };
            (4, open(&name, spec_toml))
        }
        Cmd::Attach { run } => {
            // A malformed run id is the same local-input class as a bad spec.
            let run = match run.parse::<ulid::Ulid>() {
                Ok(run) => run,
                Err(e) => {
                    eprintln!("rezidnt: run id {run:?} is not a ULID: {e}");
                    std::process::exit(2);
                }
            };
            (4, attach(run))
        }
        // The gate verbs own their own DR-004 exit codes (0/3/5) — they
        // std::process::exit internally rather than folding into the
        // failure-class table above.
        Cmd::Vet { spec, json } => (1, vet(&spec, json)),
        Cmd::Debrief { run, json } => (1, debrief(&run, json)),
        Cmd::Gate {
            cmd: GateCmd::Why { run, json },
        } => (1, gate_why(&run, json)),
        // `spec init` is a pure local file generator (DR-036): it dials no
        // daemon and emits no fact (I3). It OWNS its DR-004 exit codes and exits
        // internally (0 written, 2 refused clobber) — like the gate/operator
        // verbs — so a placeholder class here. An unexpected IO fault (e.g. the
        // target dir is unwritable) folds into 1 via the table.
        Cmd::Spec {
            cmd:
                SpecCmd::Init {
                    dir,
                    defaults,
                    force,
                },
        } => (1, spec_init(dir.as_deref(), defaults, force)),
        // `init` (DR-036 sub-slice `init-wrapper`) chains doctor -> spec init ->
        // open IN-PROCESS. Its terminal steps exit internally with their own DR-004
        // class (doctor fail → 3; open refusal → 3, INSIDE `open`), so the only
        // failure that folds through this table is the open step's connection
        // failure — an `Err` mapped to 4 (daemon-unreachable), matching `Open`'s
        // class. The wrapper invents no new code.
        Cmd::Init {
            dir,
            defaults,
            force,
        } => (4, init(dir.as_deref(), defaults, force)),
        // `doctor` is a read-only preflight (DR-036): it dials no daemon, opens
        // no socket, emits no fact (I3/I7). It OWNS its DR-004 exit codes and
        // exits internally (0 all-pass, 3 any non-pass — the substrate-fault
        // class `rebuild` uses; NEVER 5, doctor is not a gate) — like the
        // gate/operator verbs — so a placeholder class here. An unexpected
        // internal fault (e.g. stdout write) folds into 1 via the table.
        Cmd::Doctor { json } => (1, doctor(json)),
        Cmd::Operator {
            cmd: OperatorCmd::KillRun { run },
        } => {
            // Local input phase (DR-004): a malformed/absent run ULID is exit 2,
            // the same class `attach` gives a bad run id — rejected BEFORE any
            // daemon traffic. The subcommand then owns its own 0/4/5 mapping
            // (operator_kill_run exits internally), so a placeholder class here.
            let run = match run.parse::<ulid::Ulid>() {
                Ok(run) => run,
                Err(e) => {
                    eprintln!("rezidnt: run id {run:?} is not a ULID: {e}");
                    std::process::exit(2);
                }
            };
            (1, operator_kill_run(run))
        }
        Cmd::Operator {
            cmd:
                OperatorCmd::ResolvePermit {
                    run,
                    request_id,
                    decision,
                    reason,
                    ttl_ms,
                    scope,
                },
        } => {
            // Local input phase (DR-004): a malformed/absent run ULID and a
            // decision that is not `allow`/`deny` are BOTH exit 2 (the closed
            // two-value enum on the CLI edge), rejected BEFORE any daemon traffic.
            // The subcommand then owns its own 0/4/5 mapping (operator_resolve_permit
            // exits internally), so a placeholder class here.
            let run = match run.parse::<ulid::Ulid>() {
                Ok(run) => run,
                Err(e) => {
                    eprintln!("rezidnt: run id {run:?} is not a ULID: {e}");
                    std::process::exit(2);
                }
            };
            if decision != "allow" && decision != "deny" {
                eprintln!("rezidnt: decision {decision:?} is not allow|deny (DR-033 §Decision 1)");
                std::process::exit(2);
            }
            (
                1,
                operator_resolve_permit(
                    run,
                    &request_id,
                    &decision,
                    reason.as_deref(),
                    ttl_ms,
                    scope.as_deref(),
                ),
            )
        }
        // The PEP emits its decision on stdout and fails closed to `ask`
        // internally; a hard error here (unreadable stdin / stdout write) is an
        // unexpected internal fault → 1.
        Cmd::PermitHook => (1, permit_hook::run()),
        // `mcp` proxies stdio JSON-RPC to the daemon's loopback-HTTP MCP. It OWNS
        // its DR-004 exit codes and exits internally (4 daemon-unreachable at
        // startup, 0 on stdin EOF); an unexpected IO fault (stdin read / stdout
        // write) folds into 1 via the table.
        Cmd::Mcp => (1, mcp_serve()),
    };
    if let Err(e) = result {
        eprintln!("rezidnt: {e:#}");
        std::process::exit(failure_code);
    }
}

/// The §13 filename the golden path / `open` reads (DR-036: written into the
/// target dir, always this name).
const SPEC_FILENAME: &str = "rezidnt.toml";

/// The generated `[[agent]]` defaults (DR-036 Design): the S1 native harness and
/// the sole-allocator worktree model. No `bin_override` (the test/pin seam is not
/// part of a minimal operator spec).
const DEFAULT_HARNESS: &str = "claude-code";
const DEFAULT_WORKTREE: &str = "auto";

/// `rezidnt spec init [DIR]` — generate a §13 `rezidnt.toml` the golden path
/// opens UNTOUCHED (DR-036 sub-slice `spec-init`). PURE local file writer: it
/// dials no daemon and emits no fabric fact (I3). `--defaults` writes a minimal
/// single-agent spec with no prompts; otherwise it prompts plain-CLI stdin/stdout
/// lines (I1 — line prompts, NOT a TUI). Refuses to clobber an existing
/// `rezidnt.toml` without `--force`, leaving the file byte-unchanged and exiting 2
/// (DR-004 LOCAL input/usage). Owns its own exit codes (0/2) and exits internally.
fn spec_init(dir: Option<&Path>, defaults: bool, force: bool) -> anyhow::Result<()> {
    let target_dir = dir.unwrap_or_else(|| Path::new("."));
    let path = target_dir.join(SPEC_FILENAME);

    // Clobber guard (DR-004 exit 2): decide BEFORE writing a single byte so a
    // refused clobber leaves the existing file exactly as it was.
    if path.exists() && !force {
        eprintln!(
            "rezidnt: {} already exists; pass --force to overwrite (nothing written)",
            path.display()
        );
        std::process::exit(2);
    }

    generate_spec(&path, defaults)?;
    println!("wrote {}", path.display());
    std::process::exit(0);
}

/// Generate a §13 `rezidnt.toml` and write it to `path`, overwriting whatever is
/// there (the CALLER owns the clobber decision — bare `spec init` refuses at exit
/// 2, the `init` wrapper skips or forces). Non-interactive default under
/// `defaults`; otherwise plain-CLI stdin/stdout prompts (I1 — line prompts, NOT a
/// TUI). Re-parses the bytes through the REAL `ProjectSpec` before writing, so a
/// generator that drifts from what `open` accepts fails HERE, not at open. A pure
/// local file writer: it dials no daemon and emits no fabric fact (I3).
fn generate_spec(path: &Path, defaults: bool) -> anyhow::Result<()> {
    // Build the spec fields — non-interactive default, or plain-CLI prompts.
    let spec = if defaults {
        SpecFields::default()
    } else {
        prompt_spec_fields().context("read interactive spec answers")?
    };

    let toml = render_spec_toml(&spec);

    // Sanity pin (anti-drift, DR-036 §Consequences): the bytes we are about to
    // write MUST parse into the REAL daemon spec type. A generator that drifts
    // from what `open` accepts fails HERE, loudly, rather than writing a spec the
    // golden path would reject.
    rezidnt_run::spec::ProjectSpec::from_toml_str(&toml)
        .context("internal: generated spec does not parse into ProjectSpec (generator drift)")?;

    std::fs::write(path, &toml)
        .with_context(|| format!("write generated spec {}", path.display()))?;
    Ok(())
}

/// `rezidnt init [DIR] [--defaults] [--force]` — the thin wrapper that chains
/// `doctor -> spec init -> open` IN-PROCESS (DR-036 sub-slice `init-wrapper`), so a
/// cold-machine operator reaches a first gated run with one command. It CALLS the
/// internal step functions directly (no shelling out to the `rezidnt` binary, I7 —
/// one binary), REUSING the same check-runner, generator, and open path the bare
/// verbs use. It invents NO new failure code: every non-zero exit is a sub-step's
/// DR-004 class, and each terminal step exits internally (mirroring `doctor` and
/// the operator/gate verbs), so `main`'s fold-through code is never the surfaced
/// one on the wrapper's own aborts.
///
/// The chain and its exit discipline:
///   - doctor step: run the shared checks (`run_doctor_checks`). Any `Fail` →
///     print the failing finding(s) and ABORT with doctor's class (exit 3); no spec
///     is generated, `open` is never reached. Any `Inconclusive` (none failing) →
///     WARN on stderr and PROCEED (I6 posture: inconclusive is surfaced, never
///     coerced away, but it does not gate). All-pass → proceed.
///   - spec init step: if `<DIR>/rezidnt.toml` already exists AND NOT `--force` →
///     SKIP generation, leave the file BYTE-UNCHANGED, and PROCEED to open it (the
///     WRAPPER nuance — NOT bare `spec init`'s exit-2 clobber refusal). `--force` →
///     regenerate. No existing file → generate (interactive unless `--defaults`).
///   - open step: open `<DIR>/rezidnt.toml` via the existing `open` path, surfacing
///     its DR-004 classes UNCHANGED (daemon-side refusal → 3 internally; a
///     connection failure bubbles as `Err`, which this step maps to 4,
///     daemon-unreachable). On success the chain reaches a first gated run → exit 0.
fn init(dir: Option<&Path>, defaults: bool, force: bool) -> anyhow::Result<()> {
    let target_dir = dir.unwrap_or_else(|| Path::new("."));
    let path = target_dir.join(SPEC_FILENAME);

    // --- doctor step: gate on fail, warn on inconclusive (I6). ---
    let checks = run_doctor_checks();
    let failing: Vec<&Check> = checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Fail))
        .collect();
    if !failing.is_empty() {
        for c in &failing {
            eprintln!(
                "rezidnt init: doctor check `{}` failed: {}",
                c.name, c.detail
            );
        }
        eprintln!("rezidnt init: environment preflight failed; not generating a spec or opening");
        // Surface doctor's DR-004 class (3) unchanged — no spec written, no open.
        std::process::exit(3);
    }
    for c in checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Inconclusive))
    {
        eprintln!(
            "rezidnt init: warning — doctor check `{}` inconclusive: {} (proceeding)",
            c.name, c.detail
        );
    }

    // --- spec init step: generate, or SKIP a present spec (wrapper clobber nuance). ---
    if path.exists() && !force {
        // The WRAPPER skips a present spec (byte-unchanged) and proceeds to open —
        // it does NOT reproduce bare `spec init`'s exit-2 clobber refusal.
        println!(
            "rezidnt init: {} already exists; leaving it unchanged and opening it (pass --force to regenerate)",
            path.display()
        );
    } else {
        generate_spec(&path, defaults)?;
        println!("rezidnt init: wrote {}", path.display());
    }

    // --- open step: reuse the existing open path; surface its DR-004 classes. ---
    // A daemon-side refusal exits 3 INSIDE `open`; a connection failure bubbles as
    // `Err` here, which the caller in `main` maps to 4 (daemon-unreachable).
    let (name, spec_toml) = read_spec(&path)
        .with_context(|| format!("read the spec `init` is about to open ({})", path.display()))?;
    open(&name, spec_toml)
}

/// The §13 fields the generator emits: a `[project]` (name + repo) and one
/// `[[agent]]` (name + harness + worktree). The minimal single-agent shape
/// DR-036 Design pins; no `bin_override`.
struct SpecFields {
    project_name: String,
    repo: String,
    agent_name: String,
    harness: String,
    worktree: String,
}

impl Default for SpecFields {
    fn default() -> Self {
        Self {
            project_name: "rezidnt-project".to_string(),
            repo: ".".to_string(),
            agent_name: "impl".to_string(),
            harness: DEFAULT_HARNESS.to_string(),
            worktree: DEFAULT_WORKTREE.to_string(),
        }
    }
}

/// Prompt the §13 fields on plain stdin/stdout (I1 — line prompts, NOT a TUI).
/// One prompt per line, in a fixed deterministic order (project name, repo, agent
/// name, harness); a blank answer (or EOF) accepts the default. `worktree` is not
/// prompted — it is the sole-allocator `auto` model (DR-036 Design). The flow is
/// simple and never blocks past the fields it reads.
fn prompt_spec_fields() -> anyhow::Result<SpecFields> {
    use std::io::{BufRead, Write};

    let d = SpecFields::default();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    // Read one answer, falling back to `default` on a blank line or EOF.
    let mut ask = |label: &str, default: &str| -> anyhow::Result<String> {
        let mut out = std::io::stdout();
        write!(out, "{label} [{default}]: ").context("write prompt")?;
        out.flush().context("flush prompt")?;
        match lines.next() {
            Some(line) => {
                let line = line.context("read answer line")?;
                let trimmed = line.trim();
                Ok(if trimmed.is_empty() {
                    default.to_string()
                } else {
                    trimmed.to_string()
                })
            }
            None => Ok(default.to_string()), // EOF: accept the default
        }
    };

    let project_name = ask("project name", &d.project_name)?;
    let repo = ask("repo path", &d.repo)?;
    let agent_name = ask("agent name", &d.agent_name)?;
    let harness = ask("agent harness", &d.harness)?;

    Ok(SpecFields {
        project_name,
        repo,
        agent_name,
        harness,
        worktree: d.worktree,
    })
}

/// Render the §13 TOML from the collected fields. Hand-written (not
/// `toml::to_string(&ProjectSpec)`) because the parser reads `[[agent]]` tables
/// (`RawSpec.agent`) while `ProjectSpec` serializes an `agents` field — a direct
/// serialize would drift from the parser. `spec_init` re-parses the output through
/// the REAL `ProjectSpec::from_toml_str` before writing, so this shape is pinned
/// to the consumer, not to a §13 snapshot.
fn render_spec_toml(f: &SpecFields) -> String {
    // Minimal TOML basic-string quoting (mirrors spec::agent_spec_toml): the
    // values are short identifiers / paths — `\` → `\\` then `"` → `\"`.
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    format!(
        "# rezidnt project spec (§13) — generated by `rezidnt spec init` (DR-036).\n\
         # The golden path opens this file untouched.\n\
         [project]\n\
         name = {name}\n\
         repo = {repo}\n\
         \n\
         [[agent]]\n\
         name = {agent}\n\
         harness = {harness}\n\
         worktree = {worktree}\n",
        name = q(&f.project_name),
        repo = q(&f.repo),
        agent = q(&f.agent_name),
        harness = q(&f.harness),
        worktree = q(&f.worktree),
    )
}

// ===========================================================================
// `rezidnt doctor` (DR-036 sub-slice `onboarding-doctor`) — a read-only §11
// environment preflight. NO daemon dial, NO socket, NO network, NO fabric fact,
// NO telemetry (I3/I7). Each check yields a status from the CLOSED set below; an
// unprobeable/unsatisfiable check is NEVER coerced to pass (I6).
// ===========================================================================

/// A single preflight finding. `status` is drawn from the closed honesty set
/// {pass, inconclusive, fail} — there is no fourth value, and an unknown check
/// is `inconclusive`, never a coerced `pass` (I6).
#[derive(Clone, Copy)]
enum CheckStatus {
    Pass,
    Inconclusive,
    Fail,
}

impl CheckStatus {
    /// The pinned wire string (matched by the oracle). Lowercase, closed set.
    fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Inconclusive => "inconclusive",
            CheckStatus::Fail => "fail",
        }
    }
}

/// A named preflight finding: `name` (the check label the oracle matches by
/// case-insensitive substring), `status`, and a human `detail` line (the
/// human-mode message; not part of the pinned JSON status contract).
struct Check {
    name: &'static str,
    status: CheckStatus,
    detail: String,
}

/// Produce the §11 golden-path preflight findings — the shared check-runner both
/// `doctor` (which prints + exits on them) and the `init` wrapper (which gates on
/// them) consume, so the wrapper sees exactly the findings `doctor` would. Pure
/// and read-only: probes PATH and the filesystem only, dials no daemon, emits no
/// fact (I3/I7).
fn run_doctor_checks() -> Vec<Check> {
    vec![
        check_git(),
        check_harness(),
        check_socket_writable(),
        check_wsl(),
    ]
}

/// `rezidnt doctor [--json]` — run the §11 golden-path preflight, print findings
/// (JSON object `{ "checks": [ { "name", "status" }, … ] }` under `--json`, one
/// human line per check otherwise), and exit per DR-004: 0 when every check
/// passes, 3 when any check is non-pass (fail OR inconclusive — the substrate
/// fault class `rebuild` uses; `doctor` is not a gate, so never 5). A non-pass is
/// never coerced toward 0/pass (I6). This owns its exit codes and exits
/// internally. Read-only and daemon-free: it probes PATH and the filesystem only.
fn doctor(as_json: bool) -> anyhow::Result<()> {
    let checks = run_doctor_checks();

    // DR-004 exit class: clean preflight (every check pass) → 0; any non-pass
    // (fail OR inconclusive) → 3. An inconclusive is never coerced toward 0/pass
    // (I6); `doctor` is not a gate, so 5 is never emitted.
    let all_pass = checks.iter().all(|c| matches!(c.status, CheckStatus::Pass));
    let code = if all_pass { 0 } else { 3 };

    if as_json {
        let out = serde_json::json!({
            "checks": checks
                .iter()
                .map(|c| serde_json::json!({
                    "name": c.name,
                    "status": c.status.as_str(),
                    "detail": c.detail,
                }))
                .collect::<Vec<_>>(),
        });
        println!("{out}");
    } else {
        for c in &checks {
            println!("[{}] {}: {}", c.status.as_str(), c.name, c.detail);
        }
    }

    std::process::exit(code);
}

/// `git` present/resolvable on PATH (§11 line 252 — git worktrees are the repo
/// substrate). A pure PATH-directory scan (no subprocess, side-effect-free):
/// resolvable → pass; not on PATH → fail (a required substrate is absent). This
/// discriminates deterministically off the injected `PATH` seam.
fn check_git() -> Check {
    match resolve_on_path("git") {
        Some(dir) => Check {
            name: "git",
            status: CheckStatus::Pass,
            detail: format!("git resolvable on PATH ({})", dir.display()),
        },
        None => Check {
            name: "git",
            status: CheckStatus::Fail,
            detail: "git is not resolvable on PATH (the golden path allocates git \
                     worktrees, §11)"
                .to_string(),
        },
    }
}

/// The chosen agent harness resolvable on PATH (§11 — agents run under capture).
/// The generated spec's default harness is `claude-code` (DEFAULT_HARNESS); this
/// probes for it on PATH. Resolvable → pass; absent → inconclusive (the operator
/// may run a differently-named harness, so a missing default bin is UNKNOWN, not
/// a hard fault — but never coerced to pass, I6).
fn check_harness() -> Check {
    match resolve_on_path(DEFAULT_HARNESS) {
        Some(dir) => Check {
            name: "harness",
            status: CheckStatus::Pass,
            detail: format!(
                "agent harness `{DEFAULT_HARNESS}` resolvable on PATH ({})",
                dir.display()
            ),
        },
        None => Check {
            name: "harness",
            status: CheckStatus::Inconclusive,
            detail: format!(
                "agent harness `{DEFAULT_HARNESS}` not resolvable on PATH — set up the \
                 chosen harness before a governed run (unknown, not a hard fault)"
            ),
        },
    }
}

/// The daemon socket/lockfile transport PATH is writable (§11 line 240): can the
/// daemon create its socket/lockfile there? Reads the EXISTING `REZIDNT_SOCKET` /
/// `REZIDNT_LOCKFILE` env vars (the same seams `lockfile_path()` honors) and
/// probes the PARENT directory's writability with a temp-file create+remove — a
/// filesystem probe ONLY, it NEVER binds or connects the socket (I3/criterion 3),
/// and NEVER probes a hardcoded XDG path (the env seams must force both legs).
/// Writable parent → pass; missing/read-only parent → fail (I6: an unwritable
/// path is never coerced to pass).
fn check_socket_writable() -> Check {
    // The daemon creates its lockfile at REZIDNT_LOCKFILE (the path this CLI's
    // `lockfile_path()` actually honors) and its socket at REZIDNT_SOCKET; the
    // transport DIRECTORY whose writability the daemon needs is the parent of the
    // path it will create there. Prefer REZIDNT_LOCKFILE (the lockfile is the path
    // this process is authoritative about); fall back to REZIDNT_SOCKET. In the
    // usual case both point into the same transport dir; where they diverge (e.g. a
    // caller passing a dead REZIDNT_SOCKET purely as an unreachable dial target) the
    // lockfile parent is the honest writability probe.
    let target = std::env::var_os("REZIDNT_LOCKFILE")
        .or_else(|| std::env::var_os("REZIDNT_SOCKET"))
        .map(PathBuf::from);

    let Some(target) = target else {
        // Neither seam set and no daemon to ask: the transport path is UNKNOWN,
        // not writable — inconclusive, never coerced to pass (I6).
        return Check {
            name: "socket-writable",
            status: CheckStatus::Inconclusive,
            detail: "neither REZIDNT_SOCKET nor REZIDNT_LOCKFILE is set — the daemon \
                     socket/lockfile path is unknown (cannot probe writability)"
                .to_string(),
        };
    };

    let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Check {
            name: "socket-writable",
            status: CheckStatus::Fail,
            detail: format!(
                "socket/lockfile path {} has no parent directory to write into",
                target.display()
            ),
        };
    };

    match probe_dir_writable(parent) {
        true => Check {
            name: "socket-writable",
            status: CheckStatus::Pass,
            detail: format!(
                "daemon socket/lockfile parent {} is writable",
                parent.display()
            ),
        },
        false => Check {
            name: "socket-writable",
            status: CheckStatus::Fail,
            detail: format!(
                "daemon socket/lockfile parent {} is not writable (the daemon could not \
                 create its transport there)",
                parent.display()
            ),
        },
    }
}

/// WSL2 reachable (§11 line 252 — the daemon+substrates run in WSL2). This is the
/// inherently environment-dependent, honest inconclusive-capable check: on Linux
/// (incl. WSL) we can positively observe the kernel is Microsoft-flavored (a
/// WSL2 kernel names itself so); off Linux the reachability of a WSL2 backend is
/// genuinely UNPROBEABLE from here without dialing something, so it is
/// `inconclusive` — NEVER coerced to pass (I6). Read-only, no subprocess.
fn check_wsl() -> Check {
    // On a WSL2 guest the kernel release string contains "microsoft" (or "WSL").
    // Reading /proc/sys/kernel/osrelease is a pure filesystem read (no subprocess).
    // Off Linux (host Windows/macOS) the reachability of a WSL2 backend cannot be
    // determined without dialing it — inconclusive, never coerced to pass (I6).
    #[cfg(target_os = "linux")]
    let status = wsl_status_linux();
    #[cfg(not(target_os = "linux"))]
    let status = (
        CheckStatus::Inconclusive,
        "WSL2 reachability is not probeable from this host without dialing the WSL \
         backend (read-only preflight)"
            .to_string(),
    );

    let (status, detail) = status;
    Check {
        name: "wsl",
        status,
        detail,
    }
}

/// The Linux leg of the WSL2 check: read the kernel release (a pure filesystem
/// read, no subprocess) and classify. A Microsoft/WSL-flavored kernel → pass;
/// native Linux → inconclusive (the daemon runs here, but WSL2-backend
/// reachability is not probeable from a preflight); an unreadable release →
/// inconclusive. Never coerced to pass (I6).
#[cfg(target_os = "linux")]
fn wsl_status_linux() -> (CheckStatus, String) {
    match std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        Ok(release) => {
            let lc = release.to_lowercase();
            if lc.contains("microsoft") || lc.contains("wsl") {
                (
                    CheckStatus::Pass,
                    format!("running under a WSL2 kernel ({})", release.trim()),
                )
            } else {
                (
                    CheckStatus::Inconclusive,
                    format!(
                        "native Linux kernel ({}) — not a WSL2 guest; WSL2 reachability is \
                         not probeable from here",
                        release.trim()
                    ),
                )
            }
        }
        Err(e) => (
            CheckStatus::Inconclusive,
            format!("could not read kernel release to detect WSL2: {e}"),
        ),
    }
}

/// Resolve the directory containing `bin` on the current `PATH`, or None. A pure
/// filesystem walk of `PATH` (no subprocess) — mirrors the oracle's `which_dir`.
/// On Windows also tries the `PATHEXT`-style executable suffixes so a bare name
/// resolves. Side-effect-free and read-only.
fn resolve_on_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat", ".com"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(dir);
            }
        }
    }
    None
}

/// Probe whether `dir` is a writable directory by creating and removing a
/// uniquely-named temp file inside it. A filesystem probe ONLY — it binds/connects
/// nothing. Returns false when the dir does not exist or the create fails
/// (read-only / missing parent). The probe file is best-effort removed.
fn probe_dir_writable(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    // A unique probe name so concurrent doctors never collide.
    let probe = dir.join(format!(".rezidnt-doctor-probe-{}", ulid::Ulid::new()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Read and parse the spec file locally: the success line needs
/// `[project].name`, and a spec that cannot be read or parsed should fail
/// fast with the offending path on stderr, before any daemon traffic.
fn read_spec(path: &Path) -> anyhow::Result<(String, String)> {
    let spec_toml = std::fs::read_to_string(path)
        .with_context(|| format!("read spec file {}", path.display()))?;
    let spec = rezidnt_run::spec::ProjectSpec::from_toml_str(&spec_toml)
        .with_context(|| format!("parse spec file {}", path.display()))?;
    Ok((spec.name, spec_toml))
}

/// `rebuild` = fold(log from seq 0); the log is truth, the graph is derived (I3).
fn rebuild(db: &std::path::Path, compact: bool) -> anyhow::Result<()> {
    use anyhow::Context;

    let log = rezidnt_fabric::EventLog::open(db)
        .with_context(|| format!("open event log {}", db.display()))?;
    let rows = log.read_from(1).context("read log from seq 1")?;
    let events: Vec<rezidnt_types::Event> = rows.into_iter().map(|r| r.event).collect();
    let graph = rezidnt_state::fold(events.iter());
    let out = if compact {
        serde_json::to_string(&graph)?
    } else {
        serde_json::to_string_pretty(&graph)?
    };
    println!("{out}");
    Ok(())
}

/// Unix socket client plumbing shared by `tail`/`open`/`attach`/`debrief`:
/// connect, consume + check the hello, send the request line, hand back the
/// reader. Relocated to `rezidnt-client` (DR-023) — this thin wrapper preserves
/// the CLI's `anyhow` edge (the shared client returns its own `ClientError`,
/// which `?` folds into `anyhow` unchanged behavior).
#[cfg(unix)]
fn connect_and_request(
    request: &rezidnt_proto::Request,
) -> anyhow::Result<std::io::BufReader<std::os::unix::net::UnixStream>> {
    Ok(rezidnt_client::connect_and_request(request)?)
}

#[cfg(unix)]
fn tail(subject: Option<&str>) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};

    use rezidnt_types::Event;

    // Explicit tail request (server-side subject filter) skips the daemon's
    // S0 back-compat silence window.
    let mut reader = connect_and_request(&rezidnt_proto::Request::Tail {
        subject: subject.map(String::from),
    })?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).context("read event")?;
        if n == 0 {
            return Ok(()); // daemon closed the stream
        }
        let frame = line.trim_end();
        if frame.is_empty() {
            continue;
        }
        // Validating decode (I2 applies on the wire too), then print verbatim.
        let event = Event::from_json_line(frame).context("decode event frame")?;
        if subject.is_some_and(|want| event.subject.as_str() != want) {
            continue;
        }
        writeln!(out, "{frame}")?;
    }
}

/// `board`: the read-only fleet board (S5, I1). A PURE socket client — it
/// rides the EXISTING `Request::Tail { subject: None }` op (replay-from-seq-0
/// then live), folds each event into a `rezidnt_state::Graph`, publishes each
/// snapshot on a `tokio::sync::watch<Graph>`, and renders the fleet from the
/// watch channel ONLY (never a raw Event — that is the I1 render-side proof).
/// No daemon change, no new proto op.
///
/// Two adapter tasks (each spanned): an INGEST task owns the blocking socket
/// reader and does the fold+publish; a RENDER task drives crossterm's raw-mode
/// terminal from the watch receiver. `q`/Esc/Ctrl-C quit (read-only
/// navigation, not a control-plane action). The terminal is restored on every
/// normal `Ok`/`Err` return of `render_loop`; a panic inside the draw/poll
/// closure would unwind past teardown (no Drop guard), but the process is
/// exiting at that point.
#[cfg(unix)]
fn board() -> anyhow::Result<()> {
    use rezidnt_state::Graph;
    use tokio::sync::watch;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build board runtime")?;

    runtime.block_on(async {
        // Connect on a blocking thread: `connect_and_request` returns a
        // blocking std socket reader (no async socket work in this pure
        // client — the daemon is untouched).
        let reader = tokio::task::spawn_blocking(|| {
            connect_and_request(&rezidnt_proto::Request::Tail { subject: None })
        })
        .await
        .context("join board connect task")??;

        let (tx, rx) = watch::channel(Graph::default());

        // INGEST adapter task: own the blocking reader, fold every event into
        // a running Graph, publish each snapshot on the watch sender. Runs on
        // the blocking pool (the socket read is blocking I/O).
        let ingest = tokio::task::spawn_blocking(move || {
            let span = tracing::info_span!("adapter", kind = "board-ingest");
            let _enter = span.enter();
            ingest_loop(reader, &tx)
        });

        // RENDER adapter task: drive the terminal from the watch receiver
        // ONLY. Also blocking (terminal I/O + crossterm event poll).
        let render = tokio::task::spawn_blocking(move || {
            let span = tracing::info_span!("adapter", kind = "board-render");
            let _enter = span.enter();
            render_loop(rx)
        });

        // The render loop owns the exit signal (user quit). When it returns,
        // drop its handle and abort the ingest task; the terminal is already
        // restored inside `render_loop`.
        let render_result = render.await.context("join board render task")?;
        ingest.abort();
        // Surface an ingest error only if the render side did not already fail
        // (a clean quit makes ingest's aborted/closed-channel exit expected).
        render_result
    })
}

/// The ingest side: read JSONL event frames off the blocking socket reader,
/// fold each into the watch-published Graph. Returns when the daemon closes
/// the stream or the watch receiver is gone (the board quit).
#[cfg(unix)]
fn ingest_loop(
    mut reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    tx: &tokio::sync::watch::Sender<rezidnt_state::Graph>,
) -> anyhow::Result<()> {
    use std::io::BufRead;

    use rezidnt_types::Event;

    let mut graph = rezidnt_state::Graph::default();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).context("read event frame")?;
        if n == 0 {
            return Ok(()); // daemon closed the stream
        }
        let frame = line.trim_end();
        if frame.is_empty() {
            continue;
        }
        // Validating decode (I2 applies on the wire too), then fold into the
        // derived Graph and publish the snapshot. The render loop reads state,
        // never this Event.
        let event = Event::from_json_line(frame).context("decode event frame")?;
        rezidnt_state::apply(&mut graph, &event);
        if tx.send(graph.clone()).is_err() {
            return Ok(()); // render side gone: board quit
        }
    }
}

/// The render side: crossterm raw-mode terminal, redraw from the watch
/// receiver on every change, poll for a quit key. ALWAYS restores the terminal
/// before returning (clean teardown on quit, EOF, or error).
#[cfg(unix)]
fn render_loop(mut rx: tokio::sync::watch::Receiver<rezidnt_state::Graph>) -> anyhow::Result<()> {
    use std::time::Duration;

    use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers};
    use rezidnt_tui::{draw, project};

    let mut terminal = setup_terminal().context("enter raw-mode terminal")?;

    // Run the loop and guarantee teardown regardless of how it ends.
    let outcome = (|| -> anyhow::Result<()> {
        loop {
            // Draw the current fleet state (the watch snapshot, projected —
            // never a raw Event).
            let view = project(&rx.borrow());
            terminal
                .draw(|frame| draw(frame, &view))
                .context("draw board frame")?;

            // Interleave: wait briefly for a quit key; if none, check whether
            // the watch published a fresh snapshot and redraw.
            if event::poll(Duration::from_millis(100)).context("poll terminal input")?
                && let CtEvent::Key(key) = event::read().context("read terminal input")?
                && key.kind == KeyEventKind::Press
            {
                let quit = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL));
                if quit {
                    return Ok(());
                }
            }

            // Non-blocking check for a new snapshot; also detects the ingest
            // side closing (daemon stream ended).
            match rx.has_changed() {
                Ok(true) => {
                    rx.borrow_and_update();
                }
                Ok(false) => {}
                Err(_) => return Ok(()), // sender gone: stream ended
            }
        }
    })();

    // Teardown is unconditional — never leave the terminal raw.
    restore_terminal(&mut terminal);
    outcome
}

/// Enter the alternate screen in raw mode and hand back a ratatui terminal.
#[cfg(unix)]
fn setup_terminal()
-> anyhow::Result<ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>> {
    use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
    use ratatui::Terminal;
    use ratatui::backend::CrosstermBackend;

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("construct terminal")
}

/// Best-effort terminal restore: leave raw mode and the alternate screen. Runs
/// on every exit path; failures are logged, never propagated (a teardown error
/// must not mask the real outcome, and we are exiting anyway).
#[cfg(unix)]
fn restore_terminal(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) {
    use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};

    if let Err(e) = disable_raw_mode() {
        tracing::warn!(error = %e, "board: failed to disable raw mode on teardown");
    }
    if let Err(e) = crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen) {
        tracing::warn!(error = %e, "board: failed to leave alternate screen on teardown");
    }
    if let Err(e) = terminal.show_cursor() {
        tracing::warn!(error = %e, "board: failed to restore cursor on teardown");
    }
}

/// `open`: send the spec, read the S3 request-scoped ack (`open_ok` with the
/// workspace + correlation, or a machine-readable error frame), then watch
/// the stream for THIS open's facts and print the pinned one-line identity
/// once `agent.spawned` lands on the acked correlation.
///
/// The marker ULID (minted client-side before the request) still guards the
/// `daemon.warning` arm against replayed history: warnings carry their own
/// correlation, so time-ordering (id > marker) is what scopes them to this
/// open.
#[cfg(unix)]
fn open(name: &str, spec_toml: String) -> anyhow::Result<()> {
    use std::io::BufRead;

    use rezidnt_types::Event;

    let marker = ulid::Ulid::new();
    let mut reader = connect_and_request(&rezidnt_proto::Request::Open { spec_toml })?;

    // S3: the daemon acks the request FIRST (rezidnt_proto::Reply) — the
    // acked correlation is the one every materialization fact carries, so
    // the marker/name inference of the S1 client is gone.
    let mut ack_line = String::new();
    loop {
        ack_line.clear();
        let n = reader.read_line(&mut ack_line).context("read open ack")?;
        if n == 0 {
            anyhow::bail!("daemon closed the stream before acking the open");
        }
        if !ack_line.trim().is_empty() {
            break;
        }
    }
    let correlation =
        match rezidnt_proto::decode_reply(ack_line.trim_end()).context("decode open ack frame")? {
            rezidnt_proto::Reply::OpenOk { correlation, .. } => correlation,
            rezidnt_proto::Reply::Error { code, message, .. } => {
                // Daemon-side refusal: substrate fault class (§9 → 3).
                eprintln!("rezidnt: open refused ({code}): {message}");
                std::process::exit(3);
            }
            other => anyhow::bail!("unexpected reply to open: {other:?}"),
        };

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).context("read event")?;
        if n == 0 {
            anyhow::bail!("daemon closed the stream before the open completed");
        }
        let frame = line.trim_end();
        if frame.is_empty() {
            continue;
        }
        let event = Event::from_json_line(frame).context("decode event frame")?;
        if event.id <= marker {
            continue; // replayed history from before this open
        }
        match event.subject.as_str() {
            "agent.spawned" if event.correlation == correlation => {
                let run = event.payload()["run"]
                    .as_str()
                    .context("agent.spawned payload carries no run id")?;
                // The pinned output shape (cli_verbs.rs): exactly one line.
                println!("opened {name} run {run}");
                return Ok(());
            }
            "daemon.warning" if event.payload()["what"] == "open-failed" => {
                // Daemon-side refusal: substrate fault class (§9 → 3).
                eprintln!(
                    "rezidnt: open failed: {}",
                    event.payload()["error"].as_str().unwrap_or("(no detail)")
                );
                std::process::exit(3);
            }
            _ => {}
        }
    }
}

/// `attach`: raw byte copy of the daemon's replay-then-live capture stream
/// to stdout until EOF (dtach model — no TTY work, no decoding).
#[cfg(unix)]
fn attach(run: ulid::Ulid) -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let mut reader = connect_and_request(&rezidnt_proto::Request::Attach { run })?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).context("read capture bytes")?;
        if n == 0 {
            return Ok(()); // run finished (or daemon closed): EOF
        }
        out.write_all(&buf[..n]).context("write capture bytes")?;
        out.flush().context("flush capture bytes")?;
    }
}

/// Route the computed replay-divergence alarms to the daemon's single writer
/// (DR-006, I3): connect, send `record_alarms`, and BLOCK on the ack. The
/// daemon appends each new `integrity.alarm` fact through its Fabric (dedup by
/// (run, gate, verifier) off the log) and acks only once the append is
/// durable — so the caller's report/exit stay correct and the fact is on the
/// log by the time this returns. A daemon-side failure surfaces as a
/// machine-readable error frame.
#[cfg(unix)]
fn record_alarms(alarms: &[rezidnt_gate::IntegrityAlarm]) -> anyhow::Result<()> {
    use std::io::BufRead;

    let records: Vec<rezidnt_proto::AlarmRecord> = alarms
        .iter()
        .map(|a| rezidnt_proto::AlarmRecord {
            run: a.run.clone(),
            gate: a.gate.clone(),
            verifier: a.verifier.clone(),
            recorded: verdict_str(a.recorded).to_string(),
            replayed: verdict_str(a.replayed).to_string(),
        })
        .collect();

    let mut reader =
        connect_and_request(&rezidnt_proto::Request::RecordAlarms { alarms: records })?;

    // The daemon's FIRST frame after the hello answers this request: the
    // AlarmsRecorded ack (append durable) or a machine-readable error.
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .context("read record-alarms ack")?;
        if n == 0 {
            anyhow::bail!("daemon closed the stream before acking record_alarms");
        }
        if line.trim().is_empty() {
            continue;
        }
        return match rezidnt_proto::decode_reply(line.trim_end())
            .context("decode record-alarms ack")?
        {
            rezidnt_proto::Reply::AlarmsRecorded { .. } => Ok(()),
            rezidnt_proto::Reply::Error { code, message, .. } => {
                anyhow::bail!("daemon refused record_alarms ({code}): {message}")
            }
            other => anyhow::bail!("unexpected reply to record_alarms: {other:?}"),
        };
    }
}

#[cfg(not(unix))]
fn record_alarms(_alarms: &[rezidnt_gate::IntegrityAlarm]) -> anyhow::Result<()> {
    anyhow::bail!(
        "recording integrity alarms speaks a Unix domain socket only; the Windows \
         named pipe (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

/// `REZIDNT_DB` override, else `~/.local/state/rezidnt/events.db` (mirrors the
/// daemon's `db_path`). The gate verbs read the log directly (the CLI is a
/// client, I1/I5).
fn db_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("REZIDNT_DB") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local")
        .join("state")
        .join("rezidnt")
        .join("events.db")
}

/// The default CAS root (used by the `permit-hook` PEP to pin a bulky
/// `tool_input` as a `context_ref`, I2): `REZIDNT_CAS` override, else `cas/`
/// next to the default log path. Mirrors [`cas_path`] over [`db_path`].
/// Unix-only: only the UDS `permit_hook::ask_daemon` pins bulk context.
#[cfg(unix)]
pub(crate) fn cas_dir() -> PathBuf {
    cas_path(&db_path())
}

/// `REZIDNT_CAS` override, else `cas/` next to the log (mirrors the daemon's
/// `cas_path`). Replay is log + CAS, nothing else (I3).
fn cas_path(db: &Path) -> PathBuf {
    if let Some(explicit) = std::env::var_os("REZIDNT_CAS") {
        return PathBuf::from(explicit);
    }
    db.parent()
        .map(|dir| dir.join("cas"))
        .unwrap_or_else(|| PathBuf::from("cas"))
}

/// Read every event from the log (seq 0). The gate verbs fold/scan this.
fn read_log_events(db: &Path) -> anyhow::Result<Vec<rezidnt_types::Event>> {
    let log = rezidnt_fabric::EventLog::open(db)
        .with_context(|| format!("open event log {}", db.display()))?;
    let rows = log.read_from(1).context("read log from seq 1")?;
    Ok(rows.into_iter().map(|r| r.event).collect())
}

/// `rezidnt vet <spec>`: run the three vet natives over each governed agent's
/// pinned spec blob. DR-004 exits: 5 fail, 3 inconclusive, 0 pass. The verdict
/// rides `--json` stdout verbatim (I6).
fn vet(spec_path: &Path, as_json: bool) -> anyhow::Result<()> {
    use rezidnt_gate::{
        AllowedTools, BareMode, NativeVerifier, PinnedVersion, Verdict, VerifierInput,
    };

    let spec_toml = std::fs::read_to_string(spec_path)
        .with_context(|| format!("read spec file {}", spec_path.display()))?;
    let spec = rezidnt_run::spec::ProjectSpec::from_toml_str(&spec_toml)
        .with_context(|| format!("parse spec file {}", spec_path.display()))?;

    let db = db_path();
    let cas_root = cas_path(&db);
    let cas = rezidnt_cas::Cas::open(&cas_root)
        .with_context(|| format!("open cas {}", cas_root.display()))?;

    let natives: Vec<(&str, Box<dyn NativeVerifier>)> = vec![
        ("bare-mode", Box::new(BareMode)),
        ("pinned-version", Box::new(PinnedVersion)),
        ("allowed-tools", Box::new(AllowedTools)),
    ];

    // Vet every agent whose gates name `vet` (a bare spec vets all agents).
    let governed: Vec<&rezidnt_run::spec::AgentSpec> = spec
        .agents
        .iter()
        .filter(|a| a.gates.is_empty() || a.gates.iter().any(|g| g == "vet"))
        .collect();

    for agent in governed {
        let blob = rezidnt_run::spec::agent_spec_toml(agent);
        let cas_ref = cas
            .put(blob.as_bytes(), "application/toml")
            .context("pin spec")?;
        let input = VerifierInput {
            gate: "vet".to_string(),
            workspace: None,
            refs: std::collections::BTreeMap::from([(
                "spec".to_string(),
                format!("cas:blake3:{}", cas_ref.hash),
            )]),
            params: serde_json::json!({}),
            timeout_ms: rezidnt_gate::DEFAULT_TIMEOUT_MS,
        };
        for (name, native) in &natives {
            let out = native.verify(&input, &cas).context("vet native")?;
            match out.verdict {
                Verdict::Pass => {}
                // `emit_verdict` diverges (`-> !`): it prints and exits.
                Verdict::Fail => emit_verdict(as_json, "fail", Some(name), 5),
                Verdict::Inconclusive => emit_verdict(as_json, "inconclusive", Some(name), 3),
            }
        }
    }
    emit_verdict(as_json, "pass", None, 0)
}

/// Print a `{verdict, verifier?}` object (or a plain line) and exit with the
/// DR-004 code — the verdict rides the output, the code rides the class.
fn emit_verdict(as_json: bool, verdict: &str, verifier: Option<&str>, code: i32) -> ! {
    if as_json {
        let mut obj = serde_json::json!({"verdict": verdict});
        if let Some(v) = verifier {
            obj["verifier"] = serde_json::json!(v);
        }
        println!("{obj}");
    } else {
        match verifier {
            Some(v) => println!("{verdict} ({v})"),
            None => println!("{verdict}"),
        }
    }
    std::process::exit(code);
}

/// `rezidnt debrief <run>`: replay the run's recorded verdicts from log + CAS
/// (rezidnt-gate::replay), then report `{alarms, gates, cost}` and exit per
/// DR-004: 3 if any integrity alarm or any inconclusive verdict (never
/// coerced), else 5 if any fail, else 0.
fn debrief(run: &str, as_json: bool) -> anyhow::Result<()> {
    let db = db_path();
    let cas_root = cas_path(&db);
    let cas = rezidnt_cas::Cas::open(&cas_root)
        .with_context(|| format!("open cas {}", cas_root.display()))?;
    let events = read_log_events(&db)?;

    // Replay is over the whole log; scope to this run's gate facts.
    let run_events: Vec<rezidnt_types::Event> = events
        .iter()
        .filter(|e| e.payload()["run"].as_str() == Some(run))
        .cloned()
        .collect();
    let report = rezidnt_gate::replay(&run_events, &cas).context("replay run verdicts")?;

    // The gate verdicts as recorded on the log (folded state), for the report.
    let graph = rezidnt_state::fold(events.iter());
    let gates = graph
        .agent_runs
        .get(run)
        .map(|r| &r.gates)
        .cloned()
        .unwrap_or_default();
    let cost = graph
        .agent_runs
        .get(run)
        .map(|r| {
            serde_json::json!({
                "total_usd": r.total_usd,
                "input_tokens": r.input_tokens,
                "output_tokens": r.output_tokens,
            })
        })
        .unwrap_or_else(|| serde_json::json!({}));

    // DR-004 exit class: an integrity alarm or any inconclusive is 3 (neither
    // trusted nor coerced, I6); else any recorded fail is 5; else 0.
    let has_inconclusive = gates.values().any(|g| g.verdict == "inconclusive");
    let has_fail = gates.values().any(|g| g.verdict == "fail");
    let code = if !report.alarms.is_empty() || has_inconclusive {
        3
    } else if has_fail {
        5
    } else {
        0
    };

    let alarms: Vec<serde_json::Value> = report
        .alarms
        .iter()
        .map(|a| {
            serde_json::json!({
                "verifier": a.verifier,
                "recorded": verdict_str(a.recorded),
                "replayed": verdict_str(a.replayed),
            })
        })
        .collect();

    // The divergence VERDICT is computed above from the CLI's own local log +
    // CAS read; it is the primary signal and is printed FIRST, before any
    // durable-append attempt. Printing before appending is what keeps the
    // finding alive when the daemon is unreachable (DR-006 daemon-down
    // complement): the additive audit improvement must never destroy the
    // finding it decorates.
    if as_json {
        let out = serde_json::json!({
            "run": run,
            "alarms": alarms,
            "gates": gates,
            "cost": cost,
        });
        println!("{out}");
    } else {
        println!("debrief {run}: {} alarm(s)", alarms.len());
    }

    // DR-006: a divergence must land a DURABLE `integrity.alarm` fact on the
    // log. The CLI keeps its direct READ (the report above) but routes the
    // APPEND through the daemon's single writer (I3) — never a second writer
    // racing the append-only log. The daemon dedups by (run, gate, verifier)
    // off the log, so re-running debrief appends nothing new.
    //
    // The append is BEST-EFFORT (DR-006 daemon-down complement, auditor FAIL
    // remediation): it decorates the already-printed finding with durability,
    // so its failure degrades LOUDLY on stderr but does NOT propagate — the
    // exit class stays the DR-004 divergence code (3, computed above) whether
    // or not the daemon was reachable. A hard `?` here would misclassify a real
    // divergence as a catch-all crash (main()'s Debrief failure class is 1) and
    // suppress the report. When the daemon IS up the append lands durably and
    // the fact is on the log before this returns; only the warning differs.
    if !report.alarms.is_empty()
        && let Err(e) = record_alarms(&report.alarms)
    {
        eprintln!(
            "rezidnt: WARNING: integrity alarm(s) found but NOT durably recorded — \
             the daemon was unreachable (append via the single log writer failed): {e:#}"
        );
    }

    std::process::exit(code);
}

fn verdict_str(v: rezidnt_gate::Verdict) -> &'static str {
    match v {
        rezidnt_gate::Verdict::Pass => "pass",
        rezidnt_gate::Verdict::Fail => "fail",
        rezidnt_gate::Verdict::Inconclusive => "inconclusive",
    }
}

/// `rezidnt gate why <run>`: return the failing verifier, evidence refs, and
/// EXACT recorded inputs from the run's blocking gate fact (§9). Exit 0 — the
/// interrogation succeeded; the recorded verdict rides the output verbatim.
fn gate_why(run: &str, as_json: bool) -> anyhow::Result<()> {
    let db = db_path();
    let events = read_log_events(&db)?;

    // The blocking gate fact: the most recent gate.failed / gate.inconclusive
    // for this run (the verdict that blocked it). gate.passed does not block.
    let blocker = events.iter().rfind(|e| {
        e.payload()["run"].as_str() == Some(run)
            && matches!(e.subject.as_str(), "gate.failed" | "gate.inconclusive")
    });

    let Some(event) = blocker else {
        anyhow::bail!("no blocking gate fact recorded for run {run}");
    };
    let verdict = match event.subject.as_str() {
        "gate.failed" => "fail",
        _ => "inconclusive",
    };
    let payload = event.payload();
    if as_json {
        let out = serde_json::json!({
            "run": run,
            "gate": payload["gate"],
            "verdict": verdict,
            "verifier": payload["verifier"],
            "evidence": payload["evidence"],
            "inputs": payload["inputs"],
        });
        println!("{out}");
    } else {
        println!(
            "run {run} blocked at gate {} by {} ({verdict})",
            payload["gate"], payload["verifier"]
        );
    }
    std::process::exit(0);
}

/// The operator lockfile path: `REZIDNT_LOCKFILE` override (the test uses it),
/// else the XDG default `~/.local/state/rezidnt/mcp.lock` (next to the log,
/// mirroring [`db_path`]'s state dir). The daemon announces the loopback-HTTP
/// port + operator badge here (the same 0600 lockfile `serve_http` writes).
fn lockfile_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("REZIDNT_LOCKFILE") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local")
        .join("state")
        .join("rezidnt")
        .join("mcp.lock")
}

/// `rezidnt operator kill-run <run>` (DR-032 §Decision 3). Reads the 0600
/// lockfile (port + operator badge), then POSTs a `kill_run` `tools/call` over
/// the loopback-HTTP MCP surface — NOT the bare socket (DR-032 §Decision 2: the
/// socket's UDS-identity would bypass the explicit operator authorization
/// DR-031 requires). Carries the operator badge from the lockfile.
///
/// This function OWNS its DR-004 exit codes (0/4/5) and exits internally
/// (mirroring the gate verbs): 0 ok, 4 daemon-unreachable (no lockfile / a dead
/// port), 5 tool-refused (the daemon refused the kill). The malformed-run input
/// class (exit 2) is handled by the caller before this runs. The run ULID is
/// already validated; it is passed as text on the wire.
///
/// I7: the loopback POST is a minimal hand-rolled HTTP/1.1 exchange over
/// `std::net::TcpStream` — no HTTP crate is pulled in (one static binary, no new
/// attack surface). The transport is loopback-only and lockfile-gated.
fn operator_kill_run(run: ulid::Ulid) -> anyhow::Result<()> {
    let path = lockfile_path();
    // No lockfile / unreadable / unparseable ⇒ daemon-unreachable (exit 4). A
    // client cannot reach a daemon it cannot locate (the class `tail`/`attach`
    // use for an unreachable socket).
    let lock = match rezidnt_mcp::lockfile::read(&path) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: cannot read operator lockfile {}: {e}",
                path.display()
            );
            std::process::exit(4);
        }
    };

    // The JSON-RPC tools/call for kill_run, carrying the operator badge (§12)
    // and the run. The badge TOKEN rides only the request body to the loopback
    // daemon — never printed, never logged (§12/I2).
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "kill_run",
            "arguments": {
                "badge": lock.badge,
                "run": run.to_string(),
            },
        },
    });

    // Dial the loopback port and POST. Any connect/IO failure is
    // daemon-unreachable (exit 4): a lockfile pointing at a dead port is exactly
    // the unreachable class.
    let response = match loopback_post(lock.port, &request.to_string()) {
        Ok(body) => body,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: kill_run POST to loopback:{} failed: {e:#}",
                lock.port
            );
            std::process::exit(4);
        }
    };

    // A JSON-RPC error object (protocol misuse) or an unparseable body is a
    // daemon-side fault surfacing on the kill path — treat as tool-refused (the
    // kill did not happen). A tool result with `isError: true` is the daemon
    // REFUSING the kill (badge rejected, run not live) ⇒ exit 5.
    let parsed: serde_json::Value = match serde_json::from_str(&response) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rezidnt: kill_run refused: unparseable daemon response ({e})");
            std::process::exit(5);
        }
    };
    if let Some(err) = parsed.get("error") {
        eprintln!("rezidnt: kill_run refused: {err}");
        std::process::exit(5);
    }
    let result = &parsed["result"];
    if result["isError"] == serde_json::json!(true) {
        let detail = result["content"][0]["text"]
            .as_str()
            .unwrap_or("(no detail)");
        eprintln!("rezidnt: kill_run refused: {detail}");
        std::process::exit(5);
    }
    println!("killed run {run}");
    std::process::exit(0);
}

/// `rezidnt operator resolve-permit <run> <request_id> <allow|deny> [--reason …]`
/// (DR-033 §Decision 1). Reads the 0600 lockfile (port + operator badge), then
/// POSTs a `resolve_permit` `tools/call` over the loopback-HTTP MCP surface — NOT
/// the bare socket (DR-032 §Decision 2: the socket's UDS-identity would bypass the
/// explicit operator authorization DR-031 requires). The daemon records a
/// `permit.resolved` fact the PDP applies on the agent's NEXT ask for the same
/// action.
///
/// This function OWNS its DR-004 exit codes (0/4/5) and exits internally
/// (mirroring `operator_kill_run`): 0 ok, 4 daemon-unreachable (no lockfile / a
/// dead port), 5 tool-refused (the daemon refused the resolve). The malformed run
/// / decision input class (exit 2) is handled by the caller before this runs. The
/// run ULID is already validated; `<allow|deny>` maps to the tool's `decision`
/// arg. The client sends only `{ badge, run, request_id, decision, reason?,
/// ttl_ms?, scope? }`: the daemon DERIVES the escalation's `action`/`target` from
/// the folded log by `request_id` (DR-033 §Design) — the CLI never fabricates a
/// descriptor, and passes `ttl_ms`/`scope` through verbatim (the daemon owns
/// their semantics and the DR-035 coupling guard).
///
/// I7: the loopback POST reuses the same minimal hand-rolled HTTP/1.1 exchange as
/// `operator_kill_run` ([`loopback_post`]) — no HTTP crate is pulled in.
fn operator_resolve_permit(
    run: ulid::Ulid,
    request_id: &str,
    decision: &str,
    reason: Option<&str>,
    ttl_ms: Option<u64>,
    scope: Option<&str>,
) -> anyhow::Result<()> {
    let path = lockfile_path();
    // No lockfile / unreadable / unparseable ⇒ daemon-unreachable (exit 4).
    let lock = match rezidnt_mcp::lockfile::read(&path) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: cannot read operator lockfile {}: {e}",
                path.display()
            );
            std::process::exit(4);
        }
    };

    // The JSON-RPC tools/call for resolve_permit, carrying the operator badge
    // (§12) and the resolution args. The badge TOKEN rides only the request body
    // to the loopback daemon — never printed, never logged (§12/I2).
    // The client sends only the trimmed shape { badge, run, request_id,
    // decision, reason? }: the operator supplies NO action/target — the DAEMON
    // DERIVES them from the log by request_id (DR-033 §Design). A hardcoded
    // action/target here was the /debrief FAIL (the fabricated empty target broke
    // the PDP action-identity match).
    let mut arguments = serde_json::json!({
        "badge": lock.badge,
        "run": run.to_string(),
        "request_id": request_id,
        "decision": decision,
    });
    if let (Some(reason), Some(obj)) = (reason, arguments.as_object_mut()) {
        obj.insert("reason".to_string(), serde_json::json!(reason));
    }
    // DR-035 §Decision 1: the optional TTL rides the trimmed shape when the
    // operator time-boxes the resolution; absent = permanent (today's behavior).
    if let (Some(ttl_ms), Some(obj)) = (ttl_ms, arguments.as_object_mut()) {
        obj.insert("ttl_ms".to_string(), serde_json::json!(ttl_ms));
    }
    // DR-035 §Decision 2: the optional broad scope rides the trimmed shape
    // VERBATIM — the client passes it through, the daemon owns the semantics
    // (fail-closed on unknown values; the coupling guard refuses broad-and-
    // permanent). Absent = OMITTED (never null) = DR-033 exact request-scoped
    // match. Mirrors the ttl_ms insert above.
    if let (Some(scope), Some(obj)) = (scope, arguments.as_object_mut()) {
        obj.insert("scope".to_string(), serde_json::json!(scope));
    }
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "resolve_permit", "arguments": arguments },
    });

    // Dial the loopback port and POST. Any connect/IO failure is
    // daemon-unreachable (exit 4): a lockfile pointing at a dead port is exactly
    // the unreachable class.
    let response = match loopback_post(lock.port, &request.to_string()) {
        Ok(body) => body,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: resolve_permit POST to loopback:{} failed: {e:#}",
                lock.port
            );
            std::process::exit(4);
        }
    };

    // A JSON-RPC error object (protocol misuse) / unparseable body ⇒ tool-refused
    // (the resolve did not happen). A tool result with `isError: true` is the
    // daemon REFUSING the resolve (badge rejected) ⇒ exit 5.
    let parsed: serde_json::Value = match serde_json::from_str(&response) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rezidnt: resolve_permit refused: unparseable daemon response ({e})");
            std::process::exit(5);
        }
    };
    if let Some(err) = parsed.get("error") {
        eprintln!("rezidnt: resolve_permit refused: {err}");
        std::process::exit(5);
    }
    let result = &parsed["result"];
    if result["isError"] == serde_json::json!(true) {
        let detail = result["content"][0]["text"]
            .as_str()
            .unwrap_or("(no detail)");
        eprintln!("rezidnt: resolve_permit refused: {detail}");
        std::process::exit(5);
    }
    println!("resolved permit for run {run} ({decision})");
    std::process::exit(0);
}

/// The MUTATING MCP tools — a `tools/call` for one of these needs a badge (§12).
/// The proxy injects the operator badge for these (unless the caller supplied one);
/// read-class tools (`gate_explain`, `tail_events`) and non-`tools/call` methods pass
/// through untouched. The safety comes from scoping injection to THIS set — not from
/// any daemon-side rejection of a stray field (the daemon reads args off a JSON value
/// and would silently ignore an unknown `badge`), so the proxy must never add one to a
/// read-class call in the first place.
const MUTATING_MCP_TOOLS: &[&str] = &[
    "open_project",
    "spawn_agent",
    "kill_run",
    "resolve_permit",
    "request_permission",
];

/// Inject the operator `badge` into a `tools/call` for a MUTATING tool that did not
/// already carry one, so a local client (Claude Code) never handles the token (§12).
/// Everything else is returned unchanged. An unparseable line is the caller's to
/// forward verbatim (the daemon judges it), signalled by an `Err`.
fn inject_operator_badge(line: &str, badge: &str) -> anyhow::Result<String> {
    let mut v: serde_json::Value = serde_json::from_str(line)?;
    if v.get("method").and_then(|m| m.as_str()) == Some("tools/call") {
        let name = v["params"]["name"].as_str().unwrap_or_default();
        if MUTATING_MCP_TOOLS.contains(&name) {
            let args = &mut v["params"]["arguments"];
            if args.is_null() {
                *args = serde_json::json!({});
            }
            if let Some(obj) = args.as_object_mut() {
                obj.entry("badge")
                    .or_insert_with(|| serde_json::Value::String(badge.to_string()));
            }
        }
    }
    Ok(serde_json::to_string(&v)?)
}

/// `rezidnt mcp` (§16 S3 / §9 stdio transport) — the stdio↔loopback-HTTP JSON-RPC
/// PROXY. Reads the 0600 lockfile for the daemon's loopback port + operator badge,
/// then serves line-delimited JSON-RPC on stdio: each request is forwarded to the
/// daemon's `/mcp` (operator badge injected for mutating tools, §12), the response
/// relayed to stdout. Fails closed to exit 4 (daemon-unreachable) if the lockfile is
/// unreadable at startup. A mid-session loopback failure becomes a JSON-RPC error for
/// that request id (fail-closed, but keeps serving — the daemon may return). Reuses
/// `loopback_post` (I7 — no HTTP crate); cross-platform (loopback TCP + stdio).
fn mcp_serve() -> anyhow::Result<()> {
    use std::io::{BufRead as _, Write as _};

    let path = lockfile_path();
    let lock = match rezidnt_mcp::lockfile::read(&path) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "rezidnt: daemon unreachable: cannot read MCP lockfile {}: {e}",
                path.display()
            );
            std::process::exit(4);
        }
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("read stdin JSON-RPC line")?;
        if line.trim().is_empty() {
            continue;
        }
        // Inject the operator badge into mutating calls; an unparseable line is
        // forwarded verbatim (the daemon returns the JSON-RPC parse error).
        let forward = inject_operator_badge(&line, &lock.badge).unwrap_or_else(|_| line.clone());
        match loopback_post(lock.port, &forward) {
            Ok(body) => {
                let body = body.trim();
                // A notification (no id) draws no response body; relay only non-empty.
                if !body.is_empty() {
                    writeln!(stdout, "{body}").context("write JSON-RPC response")?;
                    stdout.flush().context("flush JSON-RPC response")?;
                }
            }
            Err(e) => {
                // Mid-session daemon loss: answer this request with a JSON-RPC error
                // (fail-closed) but keep serving. A notification (no id) gets nothing.
                let id = serde_json::from_str::<serde_json::Value>(&line)
                    .ok()
                    .and_then(|v| v.get("id").cloned())
                    .unwrap_or(serde_json::Value::Null);
                if !id.is_null() {
                    let err = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32000,
                            "message": format!("rezidnt daemon unreachable: {e:#}"),
                        },
                    });
                    writeln!(stdout, "{err}").context("write JSON-RPC error")?;
                    stdout.flush().ok();
                }
            }
        }
    }
    std::process::exit(0);
}

/// Minimal hand-rolled loopback HTTP/1.1 POST to `127.0.0.1:<port>/mcp` (I7 — no
/// HTTP crate). Writes the request, reads the full response, and returns the
/// body (the bytes after the `\r\n\r\n` head/body split). Loopback-only; the
/// port comes from the 0600 lockfile.
fn loopback_post(port: u16, body: &str) -> anyhow::Result<String> {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("connect loopback:{port}"))?;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .context("write kill_run request")?;
    stream.flush().context("flush kill_run request")?;

    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .context("read kill_run response")?;
    // Split off the HTTP head; the body is the JSON-RPC response. A response
    // with no head/body split is a malformed daemon reply (surfaced by the
    // caller as a refusal).
    match raw.split_once("\r\n\r\n") {
        Some((_head, body)) => Ok(body.to_string()),
        None => anyhow::bail!("daemon response had no HTTP head/body split"),
    }
}

#[cfg(not(unix))]
fn tail(_subject: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt tail speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

#[cfg(not(unix))]
fn board() -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt board speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

#[cfg(not(unix))]
fn open(_name: &str, _spec_toml: String) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt open speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}

#[cfg(not(unix))]
fn attach(_run: ulid::Ulid) -> anyhow::Result<()> {
    anyhow::bail!(
        "rezidnt attach speaks a Unix domain socket only; the Windows named pipe \
         (\\\\.\\pipe\\rezidnt, doc §9) is designed but not yet implemented"
    )
}
