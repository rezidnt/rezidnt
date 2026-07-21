//! Sandbox substrate seam (C3a — DR-025; design `permit-sole-chokepoint-c3.md`
//! §5). The `SandboxSubstrate` (I4) wraps the S1 spawn seam so the daemon-owned
//! harness process runs CONFINED — making the deterministic permit verdict
//! (`PathConfinement`, a `PathScope` sibling in `rezidnt-gate`) UNBYPASSABLE.
//!
//! This module is the ORACLE SEAM ONLY: the trait + the policy shape + the
//! availability probe the tests drive against. The Linux `bwrap` implementation
//! (`--ro-bind`/`--bind` the folded binds, `--unshare-all`, `--die-with-parent`,
//! the daemon keeps the PTY) is the IMPLEMENTER's next job — every method below
//! is `todo!()`-stubbed so the C3a board is assert-red, not compile-red, exactly
//! as the S4 gate skeleton was.
//!
//! ## The load-bearing shape (DR-025 §Decision, the C6 escalation lesson)
//!
//! [`SandboxPolicy`] is the confinement authority. Its `binds` field is PRIVATE
//! and settable ONLY through [`SandboxPolicy::from_folded_authority`] — there is
//! deliberately NO constructor that takes a bind from a [`SpawnPlan`] arg, an
//! env var, or any run-supplied value. This is the DR-024/DR-016 privilege-
//! escalation guard expressed in the type system: an input that WIDENS
//! confinement must come from folded authority, or the sandbox is escapable-by-
//! argument (design §5, §8.3). Criterion 3's test drives exactly this seam.

use std::path::PathBuf;

use crate::RunError;
use crate::spawner::SpawnPlan;

/// One confinement bind: a host path made visible inside the sandbox, and
/// whether the agent may write it. The Linux impl renders a read-only bind as
/// `--ro-bind SRC DST` and a writable bind as `--bind SRC DST`; DST defaults to
/// SRC (the worktree keeps its own path inside the namespace).
///
/// A bind is the confinement POLICY, not the action target: it says "the agent
/// may see/touch this path at all", the mechanism that makes the permit verdict
/// unbypassable. It is minted only inside [`SandboxPolicy::from_folded_authority`]
/// from folded state — never from a spawn arg (DR-025 §Decision, C6 lesson).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    /// The host path exposed inside the sandbox.
    pub host_path: PathBuf,
    /// `true` → `--bind` (writable); `false` → `--ro-bind` (read-only).
    pub writable: bool,
}

impl Bind {
    /// A read-only bind (`--ro-bind`): the toolchain, read-only project data.
    pub fn read_only(host_path: impl Into<PathBuf>) -> Self {
        Self {
            host_path: host_path.into(),
            writable: false,
        }
    }

    /// A writable bind (`--bind`): the agent's own worktree.
    pub fn writable(host_path: impl Into<PathBuf>) -> Self {
        Self {
            host_path: host_path.into(),
            writable: true,
        }
    }
}

/// The confinement policy for one sandboxed spawn: the allowed binds and
/// unshared namespaces, folded from the project spec `[gates.permit]`/role layer
/// (DR-025 §Decision — "folded authority, never a self-declared spawn arg").
///
/// The `binds` are PRIVATE so they can be set ONLY through
/// [`SandboxPolicy::from_folded_authority`]. A `SpawnPlan` (which carries
/// run-supplied argv/env) can NEVER contribute a bind — the C6 escalation guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    /// The allowed binds — the ONLY paths the confined process may see. PRIVATE:
    /// the type-system half of the no-widening guard. Read via [`Self::binds`].
    binds: Vec<Bind>,
    /// Whether the impl passes `--unshare-all` (network + pid + ipc + …). Folded
    /// like the binds; a run arg cannot add an unshare-EXCEPTION (DR-025 crit 3).
    unshare_all: bool,
}

impl SandboxPolicy {
    /// Build a policy FROM FOLDED AUTHORITY — the ONLY constructor (DR-025
    /// §Decision; the C6/DR-024 lesson). Callers pass binds derived from the
    /// folded project-spec/role layer; there is intentionally no `SpawnPlan`
    /// parameter here, so a run-supplied value cannot reach `binds`.
    ///
    /// The daemon is the sole caller in production (it holds the folded state);
    /// tests construct a policy this way to STAND IN for that fold, exactly as
    /// the C6 unit tests feed the folded axis directly into params.
    pub fn from_folded_authority(binds: Vec<Bind>, unshare_all: bool) -> Self {
        Self { binds, unshare_all }
    }

