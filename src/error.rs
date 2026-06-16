//! Typed errors. Every failure path through the graph compile
//! returns one of these; consumers never see a generic
//! `anyhow::Error` from engawa.

use thiserror::Error;

use crate::node::NodeId;
use crate::resource::ResourceId;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EngawaError {
    #[error("validation failed: {0}")]
    Validation(#[from] ValidationError),
}

/// Validation errors caught at compile time — before any wgpu
/// command lands on the GPU. Every variant carries enough
/// context that an operator looking at the error message can
/// trace it back to the offending declaration in their effect
/// catalog.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    #[error("duplicate node id: {0:?}")]
    DuplicateNode(NodeId),

    #[error("duplicate resource id: {0:?}")]
    DuplicateResource(ResourceId),

    #[error(
        "node {node:?} references resource {resource:?} that no other node produces \
         and that isn't declared as a graph input"
    )]
    UnboundInput {
        node: NodeId,
        resource: ResourceId,
    },

    #[error(
        "cycle detected in render graph; involved nodes: {0:?}"
    )]
    Cycle(Vec<NodeId>),

    #[error(
        "graph output {0:?} is not produced by any node"
    )]
    UnboundOutput(ResourceId),

    #[error(
        "resource {0:?} is produced by multiple nodes; each resource must have a single writer"
    )]
    MultipleWriters(ResourceId),

    #[error(
        "compute node {0:?} has no dispatch grid; a PassKind::Compute node must carry a ComputeDispatch"
    )]
    ComputeWithoutDispatch(NodeId),
}
