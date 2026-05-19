//! Materials — shader source + uniform/binding declarations.
//!
//! A `Material` is the *what to draw* layer that pairs with a
//! `Node`'s *where in the graph* layer. The split mirrors bevy's
//! material/render-graph split but stays pure-data: no wgpu
//! types, no rust closures, no compile-time codegen. The
//! consumer turns a Material into a wgpu::RenderPipeline at
//! compile time.
//!
//! Tatara-lisp `(defmaterial …)` forms compile to this struct
//! one-to-one (planned, v0.3+).

use serde::{Deserialize, Serialize};

use crate::resource::ResourceId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Material {
    /// Operator-friendly name. Used in error messages + as the
    /// key when an effect catalog hot-reloads.
    pub name: String,
    /// WGSL source. Either inline (for short test materials) or
    /// a path the consumer's hot-reload watcher monitors.
    pub shader: ShaderSource,
    /// Declared uniform / texture / sampler / storage bindings,
    /// in `@group(0) @binding(0..N)` order.
    pub bindings: Vec<UniformBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShaderSource {
    /// Inline WGSL string. Used by built-in effects + tests.
    /// Struct variant (not newtype) because serde's internally-
    /// tagged representation can't hold a primitive in a tuple
    /// variant.
    Inline { wgsl: String },
    /// Path to a `.wgsl` file. The consumer's notify watcher
    /// reloads + recompiles on change.
    Path { path: String },
}

impl ShaderSource {
    /// Convenience constructor for the common inline case.
    #[must_use]
    pub fn inline(wgsl: impl Into<String>) -> Self {
        Self::Inline { wgsl: wgsl.into() }
    }

    /// Convenience constructor for the path case.
    #[must_use]
    pub fn path(path: impl Into<String>) -> Self {
        Self::Path { path: path.into() }
    }

    /// Operator-facing text — `inline:<first 40 chars>` for
    /// inline sources, `path:<the path>` for paths. Useful in
    /// log lines + validation errors so the operator can find
    /// what they're looking at.
    #[must_use]
    pub fn display_short(&self) -> String {
        match self {
            ShaderSource::Inline { wgsl } => {
                let preview: String = wgsl.chars().take(40).collect();
                format!("inline:{preview}")
            }
            ShaderSource::Path { path } => format!("path:{path}"),
        }
    }
}

/// One binding slot in the material's `@group(0)`. The compiler
/// emits the bind-group-layout entry from this declaration and
/// pairs it with the corresponding `ResourceId` at compile time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UniformBinding {
    /// Binding index — matches the `@binding(N)` in WGSL.
    pub binding: u32,
    /// What this slot accepts. Determines bind-group-layout
    /// entry type + the resource-id kind that must satisfy it.
    pub kind: BindingKind,
    /// Resource this binding draws from. The graph compile step
    /// validates that the bound resource's kind matches this
    /// binding's kind.
    pub resource: ResourceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    /// `var<uniform>` — fixed-size per-frame data (resolution,
    /// time, cell metrics, cursor position).
    Uniform,
    /// `var<storage, read>` — read-only storage buffer.
    StorageRead,
    /// `var<storage, read_write>` — compute-shader scratch.
    StorageReadWrite,
    /// `texture_2d<f32>` — sampled texture input.
    Texture,
    /// `sampler` — sampling state for an adjacent texture binding.
    Sampler,
}
