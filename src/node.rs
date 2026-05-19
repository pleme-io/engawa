//! Render-graph nodes — the unit of work in the engawa IR.
//!
//! A `Node` is a single unit of GPU work: read N input resources,
//! produce M output resources, via a `PassKind` (render / compute /
//! blit). The graph topology is just `Node`s + their input/output
//! `ResourceId`s; the compiler topo-sorts them into execution
//! order.

use serde::{Deserialize, Serialize};

use crate::material::Material;
use crate::pass::PassKind;
use crate::resource::ResourceId;

/// Operator-friendly node identifier. Short, stable, distinct
/// across the whole graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// One unit of GPU work in the render graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub pass: PassKind,
    /// Resources this node reads (texture samples, uniform
    /// reads, storage reads). The compiler enforces that each
    /// listed resource is either a graph input OR an output of
    /// some other node.
    pub inputs: Vec<ResourceId>,
    /// Resources this node writes (color attachments, storage
    /// writes). The compiler enforces that each output has
    /// exactly one writer node — pleme-io's "solve once, in one
    /// place" rule applied to GPU resources.
    pub outputs: Vec<ResourceId>,
    /// Optional material to dispatch. `None` is valid for nodes
    /// that just clear a target or do a pure blit (no shader).
    pub material: Option<Material>,
}

impl Node {
    /// Convenience constructor for the common case: a render-pass
    /// node with one input texture (the prior scene) and one
    /// output texture (the post-processed result).
    #[must_use]
    pub fn fullscreen_effect(
        id: impl Into<NodeId>,
        material: Material,
        input: impl Into<ResourceId>,
        output: impl Into<ResourceId>,
    ) -> Self {
        Self {
            id: id.into(),
            pass: PassKind::Render,
            inputs: vec![input.into()],
            outputs: vec![output.into()],
            material: Some(material),
        }
    }

    /// Convenience constructor for a clear-only node — produces
    /// one output with no inputs, no material.
    #[must_use]
    pub fn clear(id: impl Into<NodeId>, output: impl Into<ResourceId>) -> Self {
        Self {
            id: id.into(),
            pass: PassKind::Render,
            inputs: vec![],
            outputs: vec![output.into()],
            material: None,
        }
    }
}
