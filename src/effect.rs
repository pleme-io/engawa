//! Operator-facing effect — material + enable bit + ordering
//! priority. Effects are the unit operators turn on/off in
//! their YAML / lisp config; a node is the IR after compile.
//!
//! A single effect may compile into multiple nodes (e.g. a
//! two-pass blur — separable horizontal then vertical). The
//! `Effect → Node` lowering happens in a consumer-side compile
//! step that engawa doesn't dictate; engawa just owns the IR
//! both sides agree on.

use serde::{Deserialize, Serialize};

use crate::material::Material;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Effect {
    /// Operator-friendly name. Matches the YAML key (`effects.
    /// scanlines.enabled = true`) and the tatara-lisp form name.
    pub name: String,
    /// Whether this effect contributes nodes to the compiled
    /// graph. Operators toggle this from their YAML; engawa's
    /// compile path skips disabled effects entirely.
    pub enabled: bool,
    /// Render-order priority. Higher runs later — closer to the
    /// final swapchain present. Conventions:
    ///   * 0..=99   pre-scene  (clear, background overlay)
    ///   * 100..=199 scene     (cell grid, cursor, text)
    ///   * 200..=799 post      (bloom, scanlines, CRT curve)
    ///   * 800..=899 chrome    (status bar, popup overlays)
    ///   * 900..=999 final     (composite to swapchain)
    pub priority: u16,
    /// Material this effect contributes. The consumer's lowering
    /// step decides how many nodes to emit from one material
    /// (single-pass effects → one node; multi-pass effects like
    /// separable blur → multiple).
    pub material: Material,
}
