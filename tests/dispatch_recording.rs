//! Tests for the `Dispatcher` trait + `RecordingDispatcher`
//! reference impl.

use engawa::{
    DispatchError, Dispatcher, Material, Node, RecordingDispatcher, RenderGraph,
    ResourceBindings, ResourceHandle, ResourceKind, ShaderSource,
};

fn dummy_material(name: &str) -> Material {
    Material {
        name: name.to_string(),
        shader: ShaderSource::inline("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }"),
        bindings: vec![],
    }
}

fn linear_chain_graph() -> engawa::CompiledGraph {
    RenderGraph::default()
        .with_resource("swap", ResourceKind::External)
        .with_resource(
            "A",
            ResourceKind::Texture {
                width: Some(800),
                height: Some(600),
            },
        )
        .with_resource(
            "B",
            ResourceKind::Texture {
                width: Some(800),
                height: Some(600),
            },
        )
        .with_input("swap")
        .with_output("B")
        .with_node(Node::clear("clear", "A"))
        .with_node(Node::fullscreen_effect(
            "fx",
            dummy_material("fx"),
            "A",
            "B",
        ))
        .compile()
        .expect("compile")
}

fn bindings_for_chain() -> ResourceBindings {
    ResourceBindings::new()
        .with("swap", ResourceHandle::External("surface".into()))
        .with("A", ResourceHandle::Texture("scene".into()))
        .with("B", ResourceHandle::Texture("post".into()))
}

// ── recording dispatcher records dispatch order ───────────────

#[test]
fn recording_dispatcher_captures_execution_order() {
    let graph = linear_chain_graph();
    let bindings = bindings_for_chain();
    let mut dispatcher = RecordingDispatcher::new();
    dispatcher.dispatch_graph(&graph, &bindings).expect("dispatch");

    assert_eq!(dispatcher.len(), 2);
    let ids: Vec<_> = dispatcher.iter().map(|d| d.node_id.as_str()).collect();
    assert_eq!(ids, vec!["clear", "fx"]);
}

#[test]
fn recording_dispatcher_resolves_each_input_to_its_handle() {
    let graph = linear_chain_graph();
    let bindings = bindings_for_chain();
    let mut dispatcher = RecordingDispatcher::new();
    dispatcher.dispatch_graph(&graph, &bindings).expect("dispatch");

    // The "fx" node reads "A" and writes "B".
    let fx = &dispatcher.tape()[1];
    assert_eq!(fx.node_id.as_str(), "fx");
    assert_eq!(fx.inputs.len(), 1);
    assert_eq!(
        fx.inputs[0].1,
        ResourceHandle::Texture("scene".into()),
        "fx input A binds to scene"
    );
    assert_eq!(fx.outputs.len(), 1);
    assert_eq!(
        fx.outputs[0].1,
        ResourceHandle::Texture("post".into()),
        "fx output B binds to post"
    );
}

#[test]
fn recording_dispatcher_clear_node_has_no_inputs() {
    let graph = linear_chain_graph();
    let bindings = bindings_for_chain();
    let mut dispatcher = RecordingDispatcher::new();
    dispatcher.dispatch_graph(&graph, &bindings).expect("dispatch");
    let clear = &dispatcher.tape()[0];
    assert_eq!(clear.node_id.as_str(), "clear");
    assert!(clear.inputs.is_empty());
    assert_eq!(clear.outputs.len(), 1);
}

// ── missing-binding errors surface cleanly ────────────────────

#[test]
fn missing_input_binding_errors() {
    let graph = linear_chain_graph();
    // Omit the "A" binding.
    let bindings = ResourceBindings::new()
        .with("swap", ResourceHandle::External("surface".into()))
        .with("B", ResourceHandle::Texture("post".into()));
    let mut dispatcher = RecordingDispatcher::new();
    let err = dispatcher
        .dispatch_graph(&graph, &bindings)
        .expect_err("must error");
    match err {
        DispatchError::MissingBinding { node, resource } => {
            // Either clear (output A) or fx (input A) fires
            // first; both reference the missing "A" binding.
            assert!(node.as_str() == "clear" || node.as_str() == "fx");
            assert_eq!(resource.as_str(), "A");
        }
        DispatchError::Backend(other) => panic!("expected MissingBinding, got Backend({other})"),
    }
}

#[test]
fn missing_output_binding_errors() {
    let graph = linear_chain_graph();
    // Omit the "B" binding.
    let bindings = ResourceBindings::new()
        .with("swap", ResourceHandle::External("surface".into()))
        .with("A", ResourceHandle::Texture("scene".into()));
    let mut dispatcher = RecordingDispatcher::new();
    let err = dispatcher
        .dispatch_graph(&graph, &bindings)
        .expect_err("must error");
    match err {
        DispatchError::MissingBinding { node, resource } => {
            assert_eq!(node.as_str(), "fx");
            assert_eq!(resource.as_str(), "B");
        }
        DispatchError::Backend(other) => panic!("expected MissingBinding, got Backend({other})"),
    }
}

// ── empty graph dispatches as no-op ────────────────────────────

#[test]
fn empty_graph_dispatches_as_zero_recordings() {
    let graph = RenderGraph::default().compile().unwrap();
    let bindings = ResourceBindings::new();
    let mut dispatcher = RecordingDispatcher::new();
    dispatcher.dispatch_graph(&graph, &bindings).expect("dispatch");
    assert!(dispatcher.is_empty());
}

// ── bindings utility ──────────────────────────────────────────

#[test]
fn resource_bindings_iterate_in_sorted_key_order() {
    let b = ResourceBindings::new()
        .with("z", ResourceHandle::Texture("z".into()))
        .with("a", ResourceHandle::Texture("a".into()))
        .with("m", ResourceHandle::Texture("m".into()));
    let keys: Vec<_> = b.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["a", "m", "z"]);
}

#[test]
fn resource_bindings_get_returns_correct_handle() {
    let b = ResourceBindings::new()
        .with("tex", ResourceHandle::Texture("scene".into()))
        .with("u", ResourceHandle::Uniform("frame".into()));
    assert_eq!(
        b.get(&"tex".into()),
        Some(&ResourceHandle::Texture("scene".into()))
    );
    assert_eq!(
        b.get(&"u".into()),
        Some(&ResourceHandle::Uniform("frame".into()))
    );
    assert_eq!(b.get(&"nope".into()), None);
}

#[test]
fn dispatch_runs_in_byte_identical_order_across_consecutive_calls() {
    // Re-dispatching the same graph + bindings produces the
    // same tape, byte-for-byte. Pins the determinism contract
    // that every engawa-adopting consumer relies on.
    let graph = linear_chain_graph();
    let bindings = bindings_for_chain();

    let mut a = RecordingDispatcher::new();
    a.dispatch_graph(&graph, &bindings).unwrap();
    let mut b = RecordingDispatcher::new();
    b.dispatch_graph(&graph, &bindings).unwrap();
    assert_eq!(a.tape(), b.tape());
}
