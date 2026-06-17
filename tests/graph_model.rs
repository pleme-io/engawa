//! Model-based property test for `RenderGraph::compile`.
//!
//! The existing `graph_proptest.rs` fuzzes *hand-built* chain/fanout graphs.
//! This file drives **arbitrary** graphs — arbitrary nodes, arbitrary
//! input/output edges over a small shared resource pool, arbitrary graph
//! inputs/outputs, duplicate node ids, compute-without-dispatch, multi-writers,
//! cycles — through `compile()` and asserts the accept/reject decision matches
//! an **independent reference validator** (a second implementation of "is this
//! graph well-formed?"), plus that every accepted graph is a correct topo-sort.
//!
//! This is the engawa goldmine: `compile()` is the one surface that makes
//! "an unbound input / a cycle / two writers is a COMPILE error, never a runtime
//! crash" true. A second, structurally-different validator pins it over the
//! whole small-graph space.

use std::collections::{BTreeMap, BTreeSet};

use engawa::{
    CompiledGraph, ComputeDispatch, DrawKind, Node, NodeId, PassKind, RenderGraph, ResourceId,
    ResourceKind,
};
use proptest::prelude::*;

const N_RES: u8 = 5;
const N_NODE_IDS: u8 = 6;

fn rid(i: u8) -> ResourceId {
    ResourceId::new(format!("r{i}"))
}

fn texture() -> ResourceKind {
    ResourceKind::Texture { width: Some(64), height: Some(64), format: None, sample_count: None }
}

// ── the independent oracle: a second "is this well-formed?" implementation ──

/// True iff `compile()` should succeed — computed independently of `compile`.
fn well_formed(g: &RenderGraph) -> bool {
    // 1. unique node ids
    let mut ids = BTreeSet::new();
    for n in &g.nodes {
        if !ids.insert(n.id.as_str()) {
            return false;
        }
    }
    // 2. no compute node missing its dispatch grid
    for n in &g.nodes {
        if n.pass == PassKind::Compute && n.dispatch.is_none() {
            return false;
        }
    }
    // 3. no resource with two or more distinct writers
    let mut writers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for n in &g.nodes {
        for o in &n.outputs {
            writers.entry(o.as_str()).or_default().insert(n.id.as_str());
        }
    }
    if writers.values().any(|w| w.len() >= 2) {
        return false;
    }
    let produced: BTreeSet<&str> = writers.keys().copied().collect();
    let g_inputs: BTreeSet<&str> = g.inputs.iter().map(ResourceId::as_str).collect();
    // 4. every node input is a graph input or produced by some node
    for n in &g.nodes {
        for i in &n.inputs {
            if !g_inputs.contains(i.as_str()) && !produced.contains(i.as_str()) {
                return false;
            }
        }
    }
    // 5. every declared graph output is produced or is itself a graph input
    for o in &g.outputs {
        if !produced.contains(o.as_str()) && !g_inputs.contains(o.as_str()) {
            return false;
        }
    }
    // 6. no cycle in the producer→consumer graph (single writer per resource here)
    !has_cycle(g, &writers)
}

/// Independent cycle detection over distinct producer→consumer edges (self-edges
/// excluded — a node reading its own output is not a cycle).
fn has_cycle(g: &RenderGraph, writers: &BTreeMap<&str, BTreeSet<&str>>) -> bool {
    let producer: BTreeMap<&str, &str> =
        writers.iter().map(|(r, w)| (*r, *w.iter().next().unwrap())).collect();
    let mut adj: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut indeg: BTreeMap<&str, usize> = g.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
    let mut edges: BTreeSet<(&str, &str)> = BTreeSet::new();
    for n in &g.nodes {
        for i in &n.inputs {
            if let Some(p) = producer.get(i.as_str()) {
                if *p != n.id.as_str() && edges.insert((*p, n.id.as_str())) {
                    adj.entry(*p).or_default().insert(n.id.as_str());
                    *indeg.get_mut(n.id.as_str()).unwrap() += 1;
                }
            }
        }
    }
    let mut ready: Vec<&str> =
        indeg.iter().filter(|(_, d)| **d == 0).map(|(k, _)| *k).collect();
    let mut visited = 0usize;
    while let Some(x) = ready.pop() {
        visited += 1;
        if let Some(cs) = adj.get(x) {
            for c in cs {
                let d = indeg.get_mut(c).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.push(c);
                }
            }
        }
    }
    visited != g.nodes.len()
}

