//! Integration test suite for the engawa render-graph IR.
//!
//! Tests are split by concern: schema round-trip, the happy-path
//! compile, every validation-error variant, topo-sort determinism,
//! and the operator-facing fluent builder.

use engawa::{
    BindingKind, Effect, Material, Node, PassKind, RenderGraph, ResourceKind, ShaderSource,
    UniformBinding, ValidationError,
};

// ── helpers ────────────────────────────────────────────────────

fn dummy_material(name: &str) -> Material {
    Material::new(
        name,
        ShaderSource::inline("@vertex fn vs_main() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }"),
        vec![],
    )
}

fn render_target(width: u32, height: u32) -> ResourceKind {
    ResourceKind::Texture {
        width: Some(width),
        height: Some(height),
        format: None,
        sample_count: None,
    }
}

// ── happy path ─────────────────────────────────────────────────

#[test]
fn empty_graph_compiles_to_empty_execution_order() {
    let compiled = RenderGraph::default().compile().expect("compile");
    assert_eq!(compiled.node_count(), 0);
    assert!(compiled.iter_nodes().next().is_none());
}

#[test]
fn single_node_compiles_and_runs_in_isolation() {
    let g = RenderGraph::default()
        .with_resource("output", render_target(800, 600))
        .with_output("output")
        .with_node(Node::clear("clear", "output"));
    let c = g.compile().expect("compile");
    assert_eq!(c.node_count(), 1);
    assert_eq!(c.execution_order[0].id.as_str(), "clear");
}

#[test]
fn linear_three_node_chain_compiles_in_dep_order() {
    // clear(out=A) → effect1(in=A, out=B) → effect2(in=B, out=C)
    let m1 = dummy_material("e1");
    let m2 = dummy_material("e2");
    let g = RenderGraph::default()
        .with_resource("A", render_target(800, 600))
        .with_resource("B", render_target(800, 600))
        .with_resource("C", render_target(800, 600))
        .with_output("C")
        .with_node(Node::clear("clear", "A"))
        .with_node(Node::fullscreen_effect("e1", m1, "A", "B"))
        .with_node(Node::fullscreen_effect("e2", m2, "B", "C"));
    let c = g.compile().expect("compile");
    let ids: Vec<_> = c.iter_nodes().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["clear", "e1", "e2"]);
}

#[test]
fn nodes_declared_out_of_order_compile_to_correct_order() {
    // Same chain as above but nodes added in reverse order.
    // Topo sort must restore the dependency order.
    let m1 = dummy_material("e1");
    let m2 = dummy_material("e2");
    let g = RenderGraph::default()
        .with_resource("A", render_target(800, 600))
        .with_resource("B", render_target(800, 600))
        .with_resource("C", render_target(800, 600))
        .with_output("C")
        .with_node(Node::fullscreen_effect("e2", m2, "B", "C"))
        .with_node(Node::fullscreen_effect("e1", m1, "A", "B"))
        .with_node(Node::clear("clear", "A"));
    let c = g.compile().expect("compile");
    let ids: Vec<_> = c.iter_nodes().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["clear", "e1", "e2"]);
}

#[test]
fn graph_input_satisfies_unbound_input_check() {
    // External input (e.g. the swapchain surface) declared via
    // with_input — node consumes it; compile succeeds.
    let m = dummy_material("e");
    let g = RenderGraph::default()
        .with_resource("swap", ResourceKind::External)
        .with_resource("out", render_target(800, 600))
        .with_input("swap")
        .with_output("out")
        .with_node(Node::fullscreen_effect("e", m, "swap", "out"));
    assert!(g.compile().is_ok());
}

// ── validation errors ──────────────────────────────────────────

#[test]
fn duplicate_node_id_rejected() {
    let g = RenderGraph::default()
        .with_resource("a", render_target(1, 1))
        .with_resource("b", render_target(1, 1))
        .with_node(Node::clear("dup", "a"))
        .with_node(Node::clear("dup", "b"));
    match g.compile() {
        Err(engawa::EngawaError::Validation(ValidationError::DuplicateNode(id))) => {
            assert_eq!(id.as_str(), "dup");
        }
        other => panic!("expected DuplicateNode, got {other:?}"),
    }
}

#[test]
fn unbound_input_rejected_with_node_and_resource_id() {
    let m = dummy_material("e");
    let g = RenderGraph::default()
        .with_resource("missing", render_target(1, 1))
        .with_resource("out", render_target(1, 1))
        .with_node(Node::fullscreen_effect("e", m, "missing", "out"));
    match g.compile() {
        Err(engawa::EngawaError::Validation(ValidationError::UnboundInput {
            node,
            resource,
        })) => {
            assert_eq!(node.as_str(), "e");
            assert_eq!(resource.as_str(), "missing");
        }
        other => panic!("expected UnboundInput, got {other:?}"),
    }
}

#[test]
fn multiple_writers_rejected() {
    let m1 = dummy_material("e1");
    let m2 = dummy_material("e2");
    let g = RenderGraph::default()
        .with_resource("src", render_target(1, 1))
        .with_resource("dst", render_target(1, 1))
        .with_input("src")
        .with_node(Node::fullscreen_effect("e1", m1, "src", "dst"))
        .with_node(Node::fullscreen_effect("e2", m2, "src", "dst"));
    match g.compile() {
        Err(engawa::EngawaError::Validation(ValidationError::MultipleWriters(id))) => {
            assert_eq!(id.as_str(), "dst");
        }
        other => panic!("expected MultipleWriters, got {other:?}"),
    }
}

