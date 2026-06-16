//! Game-renderer pipeline state — the typed, GPU-free additions that let an
//! engawa graph describe real mesh draws (not just full-screen effects):
//! draw calls, depth testing, texture formats, and fixed-function state.
//!
//! Every type here is `Eq + Hash` (typed enums, **no float blend factors**) so
//! `Node`/`Material` stay hashable, and every type has a `Default` that equals
//! engawa's current implicit behavior — adding these fields changes nothing for
//! existing 2D effects. Backends (`engawa-metal`, `engawa-wgpu`) expand each
//! enum to a concrete `MTLPixelFormat` / `wgpu::TextureFormat` etc.

use serde::{Deserialize, Serialize};

use crate::resource::ResourceId;

/// How a render node issues its draw call.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "draw", rename_all = "snake_case")]
pub enum DrawKind {
    /// A full-screen triangle/quad with no vertex buffer — the implicit
    /// behavior of every post-process effect today. The default.
    #[default]
    FullscreenQuad,
    /// An indexed mesh draw from a vertex-buffer + index-buffer resource.
    Indexed {
        vertices: ResourceId,
        indices: ResourceId,
        index_count: u32,
    },
    /// An instanced indexed draw — `instances` copies of the indexed mesh.
    Instanced {
        vertices: ResourceId,
        indices: ResourceId,
        index_count: u32,
        instances: u32,
    },
}

/// Depth-buffer comparison for a render node's depth attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompareFunction {
    Never,
    Less,
    Equal,
    #[default]
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// Depth test configuration for a render node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DepthSpec {
    /// The depth attachment resource (a `Texture` with a depth `format`).
    pub attachment: ResourceId,
    /// Comparison against the existing depth value.
    pub compare: CompareFunction,
    /// Whether passing fragments write their depth back.
    pub write: bool,
}

impl DepthSpec {
    /// Standard 3D depth test: `LessEqual`, depth-write enabled.
    #[must_use]
    pub fn new(attachment: impl Into<ResourceId>) -> Self {
        Self {
            attachment: attachment.into(),
            compare: CompareFunction::LessEqual,
            write: true,
        }
    }
}

/// Texture pixel format. GPU-free typed mirror; backends map to
/// `MTLPixelFormat` / `wgpu::TextureFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextureFormat {
    #[default]
    Rgba8Unorm,
    /// The `CAMetalLayer` / typical swapchain color format.
    Bgra8Unorm,
    Rgba8UnormSrgb,
    Bgra8UnormSrgb,
    Rgba16Float,
    R8Unorm,
    Depth32Float,
    Depth24PlusStencil8,
}

impl TextureFormat {
    /// Whether this is a depth/stencil format (vs a color format).
    #[must_use]
    pub fn is_depth(self) -> bool {
        matches!(self, Self::Depth32Float | Self::Depth24PlusStencil8)
    }
}

/// Alpha-blending mode for a material. Typed (no float factors) so `Material`
/// stays `Eq + Hash`; each backend expands it to a fixed blend state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// Opaque — the source overwrites the destination (no blend). Default.
    #[default]
    Replace,
    /// `src.a * src + (1 - src.a) * dst`.
    AlphaBlend,
    /// Source already premultiplied: `src + (1 - src.a) * dst`.
    PremultipliedAlpha,
    /// `src + dst`.
    Additive,
    /// `src * dst`.
    Multiply,
}

/// Triangle face-culling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CullMode {
    #[default]
    None,
    Front,
    Back,
}

/// Front-face winding order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FrontFace {
    #[default]
    Ccw,
    Cw,
}

/// Primitive topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Topology {
    #[default]
    TriangleList,
    TriangleStrip,
    LineList,
    LineStrip,
    PointList,
}

/// Fixed-function render state for a material. **All-default equals engawa's
/// current implicit behavior** (opaque, no cull, ccw, triangle-list), so
/// existing full-screen effects render identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct RenderState {
    pub blend: BlendMode,
    pub cull: CullMode,
    pub winding: FrontFace,
    pub topology: Topology,
}
