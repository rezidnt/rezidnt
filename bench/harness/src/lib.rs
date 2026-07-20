//! rezidnt public benchmark harness (DR-022) — a HEADLESS golden-path
//! orchestrator + log-replay metrics collator.
//!
//! # Zero pixels (I1)
//!
//! This crate renders NOTHING: no TUI, no `ratatui`/terminal dependency. It is
//! a socket/log consumer that emits a machine-readable [`MetricsReport`] (an
//! out-of-band struct/JSON for this slice — DR-022 defers the
//! `bench.completed`-subject-vs-out-of-band question to a warden `/subject`).
//!
//! # The two seams
//!
//! - **Orchestration** ([`run_cases`]) drives the EXISTING S4 golden path
//!   (open→vet→spawn→diff.ready→pre_merge→merge→diff.merged→debrief) headlessly
//!   for each [`Case`], collecting the recorded facts. A case that never
//!   reaches a verified merge is a task-completion MISS (a scored zero), NOT a
//!   harness crash (I6 — the harness stays a deterministic judge of its own
//!   runs).
//! - **Collation** ([`collate`]) folds a recorded event log into the three
//!   in-repo metrics — task-completion rate, worktree merge success,
//!   cost-per-merged-verified-diff — computed FROM THE LOG (I3), replay-stable:
//!   folding the same recorded facts twice yields byte-identical numbers. Cost
//!   reads only already-shipped fields (`agent.completed.cost`, per-verifier
//!   `cost_ms` on `gate.passed`, `action.metered.spend_delta_usd`) — this slice
//!   mints NO new event field/subject.
//!
//! # RED status (oracle deliverable — implementer fills these)
//!
//! Every public function below is an UNIMPLEMENTED STUB (`todo!()`) carrying
//! ZERO real logic, so the oracle tests LINK and FAIL AT RUNTIME (the honest
//! greenfield RED). A stub that accidentally satisfied a criterion would be a
//! false oracle; there is intentionally no logic to satisfy one. The
//! implementer replaces each `todo!()` body — the signatures and the report
//! shape are the pinned contract and MUST NOT change to make a test pass.

use serde::{Deserialize, Serialize};

use rezidnt_types::Event;

/// One benchmark scenario: a named golden-path run over a target repo. The
/// orchestrator drives the EXISTING S4 path for each case; this slice mints no
/// new golden-path pieces (DR-022). `expect_merge` records the scenario's
/// intent so a DELIBERATELY-failing case (a case that never reaches a verified
/// merge) is legible in the report as a scored MISS, not a harness fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    /// Behavior-naming id (e.g. `"golden_verified_merge"`,
    /// `"deliberate_no_merge"`), surfaced per-case in the report.
    pub name: String,
    /// Absolute path to the §13 project spec the golden path opens.
    pub spec_path: std::path::PathBuf,
    /// Whether this scenario is EXPECTED to reach a verified merge. A case with
    /// `expect_merge == false` is the deliberately-failing scenario: it is
    /// scored as a completion MISS, never a crash (CRITERION 3).
    pub expect_merge: bool,
}

/// One case's recorded outcome after the orchestrator drove its golden path.
/// The harness records WHETHER the case reached a verified merge (from the
/// log's terminal facts) — it does not throw when a case fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseOutcome {
    /// The [`Case::name`] this outcome scores.
    pub name: String,
    /// `true` iff this case's run reached a VERIFIED merge (a `pre_merge`
    /// `gate.passed` FOLLOWED by a `diff.merged` for the run) on the log.
    /// `false` for the deliberately-failing scenario — a scored zero, never a
    /// panic (CRITERION 3).
    pub reached_verified_merge: bool,
    /// The run ULID the case produced, if a run was spawned (for attribution;
    /// `None` if the case never spawned).
    pub run: Option<String>,
}

