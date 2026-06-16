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
    /// A `Compute` node carries a [`ComputeDispatch`] (its
    /// threadgroup grid); `compile()` rejects one that doesn't.
    Compute,
    /// `wgpu::CommandEncoder::copy_texture_to_texture` — pure
    /// blit. Useful when an effect just needs to upsample or
    /// convert a format without running a shader.
    Blit,
}

/// Threadgroup dispatch dimensions for a [`PassKind::Compute`] node — the
/// compute analog of [`crate::pipeline::DrawKind`]. `groups` is the number of
/// threadgroups dispatched in each dimension; `threads_per_group` mirrors the
/// kernel's `@workgroup_size`. The backend issues one
/// `dispatchThreadgroups(groups, threads_per_group)` (Metal) /
/// `dispatch_workgroups(groups)` (wgpu) per node. `Copy` + no float fields, so
/// it stays `Eq`/`Hash` like the rest of the IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComputeDispatch {
    /// Number of threadgroups per dimension `[x, y, z]`.
    pub groups: [u32; 3],
    /// Threads per threadgroup per dimension `[x, y, z]` — the kernel's
    /// `@workgroup_size`.
    pub threads_per_group: [u32; 3],
}

impl ComputeDispatch {
    /// A 1-D dispatch covering `count` invocations at threadgroup size
    /// `group_size`, rounding the group count up so every element is covered.
    #[must_use]
    pub fn linear(count: u32, group_size: u32) -> Self {
        let g = group_size.max(1);
        Self {
            groups: [count.div_ceil(g).max(1), 1, 1],
            threads_per_group: [g, 1, 1],
        }
    }

    /// A 2-D dispatch over a `width × height` grid at the given 2-D threadgroup
    /// size, rounding each dimension up to cover the whole grid.
    #[must_use]
    pub fn grid_2d(width: u32, height: u32, group: [u32; 2]) -> Self {
        let (gx, gy) = (group[0].max(1), group[1].max(1));
        Self {
            groups: [width.div_ceil(gx).max(1), height.div_ceil(gy).max(1), 1],
            threads_per_group: [gx, gy, 1],
        }
    }

    /// Total thread invocations this dispatch covers (groups × threads, over
    /// all three dimensions). Useful for "did I size the buffer big enough"
    /// checks at authoring time.
    #[must_use]
    pub fn total_threads(&self) -> u64 {
        self.groups
            .iter()
            .chain(self.threads_per_group.iter())
            .map(|&n| u64::from(n))
            .product()
    }
}

#[cfg(test)]
mod tests {
    use super::ComputeDispatch;

    #[test]
    fn linear_rounds_groups_up_to_cover_every_element() {
        // 100 elements at group size 64 → 2 groups (covers 128 ≥ 100).
        let d = ComputeDispatch::linear(100, 64);
        assert_eq!(d.groups, [2, 1, 1]);
        assert_eq!(d.threads_per_group, [64, 1, 1]);
        assert!(d.total_threads() >= 100);
    }

    #[test]
    fn linear_covers_at_least_one_group_even_for_zero() {
        let d = ComputeDispatch::linear(0, 64);
        assert_eq!(d.groups, [1, 1, 1], "always dispatch at least one group");
    }

    #[test]
    fn grid_2d_rounds_each_dimension_up() {
        // 1920×1080 at 8×8 → 240×135 groups.
        let d = ComputeDispatch::grid_2d(1920, 1080, [8, 8]);
        assert_eq!(d.groups, [240, 135, 1]);
        assert_eq!(d.threads_per_group, [8, 8, 1]);
    }
}
