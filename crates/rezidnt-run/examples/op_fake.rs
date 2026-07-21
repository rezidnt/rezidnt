//! DEV-ONLY test-support: a FAKE 1Password `op` CLI for the c3-op-secrets host
//! suites (DR-030). It mimics `op read op://vault/item/field` deterministically so
//! the host oracle drives `OpSecretSource` WITHOUT a live 1Password — the DR-023
//! fixtures-stay-dev-only precedent (an `[[example]]`, NEVER linked into the daemon
//! binary, so I7's one-static-binary posture is untouched). Cross-platform: a
//! COMPILED helper, not a `.sh` (host `/vet` runs on Windows, where an `.sh` would
//! not exec — DR-030 §"the fake op").
//!
//! ## Contract the host suites rely on (keep in lockstep with them)
//!
//! The argv shape is `op_fake read op://vault/item/field`. `argv[1]` MUST be `read`
//! and `argv[2]` MUST be an `op://…` reference; any other shape exits 2 (the
//! op-shape guard), so a successful resolve proves the source exec'd `op read op://…`.
//!
//! Three env knobs steer the outcome (the `OpSecretSource` passes these to the
//! child; the host tests set them via the source's `with_child_env`, never the
//! shared-process env). `OP_SERVICE_ACCOUNT_TOKEN` present + non-empty is required,
//! else exit 1 (the AUTH-FAILURE floor, DR-030 §Decision 3 — `op` itself refuses
//! without it). `OP_FAKE_EXIT_NONZERO=1` exits 3 with NO stdout (the
//! RESOLUTION-FAILURE floor: item/field not found / no network, DR-030 §Decision 5).
//! This fake vault "holds" ONLY the exact ref `op://Prod/github-token/credential`
//! (the host suites' `OP_REF`), resolving it to the value
//! `op_resolved_secret_value_MUST_STAY_REDACTED_0xC3OPSECRETS`; any other `op://`
//! ref exits 3 (an item the vault lacks, the resolution floor), so the "resolves the
//! exact ref it was asked to read" assertion is falsifiable.
//!
//! On success it prints the value FOLLOWED BY A TRAILING NEWLINE (exactly as real
//! `op read` does — the host suite asserts `OpSecretSource` TRIMS it) and exits 0.
//!
//! The token is read only to CHECK PRESENCE — its value is NEVER echoed to stdout,
//! stderr, or the resolved value (the fake models `op`'s own token-hygiene, so a
//! test that scans for the token in any output stays honest).

use std::io::Write;

/// The one ref this fake vault "holds", and the value it resolves to. Kept in
/// lockstep with the host suites' `OP_REF`/`OP_VALUE` constants.
const HELD_REF: &str = "op://Prod/github-token/credential";
const HELD_VALUE: &str = "op_resolved_secret_value_MUST_STAY_REDACTED_0xC3OPSECRETS";

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // op-shape guard: `read <op://ref>`. A wrong shape is an unknown-subcommand exit,
    // NOT a resolved value — so "the source exec'd `op read op://…`" is provable by a
    // successful resolve of the exact ref.
    let (Some(sub), Some(reference)) = (args.first(), args.get(1)) else {
        eprintln!("op_fake: usage: op read op://vault/item/field");
        return 2;
    };
    if sub != "read" {
        eprintln!("op_fake: unknown subcommand {sub:?} (expected `read`)");
        return 2;
    }
    if !reference.starts_with("op://") {
        eprintln!("op_fake: {reference:?} is not an op:// reference");
        return 2;
    }

    // AUTH floor: `op` refuses without a service-account token. Presence only — the
    // value is NEVER read into any output (token hygiene modeled).
    match std::env::var("OP_SERVICE_ACCOUNT_TOKEN") {
        Ok(t) if !t.is_empty() => {}
        _ => {
            eprintln!("op_fake: [ERROR] no service account token — set OP_SERVICE_ACCOUNT_TOKEN");
            return 1;
        }
    }

    // RESOLUTION floor (explicit knob): the item/field not found, or no network.
    if std::env::var("OP_FAKE_EXIT_NONZERO").as_deref() == Ok("1") {
        eprintln!("op_fake: [ERROR] could not read item (resolution failure)");
        return 3;
    }

    // RESOLUTION floor (unknown item): this vault holds only HELD_REF.
    if reference != HELD_REF {
        eprintln!("op_fake: [ERROR] item not found for {reference:?}");
        return 3;
    }

    // Success: emit the value with a TRAILING NEWLINE (as real `op read` does — the
    // source must TRIM it). The token is never echoed.
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{HELD_VALUE}");
    0
}