/// Validate that an accepted compile is a correct topo-sort.
fn check_compiled(g: &RenderGraph, c: &CompiledGraph) -> Result<(), String> {
    // The execution order is a permutation of the declared nodes.
    let mut got: Vec<&str> = c.execution_order.iter().map(|n| n.id.as_str()).collect();
    let mut want: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
    got.sort_unstable();
    want.sort_unstable();
    if got != want {
        return Err(format!("execution order {got:?} is not a permutation of {want:?}"));
    }
    // Every producer of a node's inputs appears strictly earlier.
    let pos: BTreeMap<&str, usize> =
        c.execution_order.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut producer: BTreeMap<&str, &str> = BTreeMap::new();
    for n in &c.execution_order {
        for o in &n.outputs {
            producer.insert(o.as_str(), n.id.as_str());
        }
    }
    for n in &c.execution_order {
        for i in &n.inputs {
            if let Some(p) = producer.get(i.as_str()) {
                if *p != n.id.as_str() && pos[*p] >= pos[n.id.as_str()] {
                    return Err(format!(
                        "{} (pos {}) reads {} produced by {} (pos {})",
                        n.id.as_str(),
                        pos[n.id.as_str()],
                        i.as_str(),
                        p,
                        pos[*p]
                    ));
                }
            }
        }
    }
    Ok(())
}

// ── generators ─────────────────────────────────────────────────

fn arb_node() -> impl Strategy<Value = Node> {
    (
        0u8..N_NODE_IDS,
        prop_oneof![Just(PassKind::Render), Just(PassKind::Compute), Just(PassKind::Blit)],
        any::<bool>(),
        proptest::collection::btree_set(0u8..N_RES, 0..3),
        proptest::collection::btree_set(0u8..N_RES, 0..2),
    )
        .prop_map(|(idx, pass, has_dispatch, inputs, outputs)| Node {
            id: NodeId::new(format!("n{idx}")),
            pass,
            inputs: inputs.into_iter().map(rid).collect(),
            outputs: outputs.into_iter().map(rid).collect(),
            material: None,
            draw: DrawKind::default(),
            depth: None,
            dispatch: has_dispatch.then(|| ComputeDispatch::linear(64, 8)),
        })
}

fn arb_graph() -> impl Strategy<Value = RenderGraph> {
    (
        proptest::collection::vec(arb_node(), 0..6),
        proptest::collection::btree_set(0u8..N_RES, 0..3),
        proptest::collection::btree_set(0u8..N_RES, 0..3),
    )
        .prop_map(|(nodes, inputs, outputs)| {
            let mut g = RenderGraph::default();
            for i in 0..N_RES {
                g = g.with_resource(rid(i), texture());
            }
            for n in nodes {
                g = g.with_node(n);
            }
            for i in inputs {
                g = g.with_input(rid(i));
            }
            for o in outputs {
                g = g.with_output(rid(o));
            }
            g
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1500, ..ProptestConfig::default() })]

    /// `compile()` accepts EXACTLY the well-formed graphs (bidirectional vs the
    /// independent oracle) — and every accepted graph is a correct, deterministic
    /// topo-sort.
    #[test]
    fn compile_decision_matches_oracle(g in arb_graph()) {
        let expected_ok = well_formed(&g);
        let result = g.clone().compile();
        prop_assert_eq!(result.is_ok(), expected_ok,
            "compile().is_ok()={} but oracle well_formed={}\ngraph nodes: {:?}",
            result.is_ok(), expected_ok,
            g.nodes.iter().map(|n| (n.id.as_str(), n.pass, n.inputs.len(), n.outputs.len())).collect::<Vec<_>>());

        if let Ok(c) = result {
            if let Err(why) = check_compiled(&g, &c) {
                prop_assert!(false, "accepted graph is not a valid topo-sort: {why}");
            }
            // Determinism: the same graph compiles to the same order byte-for-byte.
            let again = g.compile().expect("deterministic re-compile");
            let order_a: Vec<&str> = c.execution_order.iter().map(|n| n.id.as_str()).collect();
            let order_b: Vec<&str> = again.execution_order.iter().map(|n| n.id.as_str()).collect();
            prop_assert_eq!(order_a, order_b);
        }
    }
}

// ── targeted adversarial case: a node that reads the SAME resource twice ──

/// A node listing one resource in its `inputs` more than once (produced by a
/// different node) must still compile — it is one dependency edge, not a cycle.
#[test]
fn node_reading_same_input_twice_compiles() {
    let g = RenderGraph::default()
        .with_resource("r0", texture())
        .with_resource("r1", texture())
        .with_output("r1")
        .with_node(Node::clear("producer", "r0"))
        .with_node(Node {
            id: NodeId::new("consumer"),
            pass: PassKind::Render,
            inputs: vec![ResourceId::new("r0"), ResourceId::new("r0")], // r0 twice
            outputs: vec![ResourceId::new("r1")],
            material: None,
            draw: DrawKind::default(),
            depth: None,
            dispatch: None,
        });
    let compiled = g.compile().expect("reading one resource twice is one edge, not a cycle");
    assert_eq!(compiled.node_count(), 2);
    // producer runs before consumer.
    let order: Vec<&str> = compiled.execution_order.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(order, vec!["producer", "consumer"]);
}
