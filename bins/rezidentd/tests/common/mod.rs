//! Shared harness for daemon-level S1..S4 integration tests (unix only).
//!
//! DR-023 §Decision (C): the fixture builders and socket-driving helpers moved
//! to the DEV-ONLY `rezidnt-testkit` crate (consumed here as a
//! `[dev-dependency]`). This module is now a thin re-export shim so the 15
//! `bins/rezidentd/tests/*.rs` files that `mod common; use common::{…}` stay
//! UNCHANGED (`golden_path.rs` among them — its greenness UNCHANGED is the
//! pure-move proof). Nothing daemon-driving lives here anymore; it all lives in
//! the shared testkit.
#![cfg(unix)]
#![allow(unused_imports)] // each integration test uses a subset

pub use rezidnt_testkit::*;
