//! Compute-node IR: a `PassKind::Compute` node carries a `ComputeDispatch`
//! threadgroup grid, reads/writes storage resources, and topo-sorts ahead of
//! the render node that consumes its output.
//!
//! Proves (a) a compute→render graph compiles + orders compute first,
//! (b) a compute node with no dispatch is rejected at compile,
//! (c) the storage-binding constructors carry the compute stage, and
//! (d) the new fields are serde-backward-compatible.

use engawa::{
    ComputeDispatch, Dispatcher, Material, Node, NodeId, PassKind, RecordingDispatcher,
    RenderGraph, ResourceBindings, ResourceHandle, ResourceKind, ShaderSource, UniformBinding,
    ValidationError,
};

const SIM_WGSL: &str = "@compute @workgroup_size(64) fn cs_main() {}";
const PRESENT_WGSL: &str =
    "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }";

/// A compute material that writes a `var<storage, read_write>` particle buffer.
fn sim_material() -> Material {
    Material::new(
        "particle_sim",
        ShaderSource::inline(SIM_WGSL),
        vec![UniformBinding::storage_rw(0, "particles")],
    )
}

#[test]
fn compute_then_render_compiles_and_orders_compute_first() {
    // compute "sim" writes `particles`; render "present" reads it.
    let sim = Node::compute(
        "sim",
        sim_material(),
        ComputeDispatch::linear(1024, 64),
        vec![],
        vec!["particles".into()],
    );
    let present = Node {
        id: NodeId::new("present"),
        inputs: vec!["particles".into()],
        outputs: vec!["color".into()],
        ..Node::fullscreen_effect(
            "present",
            Material::new("present", ShaderSource::inline(PRESENT_WGSL), vec![]),
            "particles",
            "color",
        )
    };

    let graph = RenderGraph::default()
        .with_resource("particles", ResourceKind::Storage { size_bytes: 1024 * 16 })
        .with_resource(
            "color",
            ResourceKind::Texture { width: None, height: None, format: None, sample_count: None },
        )
        .with_output("color")
        .with_node(present) // declared out of order on purpose
        .with_node(sim);

    let compiled = graph.compile().expect("compute→render graph compiles");
    assert_eq!(compiled.node_count(), 2);
    let order: Vec<&str> = compiled.iter_nodes().map(|n| n.id.as_str()).collect();
    assert_eq!(order, vec!["sim", "present"], "compute writer runs before its reader");

    // The compute node carries its dispatch grid.
    let sim_node = compiled.iter_nodes().find(|n| n.id.as_str() == "sim").unwrap();
    assert_eq!(sim_node.pass, PassKind::Compute);
    assert_eq!(sim_node.dispatch, Some(ComputeDispatch::linear(1024, 64)));

    // IR-level dispatch records both nodes in order.
    let bindings = ResourceBindings::new()
        .with("particles", ResourceHandle::Storage("particles_buf".into()))
        .with("color", ResourceHandle::Texture("color_tex".into()));
    let mut rec = RecordingDispatcher::default();
    rec.dispatch_graph(&compiled, &bindings).expect("dispatch records");
    assert_eq!(rec.tape().len(), 2);
}

#[test]
fn compute_node_without_dispatch_is_rejected() {
    // Hand-build a Compute node with dispatch stripped — compile must refuse it.
    let mut bad = Node::compute(
        "sim",
        sim_material(),
        ComputeDispatch::linear(64, 64),
        vec![],
        vec!["particles".into()],
    );
    bad.dispatch = None;

    let graph = RenderGraph::default()
        .with_resource("particles", ResourceKind::Storage { size_bytes: 16 })
        .with_node(bad);

    let err = graph.compile().expect_err("a dispatch-less compute node is invalid");
    assert!(matches!(
        err,
        engawa::EngawaError::Validation(ValidationError::ComputeWithoutDispatch(ref id))
            if id.as_str() == "sim"
    ));
}

#[test]
fn storage_binding_constructors_carry_the_compute_stage() {
    let rw = UniformBinding::storage_rw(0, "particles");
    assert!(rw.stages.compute);
    assert!(!rw.stages.vertex && !rw.stages.fragment);
    let ro = UniformBinding::storage_read(1, "spawn_seeds");
    assert!(ro.stages.compute);
}

#[test]
fn pre_compute_shader_stages_deserialize_with_compute_false() {
    // A ShaderStages serialized before the `compute` bit existed.
    let legacy = r#"{ "vertex": true, "fragment": false }"#;
    let stages: engawa::ShaderStages = serde_json::from_str(legacy).expect("legacy stages");
    assert!(stages.vertex && !stages.fragment);
    assert!(!stages.compute, "absent compute bit defaults to false");

    // A compute Node round-trips its dispatch.
    let node = Node::compute(
        "sim",
        sim_material(),
        ComputeDispatch::grid_2d(256, 256, [16, 16]),
        vec![],
        vec!["out".into()],
    );
    let json = serde_json::to_string(&node).expect("serialize");
    let back: Node = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(node, back);
    assert!(json.contains("dispatch"));
}
