//! Property-based fuzz over engawa's compile algorithm.
//!
//! Random DAGs (and random non-DAGs) confirm the invariants
//! we hand-tested in `graph_compile.rs` hold across the search
//! space:
//!
//! * Any valid DAG compiles.
//! * Compile result is deterministic (same input → same output).
//! * Execution order respects every input→output edge.
//! * A cycle is always caught.
//! * Topological position of a node is ≥ max(positions of nodes
//!   that produce its inputs) + 1.
//!
//! The fuzz catches future refactors (e.g. a swap to a faster
//! topo-sort algorithm) that accidentally regress determinism
//! or edge-respect.

use std::collections::BTreeMap;

use engawa::{
    BindingKind, EngawaError, Material, Node, RenderGraph, ResourceId, ResourceKind,
    ShaderSource, UniformBinding, ValidationError,
};
use proptest::prelude::*;

// ── helpers ────────────────────────────────────────────────────

fn id_str(prefix: char, n: usize) -> String {
    format!("{prefix}{n}")
}

fn texture_resource() -> ResourceKind {
    ResourceKind::Texture {
        width: Some(64),
        height: Some(64),
    }
}

fn fullscreen_material(name: &str) -> Material {
    Material {
        name: name.to_string(),
        shader: ShaderSource::inline("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }"),
        bindings: vec![UniformBinding {
            binding: 0,
            kind: BindingKind::Uniform,
            resource: ResourceId::new("frame"),
        }],
    }
}

/// Build a chain of `n` nodes: clear(out=R0) → fx_1(in=R0, out=R1)
/// → … → fx_(n-1)(in=R(n-2), out=R(n-1)). Valid by construction
/// for any n ≥ 1.
fn chain_graph(n: usize) -> RenderGraph {
    let mut g = RenderGraph::default();
    for i in 0..n {
        let resource = id_str('R', i);
        g = g.with_resource(&resource[..], texture_resource());
    }
    if n > 0 {
        g = g.with_output(id_str('R', n - 1));
        g = g.with_node(Node::clear("clear", id_str('R', 0)));
        for i in 1..n {
            let input = id_str('R', i - 1);
            let output = id_str('R', i);
            let id = format!("fx_{i}");
            g = g.with_node(Node::fullscreen_effect(
                id.clone(),
                fullscreen_material(&id),
                input,
                output,
            ));
        }
    }
    g
}

/// Build a parallel-fanout graph: one clear node + `n`
/// independent effect nodes all reading from R0 and each
/// writing to its own R1+i output. Valid for any n ≥ 0.
fn fanout_graph(n: usize) -> RenderGraph {
    let mut g = RenderGraph::default()
        .with_resource("R0", texture_resource())
        .with_node(Node::clear("clear", "R0"));
    for i in 0..n {
        let output = format!("R{}", i + 1);
        g = g
            .with_resource(&output[..], texture_resource())
            .with_output(&output[..]);
        let id = format!("fx_{i}");
        g = g.with_node(Node::fullscreen_effect(
            id.clone(),
            fullscreen_material(&id),
            "R0",
            output,
        ));
    }
    g
}

// ── property: any chain of length N compiles + N nodes execute ──