    /// The confinement binds (read-only view — the field is private so it is
    /// never widened after construction).
    pub fn binds(&self) -> &[Bind] {
        &self.binds
    }

    /// Whether the impl unshares all namespaces (`--unshare-all`).
    pub fn unshare_all(&self) -> bool {
        self.unshare_all
    }

    /// Is `path` inside confinement — i.e. covered by some bind? The pure
    /// deterministic predicate the `PathConfinement` verifier and the mechanism
    /// agree on (design §5). A path under a writable bind is writable; under a
    /// read-only bind it is readable-only; under no bind it is DENIED.
    ///
    /// Implementer stub: the containment check (prefix / canonicalization
    /// discipline) is the implementer's, matching the `PathScope` glob shape.
    pub fn confines(&self, path: &std::path::Path) -> Confinement {
        // First bind (in policy order) that COVERS this path decides: a writable
        // bind → Writable, a read-only bind → ReadOnly. Under NO bind → Denied.
        // Segment-boundary containment, no filesystem touch (pure, determinism
        // BINDING — the same predicate the `PathConfinement` verdict uses).
        for bind in &self.binds {
            if path_within(&bind.host_path, path) {
                return if bind.writable {
                    Confinement::Writable
                } else {
                    Confinement::ReadOnly
                };
            }
        }
        Confinement::Denied
    }
}

/// Is `path` inside `bind` — equal to it or a `/`-boundary descendant? Pure and
/// deterministic (no canonicalization / filesystem touch): the containment
/// predicate the sandbox mechanism and the `PathConfinement` verifier agree on
/// (DR-025 design §5). `/opt/toolchain` covers `/opt/toolchain/bin` but NOT
/// `/opt/toolchain-evil`.
fn path_within(bind: &std::path::Path, path: &std::path::Path) -> bool {
    path == bind || path.starts_with(bind)
}

/// The confinement decision for a single path — the pure predicate the sandbox
/// mechanism and the `PathConfinement` verifier share (design §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    /// Inside a writable bind — read and write allowed.
    Writable,
    /// Inside a read-only bind — read allowed, write DENIED.
    ReadOnly,
    /// Outside every bind — read and write DENIED (criterion 2).
    Denied,
}

/// Whether a sandbox backend is usable on this host. The degrade contract
/// (DR-025 §Decision, I6): an [`Availability::Unavailable`] backend does NOT
/// silently spawn unsandboxed — it announces itself with a `sandbox.unavailable`
/// fact and degrades LOUDLY (criterion 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// The backend binary is present and usable.
    Available,
    /// The backend is absent/unusable; `reason` is the loggable degrade cause
    /// (e.g. `"bwrap not found on PATH"`). The run degrades to an unsandboxed
    /// spawn, but only AFTER a `sandbox.unavailable` fact lands (never silent).
    Unavailable { reason: String },
}

impl Availability {
    /// Convenience: is the backend available?
    pub fn is_available(&self) -> bool {
        matches!(self, Availability::Available)
    }
}

/// The result of a confined spawn: the daemon-owned child plus the backend it
/// ran under. The daemon keeps the PTY / process handle (S1 invariant — the
/// daemon owns the process, not the client); this type carries only what the
/// caller needs to reap and to log the `sandbox.spawned` backend.
///
/// Oracle placeholder: the concrete child handle shape (a `tokio::process::Child`
/// wrapper vs. the reaper's pid handle) is the implementer's — the tests here do
/// not spawn a real child on the host path, and the unix integration test drives
/// the real one behind `#[cfg(unix)]`.
#[derive(Debug)]
pub struct SandboxedChild {
    /// The backend the spawn actually ran under (`"bwrap"`, or `"none"` on a
    /// loud degrade) — recorded on the `sandbox.spawned` fact for replay.
    pub backend: String,
    /// The child pid, when the backend surfaced one (the reaper's liveness key).
    pub pid: Option<u32>,
}

