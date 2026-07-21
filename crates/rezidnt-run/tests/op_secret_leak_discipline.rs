//! c3-op-secrets oracle (DR-030) — CRITERION 4 (HOST-provable): I2/DR-026
//! LEAK-DISCIPLINE for the op backend. The op-RESOLVED secret VALUE rides NO fact
//! (only the `op://` ref rides, as `secret_ref`); `.expose()` remains the SINGLE
//! call-site in the run crate (a structural / grep-style guard); and the
//! service-account token is never placed in the confined agent's env (the child env
//! is `env_clear`'d + folded — the token isn't in the folded child env). DR-030
//! §Decision 3 / §Invariant-fit I2.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE. Pure fact-shape + structural-source-scan +
//! folded-env inspection. No netns, no live op. Windows /vet included. The op-arm
//! resolve is driven through the fold with the compiled fake op where a value is
//! needed; the never-in-a-LIVE-fact scan is the WSL crit-5 suite.
//!
//! ## RED MODE — mixed.
//!   - the by-reference `injected_fact` value-absence scan for an op:// ref: the
//!     `injected_fact` constructor EXISTS (DR-029) and carries only `secret_ref` —
//!     so this arm proves the op:// ref (not the resolved value) is what rides. It
//!     holds GREEN today and STAYS the forcing function against a careless
//!     `.expose()` inline for an op-resolved secret.
//!   - the SINGLE-`.expose()` structural scan is a guard that FAILS if the op
//!     backend adds a second `.expose()` call-site (e.g. resolving by exposing into
//!     a log/argv). It scans `crates/rezidnt-run/src` and asserts exactly ONE real
//!     call-site (the sanctioned upstream-write, egress.rs). A careless op impl that
//!     logs `.expose()` breaks this FIRST.
//!   - the token-not-in-the-folded-child-env arm is COMPILE-RED against the folded
//!     child-env seam (`scrubbed_env`/the composed folded env) — it asserts the
//!     service-account token, even when present in the DAEMON env, is NOT in the
//!     env handed to the confined agent.

use std::path::{Path, PathBuf};

use rezidnt_run::egress::{BrokeredSecret, injected_fact};
use rezidnt_run::secret::{OpSecretSource, SecretSource};
use rezidnt_types::refs::CasRef;

/// A distinctive op-resolved value that must ride NO fact.
const OP_VALUE: &str = "op_resolved_value_MUST_NEVER_RIDE_A_FACT_0xC3LEAK";
/// The op reference — the ONLY secret-identifying thing that may ride a fact.
const OP_REF: &str = "op://Prod/github-token/credential";
/// A distinctive service-account token that must be in NO agent env / NO fact.
const SA_TOKEN: &str = "ops_service_account_token_MUST_NEVER_LEAK_0xC3LEAK";

const RUN: &str = "01C3OPSECRETSLEAKDISC00RN1";

fn op_fake_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current test exe");
    let debug = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("target/debug dir");
    let mut p = debug.join("examples").join("op_fake");
    if cfg!(windows) {
        p.set_extension("exe");
    }
    p
}

/// CRITERION 4 (value in no fact — only the op:// ref rides) — an op-resolved
/// `BrokeredSecret`'s `credential.injected` fact carries the `op://` REF as its
/// `secret_ref` (a NAME: vault/item/field), and the resolved VALUE literally cannot
/// be found in the serialized fact. The op:// path is a reference, so it rides
/// exactly as a plain label does (DR-030 §Decision 2, by-ref-never-value).
#[test]
fn op_injected_fact_carries_the_op_ref_never_the_resolved_value() {
    let policy_ref = CasRef {
        hash: "po11c3opsecrets00000000000000000000000000000000000000000000op".to_string(),
        bytes: 128,
        mime: "application/octet-stream".to_string(),
    };
    // A brokered secret as the op backend would build it: secret_ref = the op:// REF,
    // value = the op-resolved token.
    let secret = BrokeredSecret::new(OP_REF, OP_VALUE);
    let (subject, payload) = injected_fact(RUN, "github.com", &secret, &policy_ref);

    assert_eq!(subject, "credential.injected");
    assert_eq!(
        payload["secret_ref"].as_str(),
        Some(OP_REF),
        "CRITERION 4: the op:// REFERENCE rides as secret_ref — a NAME, not a value (DR-030 §Decision 2)"
    );

    let serialized = serde_json::to_string(&payload).expect("payload serializes");
    assert!(
        !serialized.contains(OP_VALUE),
        "CRITERION 4 VIOLATION (CATASTROPHIC): the op-RESOLVED VALUE appeared in the \
         credential.injected payload — only the op:// secret_ref may ride, NEVER the value \
         (DR-030 §Invariant-fit I2, DR-026 crit 5). Payload: {serialized}"
    );
    assert!(
        serialized.contains(OP_REF),
        "non-vacuous: the op:// ref rides the fact (the by-reference label is present) — so the \
         value-absence scan is meaningful"
    );
}

