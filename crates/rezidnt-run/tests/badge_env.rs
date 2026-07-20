//! S1 oracle: badges (doc §12) and env scrubbing at spawn.

use rezidnt_run::badge::{BADGE_ENV_VAR, Badge, scrubbed_env};

#[test]
fn badge_tokens_are_256_bit_and_unique() {
    let a = Badge::mint().expect("mint a");
    let b = Badge::mint().expect("mint b");
    assert_eq!(a.token_hex().len(), 64, "32 bytes hex-encoded");
    assert!(a.token_hex().chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a.token_hex(), b.token_hex(), "two mints must differ");
    assert_ne!(a.id(), b.id());
}

/// The badge id is loggable; it must not leak the token.
#[test]
fn badge_id_is_not_the_token() {
    let badge = Badge::mint().expect("mint");
    assert!(!badge.token_hex().contains(badge.id()));
    let debug = format!("{badge:?}");
    assert!(
        !debug.contains(&badge.token_hex()),
        "Debug must omit the token"
    );
}

/// Spawn env discipline: denylisted secrets are dropped, boring vars survive,
/// and exactly one badge var is injected carrying the token.
///
/// SP4b (DR-017): `scrubbed_env` now takes the badge TOKEN string (the value
/// carried under `REZIDNT_BADGE` — a serialized agent macaroon in production),
/// not a `&Badge`. The env SEAM is unchanged; this pins scrubbing + exactly-once
/// injection independent of the token's shape, so a token string stands in.
#[test]
fn scrubbed_env_drops_secrets_keeps_boring_injects_badge() {
    let badge_token = "run-macaroon-wire-token";
    let parent = vec![
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("HOME".to_string(), "/home/u".to_string()),
        ("GITHUB_TOKEN".to_string(), "ghp_secret".to_string()),
        ("ANTHROPIC_API_KEY".to_string(), "sk-secret".to_string()),
        (
            "AWS_SECRET_ACCESS_KEY".to_string(),
            "aws-secret".to_string(),
        ),
        ("DB_PASSWORD".to_string(), "hunter2".to_string()),
        (
            "SQL_CONNECTION_STRING".to_string(),
            "Server=...".to_string(),
        ),
        ("LANG".to_string(), "C.UTF-8".to_string()),
    ];
    let child = scrubbed_env(parent.into_iter(), badge_token);

    let names: Vec<&str> = child.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"PATH"));
    assert!(names.contains(&"HOME"));
    assert!(names.contains(&"LANG"));
    for secret in [
        "GITHUB_TOKEN",
        "ANTHROPIC_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "DB_PASSWORD",
        "SQL_CONNECTION_STRING",
    ] {
        assert!(!names.contains(&secret), "{secret} must be scrubbed");
    }
    let badges: Vec<_> = child.iter().filter(|(k, _)| k == BADGE_ENV_VAR).collect();
    assert_eq!(badges.len(), 1, "exactly one badge var");
    assert_eq!(
        badges[0].1, badge_token,
        "the injected value is the token verbatim"
    );
}
