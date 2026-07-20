//! rezidnt shared socket-driving client (DR-023).
//!
//! The connect → consume+check-hello → send-`Request` → tail/read primitive
//! BOTH the `rezidnt` CLI and the benchmark harness's `DaemonDriver` drive the
//! daemon on. This is a RELOCATION of the CLI's former private
//! `connect_and_request` (`bins/rezidnt/src/main.rs`), which already sat on
//! `rezidnt-proto`'s public `socket_path`/`decode_hello`/`check_hello`/
//! `encode_request` — so this crate speaks the EXISTING wire (I5), never a
//! parallel protocol.
//!
//! # Invariant-fit (DR-023)
//! - **I5**: rides `rezidnt-proto`'s `Request`/`Hello` — the shared wire.
//! - **I7**: NO new external dependency. `rezidnt-proto` + `rezidnt-types` +
//!   `serde_json` (already-approved wire-serde) + std UDS only. The client's
//!   error type is hand-rolled (not `thiserror`) precisely to hold that closed
//!   dependency closure the `client_deps_hygiene.rs` guard pins; the one
//!   lib-convention exception (thiserror-in-libs) is documented here and bought
//!   by the no-new-external-dep constraint the DR mandates.
//! - **I2**: carries wire frames (facts/refs) only; renders nothing (I1).
//!
//! The UDS path is `#[cfg(unix)]` (as the CLI's client and `golden_path.rs`
//! already are); host builds compile-skip it.

#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Re-export the wire `Request` type so consumers drive the client without a
/// direct `rezidnt-proto` dependency — the client IS the socket seam (DR-023),
/// so the wire request shape it sends rides through it. The harness's
/// `DaemonDriver` names `rezidnt_client::Request` and stays off a direct proto
/// dep (keeping its production dep graph to the testkit_dev_only allow-list).
pub use rezidnt_proto::Request;

/// Client-domain error. Hand-rolled (no `thiserror`) to keep the crate's
/// dependency closure to internal `rezidnt-*` crates + the approved wire-serde
/// only — the I7 constraint DR-023 §Invariant-fit pins (and the
/// `client_deps_hygiene.rs` guard enforces).
#[derive(Debug)]
pub enum ClientError {
    /// A UDS connect/read/write failure, with the operation that failed.
    Io {
        /// What the client was doing when the I/O failed (e.g. `"connect to
        /// daemon"`, `"read hello"`).
        doing: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// A protocol failure: hello decode, hello major-version check, or request
    /// encode (all from `rezidnt-proto`).
    Proto {
        /// What the client was doing (e.g. `"decode hello"`, `"proto check"`).
        doing: String,
        /// The underlying protocol error.
        source: rezidnt_proto::ProtoError,
    },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io { doing, source } => write!(f, "{doing}: {source}"),
            ClientError::Proto { doing, source } => write!(f, "{doing}: {source}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::Io { source, .. } => Some(source),
            ClientError::Proto { source, .. } => Some(source),
        }
    }
}

/// Connect to the daemon over its Unix domain socket (resolved via
/// `rezidnt_proto::socket_path`, i.e. `REZIDNT_SOCKET`/XDG/HOME), consume +
/// check the versioned hello, send `request` as one JSONL frame, and hand back
/// the positioned reader (ready to tail/read facts).
///
/// This is the exact connect→hello-check→send pattern the CLI's `tail` / `open`
/// / `attach` / `debrief` (via `record_alarms`) sit on; a caller reads the
/// reply/fact frames off the returned reader with `read_line` (facts are
/// `rezidnt_types::Event` JSONL, replies are `rezidnt_proto::Reply` frames).
#[cfg(unix)]
pub fn connect_and_request(
    request: &rezidnt_proto::Request,
) -> Result<BufReader<UnixStream>, ClientError> {
    connect_and_request_at(&rezidnt_proto::socket_path(), request)
}

/// Like [`connect_and_request`], but against an EXPLICIT socket path rather than
/// the process-env-resolved one. Used by drivers (e.g. the benchmark harness's
/// `DaemonDriver`) that manage their own daemon socket out-of-band and must not
/// mutate process-global `REZIDNT_SOCKET` (which would be racy across concurrent
/// drives). The CLI keeps using [`connect_and_request`] — a pure move, no
/// behavior change.
#[cfg(unix)]
pub fn connect_and_request_at(
    sock: &std::path::Path,
    request: &rezidnt_proto::Request,
) -> Result<BufReader<UnixStream>, ClientError> {
    use rezidnt_proto::{check_hello, decode_hello, encode_request};

    let stream = UnixStream::connect(sock).map_err(|source| ClientError::Io {
        doing: format!("connect to daemon at {}", sock.display()),
        source,
    })?;
    let mut reader = BufReader::new(stream);

    let mut hello_line = String::new();
    reader
        .read_line(&mut hello_line)
        .map_err(|source| ClientError::Io {
            doing: "read hello".to_string(),
            source,
        })?;
    let hello = decode_hello(hello_line.trim_end()).map_err(|source| ClientError::Proto {
        doing: "decode hello".to_string(),
        source,
    })?;
    check_hello(&hello).map_err(|source| ClientError::Proto {
        doing: "proto check".to_string(),
        source,
    })?;

    let frame = encode_request(request).map_err(|source| ClientError::Proto {
        doing: "encode request".to_string(),
        source,
    })?;
    let stream = reader.get_mut();
    stream
        .write_all(frame.as_bytes())
        .map_err(|source| ClientError::Io {
            doing: "send request".to_string(),
            source,
        })?;
    stream.write_all(b"\n").map_err(|source| ClientError::Io {
        doing: "send request newline".to_string(),
        source,
    })?;
    Ok(reader)
}
