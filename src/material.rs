//! Materials — shader source + uniform/binding declarations.
//!
//! A `Material` is the *what to draw* layer that pairs with a
//! `Node`'s *where in the graph* layer. The split mirrors bevy's
//! material/render-graph split but stays pure-data: no wgpu
//! types, no rust closures, no compile-time codegen. The
//! consumer turns a Material into a `wgpu::RenderPipeline` (or an
//! `MTLRenderPipelineState`) at compile time, driven by `state`.
//!
//! Tatara-lisp `(defmaterial …)` forms compile to this struct
//! one-to-one (planned, v0.3+).

use serde::{Deserialize, Serialize};

use crate::pipeline::RenderState;
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
    /// Fixed-function render state — blend / cull / winding /
    /// topology. All-default = opaque, no cull, ccw, triangle-list
    /// (so existing full-screen effects are unchanged).
    #[serde(default)]
    pub state: RenderState,
}

impl Material {
    /// Construct a material with default render state (opaque, no
    /// cull, ccw, triangle-list). Use [`Material::with_state`] to
    /// override.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        shader: ShaderSource,
        bindings: Vec<UniformBinding>,
    ) -> Self {
        Self {
            name: name.into(),
            shader,
            bindings,
            state: RenderState::default(),
        }
    }

    /// Override the fixed-function render state. Builder-style.
    #[must_use]
    pub fn with_state(mut self, state: RenderState) -> Self {
        self.state = state;
        self
    }
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
    /// **Raw Metal Shading Language** source + explicit entry-point
    /// names — the typed escape hatch for shader stages naga cannot
    /// emit from WGSL (mesh / object / amplification shaders, tile /
    /// programmable-blending kernels, procedural-AABB ray-query). The
    /// backend compiles it with `newLibraryWithSource` and looks up
    /// `vertex`/`fragment`/`compute` by the author-given names (no
    /// naga rename pass). Author-controlled names because the author
    /// wrote the MSL. This is the Metal-4-frontier authoring surface;
    /// a backend that can't accept MSL (e.g. a wgpu preview) rejects it
    /// at dispatch with a typed `UnsupportedShaderSource`.
    Msl {
        source: String,
        /// Vertex (or mesh/object) entry-point function name.
        vertex: Option<String>,
        /// Fragment entry-point function name.
        fragment: Option<String>,
        /// Compute entry-point function name.
        compute: Option<String>,
    },
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

    /// A raw-MSL render shader with explicit vertex + fragment entry names.
    #[must_use]
    pub fn msl_render(
        source: impl Into<String>,
        vertex: impl Into<String>,
        fragment: impl Into<String>,
    ) -> Self {
        Self::Msl {
            source: source.into(),
            vertex: Some(vertex.into()),
            fragment: Some(fragment.into()),
            compute: None,
        }
    }

    /// A raw-MSL compute kernel with an explicit entry name.
    #[must_use]
    pub fn msl_compute(source: impl Into<String>, compute: impl Into<String>) -> Self {
        Self::Msl {
            source: source.into(),
            vertex: None,
            fragment: None,
            compute: Some(compute.into()),
        }
    }

    /// Operator-facing text — `inline:<first 40 chars>` for inline
    /// sources, `path:<the path>` for paths, `msl:<vs>/<fs>` for raw MSL.
    /// Useful in log lines + validation errors so the operator can find
    /// what they're looking at.
    #[must_use]
    pub fn display_short(&self) -> String {
        match self {
            ShaderSource::Inline { wgsl } => {
                let preview: String = wgsl.chars().take(40).collect();
                format!("inline:{preview}")
            }
            ShaderSource::Path { path } => format!("path:{path}"),
            ShaderSource::Msl { vertex, fragment, compute, .. } => {
                let v = vertex.as_deref().unwrap_or("-");
                let f = fragment.as_deref().unwrap_or("-");
                let c = compute.as_deref().unwrap_or("-");
                format!("msl:{v}/{f}/{c}")
            }
        }
    }
}

