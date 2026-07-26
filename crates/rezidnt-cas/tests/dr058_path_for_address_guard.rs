//! DR-058 ORACLE — §Decision 4: `Cas::path_for` gains a CRATE-LEVEL
//! address-shape guard.
//!
//! The ruled text: the same 64-lowercase-hex rule `rezidnt-mcp`'s
//! `is_cas_address` already enforces at its door, "returning a new
//! `CasError::InvalidAddress` rather than joining an unvalidated string".
//! Until this lands, `path_for` is a bare `root.join(hash)`: `PathBuf::join`
//! REPLACES the root on an absolute component and normalizes no `..`, so every
//! caller that reaches it with an unvalidated string (DR-058 Context names
//! three) is one join away from an arbitrary host path.
//!
//! ## API this board PINS (the work order)
//!
//! - `Cas::path_for(&self, hash: &str) -> Result<PathBuf, CasError>` — the
//!   only signature under which "returning a new error rather than joining"
//!   is expressible. Every non-address argument is `Err(CasError::InvalidAddress
//!   { .. })`; a 64-lowercase-hex argument is `Ok(root/<hex>)`.
//! - `Cas::get` inherits the guard (its first act is `path_for`), so a
//!   non-address `CasRef.hash` is `Err(InvalidAddress)` — never `NotFound`
//!   (which would claim the store LOOKED), never `Corrupt` (which would prove
//!   the store READ something), never `Ok`.
//!
//! ## "No filesystem access attempted", judged behaviourally
//!
//! The traversal and absolute shapes below have REAL files planted at their
//! targets. If the guard let a syscall through, the error could only be
//! `Corrupt` (a read reached the planted bytes and re-hashed them) or a
//! served blob; if it let a stat through and refused on absence elsewhere, the
//! planted-vs-absent split would show. Asserting the exact `InvalidAddress`
//! variant for BOTH planted and unplanted targets is therefore a behavioural
//! proof that no syscall was attempted — the variant is decidable from the
//! argument alone.
//!
//! ## RED MODE (against the tree at cut time)
//!
//! COMPILE-RED: `CasError::InvalidAddress` does not exist and `path_for`
//! returns a bare `PathBuf` (no `Err` to match on). Red for the right reason:
//! the guard is unbuilt.

use rezidnt_cas::{Cas, CasError};
use rezidnt_types::refs::CasRef;

/// A store rooted one level BELOW the tempdir, so `../` traversal targets
/// land inside the tempdir (plantable, cleaned up) rather than in the
/// checkout.
fn temp_store() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(&dir.path().join("cas")).expect("open cas");
    (dir, cas)
}

fn ref_for(hash: &str) -> CasRef {
    CasRef {
        hash: hash.to_string(),
        bytes: 0,
        mime: String::new(),
    }
}

/// The malformed shapes, one per class of wrong. Uppercase is 64 valid hex
/// DIGITS in the wrong case — the Windows hazard: a case-insensitive
/// filesystem resolves it to a DIFFERENT-cased real blob.
const TRAVERSAL: &str = "../outside.txt";
const UPPERCASE: &str = "AA11BB22CC33DD44EE55FF660718293A4B5C6D7E8F90A1B2C3D4E5F607182930";
const TOO_SHORT: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f60718293";
const TOO_LONG: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f6071829300";
const NON_HEX: &str = "zz11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";
const EMPTY: &str = "";

/// A well-formed address this store does not hold.
const ABSENT: &str = "aa11bb22cc33dd44ee55ff660718293a4b5c6d7e8f90a1b2c3d4e5f607182930";

