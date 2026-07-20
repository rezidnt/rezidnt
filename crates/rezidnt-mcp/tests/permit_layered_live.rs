//! SP4c-wire oracle — the LIVE three-source layered permit merge (DR-020,
//! ACCEPTED 2026-07-20). This is the MCP-live end-to-end proof for C8 that the
//! `rezidnt-gate`-seam file `crates/rezidnt-gate/tests/permit_layered_precedence.rs`
//! deferred: an admin-layer deny is non-overridable by a session-layer allow
//! THROUGH the live `request_permission` path (`decide_permit`), and the emitted
//! `permit.denied` fact surfaces the deciding `layer == "admin"` on the wire.
//!
//! DR-020 §Decision fences the still-unbuilt wiring to exactly this slice:
//!   3. `McpCore::with_layered_permit_config(admin, dev, session)` — a builder
//!      mirroring `with_permit_config` (crates/rezidnt-mcp/src/lib.rs:392) that
//!      injects THREE already-resolved layers (each a `Vec<PermitVerifierSpec>`,
//!      the resolved `[gates.permit]` block for that authority level) and stores
//!      the merged set as `PermitConfig::from_specs(permit::compose_layers(admin,
//!      dev, session))`. The gate-side `compose_layers` / `PermitLayer` /
//!      `PermitOutcome::deciding_layer` are ALREADY shipped (permit.rs); this
//!      slice only feeds three real layers through them at the MCP seam.
//!   4. The emit path (`lib.rs:844-848`) pins `outcome.deciding_layer` into the
//!      CAS policy blob alongside `deciding_verifier`, so `gate_explain`'s
//!      `policy_ref` blob carries `"layer": "admin"` — DR-019 criterion 2's
//!      interrogability made LIVE on the wire, not latent in the type.
//!
//! RED MODE: **compile-red** first. Every test references
//! `McpCore::with_layered_permit_config`, which does NOT exist yet — the whole
//! file fails to compile until the implementer lands the builder. Then, for
//! CRITERION 3, **assert-red**: the emit path does not yet pin `"layer"` in the
//! policy blob, so the `deciding_layer` read-back is absent until DR-020 §Decision
//! 4 lands. These are NOT `#[ignore]`-gated: DR-020 RATIFIES the seam, so they are
//! standard oracle-first RED that the implementer turns green in THIS slice
//! BEFORE `/vet` runs.
//!
//! The deterministic-judge lever is the `tool-allowlist` native (mirroring
//! `permit_wire_dispatch.rs`): a requested tool IN a layer's `allow` → Pass, a
//! tool EXCLUDED → Fail. So every aggregate outcome is a pure function of the
//! merged admin→dev→session order — no exec subprocess, no fixtures, fully
//! replayable (testing-oracles).
//!
//! IMPLEMENTER NOTE (minimal target API this file assumes — keep it small):
//!   - `McpCore::with_layered_permit_config(admin: Vec<PermitVerifierSpec>, dev:
//!     Vec<PermitVerifierSpec>, session: Vec<PermitVerifierSpec>) -> Self`
//!     (builder-style, mirrors `with_permit_config`; stores
//!     `PermitConfig::from_specs(rezidnt_gate::permit::compose_layers(admin, dev,
//!     session))`).
//!   - the emit path adds `"layer": outcome.deciding_layer.map(|l| l.as_str())`
//!     to the policy blob at `lib.rs:844-848` (DR-020 §Decision 4). The daemon's
//!     `permit_config_for` three-source merge is proven separately (bins/rezidentd);
//!     this file pins the CORE seam + the emitted-layer interrogability.

mod util;

use std::sync::Arc;

use rezidnt_cas::Cas;
use rezidnt_fabric::{EventLog, Fabric};
use rezidnt_gate::permit::{PermitLayer, PermitVerifierSpec};
use rezidnt_mcp::{BadgeBook, McpCore};
use rezidnt_run::badge::Badge;
use rezidnt_types::refs::CasRef;
use rezidnt_types::{Event, SourceId, Subject};
use serde_json::json;
use ulid::Ulid;