/// The sandbox substrate (I4 — DR-025 §Decision; design §3, §5). Wraps the S1
/// spawn seam: given a resolved [`SpawnPlan`] and a folded [`SandboxPolicy`], it
/// launches the harness CONFINED (Linux `bwrap`), or reports it cannot and lets
/// the daemon degrade loudly.
///
/// Selected by platform exactly like the run/git substrates (DR-001): the Linux
/// `bwrap` backend is C3a; macOS `sandbox-exec` is a later backend behind the
/// SAME trait; Windows is gated behind the deferred native-Windows Platform phase
/// (DR-025 §Decision, design §6).
pub trait SandboxSubstrate {
    /// A stable backend name for the `sandbox.*` facts (`"bwrap"`, `"none"`).
    fn backend(&self) -> &'static str;

    /// Probe whether this backend can confine on this host — the degrade gate
    /// (criterion 4). `bwrap` absent ⇒ [`Availability::Unavailable`], so the
    /// daemon logs `sandbox.unavailable` and degrades LOUDLY. NEVER panics on a
    /// missing binary (a missing tool is a VERDICT, not a crash — mirrors the
    /// exec-runner could-not-run discipline).
    fn availability(&self) -> Availability;

    /// Spawn the harness CONFINED under `policy`. The binds come from the folded
    /// `policy` ONLY — `plan` supplies argv/env/cwd, never a bind (the C6 guard
    /// is enforced by [`SandboxPolicy`]'s private `binds`, so this signature
    /// cannot be handed a widening bind). The daemon keeps the PTY/process (S1).
    ///
    /// Returns [`RunError::Spawn`] when the backend is available but the confined
    /// launch fails; the daemon's DEGRADE path (when availability is
    /// `Unavailable`) is a separate branch — the substrate never silently falls
    /// back to an unsandboxed spawn inside `spawn_confined` (that would defeat
    /// the loud-degrade contract, I6).
    fn spawn_confined(
        &self,
        plan: &SpawnPlan,
        policy: &SandboxPolicy,
    ) -> Result<SandboxedChild, RunError>;
}

/// Render the `bwrap` argv for a plan + folded policy — the pure, inspectable
/// arg-building seam the tests pin WITHOUT spawning anything (mirrors
/// [`SpawnPlan`] being pure so `spawn_plan.rs` pins it host-side). The Linux impl
/// calls this and hands the result to `bwrap`; the host-runnable tests assert the
/// argv confines (every bind present, `--unshare-all`, `--die-with-parent`) and
/// that NO run-supplied path leaked in.
///
/// Oracle stub: the implementer writes the real renderer. It exists as a `pub`
/// pure fn so the no-widening + confinement-argv tests can drive it host-side
/// (no `bwrap` needed to inspect the argv it WOULD run).
pub fn bwrap_argv(plan: &SpawnPlan, policy: &SandboxPolicy) -> Vec<String> {
    // The C3a-alone posture: the full unshare set (network INCLUDED). bwrap makes
    // its own fresh netns via `--unshare-all` — no shared netns to inherit.
    bwrap_argv_inner(plan, policy, /* share_net */ false)
}

/// Render the `bwrap` argv for the SHARED-NETNS composition (DR-028 §Decision 1):
/// the unshare-all-MINUS-net posture. Every namespace the C3a posture drops is
/// still dropped (user, pid, ipc, uts, cgroup) EXCEPT the network — so the agent
/// INHERITS the netns bwrap runs inside (pasta's already-sealed, proxy-only netns)
/// rather than landing in a fresh empty one with no route out.
///
/// This is the ONE argv difference that makes pasta-outer work: a bare
/// `--unshare-all` (or `--unshare-net`) here would re-unshare the network and drop
/// the agent into an empty netns — mediation would be dead, not enforced. The
/// composed spawn calls THIS renderer (not [`bwrap_argv`]) only when egress is
/// active; the binds/`--die-with-parent`/no-widening discipline are identical.
pub fn bwrap_argv_shared_netns(plan: &SpawnPlan, policy: &SandboxPolicy) -> Vec<String> {
    bwrap_argv_inner(plan, policy, /* share_net */ true)
}