/// CRITERION 4 (`.expose()` remains the SINGLE call-site) — a structural scan of the
/// run crate's source: exactly ONE real `.expose()` call-site (the sanctioned
/// upstream-write in `egress.rs`, on the plaintext the agent never sees). The op
/// backend must NOT add a second `.expose()` (e.g. exposing into a log line, an
/// argv, or a fact). This is the grep-style guard DR-030 §Decision 3 / DR-026 crit 5
/// names — it FAILS first if a careless op impl leaks the value through a new
/// call-site.
#[test]
fn expose_remains_the_single_call_site_in_the_run_crate() {
    // Walk `crates/rezidnt-run/src` from this test's manifest dir.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut call_sites: Vec<String> = Vec::new();
    scan_expose(&src, &mut call_sites);

    assert_eq!(
        call_sites.len(),
        1,
        "CRITERION 4 VIOLATION: `.expose()` is called at {} site(s), expected EXACTLY ONE (the \
         sanctioned upstream-write on the plaintext the agent never sees). The op backend must not \
         add a call-site (resolving must capture stdout into a BrokeredSecret, NEVER `.expose()` \
         into a log/argv/fact — DR-030 §Decision 3 / DR-026 crit 5). Sites:\n{}",
        call_sites.len(),
        call_sites.join("\n")
    );
    assert!(
        call_sites[0].contains("egress.rs"),
        "the single `.expose()` call-site lives on the upstream-write path in egress.rs; got {:?}",
        call_sites[0]
    );
}

/// Recursively scan `.rs` files under `dir` for REAL `.expose()` call-sites,
/// skipping line comments (`//`) so the many doc/comment mentions of `.expose()`
/// are not miscounted. Records `path:line` for each hit.
fn scan_expose(dir: &Path, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).expect("read src dir");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            scan_expose(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let text = std::fs::read_to_string(&path).expect("read rs file");
            for (i, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if code.contains(".expose()") {
                    out.push(format!("{}:{}", path.display(), i + 1));
                }
            }
        }
    }
}

/// CRITERION 4 (the token is never in the confined agent's env) — the service-account
/// token, even when present in the DAEMON's env, is NOT in the env folded to the
/// confined agent. The confined child env is `env_clear`'d + folded (sandbox.rs:371,
/// `scrubbed_env`), so a daemon-side secret cannot leak into the agent (DR-030
/// §Decision 3).
///
/// HONESTY NOTE (oracle discipline): this arm is ALREADY GREEN — `scrubbed_env`'s
/// DEFAULT denylist drops any `*_TOKEN` name (badge.rs:108), and
/// `OP_SERVICE_ACCOUNT_TOKEN` ends in `_TOKEN`, so the existing scrub already covers
/// it. It is deliberately a REGRESSION GUARD, not a false-red: it FAILS if the op
/// wiring weakens the scrub (whitelists the token to "make op work in the sandbox" —
/// it must not; op runs DAEMON-side pre-seal) or renames the var off the `_TOKEN`
/// suffix. Stated so the auditor is not misled that it drives new code.
#[test]
fn service_account_token_is_not_in_the_confined_agent_env() {
    // The daemon-side env the daemon runs with (token present, as in production).
    let daemon_env: Vec<(String, String)> = vec![
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("OP_SERVICE_ACCOUNT_TOKEN".to_string(), SA_TOKEN.to_string()),
        ("HOME".to_string(), "/root".to_string()),
    ];

    // The folded env handed to the CONFINED agent — the scrubbed/badge env the
    // sandbox builds. `scrubbed_env` EXISTS (DR-005/DR-017); this asserts the op
    // service-account token is scrubbed like every other daemon secret.
    let child_env = rezidnt_run::badge::scrubbed_env(daemon_env.into_iter(), "badge-value");

    assert!(
        !child_env
            .iter()
            .any(|(k, _)| k == "OP_SERVICE_ACCOUNT_TOKEN"),
        "CRITERION 4 VIOLATION: OP_SERVICE_ACCOUNT_TOKEN survived into the confined agent's env — \
         the service-account token is DAEMON-side only (op resolves pre-seal); it must NEVER be in \
         the agent's env (DR-030 §Decision 3). Folded child env keys: {:?}",
        child_env.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );
    // And the token VALUE appears in NO folded env value either.
    assert!(
        !child_env.iter().any(|(_, v)| v.contains(SA_TOKEN)),
        "CRITERION 4 VIOLATION: the service-account token VALUE appeared in a folded child-env \
         value — the token must not leak into the agent under any key (DR-030 §Decision 3)"
    );
}

/// CRITERION 4 (the token is never in the op-resolve outcome the daemon holds) — a
/// full op resolve through the injected fake, then a scan of the resolved
/// `BrokeredSecret`'s Debug/Display + secret_ref: the service-account token never
/// appears (only the op:// ref and the redaction sentinel do). The token authed the
/// child via env; it must not ride back in the resolved secret's surfaces.
///
/// COMPILE-RED until the op source exists.
#[test]
fn service_account_token_never_rides_the_resolved_secrets_surfaces() {
    let source = OpSecretSource::new()
        .with_binary(op_fake_bin())
        .with_child_env(vec![(
            "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
            SA_TOKEN.to_string(),
        )]);
    let resolved = source.resolve(OP_REF);
    // Whether it resolved (fake present) or dropped (fake absent), NO surface carries
    // the token. Scan the whole outcome Debug + the resolved secret_ref/Debug/Display.
    let outcome_dbg = format!("{resolved:?}");
    assert!(
        !outcome_dbg.contains(SA_TOKEN),
        "CRITERION 4 VIOLATION: the service-account token appeared in the op-resolve outcome \
         ({outcome_dbg:?}) — env-only, never in a return/log surface (DR-030 §Decision 3)"
    );
    if let Ok(Some(secret)) = resolved {
        let dbg = format!("{secret:?}");
        let disp = format!("{secret}");
        assert!(
            !secret.secret_ref().contains(SA_TOKEN)
                && !dbg.contains(SA_TOKEN)
                && !disp.contains(SA_TOKEN),
            "CRITERION 4 VIOLATION: the service-account token rode the resolved secret's secret_ref/\
             Debug/Display — the auth token never surfaces on the brokered secret (DR-030 §Decision 3)"
        );
    }
}
