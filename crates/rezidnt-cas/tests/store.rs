//! S1 oracle: content-addressed store contract (doc §10).

use rezidnt_cas::{Cas, CasError};

fn temp_store() -> (tempfile::TempDir, Cas) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, cas)
}

/// Pins the addressing scheme itself with an independently computed vector:
/// blake3 of the exact content, lowercase hex, byte count, mime preserved.
#[test]
fn put_returns_blake3_addressed_ref() {
    let (_dir, cas) = temp_store();
    let content = b"rezidnt cas probe";
    let r = cas.put(content, "text/plain").expect("put");
    // Independent known answer: blake3::hash over the same bytes.
    let expected = blake3::hash(content).to_hex().to_string();
    assert_eq!(
        r.hash, expected,
        "hash must be lowercase blake3 hex of content"
    );
    assert_eq!(r.hash.len(), 64);
    assert_eq!(r.bytes, content.len() as u64);
    assert_eq!(r.mime, "text/plain");
}

/// Write-once semantics: identical content stored twice yields the identical
/// ref and exactly one blob on disk.
#[test]
fn put_is_write_once_idempotent() {
    let (dir, cas) = temp_store();
    let a = cas.put(b"same bytes", "text/plain").expect("first put");
    let b = cas.put(b"same bytes", "text/plain").expect("second put");
    assert_eq!(a, b);
    let blobs = std::fs::read_dir(dir.path())
        .expect("read root")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .count();
    assert_eq!(blobs, 1, "identical content must not produce a second blob");
}

#[test]
fn get_round_trips_content() {
    let (_dir, cas) = temp_store();
    let content = b"round trip me".to_vec();
    let r = cas.put(&content, "application/octet-stream").expect("put");
    let back = cas.get(&r).expect("get");
    assert_eq!(back, content);
}

/// A tampered blob is an error at read time — the store re-verifies content
/// against the addressed hash and never silently returns corrupt data.
#[test]
fn corrupted_blob_detected_on_get() {
    let (_dir, cas) = temp_store();
    let r = cas.put(b"pristine content", "text/plain").expect("put");
    std::fs::write(cas.path_for(&r.hash), b"tampered!").expect("tamper blob on disk");
    match cas.get(&r) {
        Err(CasError::Corrupt { addressed, actual }) => {
            assert_eq!(addressed, r.hash);
            assert_ne!(actual, r.hash);
        }
        other => panic!("tampered blob must yield CasError::Corrupt, got {other:?}"),
    }
}

/// The ref wire shape is pinned: `{hash, bytes, mime}`, nothing else.
#[test]
fn casref_wire_shape_pinned() {
    let (_dir, cas) = temp_store();
    let r = cas.put(b"wire", "text/plain").expect("put");
    let json = serde_json::to_value(&r).expect("serialize");
    let obj = json.as_object().expect("object");
    let mut keys: Vec<_> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["bytes", "hash", "mime"]);
}