/// THE GUARD — every non-address is refused `InvalidAddress` by `path_for`
/// itself, planted target or not. The refusal is a pure function of the
/// argument: nothing on disk can move it.
#[test]
fn path_for_refuses_every_non_address_with_invalid_address() {
    let (dir, cas) = temp_store();

    // Plant REAL files at the two escape targets, so an implementation that
    // joined-then-touched would have something to find.
    std::fs::write(dir.path().join("outside.txt"), b"planted outside the root")
        .expect("plant traversal target");
    let absolute = dir.path().join("absolute.txt");
    std::fs::write(&absolute, b"planted absolute target").expect("plant absolute target");
    let absolute = absolute.to_string_lossy().to_string();

    let shapes: [(&str, &str); 7] = [
        ("path traversal", TRAVERSAL),
        ("absolute path", absolute.as_str()),
        ("uppercase hex", UPPERCASE),
        ("wrong length (63)", TOO_SHORT),
        ("wrong length (65)", TOO_LONG),
        ("non-hex character", NON_HEX),
        ("empty string", EMPTY),
    ];

    for (label, hash) in shapes {
        let err = cas.path_for(hash).expect_err(label);
        assert!(
            matches!(err, CasError::InvalidAddress { .. }),
            "{label}: a non-address must be CasError::InvalidAddress — \
             not Io, not NotFound (the store never looked), got {err:?}"
        );
    }
}

/// `Cas::get` inherits the guard. The traversal target holds real bytes, so a
/// `Corrupt` here would prove the read REACHED the planted file (its content
/// hashes to something other than the address) and a `NotFound` would prove a
/// lookup was attempted. Only `InvalidAddress` proves no syscall happened.
#[test]
fn get_refuses_a_non_address_before_any_filesystem_access() {
    let (dir, cas) = temp_store();
    std::fs::write(dir.path().join("outside.txt"), b"bytes a read would find")
        .expect("plant traversal target");

    for (label, hash) in [
        ("planted traversal", TRAVERSAL),
        ("uppercase hex", UPPERCASE),
        ("wrong length", TOO_SHORT),
        ("non-hex", NON_HEX),
        ("empty", EMPTY),
    ] {
        let err = cas.get(&ref_for(hash)).expect_err(label);
        assert!(
            matches!(err, CasError::InvalidAddress { .. }),
            "{label}: get on a non-address is InvalidAddress — a Corrupt here \
             means the bytes were READ, a NotFound means the store LOOKED; \
             both are filesystem access the guard must prevent. Got {err:?}"
        );
    }
}

/// The Windows-specific hazard, pinned: an UPPERCASE variant of a REAL blob's
/// address must not resolve to the lowercase-named blob on a case-insensitive
/// filesystem (where, pre-guard, `get` reads the real bytes and reports
/// `Corrupt` — a content-hash oracle). `InvalidAddress`, both platforms.
#[test]
fn an_uppercased_real_address_does_not_reach_the_real_blob() {
    let (_dir, cas) = temp_store();
    let stored = cas
        .put(b"real blob, lowercase address", "text/plain")
        .expect("put");
    let upper = stored.hash.to_ascii_uppercase();
    assert_ne!(upper, stored.hash, "blake3 hex has letters to uppercase");

    let err = cas.get(&ref_for(&upper)).expect_err("uppercase address");
    assert!(
        matches!(err, CasError::InvalidAddress { .. }),
        "an uppercase address is not an address; on a case-insensitive \
         filesystem the join would find the REAL blob and report Corrupt \
         (leaking its content hash). Got {err:?}"
    );
}

/// NON-VACUITY — the guard admits every address this store can actually
/// hold. `put`'s own returned hash resolves through `path_for` to the real
/// blob on disk, `get` round-trips, and a well-formed ABSENT address is still
/// honestly `NotFound` (the store looked, and says so) — a guard refusing
/// everything would pass the tests above.
#[test]
fn a_valid_address_still_resolves_and_an_absent_one_is_still_not_found() {
    let (_dir, cas) = temp_store();
    let content = b"the blob a valid address resolves to";
    let stored = cas.put(content, "text/plain").expect("put");

    let path = cas
        .path_for(&stored.hash)
        .expect("a 64-lowercase-hex address is valid");
    assert_eq!(
        std::fs::read(&path).expect("the resolved path is the blob"),
        content,
        "path_for(valid) resolves to the stored bytes"
    );

    assert_eq!(cas.get(&stored).expect("get round-trips"), content.to_vec());

    let err = cas.get(&ref_for(ABSENT)).expect_err("absent blob");
    assert!(
        matches!(err, CasError::NotFound { .. }),
        "a well-formed address the store does not hold is NotFound, DISTINCT \
         from InvalidAddress — collapsing them would misstate why (I6). Got {err:?}"
    );
}