/// A core whose permit gate is CONFIGURED with three resolved layers
/// (admin/dev/session), badge pre-admitted, over a fresh temp log. A caller-owned
/// CAS is wired via `with_cas` so the test can read back the pinned policy blob
/// (`policy_ref`) and assert the emitted `deciding_layer` (CRITERION 3). The
/// `with_layered_permit_config` builder is the SP4c-wire seam DR-020 ratifies —
/// it does not exist yet, so this file is compile-red against it.
fn core_with_layers(
    badge: &Badge,
    cas: Arc<Cas>,
    admin: Vec<PermitVerifierSpec>,
    dev: Vec<PermitVerifierSpec>,
    session: Vec<PermitVerifierSpec>,
) -> (tempfile::TempDir, Arc<McpCore>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = EventLog::open(&dir.path().join("events.db")).expect("open log");
    let fabric = Fabric::new(log, 1024);
    let mut book = BadgeBook::new();
    book.admit(badge);
    let core = McpCore::new(fabric, book)
        .with_cas(cas)
        .with_layered_permit_config(admin, dev, session);
    (dir, Arc::new(core))
}

/// A caller-owned CAS the test both wires into the core AND reads the pinned
/// policy blob back out of. Returns the tempdir guard so the store outlives the
/// test.
fn shared_cas() -> (tempfile::TempDir, Arc<Cas>) {
    let dir = tempfile::tempdir().expect("cas tempdir");
    let cas = Cas::open(dir.path()).expect("open cas");
    (dir, Arc::new(cas))
}

/// Publish an `agent.spawned` so the run exists on the log for `decide_permit`
/// to resolve against (mirrors `permit_wire_dispatch.rs::seed_run_with_intent`
/// minus the intent — the layered proof turns only on `tool-allowlist`, no
/// intent axis).
fn seed_run(core: &McpCore, run: &str) {
    let spawned = Event::new(
        SourceId::new("rezidnt-run"),
        None,
        Subject::new("agent.spawned"),
        Ulid::new(),
        None,
        1,
        json!({"run": run, "agent": "impl", "harness": "claude-code"}),
    )
    .expect("spawned envelope");
    core.fabric().publish(spawned).expect("publish spawned");
}

/// A layer that DENIES the request tool: a `tool-allowlist` whose `allow`
/// excludes it → Fail. Stamped with `layer` provenance (the SP4c dispatch unit).
fn deny_layer(layer: PermitLayer) -> Vec<PermitVerifierSpec> {
    vec![PermitVerifierSpec::native_in_layer(
        layer,
        "tool-allowlist",
        json!({ "allow": ["Read"] }), // "Edit" excluded → Fail
    )]
}

/// A layer that GRANTS the request tool: a `tool-allowlist` whose `allow`
/// includes it → Pass. Stamped with `layer` provenance.
fn grant_layer(layer: PermitLayer) -> Vec<PermitVerifierSpec> {
    vec![PermitVerifierSpec::native_in_layer(
        layer,
        "tool-allowlist",
        json!({ "allow": ["Read", "Edit", "Bash"] }),
    )]
}

/// Drive an `Edit` request through the LIVE `request_permission` path and return
/// the machine-readable payload (`decision`, ...).
async fn request_edit(core: &McpCore, id: u64, badge: &Badge, run: &str) -> serde_json::Value {
    let result = util::tool_call(
        core,
        id,
        "request_permission",
        json!({"badge": badge.token_hex(), "run": run, "action": "tool.invoke", "tool": "Edit"}),
    )
    .await;
    util::tool_payload(&result)
}

// ---------------------------------------------------------------------------
// CRITERION 2 (HEADLINE) — admin deny is NOT overridable by a session allow,
// LIVE through `request_permission` (stricter-wins END-TO-END, not just the gate
// unit). DR-020 §Acceptance-criteria sketch 2.
// ---------------------------------------------------------------------------

