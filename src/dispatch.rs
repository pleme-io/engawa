//! Dispatcher contract — the API consumers (mado, future
//! ayatsuri) implement to drive a `CompiledGraph` against
//! actual GPU resources.
//!
//! Engawa stays GPU-free. The `Dispatcher` trait is pure
//! abstraction: takes a compiled graph + per-resource concrete
//! handles + a per-frame uniform set, walks the execution
//! order, dispatches each node. Implementations:
//!
//! * **`RecordingDispatcher`** (this module, always available)
//!   — records the dispatch order as `RecordedDispatch` events
//!   so tests can assert "node X ran before node Y with these
//!   bindings." Zero GPU; used by every engawa-adopting
//!   consumer's test suite.
//! * **`WgpuDispatcher`** — real wgpu-backed dispatch via
//!   `garasu::GpuContext`. Lives in a separate crate
//!   (`engawa-wgpu`) so engawa's core stays free of the wgpu dep
//!   for non-rendering consumers (graph linting, visualization,
//!   lisp-side authoring).
//!
//!   ★ CORRECTED 2026-08-30. This said "(planned, v0.2)" and had
//!   been wrong for some time: `engawa-wgpu` ships at 0.1.10 on
//!   crates.io and mado calls `dispatch_with` in production. The
//!   error propagated — a fleet census read this comment, recorded
//!   "blocked on engawa v0.2", and a promotion plan was built on
//!   it. A stale doc comment is not inert; it is an assertion that
//!   gets believed.
//! * **`MetalDispatcher`** (`asobi/crates/engawa-metal`) — a
//!   SECOND backend, 2,348 lines, implementing the capability set
//!   engawa-wgpu lacks. Two independent backends over one IR is
//!   why `capability` exists.
//!
//! The two-impl pattern mirrors what every other pleme-io
//! primitive does: shikumi's `ConfigStore` ships in-process
//! tests alongside the live notify-watcher path; garasu's
//! `HeadlessTarget` lets every GPU consumer ship deterministic
//! pixel tests without a real surface. The recording dispatcher
//! is the equivalent for render graphs — a typed, deterministic
//! test substrate that catches dispatch-order regressions
//! before any real GPU is touched.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::graph::CompiledGraph;
use crate::node::{Node, NodeId};
use crate::resource::ResourceId;

/// Concrete handle for a `ResourceId` at dispatch time.
///
/// Engawa stays GPU-free, so this is a sum of opaque tags —
/// each variant carries a `String` identity the consumer maps
/// to its own wgpu / metal / vulkan handle. Real-impls (e.g.
/// `engawa-wgpu`) wrap this in their own resource map.
///
/// The recording dispatcher just emits the variant name in
/// every dispatch event so test assertions can check "node X
/// read texture `scene`, wrote texture `post`."
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceHandle {
    /// A 2D texture identified by name.
    Texture(String),
    /// A uniform buffer identified by name.
    Uniform(String),
    /// A storage buffer identified by name.
    Storage(String),
    /// A sampler identified by name.
    Sampler(String),
    /// An external handle owned by the consumer (e.g. the
    /// swapchain surface). Engawa never inspects the payload.
    External(String),
}

/// Per-frame resource bindings the consumer provides at
/// dispatch. Maps each `ResourceId` the graph references to a
/// concrete handle.
///
/// At compile time, engawa already validated that every node
/// input either comes from another node's output OR appears in
/// the graph's declared inputs. The dispatcher's
/// `dispatch_graph` validates that every input/output ID has
/// a binding in this map before walking the execution order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceBindings {
    inner: BTreeMap<ResourceId, ResourceHandle>,
}

