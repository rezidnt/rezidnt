//! c3-wire oracle (DR-028) — CRITERION 2 (WSL, #[cfg(unix)]): shared-netns
//! INESCAPABILITY UNDER COMPOSITION (DR-026 crit 3, now through the live composed
//! run-loop hand-off, not the enforce suite's self-made netns). PLUS criterion 4's
//! live sealed-netns arm (the injected token rides the upstream, never the agent).
//!
//! ## What this proves that `egress_mediation_c3bc.rs` cannot (the compose gap)
//! The enforce suite (`egress_mediation_c3bc.rs`) drives `pasta -- <probe>`: pasta
//! makes its OWN netns and execs the dev probe DIRECTLY. That proves the dataplane
//! mediates, but NOT that it still mediates when C3a's `bwrap` is spliced between
//! pasta and the confined program — the pasta-outer -> bwrap -> agent composition
//! DR-028 §Decision 1 settles. THIS suite drives the SAME falsifiable escape probe
//! through the COMPOSED hand-off (`pasta -- bwrap -- <probe>`): the escape probe
//! runs from the RUNNING AGENT's netns (bwrap's child inheriting pasta's sealed
//! netns), and must STILL find no non-proxy exit. If bwrap re-unshared net (a
//! composition bug), the agent would land in a fresh empty netns — the ViaProxy
//! path would DIE (non-vacuous guard catches it), not silently escape.
//!
//! ## SUITE PLACEMENT — WSL-ONLY, #[cfg(unix)] + requires BOTH `pasta` (or
//! `slirp4netns`) AND `bwrap` on the box, + unprivileged user/net namespaces. This
//! whole file is `#[cfg(unix)]`, so the HOST (Windows) compiles it to ZERO tests —
//! host /vet neither runs nor is satisfied by it
//! ([[vet-is-host-side-wsl-insufficient]]). The DECISION/argv/fact host analogues
//! for the same criteria are the host suites `compose_wire_c3.rs` (crit 1 argv +
//! crit 4 decision) and `compose_no_widening_c3_wire.rs` (crit 3).
//!
//! Run WSL-side, single-threaded (netns setup is process-global-ish; parallel
//! netns spawns flake — [[vet-concurrency-flake]]):
//!   CARGO_TARGET_DIR=~/.cache/rezidnt-target \
//!     cargo test -p rezidnt-run --test compose_shared_netns_c3_wire -- --test-threads=1
//!
//! ## RED MODE — COMPILE-RED then LIVE-RED. This asserts against a NEW composed-
//! dataplane seam the implementer must add: `compose::start_composed_dataplane`
//! (or equivalent) that splices `bwrap` between pasta and the confined program on
//! ONE shared netns and returns the same `DataplaneHandle`. Until it exists the
//! target is COMPILE-RED (the honest S4-skeleton signal). Once the seam exists but
//! the composition is unwired, it is LIVE-RED (the probe cannot run through the
//! composed hand-off). On a box WITHOUT both backends, `backends_absent`
//! early-returns — the C3a/c3bc precedent, NEVER a fake pass.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use rezidnt_run::egress::{
    BrokeredSecret, CapturedExchange, DataplaneHandle, Destination, EgressAvailability,
    EgressPolicy, EgressProxy, EnforceWiring, PastaProxy, ProbeReport,
};
use rezidnt_run::sandbox::{Availability, Bind, BwrapSubstrate, SandboxPolicy, SandboxSubstrate};

// The COMPOSED-dataplane seam the implementer must add (DR-028 §Decision 1/2): it
// splices bwrap between pasta and the confined program on ONE shared netns and
// returns the enforce `DataplaneHandle` the escape/injection probe drives — so the
// SAME falsifiable probe now runs from the running AGENT's netns, through the live
// composed run-loop hand-off. Referenced (not defined) here so this file pins the
// entry-point NAME; until it exists this cfg(unix) target fails to compile on WSL.
use rezidnt_run::compose::start_composed_dataplane;