/// The machine-readable metrics report — the harness's ONE emitted artifact
/// (DR-022: an out-of-band struct/JSON for this slice; the on-log-vs-out-of-band
/// shape is a deferred warden `/subject`). Emitted to socket/log + CLI only, no
/// TUI (I1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsReport {
    /// Task-completion rate: fraction of cases whose run reached a verified
    /// merge, folded from the log (CRITERION 2). The deliberately-failing case
    /// counts as a MISS in the denominator (CRITERION 3).
    pub task_completion: MetricValue,
    /// Worktree merge success: fraction of merges that succeeded, folded from
    /// `diff.merged` facts on the log (CRITERION 2).
    pub worktree_merge_success: MetricValue,
    /// Cost per merged verified diff, folded from ALREADY-SHIPPED cost fields
    /// (`agent.completed.cost`, per-verifier `cost_ms`, `action.metered`) —
    /// no new field minted (CRITERION 2).
    pub cost_per_merged_verified_diff: MetricValue,
    /// The precision/recall seam (CRITERION 4). With no labeled set supplied it
    /// is [`Seam::Inconclusive`] — never a fabricated score, never a blank read
    /// as zero (I6). The with-labeled-set path is permanently external (§17).
    pub precision_recall: Seam,
    /// Per-case outcomes — the report can name which cases each rate folded
    /// from (CRITERION 2 interrogability).
    pub cases: Vec<CaseOutcome>,
}

/// A single folded metric plus the FACTS it folded from — the report can name
/// the exact facts each number was derived from (CRITERION 2 interrogability,
/// I6). Replay-stable: the same recorded facts always fold to the same value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricValue {
    /// The folded number (a rate in `0.0..=1.0`, or a cost in USD).
    pub value: f64,
    /// The event ids (ULIDs, canonical text) this number folded from, in log
    /// order — the interrogability trail (I6). Naming the facts is what makes
    /// the metric interrogable and the replay auditable.
    pub folded_from: Vec<String>,
}

/// The gate precision/recall seam (CRITERION 4). Present in the report always;
/// its value is [`Seam::Inconclusive`] when no labeled set is supplied. Modeled
/// as an enum so a missing measurement ANNOUNCES itself (I6 — never coerced to a
/// pass, never silently read as a zero score). The `Scored` variant exists so
/// the seam is real, but is exercised ONLY by the external private artifact
/// (§17); in-repo, the seam is always `Inconclusive`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Seam {
    /// No labeled set present — the honest report of a permanently-external
    /// measurement (§17). The `reason` is the fixed I6 disclosure string
    /// [`NO_LABELED_SET`].
    Inconclusive { reason: String },
    /// Precision/recall against a supplied labeled set. Reached only by the
    /// external private artifact; never fabricated in-repo.
    Scored { precision: f64, recall: f64 },
}

/// The fixed disclosure string the seam returns when unfed (CRITERION 4). A
/// missing measurement announces itself with this exact reason (I6).
pub const NO_LABELED_SET: &str = "inconclusive — no labeled set present";

/// The per-case DRIVER seam (I4-shaped): drives ONE case's golden path
/// (open→vet→spawn→diff.ready→pre_merge→merge→diff.merged→debrief) and returns
/// its recorded outcome, reading the terminal facts off the log. Injecting this
/// is what DECOUPLES the orchestration loop (batch iteration, name attribution,
/// miss-doesn't-abort resilience) from the real daemon — so the loop LOGIC is
/// tested against a deterministic fake driver the test controls, not against a
/// `Case::expect_merge` echo. `reached_verified_merge` on the returned outcome
/// is the DRIVER's finding from the log, NEVER a parrot of `Case::expect_merge`.
///
/// A driver that cannot complete a case (daemon fault, a genuinely-failing run)
/// returns a MISS outcome — it does NOT panic the batch (CRITERION 3). The
/// orchestrator ([`run_cases`]) is additionally resilient to a driver that DOES
/// panic on one case (a real DaemonDriver could): it isolates the failure and
/// scores that case a miss rather than aborting the whole run.
pub trait Driver {
    /// Drive one case end-to-end and report its recorded outcome. Reads the
    /// verified-merge terminal facts off the log; the `reached_verified_merge`
    /// it returns is the log-derived finding, not the case's declared intent.
    fn drive(&self, case: &Case) -> CaseOutcome;
}

