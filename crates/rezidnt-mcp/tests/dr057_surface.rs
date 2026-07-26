//! DR-057 ORACLE — the `diff_view`/`cas_read` §9 surface: served, no-drift,
//! and shaped as the record ruled (DR-057 §Decision 1/2/3/4).
//!
//! Three-way no-drift, the `jsonrpc_surface.rs`/DR-039 pattern hardened with
//! committed goldens (the DR-042/DR-044 `.schema.golden.json` pattern):
//! served `inputSchema` == `schemars::schema_for!` of the `rezidnt-types`
//! shape == the committed golden. The golden leg matters here because
//! generated==served alone is self-referential — it passes whatever shape the
//! implementer types; the committed golden pins the shape the RECORD ruled
//! (worktree-keyed args; a FULL `CasRef`, never a bare hash).
//!
//! Shape pins carried by the schemas themselves:
//!
//! - `diff_view` args require `worktree` and carry NO `run` property — the
//!   tool is keyed by worktree, never by run (DR-057 §Decision 3: `RunRow`
//!   holds no worktree reference and the ordinary allocator is the bare
//!   string `"rezidnt"`, so a run key has nothing sound to join on; DR-049
//!   ruled the correlation join UNSOUND).
//! - `cas_read` args are the full `{hash, bytes, mime}` triple, ALL required
//!   — the caller's own ref, echoed and verified; never a bare hash the
//!   daemon would have to invent metadata for (mime lives only in event
//!   payloads; the CAS at rest is content-only, `crates/rezidnt-cas`).
//! - NEITHER schema carries a `badge` property: read-class, unbadged
//!   (DR-057 §Decision 4; DR-005/DR-039 precedent).
//!
//! ## RED MODE (against the tree at cut time — post-`1094f40`)
//!
//! COMPILE-RED: `rezidnt_types::mcp::DiffViewArgs` and
//! `rezidnt_types::mcp::CasReadArgs` do not exist (verified by grep this
//! session). Once they exist, ASSERT-RED until `tools_list()` advertises both
//! tools with the generated schemas. The goldens were generated with the
//! workspace-locked schemars 1.2.1 emission (probed this session), so the
//! generated==golden leg goes green the moment the types match the ruled
//! shape — it is the served leg that stays red until the surface serves them.

mod util;

use serde_json::{Value, json};

fn golden(name: &str) -> Value {
    let path = util::fixtures_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden {} must exist: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: golden must parse: {e}"))
}

/// Both DR-057 read tools are served in `tools/list`.
#[tokio::test]
async fn tools_list_serves_diff_view_and_cas_read() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    util::find_tool(&tools, "diff_view");
    util::find_tool(&tools, "cas_read");
}

/// §9 BINDING no-drift, three ways: generated == committed golden (the shape
/// the record ruled), and served == generated (the surface cannot drift from
/// the published types).
#[tokio::test]
async fn dr057_schemas_match_types_and_goldens() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;

    let cases = [
        (
            "diff_view",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::DiffViewArgs))
                .expect("DiffViewArgs schema serializes"),
            golden("dr057_diff_view_args.schema.golden.json"),
        ),
        (
            "cas_read",
            serde_json::to_value(schemars::schema_for!(rezidnt_types::mcp::CasReadArgs))
                .expect("CasReadArgs schema serializes"),
            golden("dr057_cas_read_args.schema.golden.json"),
        ),
    ];

    for (name, generated, committed) in cases {
        assert_eq!(
            generated, committed,
            "{name}: schemars::schema_for! of the rezidnt-types shape must \
             EQUAL the committed golden — the golden pins the shape DR-057 \
             ruled, not whatever shape got typed"
        );
        let tool = util::find_tool(&tools, name);
        assert_eq!(
            tool["inputSchema"], generated,
            "{name}: served inputSchema must EQUAL the generated schema \
             (no drift, doc §9 BINDING)"
        );
    }
}

/// `diff_view` is worktree-keyed and ONLY worktree-keyed: `worktree` required,
/// no `run` property, no `badge` property (read-class). A run-keyed alias
/// property appearing here is the drift DR-057 §Decision 3 forbids.
#[tokio::test]
async fn diff_view_schema_is_worktree_keyed_and_unbadged() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let schema = &util::find_tool(&tools, "diff_view")["inputSchema"];

    assert_eq!(
        schema["required"],
        json!(["worktree"]),
        "diff_view requires exactly the worktree key: {schema:#}"
    );
    let props = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("diff_view schema has properties: {schema:#}"));
    assert!(
        !props.contains_key("run"),
        "diff_view must NOT offer a run key — a run has nothing sound to join \
         on (DR-057 §Decision 3, DR-049 precedent): {schema:#}"
    );
    assert!(
        !props.contains_key("badge"),
        "diff_view is read-class, unbadged (DR-057 §Decision 4): {schema:#}"
    );
}

/// `cas_read` takes the FULL ref — hash AND bytes AND mime, all required —
/// and no badge. A bare-hash schema would force the daemon to invent metadata
/// the CAS never persists at rest (DR-057 §Decision 2).
#[tokio::test]
async fn cas_read_schema_requires_the_full_ref_and_no_badge() {
    let (_dir, core) = util::core();
    let tools = util::list_tools(&core).await;
    let schema = &util::find_tool(&tools, "cas_read")["inputSchema"];

    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap_or_else(|| panic!("cas_read schema has required: {schema:#}"))
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for field in ["hash", "bytes", "mime"] {
        assert!(
            required.contains(&field),
            "cas_read must REQUIRE {field} — the caller presents its own full \
             ref, never a bare hash (DR-057 §Decision 2): {schema:#}"
        );
    }
    let props = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("cas_read schema has properties: {schema:#}"));
    assert!(
        !props.contains_key("badge"),
        "cas_read is read-class, unbadged (DR-057 §Decision 4): {schema:#}"
    );
}