/// The seeded token the agent must NEVER see (criterion 4). Distinctive so the
/// agent-side/upstream capture assertions are adversarial.
const FOLDED_TOKEN: &str = "ghp_composed_secret_the_agent_never_holds_0xC3WIRE";
const TOKEN_REF: &str = "github-token";

/// The folded egress policy: `github.com` allowlisted, mapped to a brokered token.
fn folded_egress() -> EgressPolicy {
    let mut injection = BTreeMap::new();
    injection.insert(
        "github.com".to_string(),
        BrokeredSecret::new(TOKEN_REF, FOLDED_TOKEN),
    );
    EgressPolicy::from_folded_authority(vec![Destination::host("github.com")], injection)
}

/// The folded sandbox policy: the worktree writable + the toolchain read-only (so
/// the confined probe's interpreter/libs resolve inside the namespace on a
/// usr-merged distro — the C3a fixture-repair discipline). `unshare_all = true`;
/// the composed spawn under ACTIVE egress DROPS `--unshare-net` from this (the
/// agent inherits pasta's sealed netns — DR-028 §Decision 1), which is the
/// compose seam's job, not this policy's.
fn folded_sandbox(worktree: &std::path::Path) -> SandboxPolicy {
    let mut binds = vec![
        Bind::writable(worktree),
        Bind::read_only("/usr"),
        Bind::read_only("/bin"),
        Bind::read_only("/lib"),
        Bind::read_only("/lib64"),
    ];
    // Fixture repair (the sanctioned C3a discipline, mirroring
    // `sandbox_bwrap_confinement_c3a.rs`'s `/bin`-only → full-toolchain repair):
    // the confined program is the PROBE, which lives under the cargo target's
    // `debug/examples` dir — OUTSIDE the toolchain binds above. bwrap seals the
    // mount-ns and `execvp`s it, so its directory must be bind-mounted or bwrap
    // reports `No such file or directory`. This is a bind the daemon's real fold
    // supplies for its own harness (`compose::confined_program_binds`); binding it
    // here is the exact same "bind what you're about to exec" property — NO
    // criterion weakened, NO assertion touched.
    if let Some(dir) = probe_bin().parent() {
        binds.push(Bind::read_only(dir));
    }
    SandboxPolicy::from_folded_authority(binds, true)
}

/// Skip the body unless BOTH backends are usable — the composed arm needs pasta
/// (netns + proxy route) AND bwrap (the filesystem confinement spliced inside).
/// The absent arms are host-covered (the degrade suites). Returns true when the
/// caller should early-return.
fn backends_absent(proxy: &PastaProxy, sandbox: &BwrapSubstrate) -> bool {
    !matches!(proxy.availability(), EgressAvailability::Available)
        || !matches!(sandbox.availability(), Availability::Available)
}

/// Locate the dev-only probe/capture example (`egress_c3bc_probe`), the SAME
/// test-support the enforce suite uses. Dev-only (DR-023) — never the shipped
/// daemon binary (I7).
fn probe_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
    let debug = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("target/debug dir");
    debug.join("examples").join("egress_c3bc_probe")
}

/// A live COMPOSED dataplane handle PLUS the test-support upstream capture server
/// it drives. Owns the capture child + temp files so they outlive the run;
/// delegates the `DataplaneHandle` methods to the composed handle. Dropping tears
/// the capture server down. Mirrors the enforce suite's `TestDataplane`, but the
/// inner handle comes from the COMPOSED start (bwrap spliced inside pasta).
struct ComposedTestDataplane {
    inner: Box<dyn DataplaneHandle>,
    capture_child: Child,
    _tmp: tempfile::TempDir,
}

impl DataplaneHandle for ComposedTestDataplane {
    fn proxy_addr(&self) -> &str {
        self.inner.proxy_addr()
    }
    fn run_escape_probe(&self) -> Result<ProbeReport, rezidnt_run::RunError> {
        self.inner.run_escape_probe()
    }
    fn drive_injected_egress(
        &self,
        host: &str,
    ) -> Result<(CapturedExchange, Option<String>), rezidnt_run::RunError> {
        self.inner.drive_injected_egress(host)
    }
}