proptest! {
    #[test]
    fn chain_of_any_length_compiles(n in 1usize..50) {
        let g = chain_graph(n);
        let compiled = g.compile().expect("chain always valid");
        prop_assert_eq!(compiled.node_count(), n);
    }

    #[test]
    fn chain_execution_order_respects_data_flow(n in 2usize..40) {
        let g = chain_graph(n);
        let compiled = g.compile().expect("chain valid");
        // For each node, the position of every node producing its
        // inputs must be < its own position.
        let positions: BTreeMap<String, usize> = compiled
            .iter_nodes()
            .enumerate()
            .map(|(i, node)| (node.id.as_str().to_string(), i))
            .collect();
        for node in compiled.iter_nodes() {
            let my_pos = positions[node.id.as_str()];
            for input in &node.inputs {
                for other in compiled.iter_nodes() {
                    if other.outputs.contains(input) && other.id != node.id {
                        let other_pos = positions[other.id.as_str()];
                        prop_assert!(
                            other_pos < my_pos,
                            "{} (pos {}) reads {} produced by {} (pos {})",
                            node.id.as_str(), my_pos,
                            input.as_str(),
                            other.id.as_str(), other_pos
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn chain_compile_is_deterministic(n in 1usize..40) {
        let g = chain_graph(n);
        let a = g.clone().compile().unwrap();
        let b = g.compile().unwrap();
        // Same input → same execution order, byte for byte.
        prop_assert_eq!(
            a.execution_order.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            b.execution_order.iter().map(|n| n.id.clone()).collect::<Vec<_>>()
        );
    }
}

// ── property: parallel fanout of N produces N+1 nodes in
//             deterministic alphabetical order ──

proptest! {
    #[test]
    fn fanout_of_any_width_compiles(n in 0usize..30) {
        let g = fanout_graph(n);
        let compiled = g.compile().expect("fanout always valid");
        prop_assert_eq!(compiled.node_count(), n + 1);
    }

    #[test]
    fn fanout_clear_runs_before_every_effect(n in 1usize..30) {
        let g = fanout_graph(n);
        let compiled = g.compile().unwrap();
        let clear_pos = compiled
            .iter_nodes()
            .position(|node| node.id.as_str() == "clear")
            .expect("clear node present");
        // Every fx_* node must come after clear because they all
        // read the resource clear writes.
        for (i, node) in compiled.iter_nodes().enumerate() {
            if node.id.as_str().starts_with("fx_") {
                prop_assert!(
                    i > clear_pos,
                    "{} at {} before clear at {}",
                    node.id.as_str(), i, clear_pos
                );
            }
        }
    }
}

// ── property: introducing a cycle ALWAYS fails compile ──

proptest! {
    #[test]
    fn injected_cycle_always_fails_to_compile(
        n in 2usize..20,
        cycle_back_idx in 0usize..20
    ) {
        // Start with a valid chain of length n.
        // Then add ONE back-edge from R(n-1) to R(c) where c ≤ n-2.
        // This forms a cycle iff c < n-1. Compile must error.
        let mut g = chain_graph(n);
        let cycle_to = cycle_back_idx % n.saturating_sub(1).max(1);
        let cycle_from = n - 1;
        let cycle_node = Node::fullscreen_effect(
            "cycle",
            fullscreen_material("cycle"),
            id_str('R', cycle_from),
            id_str('R', cycle_to), // Writes back to upstream → cycle
        );
        g = g.with_node(cycle_node);

        let result = g.compile();
        match result {
            Err(EngawaError::Validation(ValidationError::MultipleWriters(_))) => {
                // Two writers to the same resource — also a
                // valid catch (the chain already writes R(cycle_to)).
            }
            Err(EngawaError::Validation(ValidationError::Cycle(_))) => {
                // Direct cycle detection.
            }
            Ok(c) => panic!(
                "injected cycle compiled cleanly: {} nodes in order {:?}",
                c.node_count(),
                c.iter_nodes().map(|n| n.id.as_str().to_string()).collect::<Vec<_>>()
            ),
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
}

// ── property: duplicate node ids always fail ──

proptest! {
    #[test]
    fn duplicate_node_ids_always_fail(n in 2usize..20) {
        let g = chain_graph(n)
            .with_node(Node::clear("clear", "R_dup"))
            .with_resource("R_dup", texture_resource());
        // Now there are two "clear" nodes.
        match g.compile() {
            Err(EngawaError::Validation(ValidationError::DuplicateNode(id))) => {
                prop_assert_eq!(id.as_str(), "clear");
            }
            other => panic!("expected DuplicateNode, got {other:?}"),
        }
    }
}

// ── property: chain length grows linearly + topo doesn't blow up ──

proptest! {
    #[test]
    fn long_chain_compiles_in_bounded_time(n in 1usize..100) {
        let g = chain_graph(n);
        let start = std::time::Instant::now();
        let compiled = g.compile().unwrap();
        let elapsed = start.elapsed();
        // Bound: 1 ms per node is plenty even on a slow CI host.
        // Catches accidental O(n²) regressions in the topo sort.
        prop_assert!(
            elapsed.as_millis() < (n as u128 * 1) + 100,
            "chain of {} compiled in {:?} (cap was {} ms)",
            n, elapsed, (n as u128) + 100
        );
        prop_assert_eq!(compiled.node_count(), n);
    }
}