/// The shared renderer both public `bwrap_argv*` fns delegate to. `share_net`
/// selects the netns posture: `false` → the C3a full `--unshare-all` (fresh
/// netns), `true` → the unshare-all-MINUS-net set (inherit the shared netns —
/// DR-028 §Decision 1). Binds and unshare come from `policy` ONLY, never
/// `plan.args`/`plan.env` (the C6/DR-024 no-widening guard — `_plan` is unused for
/// wrapper directives; the run-supplied argv is the CHILD's, composed AFTER `--`).
fn bwrap_argv_inner(_plan: &SpawnPlan, policy: &SandboxPolicy, share_net: bool) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();
    // Namespaces first (the folded unshare posture). A run arg cannot add an
    // unshare-EXCEPTION because we never read the plan here.
    if policy.unshare_all() {
        if share_net {
            // Unshare-all MINUS net AND MINUS user: drop the namespaces
            // `--unshare-all` would drop EXCEPT the network AND the user namespace,
            // because BOTH are pasta's under pasta-outer (DR-028 §Decision 1). The
            // net stays shared so the agent inherits pasta's already-sealed,
            // proxy-only netns rather than a fresh empty one. The USER namespace
            // stays pasta's too: pasta created the netns in ITS user-ns, so a
            // process must remain in that user-ns to hold `CAP_NET_ADMIN` over the
            // netns (seal its routes, reach the proxy). A bwrap `--unshare-user`
            // here would put the agent in a NEW user-ns that does NOT own pasta's
            // netns — stripping net-admin caps and breaking the sealed-route / proxy
            // path (RTNETLINK EPERM). We still unshare ipc/pid/uts/cgroup + confine
            // the filesystem to the folded binds — the confinement bwrap owns.
            // Deliberately NO `--unshare-net`, `--unshare-all`, or `--unshare-user`.
            argv.push("--unshare-ipc".to_string());
            argv.push("--unshare-pid".to_string());
            argv.push("--unshare-uts".to_string());
            argv.push("--unshare-cgroup".to_string());
        } else {
            // The C3a-alone posture: `--unshare-all` drops network + pid + ipc +
            // uts + cgroup + user (a fresh empty netns, no route to inherit).
            argv.push("--unshare-all".to_string());
        }
    }
    // The child dies when the daemon-owned parent does — no orphaned confined
    // process outlives the run (S1: the daemon owns the process lifetime).
    argv.push("--die-with-parent".to_string());
    // A FRESH devtmpfs + procfs inside the namespace (NOT host binds — new
    // namespace-isolated pseudo-filesystems, so no confinement leak). Required for
    // a confined program to spawn children of its own: a dynamically-linked child
    // exec'd via `posix_spawn` (glibc, as Rust's `std::process::Command` uses) needs
    // `/dev` present or the spawn fails with a spurious ENOENT even though the
    // target binary is reachable — a shell's plain `fork+execvp` does not, which is
    // why a shell harness masks this. A confined agent that runs tools (`git`, `ip`,
    // …) therefore needs `/dev` + `/proc`; without them the sandbox is exec-broken
    // for real dynamically-linked programs.
    argv.push("--dev".to_string());
    argv.push("/dev".to_string());
    argv.push("--proc".to_string());
    argv.push("/proc".to_string());
    // One directive per folded bind, in policy order (deterministic): writable →
    // `--bind SRC DST`, read-only → `--ro-bind SRC DST`. DST defaults to SRC (the
    // worktree keeps its own path inside the namespace).
    for bind in policy.binds() {
        let flag = if bind.writable { "--bind" } else { "--ro-bind" };
        let src = bind.host_path.to_string_lossy().into_owned();
        argv.push(flag.to_string());
        argv.push(src.clone());
        argv.push(src);
    }
    argv
}

