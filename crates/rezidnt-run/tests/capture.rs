//! S1 oracle: ring buffer + CAS chunking (I2 — refs on the fabric, never bytes).

use rezidnt_cas::Cas;
use rezidnt_run::RunId;
use rezidnt_run::capture::{DEFAULT_CHUNK_BYTES, DEFAULT_RING_BYTES, RingBuffer, chunk_into_cas};
use rezidnt_types::MAX_PAYLOAD_BYTES;
use ulid::Ulid;

/// Under overflow the ring keeps exactly the newest `capacity` bytes, oldest
/// first — the live-tail contract `attach` replays. (The DEFAULT capacity pin
/// lives here rather than alone: a constants-only test would pass before the
/// implementation exists, which the honesty rule forbids.)
#[test]
fn ring_keeps_newest_bytes_in_order() {
    assert_eq!(DEFAULT_RING_BYTES, 256 * 1024);
    assert_eq!(
        RingBuffer::with_capacity(DEFAULT_RING_BYTES).capacity(),
        DEFAULT_RING_BYTES
    );

    let mut ring = RingBuffer::with_capacity(8);
    ring.push(b"abcdefgh");
    assert_eq!(ring.snapshot(), b"abcdefgh");
    ring.push(b"XY");
    assert_eq!(
        ring.snapshot(),
        b"cdefghXY",
        "oldest two bytes must be evicted"
    );
    ring.push(b"0123456789AB"); // larger than capacity in one push
    assert_eq!(ring.snapshot(), b"456789AB");
}

#[test]
fn ring_short_content_snapshots_whole() {
    let mut ring = RingBuffer::with_capacity(1024);
    ring.push(b"short");
    assert_eq!(ring.snapshot(), b"short");
}

/// Chunking pins three properties at once: (1) manifest is ordered and
/// complete — concatenating the CAS blobs reproduces the stream exactly;
/// (2) every manifest entry serializes under the I2 cap by construction;
/// (3) no manifest payload contains the stream bytes inline.
#[test]
fn chunking_round_trips_and_manifests_carry_refs_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    let run = RunId::new(Ulid::from_parts(1, 42));

    // 150 KiB of recognizable, non-repeating-ish content → 3 chunks at 64 KiB.
    let stream: Vec<u8> = (0..150 * 1024).map(|i| (i % 251) as u8).collect();
    let manifest =
        chunk_into_cas(&cas, run, &stream, DEFAULT_CHUNK_BYTES).expect("chunking succeeds");

    assert_eq!(manifest.len(), 3, "150 KiB at 64 KiB chunks = 3 entries");
    let mut reassembled = Vec::new();
    for (i, entry) in manifest.iter().enumerate() {
        assert_eq!(
            entry.chunk, i as u64,
            "manifest must be ordered by chunk ordinal"
        );
        assert_eq!(entry.run, run);
        reassembled.extend(cas.get(&entry.r#ref).expect("chunk blob readable"));

        let payload = serde_json::to_string(entry).expect("entry serializes");
        assert!(
            payload.len() <= MAX_PAYLOAD_BYTES,
            "manifest payload must fit the I2 cap, got {} bytes",
            payload.len()
        );
        // The manifest carries hashes and counts — never content. A ref-only
        // entry is a few hundred bytes; a 64 KiB chunk inlined cannot hide.
        assert!(
            payload.len() < 1024,
            "manifest entry must be ref-sized, not content-sized: {} bytes",
            payload.len()
        );
    }
    assert_eq!(
        reassembled, stream,
        "concatenated chunks must reproduce the stream"
    );
}
