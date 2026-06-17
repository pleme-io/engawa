//! `ShaderSource::Msl` — the raw-MSL escape hatch for the Metal-4 frontier
//! (mesh / tile / procedural-ray-tracing shaders naga can't emit from WGSL).
//! Proves the typed variant constructs, carries explicit entry names, serde
//! round-trips, and is backward-compatible with pre-Msl IR.

use engawa::{Material, ShaderSource, UniformBinding};

#[test]
fn msl_render_carries_explicit_entry_names() {
    let s = ShaderSource::msl_render("// msl", "vs_main", "fs_main");
    match &s {
        ShaderSource::Msl { source, vertex, fragment, compute } => {
            assert_eq!(source, "// msl");
            assert_eq!(vertex.as_deref(), Some("vs_main"));
            assert_eq!(fragment.as_deref(), Some("fs_main"));
            assert_eq!(*compute, None);
        }
        other => panic!("expected Msl, got {other:?}"),
    }
    assert_eq!(s.display_short(), "msl:vs_main/fs_main/-");
}

#[test]
fn msl_compute_carries_only_the_compute_entry() {
    let s = ShaderSource::msl_compute("// kernel", "cs_main");
    match &s {
        ShaderSource::Msl { vertex, fragment, compute, .. } => {
            assert!(vertex.is_none() && fragment.is_none());
            assert_eq!(compute.as_deref(), Some("cs_main"));
        }
        other => panic!("expected Msl, got {other:?}"),
    }
    assert_eq!(s.display_short(), "msl:-/-/cs_main");
}

#[test]
fn msl_material_serde_roundtrips() {
    let m = Material::new(
        "mesh_frontier",
        ShaderSource::msl_render("// mesh msl", "mesh_main", "frag_main"),
        vec![UniformBinding::uniform(0, "globals")],
    );
    let json = serde_json::to_string(&m).expect("serialize");
    let back: Material = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(m, back);
    // The variant tag + entry names are actually on the wire.
    assert!(json.contains("\"kind\":\"msl\""), "json: {json}");
    assert!(json.contains("mesh_main") && json.contains("frag_main"));
}

#[test]
fn pre_msl_inline_wgsl_still_deserializes() {
    // An Inline source serialized before the Msl variant existed.
    let legacy = r#"{ "kind": "inline", "wgsl": "@fragment fn fs() {}" }"#;
    let s: ShaderSource = serde_json::from_str(legacy).expect("legacy inline deserializes");
    assert!(matches!(s, ShaderSource::Inline { .. }));
}