impl Drop for ComposedTestDataplane {
    fn drop(&mut self) {
        let _ = self.capture_child.kill();
        let _ = self.capture_child.wait();
    }
}

/// Start the live COMPOSED dataplane for a confined mediated-egress run — the seam
/// c3-wire adds. Stands up the capturing upstream TLS server (host namespace),
/// wires the `PastaProxy` enforce wiring + the confined probe binary + the folded
/// SANDBOX policy, and calls the composed start so pasta seals the netns and bwrap
/// runs the probe INSIDE it (pasta-outer -> bwrap -> probe). The returned handle
/// carries a genuine SHARED-netns -> proxy -> upstream byte-path (no fake pass).
fn composed_handle(
    proxy: &PastaProxy,
    sandbox: &BwrapSubstrate,
    egress: &EgressPolicy,
    sbx_policy: &SandboxPolicy,
) -> Box<dyn DataplaneHandle> {
    let tmp = tempfile::tempdir().expect("dataplane tempdir");
    let ca_der_out = tmp.path().join("upstream-ca.der");
    let capture_out = tmp.path().join("upstream-capture.json");

    let upstream_addr = {
        let l = TcpListener::bind("127.0.0.1:0").expect("reserve upstream port");
        l.local_addr().expect("upstream addr")
    };

    let mut capture_child = Command::new(probe_bin())
        .arg("upstream-capture")
        .arg(upstream_addr.to_string())
        .arg(&ca_der_out)
        .arg(&capture_out)
        .arg("github.com")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn upstream capture server");

    let stdout = capture_child.stdout.take().expect("capture stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read capture ready line");
    assert!(
        line.starts_with("UPSTREAM_READY"),
        "capture server signalled ready (got {line:?})"
    );

    let upstream_ca_der = std::fs::read(&ca_der_out).expect("read upstream CA der");

    let wired_proxy = PastaProxy {
        connector_bin: proxy.connector_bin.clone(),
        enforce: Some(EnforceWiring {
            probe_bin: probe_bin(),
            upstream_addr,
            upstream_ca_der,
            upstream_capture_path: capture_out,
        }),
    };

    // The COMPOSED start (DR-028): pasta seals the netns and execs bwrap-execs-probe
    // INSIDE it (shared netns), rather than pasta exec'ing the probe directly. This
    // is the run-loop hand-off criterion 2 must drive — the SAME probe, now confined
    // by bwrap inside pasta's sealed netns.
    let inner = start_composed_dataplane(&wired_proxy, sandbox, egress, sbx_policy).expect(
        "the live COMPOSED egress dataplane starts (pasta netns + bwrap confinement inside it + \
         rustls listener + upstream capture)",
    );

    Box::new(ComposedTestDataplane {
        inner,
        capture_child,
        _tmp: tmp,
    })
}