/// Orchestrate the EXISTING golden path headlessly for each case with an
/// INJECTED driver, returning the per-case outcomes (CRITERION 1/3). This is the
/// pure orchestration LOOP: iterate the cases, drive each via `driver`, attribute
/// each outcome to its case name, and — CRITERION 3 — ensure one case's failure
/// (a driver MISS, or even a driver PANIC) does NOT abort the batch; the other
/// cases still produce outcomes and the failing case scores a miss. The driver
/// is injectable so the loop is testable against a deterministic fake decoupled
/// from `Case::expect_merge` (the driver's result, not the case's intent, decides
/// `reached_verified_merge`). `run_cases_default` wires the real daemon driver.
///
/// IMPLEMENTER-OWNED STUB: `todo!()`, zero logic. RED at runtime; the fake-driver
/// orchestration test asserts the loop against the DRIVER's controlled results,
/// so a stub that echoed `expect_merge` could not satisfy it.
pub fn run_cases(cases: &[Case], driver: &dyn Driver) -> Vec<CaseOutcome> {
    cases
        .iter()
        .map(|case| {
            // CRITERION 3: a driver that PANICS on one case (a real
            // DaemonDriver can blow up mid-drive) must NOT abort the batch. We
            // isolate each drive under `catch_unwind` and, on a caught panic,
            // synthesize a MISS outcome for that case and continue — the other
            // cases still produce outcomes. `&dyn Driver` is not `UnwindSafe`,
            // so we wrap the borrow in `AssertUnwindSafe`: the driver is only
            // read across the boundary (no shared mutable state we could leave
            // torn), so asserting unwind-safety is sound here.
            let driven =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.drive(case)));
            match driven {
                // The outcome follows the DRIVER's finding, never
                // `Case::expect_merge` (the anti-echo pin): we return exactly
                // what the driver reported.
                Ok(outcome) => outcome,
                // A caught panic is a scored MISS, not a crash (I6 — the
                // harness stays a deterministic judge of its own runs).
                Err(_) => CaseOutcome {
                    name: case.name.clone(),
                    reached_verified_merge: false,
                    run: None,
                },
            }
        })
        .collect()
}

/// Convenience: orchestrate with the real [`DaemonDriver`] (the production
/// wiring — stands up / connects to the daemon and drives each case's golden
/// path for real). This is what the CLI entry point calls; the REAL-driving
/// integration test (`tests/real_driver.rs`, `#[cfg(unix)]`, WSL-only) exercises
/// it against an actual fixture spec reaching a verified merge.
///
/// The production wiring: orchestrate the golden path with the real
/// [`DaemonDriver`]. Same orchestration LOOP as [`run_cases`], over the
/// production driver — now UNBLOCKED (DR-023): `DaemonDriver::drive` drives the
/// real daemon via the shared `rezidnt-client` seam.
pub fn run_cases_default(cases: &[Case]) -> Vec<CaseOutcome> {
    run_cases(cases, &DaemonDriver::default())
}

/// The production [`Driver`]: drives a case's golden path against the real
/// daemon (open→vet→spawn→…→diff.merged→debrief) and reads the verified-merge
/// terminal facts off the log. Its REAL behavior is pinned by the
/// `#[cfg(unix)]` WSL-only integration test (`tests/real_driver.rs`); the unit
/// orchestration test uses a fake `Driver` so the loop logic is host-testable.
///
/// DR-023: the daemon-driving is done through the shared `rezidnt-client`
/// socket seam (I5 — the existing wire) and the `rezidentd`/`rezidnt` binaries
/// resolved at runtime; NO test-only scaffolding and NO `tempfile`/fixture
/// crate enter the harness's production dep graph (I7, testkit_dev_only guard).
#[derive(Debug, Default)]
pub struct DaemonDriver {
    /// Implementer-owned: socket/db wiring, timeouts, spec staging. Left opaque
    /// here — the oracle pins the loop via the trait, not this struct's guts.
    _private: (),
}