impl ResourceBindings {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, id: impl Into<ResourceId>, handle: ResourceHandle) -> Self {
        self.inner.insert(id.into(), handle);
        self
    }

    pub fn insert(&mut self, id: impl Into<ResourceId>, handle: ResourceHandle) {
        self.inner.insert(id.into(), handle);
    }

    #[must_use]
    pub fn get(&self, id: &ResourceId) -> Option<&ResourceHandle> {
        self.inner.get(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate every (id, handle) pair in deterministic order
    /// (`BTreeMap` is sorted).
    pub fn iter(&self) -> impl Iterator<Item = (&ResourceId, &ResourceHandle)> {
        self.inner.iter()
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DispatchError {
    #[error("node {node:?} references resource {resource:?} but no binding was supplied")]
    MissingBinding { node: NodeId, resource: ResourceId },
    /// The graph declares a feature this backend does not honour.
    ///
    /// ★ This variant is the whole point of the capability contract. Before it
    /// existed, a backend that ignored `node.draw` did not fail — it drew a
    /// fullscreen triangle and returned `Ok`. An unhonoured declaration is now
    /// a refusal that NAMES the node and the feature, so the operator learns it
    /// at dispatch instead of by looking at a wrong picture.
    #[error(
        "node {node:?} requires {} which this backend does not honour. The \
         declaration(s) would have been SILENTLY IGNORED — engawa refuses rather \
         than render something the graph did not describe.",
        .capabilities.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    )]
    Unsupported {
        node: NodeId,
        /// EVERY gap for this node, not the first. A partial answer sends the
        /// reader back for a second round-trip after fixing one of them.
        capabilities: Vec<crate::capability::Capability>,
    },
    /// Per-consumer dispatch failure surfaces here so the trait
    /// returns one typed error variant regardless of backend.
    #[error("dispatch backend failure: {0}")]
    Backend(String),
}

/// What a dispatcher does, abstractly. The default
/// `dispatch_graph` walks the compiled execution order +
/// validates every input/output binding is present + delegates
/// each node to `dispatch_node`.
///
/// Consumers implementing this trait only need to override
/// `dispatch_node` with their backend's encoder dispatch (a
/// wgpu render pass, a metal encoder call, etc.). The default
/// `dispatch_graph` handles the "walk the topology + check
/// bindings" plumbing for free.
pub trait Dispatcher {
    /// Dispatch a single node. Called by `dispatch_graph` in
    /// execution order. Implementations record / encode /
    /// dispatch as appropriate for their backend.
    fn dispatch_node(
        &mut self,
        node: &Node,
        bindings: &ResourceBindings,
    ) -> Result<(), DispatchError>;

    /// What this backend actually honours.
    ///
    /// ★ DEFAULTS TO [`Capabilities::MINIMAL`], deliberately: a backend that
    /// has not declared anything is assumed to honour nothing beyond a
    /// fullscreen draw. Defaulting to "everything" would preserve exactly the
    /// silent-wrong-picture failure this contract removes, and would do it for
    /// every backend written after this line.
    fn capabilities(&self) -> crate::capability::Capabilities {
        crate::capability::Capabilities::MINIMAL
    }

    /// Walk `graph.execution_order`, validating bindings + then
    /// calling `dispatch_node` for each. The default impl
    /// handles all engawa-side concerns; backends typically
    /// don't override this.
    fn dispatch_graph(
        &mut self,
        graph: &CompiledGraph,
        bindings: &ResourceBindings,
    ) -> Result<(), DispatchError> {
        // ★ CAPABILITY CHECK FIRST, before any node is dispatched. A graph
        // that declares what this backend cannot honour must fail whole rather
        // than render a partly-wrong frame: half a picture is harder to
        // diagnose than none, because it looks like a different bug.
        let have = self.capabilities();
        for node in graph.iter_nodes() {
            let gaps = crate::capability::required_for_node(node).missing_from(&have);
            if !gaps.is_empty() {
                return Err(DispatchError::Unsupported {
                    node: node.id.clone(),
                    capabilities: gaps,
                });
            }
        }

        for node in graph.iter_nodes() {
            for input in &node.inputs {
                if bindings.get(input).is_none() {
                    return Err(DispatchError::MissingBinding {
                        node: node.id.clone(),
                        resource: input.clone(),
                    });
                }
            }
            for output in &node.outputs {
                if bindings.get(output).is_none() {
                    return Err(DispatchError::MissingBinding {
                        node: node.id.clone(),
                        resource: output.clone(),
                    });
                }
            }
            self.dispatch_node(node, bindings)?;
        }
        Ok(())
    }
}

/// One entry in a recording dispatcher's tape. Captures
/// everything a test might want to assert about a dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedDispatch {
    pub node_id: NodeId,
    pub inputs: Vec<(ResourceId, ResourceHandle)>,
    pub outputs: Vec<(ResourceId, ResourceHandle)>,
}

/// Test-time dispatcher that records every `dispatch_node`
/// call to a tape. Operators (and CI) get deterministic
/// regression coverage of dispatch-order + binding-resolution
/// behaviour without touching wgpu.
///
/// The pattern: a consumer (mado) compiles its graph, builds
/// `ResourceBindings`, dispatches into a `RecordingDispatcher`
/// in its test suite, and asserts on the tape. Any future
/// `WgpuDispatcher` impl is checked against the same tape —
/// same node order, same per-node bindings, same input/output
/// resolution.
#[derive(Debug, Clone, Default)]
pub struct RecordingDispatcher {
    tape: Vec<RecordedDispatch>,
}

impl RecordingDispatcher {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the recorded tape. Useful in tests after a
    /// `dispatch_graph` call.
    #[must_use]
    pub fn tape(&self) -> &[RecordedDispatch] {
        &self.tape
    }

    /// Number of recorded dispatches.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tape.len()
    }

    /// True iff no dispatches were recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tape.is_empty()
    }

    /// Iterate dispatch events in recorded order.
    pub fn iter(&self) -> impl Iterator<Item = &RecordedDispatch> {
        self.tape.iter()
    }
}

impl Dispatcher for RecordingDispatcher {
    /// The recorder honours every capability BY CONSTRUCTION — it records the
    /// node and renders nothing, so there is no feature it can silently drop.
    ///
    /// ★ It must still SAY so. When the capability check landed, this
    /// dispatcher had declared nothing, inherited `MINIMAL`, and immediately
    /// refused `game_extensions::compiles_and_dispatches_a_depth_tested_indexed_mesh`
    /// with `Unsupported { node: "mesh", capability: GeometryDraw }`. That is
    /// the fail-closed default doing its job on its first run, against a real
    /// test, before any backend was touched.
    fn capabilities(&self) -> crate::capability::Capabilities {
        crate::capability::Capabilities::all()
    }

    fn dispatch_node(
        &mut self,
        node: &Node,
        bindings: &ResourceBindings,
    ) -> Result<(), DispatchError> {
        let inputs = node
            .inputs
            .iter()
            .map(|id| {
                let handle = bindings
                    .get(id)
                    .ok_or_else(|| DispatchError::MissingBinding {
                        node: node.id.clone(),
                        resource: id.clone(),
                    })?
                    .clone();
                Ok((id.clone(), handle))
            })
            .collect::<Result<Vec<_>, DispatchError>>()?;
        let outputs = node
            .outputs
            .iter()
            .map(|id| {
                let handle = bindings
                    .get(id)
                    .ok_or_else(|| DispatchError::MissingBinding {
                        node: node.id.clone(),
                        resource: id.clone(),
                    })?
                    .clone();
                Ok((id.clone(), handle))
            })
            .collect::<Result<Vec<_>, DispatchError>>()?;
        self.tape.push(RecordedDispatch {
            node_id: node.id.clone(),
            inputs,
            outputs,
        });
        Ok(())
    }
}