/// The Linux `bwrap` sandbox backend (C3a — DR-025 §Decision; design §5): execs
/// `bwrap` with the folded binds (`--ro-bind`/`--bind`), `--unshare-all`,
/// `--die-with-parent`, keeping the PTY daemon-side (S1). Selected by platform
/// like the run/git substrates (DR-001).
///
/// ORACLE STUB: the fields and the impl body are the IMPLEMENTER's. It exists as
/// a named type so the WSL-only `#[cfg(unix)]` integration suite compiles
/// (assert-red / `#[ignore]`-gated) rather than compile-red — the S4 gate-skeleton
/// precedent. Every trait method is `todo!()` until the implementer writes it.
#[derive(Debug, Default)]
pub struct BwrapSubstrate {
    /// The `bwrap` binary name/path to exec (defaults to `"bwrap"` on PATH). The
    /// availability probe and `spawn_confined` both resolve through this, so a
    /// test can point a substrate at a missing binary to exercise the degrade.
    pub bin: Option<String>,
}

impl SandboxSubstrate for BwrapSubstrate {
    fn backend(&self) -> &'static str {
        "bwrap"
    }

    fn availability(&self) -> Availability {
        probe_backend(self.bin.as_deref().unwrap_or("bwrap"))
    }

    fn spawn_confined(
        &self,
        plan: &SpawnPlan,
        policy: &SandboxPolicy,
    ) -> Result<SandboxedChild, RunError> {
        // Adapter task span (rust-conventions): every confined spawn is traced.
        let span = tracing::info_span!("adapter", kind = "sandbox", backend = "bwrap");
        let _guard = span.enter();

        let bin = self.bin.clone().unwrap_or_else(|| "bwrap".to_string());
        // The confinement wrapper argv comes from the folded `policy` ONLY
        // (bwrap_argv never reads `plan`). The child command is composed AFTER
        // `--` from `plan.bin`/`plan.args` — the run-supplied argv lands as the
        // CONFINED program's args, NEVER as bwrap bind/unshare directives (the
        // C6 no-widening guard). The daemon keeps the process handle (S1 — the
        // daemon owns the process, not the client); this returns a reap seam.
        let mut cmd = std::process::Command::new(&bin);
        cmd.args(bwrap_argv(plan, policy));
        cmd.arg("--");
        cmd.arg(&plan.bin);
        cmd.args(&plan.args);
        // The child runs with the plan's scrubbed env (the badge + any permit
        // wiring). `env_clear` first so no ambient host secret leaks past the
        // sandbox boundary, then the folded env only.
        cmd.env_clear();
        cmd.envs(plan.env.iter().map(|(k, v)| (k.clone(), v.clone())));

        let mut child = cmd
            .spawn()
            .map_err(|e| RunError::Spawn(format!("bwrap confined spawn failed: {e}")))?;
        let pid = child.id();
        // The daemon owns the process (S1) — the caller gets the backend + pid
        // (the reaper's liveness key), NEVER the child/PTY handle. A detached
        // waiter reaps the confined child so it does not become a zombie; the
        // concrete daemon-owned handle shape (a `tokio::process::Child` the
        // reaper adopts) is the wider run-loop's, out of this seam (DR-025 §5).
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(SandboxedChild {
            backend: "bwrap".to_string(),
            pid: Some(pid),
        })
    }
}

/// Probe the host for a usable `bwrap` (the degrade gate, criterion 4). Pointed
/// at a backend binary NAME/PATH; a missing binary is [`Availability::Unavailable`]
/// with a loggable reason, NEVER a panic. `pub` so the daemon and the tests share
/// one probe (the test points it at a deliberately-missing path to exercise the
/// degrade arm without uninstalling `bwrap`).
pub fn probe_backend(bin: &str) -> Availability {
    // Try to run the backend's `--version` — the could-not-run discipline: a
    // spawn error (ENOENT/EACCES) is a VERDICT (Unavailable with a loggable
    // reason), never a panic. We do NOT unwrap; a missing binary is the honest
    // degrade signal the daemon logs as `sandbox.unavailable`. `std::process`
    // (exec'd like the git-CLI) — no linked sandbox crate (I7, criterion 5).
    match std::process::Command::new(bin)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Availability::Available,
        // The binary ran but `--version` was non-zero: usable-enough is not a
        // safe assumption — report the odd exit as the loggable reason rather
        // than a false Available (never a silent allow, I6).
        Ok(status) => Availability::Unavailable {
            reason: format!("{bin} --version exited with {status}"),
        },
        Err(e) => Availability::Unavailable {
            reason: format!("{bin} not runnable on PATH: {e}"),
        },
    }
}
