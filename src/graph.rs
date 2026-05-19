//! The render graph proper — typed DAG of nodes + compile-time
//! validation + topo sort into execution order.
//!
//! Operators construct a `RenderGraph` declaratively (or via the
//! tatara-lisp `(defgraph …)` form, planned v0.3), `compile()` it
//! into a `CompiledGraph`, and hand the `CompiledGraph` to a
//! consumer (mado, future ayatsuri) that owns the wgpu context
//! and dispatches the nodes in order.
//!
//! All validation happens at compile — never at dispatch.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{EngawaError, ValidationError};
use crate::node::{Node, NodeId};
use crate::resource::{ResourceId, ResourceKind};

/// Pre-compile render graph — operator-authored, mutable, holds
/// the declared topology before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RenderGraph {
    /// Resources declared in the graph + their kind. Graph
    /// inputs (resources produced *externally* by the consumer
    /// — e.g. the swapchain surface texture, a per-frame uniform
    /// the consumer fills) live here too.
    pub resources: BTreeMap<ResourceId, ResourceKind>,
    /// Resource ids that are provided by the consumer at
    /// dispatch (not produced by any node). The compile step
    /// validates against this list.
    pub inputs: BTreeSet<ResourceId>,
    /// Resource ids the consumer expects to read after the graph
    /// runs (typically the final color attachment that gets
    /// presented to the swapchain).
    pub outputs: BTreeSet<ResourceId>,
    /// Nodes declared in this graph. Order is irrelevant —
    /// compile re-orders via topo sort.
    pub nodes: Vec<Node>,
}

impl RenderGraph {
    /// Add a resource declaration. Returns self for fluent
    /// construction.
    #[must_use]
    pub fn with_resource(mut self, id: impl Into<ResourceId>, kind: ResourceKind) -> Self {
        self.resources.insert(id.into(), kind);
        self
    }

    /// Mark a resource as consumer-provided (graph input).
    #[must_use]
    pub fn with_input(mut self, id: impl Into<ResourceId>) -> Self {
        self.inputs.insert(id.into());
        self
    }

    /// Mark a resource as a graph output (consumer reads after
    /// the graph runs).
    #[must_use]
    pub fn with_output(mut self, id: impl Into<ResourceId>) -> Self {
        self.outputs.insert(id.into());
        self
    }

    /// Add a node. Returns self for fluent construction.
    #[must_use]
    pub fn with_node(mut self, node: Node) -> Self {
        self.nodes.push(node);
        self
    }

    /// Validate + topo-sort. Catches duplicates, cycles, unbound
    /// inputs, multiple writers, and unbound outputs. Returns a
    /// `CompiledGraph` whose `execution_order` is a deterministic
    /// node order ready for dispatch.
    pub fn compile(self) -> Result<CompiledGraph, EngawaError> {
        // Reject duplicate node ids.
        let mut seen_nodes: BTreeSet<NodeId> = BTreeSet::new();
        for n in &self.nodes {
            if !seen_nodes.insert(n.id.clone()) {
                return Err(ValidationError::DuplicateNode(n.id.clone()).into());
            }
        }

        // Build producer map (resource → node that writes it).
        // Reject multiple-writer collisions.
        let mut producer: BTreeMap<ResourceId, NodeId> = BTreeMap::new();
        for n in &self.nodes {
            for out in &n.outputs {
                if let Some(prior) = producer.insert(out.clone(), n.id.clone())
                    && prior != n.id
                {
                    return Err(ValidationError::MultipleWriters(out.clone()).into());
                }
            }
        }

        // Every node input must come from either a graph input
        // OR another node's output. Otherwise it's unbound.
        for n in &self.nodes {
            for input in &n.inputs {
                if !self.inputs.contains(input) && !producer.contains_key(input) {
                    return Err(ValidationError::UnboundInput {
                        node: n.id.clone(),
                        resource: input.clone(),
                    }
                    .into());
                }
            }
        }

        // Every declared graph output must be produced.
        for out in &self.outputs {
            if !producer.contains_key(out) && !self.inputs.contains(out) {
                return Err(ValidationError::UnboundOutput(out.clone()).into());
            }
        }

        // Topo-sort via Kahn's algorithm — deterministic order
        // because we visit nodes in BTreeMap order on each step.
        let nodes_by_id: BTreeMap<NodeId, Node> =
            self.nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();
        let mut indeg: BTreeMap<NodeId, usize> = BTreeMap::new();
        for n in &self.nodes {
            let mut count = 0usize;
            for input in &n.inputs {
                if let Some(producer_id) = producer.get(input)
                    && *producer_id != n.id
                {
                    count += 1;
                }
            }
            indeg.insert(n.id.clone(), count);
        }
        let mut ready: BTreeSet<NodeId> = indeg
            .iter()
            .filter(|&(_, d)| *d == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut order: Vec<NodeId> = Vec::with_capacity(self.nodes.len());
        while let Some(next_id) = ready.iter().next().cloned() {
            ready.remove(&next_id);
            order.push(next_id.clone());
            // Decrement indeg of consumers of `next_id`'s outputs.
            let node = &nodes_by_id[&next_id];
            for out in &node.outputs {
                for other in &self.nodes {
                    if other.id == next_id {
                        continue;
                    }
                    if other.inputs.contains(out)
                        && let Some(d) = indeg.get_mut(&other.id)
                    {
                        if *d > 0 {
                            *d -= 1;
                        }
                        if *d == 0 {
                            ready.insert(other.id.clone());
                        }
                    }
                }
            }
        }
        if order.len() != self.nodes.len() {
            // Whatever's left has a non-zero in-degree → cycle.
            let stuck: Vec<NodeId> = indeg
                .into_iter()
                .filter(|(id, d)| *d > 0 && !order.contains(id))
                .map(|(id, _)| id)
                .collect();
            return Err(ValidationError::Cycle(stuck).into());
        }

        Ok(CompiledGraph {
            resources: self.resources,
            inputs: self.inputs,
            outputs: self.outputs,
            execution_order: order
                .into_iter()
                .map(|id| nodes_by_id[&id].clone())
                .collect(),
        })
    }
}

/// Validated + topo-sorted graph ready for the consumer to
/// dispatch against a wgpu context. Engawa does not own the
/// wgpu side; consumers walk `execution_order` in their own
/// render loop and bind the typed `resources` to concrete
/// wgpu handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledGraph {
    pub resources: BTreeMap<ResourceId, ResourceKind>,
    pub inputs: BTreeSet<ResourceId>,
    pub outputs: BTreeSet<ResourceId>,
    pub execution_order: Vec<Node>,
}

impl CompiledGraph {
    /// How many nodes are in the compiled graph. Useful for
    /// "did this effect contribute anything" checks.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.execution_order.len()
    }

    /// Iterate nodes in execution order. The consumer walks this
    /// to drive its render loop.
    pub fn iter_nodes(&self) -> impl Iterator<Item = &Node> {
        self.execution_order.iter()
    }
}
