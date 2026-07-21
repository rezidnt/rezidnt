//! c3-wire oracle (DR-028) — CRITERION 5 (handle-shape arm, HOST-provable): the
//! composed spawn returns a DAEMON-OWNED `tokio::process::Child` (stdout piped,
//! adoptable by the daemon reaper), NOT only a pid plus a detached waiter.
//!
//! ## Why a separate host file (the type-shape pin)
//! DR-028 §Decision 2 (the seam reshape) is the whole point of this arm: today
//! `SandboxSubstrate::spawn_confined` returns a `SandboxedChild { backend, pid }`
//! and DETACHES its own reaper thread (`sandbox.rs:328`), explicitly deferring
//! "the concrete daemon-owned handle shape … to the wider run-loop". c3-wire
//! finally threads the real `tokio::process::Child` so `runs.rs` can pipe stdout
//! and the daemon reaper can adopt it. This file pins that RETURN TYPE at the
//! seam — the live reap under bwrap is the WSL daemon suite
//! (`bins/rezidentd/tests/spawn_composed_c3_wire.rs`), but the OWNERSHIP SHAPE (a
//! real tokio Child, not a pid + orphan waiter) is host-provable at the type
//! level, and that is exactly the S1 contract the reshape must satisfy.
//!
//! ## SUITE PLACEMENT — HOST-RUNNABLE, no #[cfg(unix)]. A type-level assertion on
//! the composed-spawn return type; compiles and runs on every host.
//!
//! ## RED MODE — COMPILE-RED. The `compose::ComposedChild` type + its accessor
//! returning `&mut tokio::process::Child` do not exist yet; this target fails to
//! compile until the implementer adds the daemon-owned composed-child handle. A
//! `SandboxedChild { pid }`-only return (the deferred shape) does NOT satisfy this
//! pin — that is the regression it guards.

// The composed-child handle the implementer must add (DR-028 §Decision 2). The
// composed spawn returns THIS — a daemon-owned `tokio::process::Child` the run
// loop consumes stdout from and the reaper adopts, not a bare pid.
use rezidnt_run::compose::ComposedChild;

/// CRITERION 5 (handle shape) — `ComposedChild` exposes the daemon-owned
/// `tokio::process::Child`. This is a COMPILE-TIME pin: the accessor's return type
/// must be `tokio::process::Child` (owned) / `&mut tokio::process::Child`, so the
/// run loop can `.stdout.take()` and the reaper can `.wait()`/adopt it — the S1
/// "daemon owns the process" contract the reshape threads (DR-028 §Decision 2).
///
/// It is a static type assertion, not a spawn (spawning bwrap/pasta is the WSL
/// suite). If the composed spawn returns only a pid + detached waiter (the shape
/// `sandbox.rs` deferred), this function does not type-check and the target is RED.
#[allow(dead_code, unused_variables, unused_mut)]
fn composed_child_yields_a_daemon_owned_tokio_child(mut child: ComposedChild) {
    // The run loop must be able to TAKE the piped stdout off the owned child (the
    // capture seam `runs.rs:791` needs). The accessor returns an owned tokio Child.
    let owned: tokio::process::Child = child.into_child();
    // And the reaper adopts the SAME owned handle (S1) — `wait()` is a
    // `tokio::process::Child` method, so this only compiles if the type is right.
    let _reap_fut = owned; // ownership moved to the (daemon) reaper, not detached here
}

/// A companion pin on the borrowing accessor: the run loop consumes stdout via a
/// `&mut tokio::process::Child` WITHOUT taking ownership (so the reaper still owns
/// it). Both accessors are on the implementer's work order; this makes the
/// dual-accessor shape a compile requirement, not a suggestion.
///
/// COMPILE-RED until `ComposedChild::child_mut(&mut self) -> &mut tokio::process::Child`.
#[allow(dead_code, unused_variables)]
fn composed_child_lends_a_tokio_child_mut(child: &mut ComposedChild) {
    let borrowed: &mut tokio::process::Child = child.child_mut();
    // The piped stdout is takeable off the borrow — the run loop's capture path.
    let _stdout = borrowed.stdout.take();
}

/// A trivial runnable test so the target is not warned-empty; the real assertions
/// above are compile-time. The presence of the two functions above is what makes
/// the target RED until the seam exists.
#[test]
fn composed_child_handle_shape_is_pinned_at_compile_time() {
    // Nothing to run: the ownership contract is enforced by the two fns above
    // type-checking. This body documents the intent for a reader of the report.
}
