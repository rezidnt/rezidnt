//! Oracle (S3-T7 LOW): the lockfile temp uses `create(true)`, not
//! `create_new`, so the `mode(0o600)` is applied only when the file is
//! CREATED. A pre-existing file at the predictable `.<name>.tmp-<pid>` path
//! keeps its OLD mode (and could keep old content on a short write) — the
//! badge token can be written into a world-readable, attacker-planted file.
//!
//! PIN: writing the lockfile when a stale/hostile tmp already exists at the
//! target tmp path must never inherit that file's mode or content — the
//! resulting lockfile is freshly 0600 and byte-correct. The honest fix is
//! O_EXCL semantics (`create_new`) with unlink-then-recreate, or a fresh
//! unpredictable tmp name; either way the mode is always minted at 0600.
//!
//! RED MODE: assert-red. Today `create(true).truncate(true)` opens the planted
//! 0666 file WITHOUT re-applying mode, so the rename lands a 0666 lockfile —
//! the `assert_eq!(mode, 0o600)` fails. Unix-only (mode assertions) — WSL.
#![cfg(unix)]

use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use rezidnt_mcp::lockfile::Lockfile;

/// The tmp path `write_atomic` derives: `.<name>.tmp-<pid>` next to the target.
/// The pid is THIS process's (write_atomic runs in-process), so the test can
/// plant a hostile file at the exact predictable path the writer will reuse.
fn tmp_path_for(target: &Path) -> std::path::PathBuf {
    let parent = target.parent().expect("target has a parent");
    let name = target
        .file_name()
        .expect("target has a file name")
        .to_string_lossy();
    parent.join(format!(".{name}.tmp-{}", std::process::id()))
}

/// THE PIN: a hostile pre-existing tmp (mode 0666, junk content) at the
/// predictable tmp path must NOT survive into the lockfile. After write_atomic,
/// the lockfile is 0600 and carries exactly the intended fields — no inherited
/// mode, no inherited bytes.
#[test]
fn hostile_preexisting_tmp_does_not_leak_mode_or_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("mcp.lock");
    let tmp = tmp_path_for(&target);

    // Plant the hostile tmp: world-readable (0666), with attacker content that
    // must never end up (in whole or part) in the final lockfile.
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o666);
        let mut planted = opts.open(&tmp).expect("plant hostile tmp");
        use std::io::Write as _;
        // Longer than the real payload, to expose a truncate-not-recreate bug
        // (a short write would leave a trailing attacker tail).
        planted
            .write_all(b"ATTACKER-CONTROLLED-PADDING-".repeat(64).as_slice())
            .expect("write hostile content");
        let mut perms = planted.metadata().expect("stat planted").permissions();
        perms.set_mode(0o666);
        std::fs::set_permissions(&tmp, perms).expect("chmod planted 0666");
    }
    let planted_mode = std::fs::metadata(&tmp)
        .expect("stat planted")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        planted_mode, 0o666,
        "sanity: the planted tmp is world-readable"
    );

    let want = Lockfile {
        pid: std::process::id(),
        port: 43210,
        url: "http://127.0.0.1:43210/mcp".to_string(),
        badge: "cd".repeat(32),
    };
    rezidnt_mcp::lockfile::write_atomic(&target, &want).expect("write_atomic over hostile tmp");

    // Mode: freshly 0600, never the planted 0666.
    let mode = std::fs::metadata(&target)
        .expect("stat lockfile")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o600,
        "the badge token must land 0600 even when a hostile 0666 tmp \
         pre-existed at the predictable path (S3-T7): create_new / O_EXCL, \
         never create+truncate that inherits the planted mode"
    );

    // Content: exactly the intended lockfile, no attacker tail.
    let got = rezidnt_mcp::lockfile::read(&target).expect("read back lockfile");
    assert_eq!(got, want, "no inherited/torn content from the hostile tmp");
    let bytes = std::fs::read(&target).expect("read raw lockfile bytes");
    assert!(
        !bytes.windows(8).any(|w| w == b"ATTACKER"),
        "attacker content survived into the lockfile — the tmp was truncated, \
         not recreated with O_EXCL semantics"
    );
}