/// CRITERION 2 (the inescapability centerpiece, UNDER COMPOSITION) — an escape
/// probe run from the RUNNING AGENT's netns (bwrap's child, inheriting pasta's
/// sealed netns) reaches NOTHING but the proxy, AND the proxy path is reached.
/// This is DR-026 crit 3 now proven through the live composed run-loop hand-off,
/// not the enforce suite's self-made netns (DR-028 crit 2).
///
/// NON-VACUOUS guard: the `ViaProxy` attempt MUST succeed — so a box that dropped
/// the agent into a fresh EMPTY netns (a bwrap-re-unshared-net composition bug)
/// fails the `proxy_path_reached` guard, NOT the escape check (a dead netns is not
/// an inescapable one). This is precisely what catches the pasta-outer composition
/// getting the shared-netns hand-off wrong.
///
/// COMPILE-RED until `compose::start_composed_dataplane` exists; LIVE-RED until the
/// composition shares the netns for real; GREEN on a WSL box with pasta + bwrap.
#[test]
fn escape_probe_from_the_agent_netns_reaches_nothing_but_the_proxy_under_composition() {
    let proxy = PastaProxy::default();
    let sandbox = BwrapSubstrate::default();
    if backends_absent(&proxy, &sandbox) {
        eprintln!(
            "pasta and/or bwrap absent on this box; the composed inescapability arm is not \
             applicable here (the decision/argv analogues are host-runnable)"
        );
        return;
    }
    let wt = tempfile::tempdir().expect("worktree tempdir");
    let egress = folded_egress();
    let sbx = folded_sandbox(wt.path());
    let h = composed_handle(&proxy, &sandbox, &egress, &sbx);

    let report: ProbeReport = h
        .run_escape_probe()
        .expect("the confined probe runs inside the AGENT's netns (bwrap inside pasta)");

    // THE inescapability assertion, under composition: NO direct-egress attempt
    // from the agent's netns reached the open internet.
    assert!(
        !report.any_direct_escape_reached_internet(),
        "CRITERION 2 VIOLATION (inescapability HOLE under composition): a direct-egress attempt \
         from the RUNNING AGENT's netns reached the OPEN INTERNET — splicing bwrap inside pasta \
         opened a non-proxy exit. Every direct attempt must reach NOTHING or land on the proxy. \
         Report: {report:?}"
    );
    // NON-VACUOUS: the proxy path itself MUST work through the composed hand-off —
    // a fresh empty netns (bwrap re-unshared net, the composition bug) would fail
    // this, proving the shared-netns inheritance is real.
    assert!(
        report.proxy_path_reached(),
        "CRITERION 2: the ViaProxy path must REACH the proxy THROUGH the composed hand-off (the \
         agent inherited pasta's sealed netns, sole route works) — else the composition dropped \
         the agent into a fresh empty netns (bwrap re-unshared net) and the escape check is \
         vacuous. Report: {report:?}"
    );
}

/// CRITERION 4 (live sealed-netns arm, UNDER COMPOSITION) — a brokered credential
/// is injected on approved egress and the AGENT never sees it, driven through the
/// COMPOSED hand-off: absent from the agent's env + its own request, present only
/// upstream, and the durable fact carries the secret_ref (never the value). Same
/// falsifiable capture as the enforce suite, now with bwrap spliced inside pasta.
///
/// COMPILE-RED until `compose::start_composed_dataplane` exists; GREEN on a WSL box
/// with pasta + bwrap.
#[test]
fn credential_injected_upstream_agent_never_sees_it_under_composition() {
    let proxy = PastaProxy::default();
    let sandbox = BwrapSubstrate::default();
    if backends_absent(&proxy, &sandbox) {
        eprintln!("pasta and/or bwrap absent; the composed injection arm is not applicable here");
        return;
    }
    let wt = tempfile::tempdir().expect("worktree tempdir");
    let egress = folded_egress();
    let sbx = folded_sandbox(wt.path());
    let h = composed_handle(&proxy, &sandbox, &egress, &sbx);

    let (exchange, secret_ref): (CapturedExchange, Option<String>) = h
        .drive_injected_egress("github.com")
        .expect("an approved+mapped egress injects upstream and captures both sides (composed)");

    assert!(
        exchange.upstream_contains(FOLDED_TOKEN),
        "CRITERION 4 (composed): the UPSTREAM request must carry the injected token — the \
         injection reached the upstream the agent never sees. Upstream headers: {:?}",
        exchange.upstream_received_headers
    );
    assert!(
        !exchange.agent_side_contains(FOLDED_TOKEN),
        "CRITERION 4 VIOLATION (composed): the brokered token appeared on the AGENT side (its \
         request headers or its environment). The confined agent must NEVER hold the secret. \
         Agent request: {:?}, agent env keys: {:?}",
        exchange.agent_request_headers,
        exchange.agent_env.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        secret_ref.as_deref(),
        Some(TOKEN_REF),
        "CRITERION 4 (composed): the injection is recorded BY REFERENCE — the secret_ref names it, \
         never the value"
    );
}