/// CRITERION 2 — the SAME `Edit` request, where the ADMIN layer contributes a
/// `tool-allowlist` that DENIES `Edit` and the SESSION layer would GRANT it,
/// drives the live decision to `deny` (`permit.denied`), NEVER `allow`. The
/// session allow cannot un-Fail the admin deny because `compose_layers` puts
/// admin FIRST and the aggregate has no allow-override primitive (DR-019 Decision
/// 1, frozen). This proves stricter-wins on the WIRE, not just in the gate.
///
/// COMPILE-RED (no `with_layered_permit_config`) then LIVE-asserts `deny`.
#[tokio::test]
async fn live_admin_deny_not_overridable_by_session_allow() {
    let badge = Badge::mint().expect("mint badge");
    let (_cas_dir, cas) = shared_cas();

    // admin DENIES Edit; dev empty; session WOULD allow Edit.
    let admin = deny_layer(PermitLayer::Admin);
    let dev: Vec<PermitVerifierSpec> = vec![];
    let session = grant_layer(PermitLayer::Session);

    let (_dir, core) = core_with_layers(&badge, cas, admin, dev, session);
    const RUN: &str = "01SP4CWIREADMINDENY000R01";
    seed_run(&core, RUN);

    let payload = request_edit(&core, 1, &badge, RUN).await;
    assert_eq!(
        payload["decision"],
        json!("deny"),
        "the ADMIN-layer deny is NON-OVERRIDABLE by a later SESSION allow through \
         the LIVE request_permission path — stricter-wins END-TO-END, never a \
         session allow rescuing an admin deny (DR-020 criterion 2). payload={payload:#}"
    );

    // The durable proof: exactly one `permit.denied` fact for this run — the
    // session allow never manufactured a `permit.granted`.
    let facts = util::log_events(&core);
    let denied = facts
        .iter()
        .filter(|e| e.subject.as_str() == "permit.denied" && e.payload()["run"] == json!(RUN))
        .count();
    let granted = facts
        .iter()
        .filter(|e| e.subject.as_str() == "permit.granted" && e.payload()["run"] == json!(RUN))
        .count();
    assert_eq!(denied, 1, "exactly one permit.denied fact was emitted");
    assert_eq!(
        granted, 0,
        "the session allow NEVER manufactured a permit.granted — admin's deny stood"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 3 — the emitted `permit.denied` fact / `gate_explain` surfaces the
// deciding `layer == "admin"`, disambiguated by AUTHORITY (two identically-named
// `tool-allowlist` verifiers in different layers). DR-020 §Acceptance-criteria
// sketch 3; §Decision 4 (pin `deciding_layer` in the policy blob).
// ---------------------------------------------------------------------------

/// CRITERION 3 — two identically-named `tool-allowlist` verifiers sit in the
/// admin and session layers; only the LAYER distinguishes which authority
/// decided. The admin one denies, so the emitted decision fact's `policy_ref`
/// blob (read back from CAS) must carry `"layer": "admin"` — `gate_explain`
/// answers "why blocked" with the deciding LAYER, not merely the (ambiguous)
/// verifier NAME (I6, DR-019 criterion 2 LIVE on the wire).
///
/// COMPILE-RED (seam) then ASSERT-RED: the emit path does not pin `"layer"` yet
/// (DR-020 §Decision 4 adds it), so the read-back key is absent until it lands.
#[tokio::test]
async fn live_permit_denied_fact_surfaces_deciding_layer_admin() {
    let badge = Badge::mint().expect("mint badge");
    let (_cas_dir, cas) = shared_cas();
    let read_cas = Arc::clone(&cas);

    // BOTH admin and session carry a `tool-allowlist` — the NAME is ambiguous;
    // only the LAYER distinguishes which authority denied. Admin denies Edit.
    let admin = deny_layer(PermitLayer::Admin);
    let dev: Vec<PermitVerifierSpec> = vec![];
    let session = grant_layer(PermitLayer::Session);

    let (_dir, core) = core_with_layers(&badge, cas, admin, dev, session);
    const RUN: &str = "01SP4CWIRELAYERADMIN00R01";
    seed_run(&core, RUN);

    let payload = request_edit(&core, 1, &badge, RUN).await;
    assert_eq!(
        payload["decision"],
        json!("deny"),
        "precondition — the admin layer decides deny. payload={payload:#}"
    );

    // Interrogate via gate_explain and pull the deciding policy's `policy_ref`.
    let explain = util::tool_call(&core, 2, "gate_explain", json!({"run": RUN})).await;
    let ex = util::tool_payload(&explain);
    assert_eq!(
        ex["verdict"],
        json!("deny"),
        "gate_explain reports the deny (interrogability precondition). explain={ex:#}"
    );

    // The `policy_ref` is an opaque CasRef {hash, bytes, mime} (warden-confirmed);
    // read the pinned policy blob back from the SAME CAS the core wrote it to and
    // assert it carries the deciding LAYER (DR-020 §Decision 4 pins it here).
    let policy_ref: CasRef = serde_json::from_value(ex["policy_ref"].clone())
        .unwrap_or_else(|e| panic!("policy_ref is an opaque CasRef ({e}): {ex:#}"));
    let blob = read_cas
        .get(&policy_ref)
        .expect("read pinned policy blob from CAS");
    let policy: serde_json::Value =
        serde_json::from_slice(&blob).expect("policy blob is the pinned policy JSON");

    assert_eq!(
        policy["verifier"],
        json!("tool-allowlist"),
        "the deciding verifier NAME is pinned (unchanged) — but it is AMBIGUOUS: \
         both admin and session hold a `tool-allowlist`. policy={policy:#}"
    );
    assert_eq!(
        policy["layer"],
        json!("admin"),
        "the pinned policy blob carries the DECIDING LAYER `admin` — `gate_explain` \
         answers 'why blocked' with the AUTHORITY, disambiguating the two \
         identically-named verifiers (I6, DR-020 §Decision 4 makes DR-019 \
         criterion 2 LIVE on the wire). policy={policy:#}"
    );
}

// ---------------------------------------------------------------------------
// CRITERION 4 — all-empty three layers → live `ask`, never a synthesized allow
// (honest-undecidable, DR-011 §3). DR-020 §Acceptance-criteria sketch 4.
// ---------------------------------------------------------------------------

/// CRITERION 4 — `with_layered_permit_config(empty, empty, empty)` composes to
/// the EMPTY verifier set, which the live `request_permission` path ESCALATES
/// (`permit.escalated` → `ask`), NEVER a synthesized `allow`. No layer's absence
/// manufactures a permission (DR-011 §3 honest-undecidable preserved per layer).
///
/// COMPILE-RED (seam) then LIVE-asserts `ask` (and NOT `allow`).
#[tokio::test]
async fn live_all_empty_layers_escalate_never_allow() {
    let badge = Badge::mint().expect("mint badge");
    let (_cas_dir, cas) = shared_cas();

    let empty: Vec<PermitVerifierSpec> = vec![];
    let (_dir, core) = core_with_layers(&badge, cas, empty.clone(), empty.clone(), empty);
    const RUN: &str = "01SP4CWIREEMPTYLAYERS0R01";
    seed_run(&core, RUN);

    let payload = request_edit(&core, 1, &badge, RUN).await;
    assert_ne!(
        payload["decision"],
        json!("allow"),
        "an all-empty three-layer resolution NEVER synthesizes an allow (I6, \
         DR-020 criterion 4). payload={payload:#}"
    );
    assert_eq!(
        payload["decision"],
        json!("ask"),
        "all-empty layers → the empty set → honest-undecidable → escalate to a \
         human (`permit.escalated` → ask, DR-011 §3). payload={payload:#}"
    );

    // The durable proof: a `permit.escalated` fact, never a `permit.granted`.
    let facts = util::log_events(&core);
    assert!(
        facts
            .iter()
            .any(|e| e.subject.as_str() == "permit.escalated" && e.payload()["run"] == json!(RUN)),
        "the empty-set decision emitted permit.escalated (honest-undecidable)"
    );
    assert!(
        !facts
            .iter()
            .any(|e| e.subject.as_str() == "permit.granted" && e.payload()["run"] == json!(RUN)),
        "no permit.granted was synthesized from an all-empty resolution"
    );
}
