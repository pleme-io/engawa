//! Typed GPU resources flowing through the render graph.
//!
//! A `Resource` is anything one node produces and another node
//! consumes: a render-target texture, an intermediate offscreen
//! buffer, a sampler, a uniform/storage buffer. Engawa's compiler
//! routes these between nodes by `ResourceId`, so the *graph
//! topology* is decoupled from the *concrete wgpu handles*. At
//! dispatch time, the consumer (e.g. mado) maps each id to a
//! concrete `wgpu::TextureView` / `wgpu::Buffer`.

use serde::{Deserialize, Serialize};

/// Operator-friendly identifier for a resource. Short, stable,
/// human-typeable. Used as the wire key in the IR + as the lookup
/// key when the consumer binds wgpu handles at dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceId(pub String);

impl ResourceId {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ResourceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ResourceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// What kind of resource this is. The compiler needs to know
/// because each kind has distinct bind-group layout + lifetime
/// semantics on the wgpu side.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceKind {
    /// A 2D texture that can be rendered into AND sampled from.
    /// The canonical mado case: the cell-grid render pass writes
    /// into a Texture; an effect pass samples it.
    Texture {
        /// Optional dimension hint. None means "match the
        /// graph's output dimensions" — most effects don't need
        /// to be told their resolution at IR time.
        width: Option<u32>,
        height: Option<u32>,
    },
    /// A uniform buffer the consumer fills with per-frame data
    /// (resolution, time, cursor position, etc.).
    Uniform { size_bytes: u32 },
    /// A storage buffer for compute passes that need scratch
    /// memory across dispatches.
    Storage { size_bytes: u32 },
    /// A texture sampler. Engawa doesn't care about the filter
    /// params at IR time; the consumer picks them.
    Sampler,
    /// External handle — a wgpu handle the consumer owns and
    /// passes in (e.g. the swapchain's surface texture). The
    /// graph treats it as an opaque sink/source.
    External,
}