impl Driver for DaemonDriver {
    fn drive(&self, case: &Case) -> CaseOutcome {
        // On unix the real drive runs; anywhere else the UDS path does not
        // exist, so the case scores a MISS (the WSL-only `real_driver.rs` is the
        // proof, and it is `#[cfg(unix)]`). A MISS is a scored zero, never a
        // panic (CRITERION 3 / I6).
        #[cfg(unix)]
        {
            match unix_drive::drive_case(case) {
                Ok(outcome) => outcome,
                // A driving fault (daemon fault, timeout, a genuinely-failing
                // run) is a scored MISS — never a batch-aborting crash. The
                // orchestrator's own `catch_unwind` is the belt; this is the
                // braces (we prefer an explicit miss to an unwind). The failure
                // is surfaced (not silently dropped) so a miss is diagnosable.
                Err(e) => {
                    eprintln!(
                        "rezidnt-bench: case {:?} scored a MISS — drive failed: {e}",
                        case.name
                    );
                    CaseOutcome {
                        name: case.name.clone(),
                        reached_verified_merge: false,
                        run: None,
                    }
                }
            }
        }
        #[cfg(not(unix))]
        {
            CaseOutcome {
                name: case.name.clone(),
                reached_verified_merge: false,
                run: None,
            }
        }
    }
}

/// The real, unix-only golden-path driving for [`DaemonDriver`]. Kept in its
/// own module so the whole UDS/process-spawn path is `#[cfg(unix)]` (the host
/// compile-skips it — the WSL run is the real gate,
/// [[vet-is-host-side-wsl-insufficient]]).
#[cfg(unix)]
mod unix_drive {
    use std::io::BufRead;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use rezidnt_types::Event;

    use crate::{Case, CaseOutcome};

    /// How long to wait for the whole verified-merge chain to land on the tail.
    /// Mirrors `golden_path.rs`'s 45 s deadline for the same flow.
    const MERGE_DEADLINE: Duration = Duration::from_secs(45);

    /// Any drive failure — a MISS, never a panic. The harness lib's error
    /// surface is this one internal enum; the public contract is `CaseOutcome`.
    /// `Display` reads every payload so a swallowed drive failure is legible
    /// (surfaced on the miss path, not silently dropped).
    #[derive(Debug)]
    pub(crate) enum DriveError {
        Io(std::io::Error),
        /// The daemon/CLI did not produce the expected observable (no run id on
        /// `open`, no verified-merge chain within the deadline, etc.).
        Protocol(String),
        Client(rezidnt_client::ClientError),
    }

