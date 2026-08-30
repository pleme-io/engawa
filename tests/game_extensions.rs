//! M2 game-renderer extensions: mesh draws (`DrawKind`), depth testing
//! (`DepthSpec`), texture formats, and fixed-function `RenderState`.
//!
//! Proves the new fields (a) compile into a real depth-tested indexed-mesh
//! graph, (b) survive serde round-trips, and (c) are backward-compatible —
//! IR authored before these fields existed still deserializes (the
//! `#[serde(default)]` contract).

use engawa::{
    BlendMode, CompareFunction, CullMode, DepthSpec, Dispatcher, DrawKind, Material, Node, NodeId,
    PassKind, RecordingDispatcher, RenderGraph, RenderState, ResourceBindings, ResourceHandle,
    ResourceKind, ShaderSource, TextureFormat,
};

const LIT_WGSL: &str =
    "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }";

/// A material with non-default render state: alpha blend + back-face cull.
fn lit_material() -> Material {
    Material::new("lit", ShaderSource::inline(LIT_WGSL), vec![]).with_state(RenderState {
        blend: BlendMode::AlphaBlend,
        cull: CullMode::Back,
        ..RenderState::default()
    })
}

/// A depth-tested indexed-mesh render node writing the `color` attachment.
fn mesh_node() -> Node {
    Node {
        id: NodeId::new("mesh"),
        pass: PassKind::Render,
        inputs: vec![],
        outputs: vec!["color".into()],
        material: Some(lit_material()),
        draw: DrawKind::Indexed {
            vertices: "verts".into(),
            indices: "idx".into(),
            index_count: 36,
        },
        depth: Some(DepthSpec::new("depth")),
        dispatch: None,
    }
}

#[test]
fn compiles_and_dispatches_a_depth_tested_indexed_mesh() {
    let graph = RenderGraph::default()
        .with_resource(
            "color",
            ResourceKind::Texture {
                width: None,
                height: None,
                format: Some(TextureFormat::Bgra8Unorm),
                sample_count: None,
            },
        )
        .with_resource(
            "depth",
            ResourceKind::Texture {
                width: None,
                height: None,
                format: Some(TextureFormat::Depth32Float),
                sample_count: None,
            },
        )
        .with_output("color")
        .with_node(mesh_node());

    let compiled = graph.compile().expect("graph compiles");
    assert_eq!(compiled.node_count(), 1);

    // The IR-level dispatcher checks every node input/output has a binding.
    let bindings =
        ResourceBindings::new().with("color", ResourceHandle::Texture("color_tex".into()));
    let mut rec = RecordingDispatcher::default();
    rec.dispatch_graph(&compiled, &bindings)
        .expect("dispatch records");
    assert_eq!(rec.tape().len(), 1);
    assert_eq!(rec.tape()[0].node_id, NodeId::new("mesh"));
}

#[test]
fn node_serde_roundtrip_preserves_draw_and_depth() {
    let node = mesh_node();
    let json = serde_json::to_string(&node).expect("serialize");
    let back: Node = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(node, back);
    // The draw + depth metadata is actually present on the wire.
    assert!(json.contains("indexed"));
    assert!(json.contains("\"index_count\":36"));
    assert!(json.contains("depth"));
}

#[test]
fn material_render_state_roundtrips() {
    let m = lit_material();
    let json = serde_json::to_string(&m).expect("serialize");
    let back: Material = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(m, back);
    assert_eq!(back.state.blend, BlendMode::AlphaBlend);
    assert_eq!(back.state.cull, CullMode::Back);
}

#[test]
fn texture_format_classifies_depth() {
    assert!(TextureFormat::Depth32Float.is_depth());
    assert!(TextureFormat::Depth24PlusStencil8.is_depth());
    assert!(!TextureFormat::Bgra8Unorm.is_depth());
    assert!(!TextureFormat::Rgba8Unorm.is_depth());
}

#[test]
fn defaults_are_backwards_compatible() {
    // Constructors leave the new fields at their no-op defaults.
    let clear = Node::clear("clr", "out");
    assert_eq!(clear.draw, DrawKind::FullscreenQuad);
    assert_eq!(clear.depth, None);
    assert_eq!(
        Material::new("m", ShaderSource::inline("x"), vec![]).state,
        RenderState::default()
    );

    // CompareFunction default is the common 3D choice.
    assert_eq!(DepthSpec::new("d").compare, CompareFunction::LessEqual);
}

