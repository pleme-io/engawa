//! Pass kinds — what wgpu encoder dispatch a node compiles to.
//!
//! Bevy's render graph distinguishes raster passes from compute
//! passes from blits because each takes a different wgpu API
//! call. Engawa keeps that distinction; the compiler routes to
//! the right encoder method.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassKind {
    /// `wgpu::CommandEncoder::begin_render_pass` — rasterize
    /// triangles + fragments into one or more color attachments.
    /// The mado cell-grid + every fullscreen effect uses this.
    Render,
    /// `wgpu::CommandEncoder::begin_compute_pass` — dispatch
    /// a compute shader. Useful for post-processing that needs
    /// neighbor reads (Gaussian blur, FXAA, particle sims).
    Compute,
    /// `wgpu::CommandEncoder::copy_texture_to_texture` — pure
    /// blit. Useful when an effect just needs to upsample or
    /// convert a format without running a shader.
    Blit,
}