#[test]
fn cycle_rejected() {
    // A → B → A
    let m1 = dummy_material("a-to-b");
    let m2 = dummy_material("b-to-a");
    let g = RenderGraph::default()
        .with_resource("A", render_target(1, 1))
        .with_resource("B", render_target(1, 1))
        .with_node(Node::fullscreen_effect("a-to-b", m1, "A", "B"))
        .with_node(Node::fullscreen_effect("b-to-a", m2, "B", "A"));
    match g.compile() {
        Err(engawa::EngawaError::Validation(ValidationError::Cycle(stuck))) => {
            assert_eq!(stuck.len(), 2);
        }
        other => panic!("expected Cycle, got {other:?}"),
    }
}

#[test]
fn unbound_output_rejected() {
    let g = RenderGraph::default()
        .with_resource("phantom", render_target(1, 1))
        .with_output("phantom");
    match g.compile() {
        Err(engawa::EngawaError::Validation(ValidationError::UnboundOutput(id))) => {
            assert_eq!(id.as_str(), "phantom");
        }
        other => panic!("expected UnboundOutput, got {other:?}"),
    }
}

// ── determinism ────────────────────────────────────────────────

#[test]
fn topo_sort_is_deterministic_across_independent_branches() {
    // clear-a, clear-b, clear-c (no deps between them) — topo
    // sort uses BTreeSet ready-pool so the order is the sorted
    // order, not insertion order.
    let g = RenderGraph::default()
        .with_resource("a", render_target(1, 1))
        .with_resource("b", render_target(1, 1))
        .with_resource("c", render_target(1, 1))
        .with_node(Node::clear("clear-c", "c"))
        .with_node(Node::clear("clear-a", "a"))
        .with_node(Node::clear("clear-b", "b"));
    let c1 = g.clone().compile().unwrap();
    let c2 = g.compile().unwrap();
    let ids1: Vec<_> = c1.iter_nodes().map(|n| n.id.clone()).collect();
    let ids2: Vec<_> = c2.iter_nodes().map(|n| n.id.clone()).collect();
    assert_eq!(ids1, ids2, "topo sort must be deterministic");
    assert_eq!(
        ids1.iter().map(engawa::NodeId::as_str).collect::<Vec<_>>(),
        vec!["clear-a", "clear-b", "clear-c"],
        "ready pool drained in sorted-id order"
    );
}

// ── serde round-trip ───────────────────────────────────────────

#[test]
fn render_graph_serde_round_trips() {
    let m = dummy_material("e");
    let g = RenderGraph::default()
        .with_resource("swap", ResourceKind::External)
        .with_resource("out", render_target(800, 600))
        .with_input("swap")
        .with_output("out")
        .with_node(Node::fullscreen_effect("e", m, "swap", "out"));
    let json = serde_json::to_string(&g).unwrap();
    let back: RenderGraph = serde_json::from_str(&json).unwrap();
    assert_eq!(g, back);
}

#[test]
fn binding_kind_serde_uses_snake_case_tag() {
    let b = UniformBinding::storage_read(0, "x");
    let json = serde_json::to_string(&b).unwrap();
    assert!(json.contains("\"storage_read\""), "json: {json}");
    let back: UniformBinding = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn pass_kind_serde_uses_snake_case() {
    for (variant, expected) in [
        (PassKind::Render, "\"render\""),
        (PassKind::Compute, "\"compute\""),
        (PassKind::Blit, "\"blit\""),
    ] {
        assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
    }
}

// ── effect operator surface ────────────────────────────────────

#[test]
fn effect_priorities_sort_predictably() {
    let mut effects = [
        Effect { name: "chrome".into(), enabled: true, priority: 800, material: dummy_material("c") },
        Effect { name: "scene".into(), enabled: true, priority: 100, material: dummy_material("s") },
        Effect { name: "post".into(), enabled: true, priority: 500, material: dummy_material("p") },
        Effect { name: "clear".into(), enabled: true, priority: 0, material: dummy_material("z") },
    ];
    effects.sort_by_key(|e| e.priority);
    let names: Vec<_> = effects.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["clear", "scene", "post", "chrome"]);
}

#[test]
fn disabled_effects_filter_cleanly() {
    let effects = [
        Effect { name: "on".into(), enabled: true, priority: 100, material: dummy_material("on") },
        Effect { name: "off".into(), enabled: false, priority: 200, material: dummy_material("off") },
        Effect { name: "on2".into(), enabled: true, priority: 300, material: dummy_material("on2") },
    ];
    let active: Vec<_> = effects.iter().filter(|e| e.enabled).map(|e| e.name.as_str()).collect();
    assert_eq!(active, vec!["on", "on2"]);
}

// ── shader source display ──────────────────────────────────────

#[test]
fn shader_source_display_short_truncates_inline() {
    let s = ShaderSource::inline("a".repeat(100));
    let d = s.display_short();
    assert!(d.starts_with("inline:"));
    assert!(d.len() <= "inline:".len() + 40);
}

#[test]
fn shader_source_display_short_keeps_path_intact() {
    let s = ShaderSource::path("/etc/mado/effects/scanlines.wgsl");
    assert_eq!(
        s.display_short(),
        "path:/etc/mado/effects/scanlines.wgsl"
    );
}

// ── fluent constructors ────────────────────────────────────────

#[test]
fn fullscreen_effect_constructor_pins_render_pass() {
    let m = dummy_material("e");
    let n = Node::fullscreen_effect("e", m, "in", "out");
    assert_eq!(n.pass, PassKind::Render);
    assert_eq!(n.inputs.len(), 1);
    assert_eq!(n.outputs.len(), 1);
    assert!(n.material.is_some());
}

#[test]
fn clear_constructor_has_no_inputs_no_material() {
    let n = Node::clear("c", "out");
    assert_eq!(n.pass, PassKind::Render);
    assert!(n.inputs.is_empty());
    assert_eq!(n.outputs.len(), 1);
    assert!(n.material.is_none());
}