/// Which shader stages see a binding. The backend binds the resource
/// only into the declared stages (Metal has independent vertex/fragment
/// argument tables, so a vertex-only uniform need not occupy a fragment
/// slot). Default = both, so a binding authored without a stage hint is
/// visible everywhere (the safe, backwards-compatible choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShaderStages {
    pub vertex: bool,
    pub fragment: bool,
    /// Visible to the compute stage. `#[serde(default)]` so IR serialized
    /// before compute support (which had only `vertex`/`fragment`) still
    /// deserializes — the absent field reads back as `false`.
    #[serde(default)]
    pub compute: bool,
}

impl Default for ShaderStages {
    fn default() -> Self {
        Self { vertex: true, fragment: true, compute: false }
    }
}

impl ShaderStages {
    /// Visible to the vertex stage only.
    pub const VERTEX: Self = Self { vertex: true, fragment: false, compute: false };
    /// Visible to the fragment stage only.
    pub const FRAGMENT: Self = Self { vertex: false, fragment: true, compute: false };
    /// Visible to both raster stages.
    pub const BOTH: Self = Self { vertex: true, fragment: true, compute: false };
    /// Visible to the compute stage only (a compute kernel's bindings).
    pub const COMPUTE: Self = Self { vertex: false, fragment: false, compute: true };
}

/// One binding slot in the material's `@group(g)`. The compiler
/// emits the bind-group-layout entry from this declaration and
/// pairs it with the corresponding `ResourceId` at compile time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UniformBinding {
    /// Binding group — matches the `@group(N)` in WGSL. Defaults to 0
    /// so single-group materials need not declare it.
    #[serde(default)]
    pub group: u32,
    /// Binding index — matches the `@binding(N)` in WGSL.
    pub binding: u32,
    /// What this slot accepts. Determines bind-group-layout
    /// entry type + the resource-id kind that must satisfy it.
    pub kind: BindingKind,
    /// Which shader stages this binding is bound into. Defaults to both.
    #[serde(default)]
    pub stages: ShaderStages,
    /// Resource this binding draws from. The graph compile step
    /// validates that the bound resource's kind matches this
    /// binding's kind.
    pub resource: ResourceId,
}

impl UniformBinding {
    /// Construct a binding. Use [`UniformBinding::uniform`] for the common
    /// `@group(0)` per-frame-uniform case.
    #[must_use]
    pub fn new(
        group: u32,
        binding: u32,
        kind: BindingKind,
        stages: ShaderStages,
        resource: impl Into<ResourceId>,
    ) -> Self {
        Self { group, binding, kind, stages, resource: resource.into() }
    }

    /// A `@group(0)` per-frame uniform (`var<uniform>`) bound into both stages.
    #[must_use]
    pub fn uniform(binding: u32, resource: impl Into<ResourceId>) -> Self {
        Self::new(0, binding, BindingKind::Uniform, ShaderStages::BOTH, resource)
    }

    /// A `@group(0)` fragment-stage `texture_2d<f32>` binding.
    #[must_use]
    pub fn texture(binding: u32, resource: impl Into<ResourceId>) -> Self {
        Self::new(0, binding, BindingKind::Texture, ShaderStages::FRAGMENT, resource)
    }

    /// A `@group(0)` fragment-stage `sampler` binding.
    #[must_use]
    pub fn sampler(binding: u32, resource: impl Into<ResourceId>) -> Self {
        Self::new(0, binding, BindingKind::Sampler, ShaderStages::FRAGMENT, resource)
    }

    /// A `@group(0)` compute-stage `var<storage, read_write>` binding — a
    /// compute kernel's scratch / output buffer.
    #[must_use]
    pub fn storage_rw(binding: u32, resource: impl Into<ResourceId>) -> Self {
        Self::new(0, binding, BindingKind::StorageReadWrite, ShaderStages::COMPUTE, resource)
    }

    /// A `@group(0)` compute-stage `var<storage, read>` binding — a compute
    /// kernel's read-only input buffer.
    #[must_use]
    pub fn storage_read(binding: u32, resource: impl Into<ResourceId>) -> Self {
        Self::new(0, binding, BindingKind::StorageRead, ShaderStages::COMPUTE, resource)
    }
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