    impl std::fmt::Display for DriveError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                DriveError::Io(e) => write!(f, "io: {e}"),
                DriveError::Protocol(msg) => write!(f, "protocol: {msg}"),
                DriveError::Client(e) => write!(f, "client: {e}"),
            }
        }
    }

    impl From<std::io::Error> for DriveError {
        fn from(e: std::io::Error) -> Self {
            DriveError::Io(e)
        }
    }
    impl From<rezidnt_client::ClientError> for DriveError {
        fn from(e: rezidnt_client::ClientError) -> Self {
            DriveError::Client(e)
        }
    }

    /// The cargo profile dir the running (test) binary lives under — the parent
    /// of `deps/`, where the built `rezidentd`/`rezidnt` sit. Resolved from
    /// `current_exe()` because `env!("CARGO_BIN_EXE_…")` is unavailable in a lib
    /// (and this is production code, not a test).
    fn target_bin_dir() -> Result<PathBuf, DriveError> {
        let exe = std::env::current_exe()?;
        let parent = exe
            .parent()
            .ok_or_else(|| DriveError::Protocol("current_exe has no parent".into()))?;
        let dir = if parent.file_name().map(|n| n == "deps").unwrap_or(false) {
            parent
                .parent()
                .ok_or_else(|| DriveError::Protocol("deps dir has no parent".into()))?
                .to_path_buf()
        } else {
            parent.to_path_buf()
        };
        Ok(dir)
    }

    fn workspace_bin(name: &str) -> Result<PathBuf, DriveError> {
        let path = target_bin_dir()?.join(name);
        if !path.exists() {
            return Err(DriveError::Protocol(format!(
                "{name} binary not found at {} — build the workspace bins first \
                 (`cargo build -p rezidentd -p rezidnt`)",
                path.display()
            )));
        }
        Ok(path)
    }

    /// A temp working dir under the system temp root (no `tempfile` dep — that
    /// is a fixture-staging crate the production harness must not carry, I7).
    /// Removed on drop (best-effort). Also owns the daemon child so a dropped
    /// guard never leaks the process.
    struct DaemonGuard {
        child: Child,
        socket: PathBuf,
        dir: PathBuf,
    }

    impl Drop for DaemonGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Create a fresh unique temp dir (pid + a monotonic-ish nonce), spawn
    /// `rezidentd` over it (REZIDNT_SOCKET/REZIDNT_DB, CAS defaults db-relative
    /// exactly as `golden_path.rs` relies on), and wait for the socket to bind.
    fn start_daemon() -> Result<DaemonGuard, DriveError> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("rezidnt-bench-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let socket = dir.join("rezidnt.sock");
        let db = dir.join("events.db");

        let child = Command::new(workspace_bin("rezidentd")?)
            .env("REZIDNT_SOCKET", &socket)
            .env("REZIDNT_DB", &db)
            .spawn()
            .inspect_err(|_| {
                let _ = std::fs::remove_dir_all(&dir);
            })?;

        // The guard owns the child + dir + socket; dropping it kills the daemon
        // and removes the temp dir even if the readiness wait below fails.
        let guard = DaemonGuard { child, socket, dir };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !guard.socket.exists() {
            if Instant::now() >= deadline {
                return Err(DriveError::Protocol(
                    "daemon socket never appeared within 5s".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(guard)
    }

    /// Drive ONE case's golden path against a freshly-spawned daemon, reading
    /// the verified-merge terminal facts (pre_merge `gate.passed` FOLLOWED by
    /// `diff.merged`) off the tail, and return the run ULID the daemon spawned.
    pub(crate) fn drive_case(case: &Case) -> Result<CaseOutcome, DriveError> {
        let daemon = start_daemon()?;

        // A `Case` names an EXISTING §13 spec on disk — the target repo's
        // `rezidnt.toml`. Production `drive` OPENS that repo; it NEVER constructs
        // one (DR-023 §(C): fixture construction — git init, harness/verifier
        // scripts, chmod, spec synthesis — is dev-only test-support in
        // `rezidnt-testkit`, never in shipped code). A missing spec is a DRIVE
        // FAULT → a scored MISS (a deterministic judge does not invent a repo).
        if !case.spec_path.exists() {
            return Err(DriveError::Protocol(format!(
                "case spec does not exist: {} — a benchmark Case must point at a \
                 real repo's rezidnt.toml; the driver does not stage a fixture",
                case.spec_path.display()
            )));
        }

        // `rezidnt open <spec>` against the daemon's socket/db. The pinned shape
        // is exactly one stdout line `opened <name> run <run-ulid>`.
        let spec = case
            .spec_path
            .to_str()
            .ok_or_else(|| DriveError::Protocol("spec path is not utf-8".into()))?;
        let open = Command::new(workspace_bin("rezidnt")?)
            .args(["open", spec])
            .env("REZIDNT_SOCKET", &daemon.socket)
            .env("REZIDNT_DB", daemon.dir.join("events.db"))
            .output()?;
        if !open.status.success() {
            return Err(DriveError::Protocol(format!(
                "open failed (exit {:?}): {}",
                open.status.code(),
                String::from_utf8_lossy(&open.stderr)
            )));
        }
        let stdout = String::from_utf8_lossy(&open.stdout);
        let run = stdout
            .split_whitespace()
            .last()
            .filter(|tok| tok.len() == 26)
            .ok_or_else(|| {
                DriveError::Protocol(format!(
                    "open did not print the pinned `opened <name> run <ulid>` line: {stdout:?}"
                ))
            })?
            .to_string();

        // Tail the daemon socket (shared client — the EXISTING wire, I5) and
        // read until `diff.merged` for THIS run. The verified-merge test is a
        // pre_merge `gate.passed` for the run FOLLOWED by a `diff.merged` for it.
        let reached = tail_for_verified_merge(&daemon.socket, &run)?;

        Ok(CaseOutcome {
            name: case.name.clone(),
            reached_verified_merge: reached,
            run: Some(run),
        })
    }

    /// Connect via `rezidnt-client`, tail from seq 0, and return `true` iff a
    /// pre_merge `gate.passed` for `run` is observed BEFORE a `diff.merged` for
    /// `run` within the deadline. Reads the finding off the LOG's facts — never
    /// an echo of any declared intent.
    fn tail_for_verified_merge(socket: &std::path::Path, run: &str) -> Result<bool, DriveError> {
        // The tail is the existing `Request::Tail { subject: None }` op — replay
        // from seq 0 then live. `rezidnt-client` does the connect + hello check
        // + request send against THIS daemon's explicit socket (never a racy
        // process-global REZIDNT_SOCKET mutation); we read fact frames off the
        // returned reader.
        let mut reader = rezidnt_client::connect_and_request_at(
            socket,
            &rezidnt_client::Request::Tail { subject: None },
        )?;

        let deadline = Instant::now() + MERGE_DEADLINE;
        let mut pre_merge_passed = false;
        let mut line = String::new();
        while Instant::now() < deadline {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break; // daemon closed the stream
            }
            let frame = line.trim_end();
            if frame.is_empty() {
                continue;
            }
            let Ok(event) = Event::from_json_line(frame) else {
                continue; // non-fact frame (e.g. a reply ack) — skip
            };
            let payload = event.payload();
            let ev_run = payload.get("run").and_then(|r| r.as_str());
            if ev_run != Some(run) {
                continue;
            }
            let subject = event.subject.as_str();
            let gate = payload.get("gate").and_then(|g| g.as_str());
            match subject {
                "gate.passed" if gate == Some("pre_merge") => {
                    pre_merge_passed = true;
                }
                "diff.merged" => {
                    // Verified merge iff the pre_merge pass preceded this merge
                    // for the run (the exact terminal-facts test the collator
                    // and `real_driver.rs` pin).
                    return Ok(pre_merge_passed);
                }
                _ => {}
            }
        }
        Err(DriveError::Protocol(format!(
            "no diff.merged for run {run} within {}s",
            MERGE_DEADLINE.as_secs()
        )))
    }
}

/// Collate the three in-repo metrics from a RECORDED event log (CRITERION 2).
/// PURE over the log (I3): no fresh wall-clock read, no rng — the same recorded
/// facts always fold to the same [`MetricsReport`] numbers (replay-stable). Cost
/// reads only already-shipped fields. The precision/recall seam is
/// [`Seam::Inconclusive`] because no labeled set is passed here (CRITERION 4).
///
/// `expected_cases` names the scenario intents so the deliberately-failing case
/// scores as a MISS in the denominator (CRITERION 3) rather than being invisible
/// to the rate.
///
/// IMPLEMENTER-OWNED STUB: `todo!()`, zero logic.
pub fn collate(log: &[Event], expected_cases: &[Case]) -> MetricsReport {
    // Fold the three in-repo metrics off the log (I3), then attach the honest
    // `Inconclusive` precision/recall seam: `collate` is passed no labeled set,
    // so the seam ANNOUNCES the permanently-external measurement (I6, §17)
    // rather than fabricating a score.
    collate_inner(
        log,
        expected_cases,
        Seam::Inconclusive {
            reason: NO_LABELED_SET.into(),
        },
    )
}

/// Collate WITH a supplied labeled set at the precision/recall seam (CRITERION 4
/// with-set path). This path is exercised ONLY by the external private artifact
/// (§17) and is NOT asserted in-repo — its presence proves the seam EXISTS. The
/// `_labeled` bytes are the opaque labeled-defect set the external artifact
/// supplies.
///
/// IMPLEMENTER-OWNED STUB: `todo!()`, zero logic.
pub fn collate_with_labeled_set(
    log: &[Event],
    expected_cases: &[Case],
    labeled: &[u8],
) -> MetricsReport {
    // The three in-repo metrics fold identically — they read the log, never the
    // labeled set. Only the precision/recall seam changes: WITH a labeled set,
    // it becomes `Scored`. That scoring is permanently external (§17); this
    // in-repo path only needs to prove the seam EXISTS and does not fabricate a
    // score in-repo. An EMPTY labeled set is still no measurement — it stays
    // `Inconclusive` (I6: never coerce a missing input into a pass).
    let seam = compute_precision_recall(labeled);
    collate_inner(log, expected_cases, seam)
}

/// The precision/recall seam's with-set computation (CRITERION 4, external
/// path). The labeled-defect set and the real scoring live OUTSIDE this repo
/// forever (§17); in-repo this only proves the seam is real. An empty set
/// carries no measurement and stays `Inconclusive` (I6 — a missing input is
/// never coerced into a score). A non-empty set is the external artifact's
/// domain; in-repo we do not fabricate a score against opaque bytes, so we
/// keep the honest `Inconclusive` disclosure here too — the SCORED variant is
/// reached only by the external artifact that owns the labeled taxonomy.
fn compute_precision_recall(_labeled: &[u8]) -> Seam {
    Seam::Inconclusive {
        reason: NO_LABELED_SET.into(),
    }
}

/// Pure log-replay fold shared by both collate entry points (CRITERIA 2/3).
/// NO fresh wall-clock, NO rng — the same recorded facts always fold to the
/// same numbers (replay-stable, I3). `seam` is threaded in so the caller
/// decides the precision/recall disclosure (CRITERION 4).
fn collate_inner(log: &[Event], expected_cases: &[Case], seam: Seam) -> MetricsReport {
    // ── First pass: attribute each run to its verified-merge status, in the
    // order the runs first appear on the log. A run reaches a VERIFIED MERGE iff
    // a pre_merge `gate.passed` for the run is FOLLOWED by a `diff.merged` for
    // that run. Both facts are derived from the log only — no filesystem read,
    // no intent echo.
    let mut runs_in_order: Vec<String> = Vec::new();
    let mut pre_merge_passed: std::collections::HashSet<String> = std::collections::HashSet::new();
    // run -> the diff.merged event id that landed for it (in log order).
    let mut merged: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    // Every merge ATTEMPT (a pre_merge gate.entered) and every landed merge,
    // for the worktree-merge-success rate.
    let mut merge_attempts: Vec<String> = Vec::new();
    let mut merges_landed: Vec<String> = Vec::new();

    for event in log {
        let subject = event.subject.as_str();
        let payload = event.payload();
        let run = payload.get("run").and_then(|r| r.as_str());
        let gate = payload.get("gate").and_then(|g| g.as_str());
        let event_id = event.id.to_string();

        // Record run first-appearance order (any fact naming a run).
        if let Some(run) = run
            && !runs_in_order.iter().any(|r| r == run)
        {
            runs_in_order.push(run.to_string());
        }

        match subject {
            "gate.entered" if gate == Some("pre_merge") => {
                if let Some(run) = run {
                    merge_attempts.push(run.to_string());
                }
            }
            "gate.passed" if gate == Some("pre_merge") => {
                if let Some(run) = run {
                    pre_merge_passed.insert(run.to_string());
                }
            }
            "diff.merged" => {
                if let Some(run) = run {
                    merges_landed.push(event_id.clone());
                    // Only the FIRST diff.merged for a run counts (idempotent
                    // fold); a re-run over the same facts stays stable.
                    merged.entry(run.to_string()).or_insert(event_id);
                }
            }
            _ => {}
        }
    }

    // A run is a VERIFIED merge iff pre_merge passed AND a diff.merged landed
    // for it (the pass necessarily precedes the merge on a well-formed log; the
    // ordering is guaranteed by the daemon that emitted the facts).
    let verified: std::collections::HashMap<&String, Option<&String>> = runs_in_order
        .iter()
        .map(|run| {
            let hit = pre_merge_passed.contains(run);
            let merged_id = merged.get(run);
            (run, if hit { merged_id } else { None })
        })
        .collect();

    // ── Attribute runs to expected_cases by index, in log-appearance order.
    // This is the anti-echo-safe attribution: the case's outcome follows the
    // LOG (its paired run's verified-merge status), never `Case::expect_merge`.
    let mut cases: Vec<CaseOutcome> = Vec::with_capacity(expected_cases.len());
    let mut completion_trail: Vec<String> = Vec::new();
    let mut hits = 0usize;
    for (idx, case) in expected_cases.iter().enumerate() {
        match runs_in_order.get(idx) {
            Some(run) => {
                let merged_id = verified.get(run).and_then(|m| *m);
                let reached = merged_id.is_some();
                if let Some(id) = merged_id {
                    hits += 1;
                    completion_trail.push(id.clone());
                }
                cases.push(CaseOutcome {
                    name: case.name.clone(),
                    reached_verified_merge: reached,
                    run: Some(run.clone()),
                });
            }
            // A case with no run on the log never spawned — a scored MISS, not
            // a crash (CRITERION 3).
            None => cases.push(CaseOutcome {
                name: case.name.clone(),
                reached_verified_merge: false,
                run: None,
            }),
        }
    }

    // ── task_completion: verified-merge hits / expected-case count. The
    // deliberately-failing case is a MISS in the DENOMINATOR (CRITERION 3), not
    // dropped. Trail names the diff.merged facts the hits folded from.
    let task_completion = MetricValue {
        value: rate(hits, expected_cases.len()),
        folded_from: completion_trail,
    };

    // ── worktree_merge_success: merges that LANDED / merge ATTEMPTS, folded
    // from `diff.merged` over pre_merge `gate.entered`. Trail names the landed
    // `diff.merged` facts.
    let worktree_merge_success = MetricValue {
        value: rate(merges_landed.len(), merge_attempts.len()),
        folded_from: merges_landed,
    };

    // ── cost_per_merged_verified_diff: total USD cost of the VERIFIED-MERGED
    // runs / count of merged verified diffs. Folds ONLY already-shipped fields
    // (`agent.completed.cost.total_usd`, `action.metered.spend_delta_usd`), and
    // names every consulted cost-bearing fact — including per-verifier
    // `cost_ms` on `gate.passed` — in the interrogability trail (I6). No new
    // field/subject is minted.
    let verified_runs: std::collections::HashSet<&String> = verified
        .iter()
        .filter_map(|(run, merged)| merged.map(|_| *run))
        .collect();
    let mut cost_usd = 0.0f64;
    let mut cost_trail: Vec<String> = Vec::new();
    for event in log {
        let payload = event.payload();
        let run = payload.get("run").and_then(|r| r.as_str());
        let Some(run) = run else { continue };
        if !verified_runs.contains(&run.to_string()) {
            continue;
        }
        let subject = event.subject.as_str();
        let event_id = event.id.to_string();
        match subject {
            "agent.completed" => {
                if let Some(total) = payload
                    .get("cost")
                    .and_then(|c| c.get("total_usd"))
                    .and_then(|v| v.as_f64())
                {
                    cost_usd += total;
                    cost_trail.push(event_id);
                }
            }
            "action.metered" => {
                if let Some(delta) = payload.get("spend_delta_usd").and_then(|v| v.as_f64()) {
                    cost_usd += delta;
                    cost_trail.push(event_id);
                }
            }
            // Per-verifier `cost_ms` on `gate.passed` is a cost fact the metric
            // consults for interrogability. It is a LATENCY (ms), not USD, so it
            // is named in the trail but not summed into the USD figure — keeping
            // the value's units clean while the trail stays complete (I6).
            "gate.passed" => {
                let has_cost_ms = payload
                    .get("verifiers")
                    .and_then(|v| v.as_array())
                    .is_some_and(|arr| arr.iter().any(|v| v.get("cost_ms").is_some()));
                if has_cost_ms {
                    cost_trail.push(event_id);
                }
            }
            _ => {}
        }
    }
    let merged_verified_diffs = verified_runs.len();
    let cost_per_merged_verified_diff = MetricValue {
        value: if merged_verified_diffs == 0 {
            0.0
        } else {
            cost_usd / merged_verified_diffs as f64
        },
        folded_from: cost_trail,
    };

    MetricsReport {
        task_completion,
        worktree_merge_success,
        cost_per_merged_verified_diff,
        precision_recall: seam,
        cases,
    }
}

/// A `hits / total` rate in `0.0..=1.0`; an empty denominator folds to `0.0`
/// (no attempts is not a division-by-zero panic — the honest empty-fold value).
fn rate(hits: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        hits as f64 / total as f64
    }
}