#[test]
fn pre_extension_ir_still_deserializes() {
    // A Node serialized BEFORE the draw/depth fields existed (they're absent
    // from the JSON). `#[serde(default)]` must fill them in — proving the
    // wire format is backward-compatible.
    let legacy = r#"{
        "id": "fx",
        "pass": "render",
        "inputs": ["a"],
        "outputs": ["b"],
        "material": null
    }"#;
    let node: Node = serde_json::from_str(legacy).expect("legacy IR deserializes");
    assert_eq!(node.draw, DrawKind::FullscreenQuad);
    assert_eq!(node.depth, None);

    // Same for a Texture resource without format/sample_count.
    let legacy_tex = r#"{ "kind": "texture", "width": 800, "height": 600 }"#;
    let tex: ResourceKind = serde_json::from_str(legacy_tex).expect("legacy texture deserializes");
    assert_eq!(
        tex,
        ResourceKind::Texture {
            width: Some(800),
            height: Some(600),
            format: None,
            sample_count: None,
        }
    );
}

/// A backend that honours nothing beyond the default — i.e. one that has not
/// declared, which is the shape EVERY backend has until someone writes the
/// declaration.
struct UndeclaredBackend(RecordingDispatcher);

impl Dispatcher for UndeclaredBackend {
    fn dispatch_node(
        &mut self,
        node: &Node,
        bindings: &ResourceBindings,
    ) -> Result<(), engawa::dispatch::DispatchError> {
        self.0.dispatch_node(node, bindings)
    }
    // `capabilities()` deliberately NOT overridden — inherits MINIMAL.
}

/// ★ THE CONTRACT. Before it existed, `engawa-wgpu` read `node.draw`,
/// `node.depth`, `material.state`, `binding.group` and `binding.stages` ZERO
/// times (grep-proven 2026-08-30), so a consumer declaring an indexed mesh got
/// a fullscreen triangle, `Ok(())`, and a wrong picture. The declaration was
/// parsed, validated, topologically sorted — and dropped.
#[test]
fn an_undeclared_backend_refuses_a_geometry_draw_rather_than_drawing_a_quad() {
    let graph = RenderGraph::default()
        .with_resource(
            "color",
            ResourceKind::Texture {
                width: None,
                height: None,
                format: Some(TextureFormat::Bgra8Unorm),
                sample_count: None,
            },
        )
        .with_output("color")
        .with_node(mesh_node());
    let compiled = graph.compile().expect("graph compiles");
    let bindings =
        ResourceBindings::new().with("color", ResourceHandle::Texture("color_tex".into()));

    let mut backend = UndeclaredBackend(RecordingDispatcher::default());
    let err = backend
        .dispatch_graph(&compiled, &bindings)
        .expect_err("a MINIMAL backend must REFUSE a geometry draw, not render a quad");

    match err {
        engawa::dispatch::DispatchError::Unsupported { node, capabilities } => {
            assert_eq!(
                capabilities,
                // ★ BOTH gaps, not the first. This node declares an indexed
                // draw AND a depth attachment; a single-capability error would
                // have named one and sent the reader back for a second
                // round-trip after fixing it. That regression is pinned here.
                vec![
                    engawa::capability::Capability::GeometryDraw,
                    engawa::capability::Capability::Depth,
                ]
            );
            assert_eq!(node.as_str(), "mesh", "the refusal must NAME the node");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
    assert!(
        backend.0.tape().is_empty(),
        "nothing may be dispatched — a partly-rendered frame is harder to \
         diagnose than none, because it looks like a different bug"
    );
}

/// ANTI-VACUITY for the test above: the SAME graph through a backend that
/// DECLARES the capability must succeed. Without this pair, the refusal test
/// would pass equally against a dispatcher that refused everything.
#[test]
fn a_declaring_backend_dispatches_the_graph_that_minimal_refused() {
    let graph = RenderGraph::default()
        .with_resource(
            "color",
            ResourceKind::Texture {
                width: None,
                height: None,
                format: Some(TextureFormat::Bgra8Unorm),
                sample_count: None,
            },
        )
        .with_output("color")
        .with_node(mesh_node());
    let compiled = graph.compile().expect("graph compiles");
    let bindings =
        ResourceBindings::new().with("color", ResourceHandle::Texture("color_tex".into()));

    let mut rec = RecordingDispatcher::default();
    assert_eq!(rec.capabilities(), engawa::capability::Capabilities::all());
    rec.dispatch_graph(&compiled, &bindings)
        .expect("a backend declaring all() dispatches what MINIMAL refused");
    assert_eq!(rec.tape().len(), 1);
}
